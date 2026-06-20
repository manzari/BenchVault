//! OrcaSlicer integration: locating the binary, opening model files in the GUI,
//! and building a fresh project (`.3mf`) from an object's STL parts.
//!
//! The binary location is user-configurable (settings key `tools.orca_path`).
//! When unset we best-effort auto-detect a local install so the feature works
//! out of the box. The configured value may be either a path to an executable
//! (e.g. an AppImage) or a command line (e.g. `flatpak run io.github.softfever.OrcaSlicer`).
//!
//! Opening the GUI is fire-and-forget — we never block the UI on the slicer.
//! The one place we *do* wait is the headless project build, which is bounded by
//! a timeout and run on a blocking thread.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sqlx::{Row, SqlitePool};

use crate::domain::{AssetId, ObjectId};
use crate::library::{self, Library};
use crate::repository;

use super::objects as svc_objects;
use super::{ServiceError, ServiceResult};

const ORCA_PATH_KEY: &str = "tools.orca_path";
const ORCA_MACHINE_KEY: &str = "tools.orca_machine";
const ORCA_FILAMENT_KEY: &str = "tools.orca_filament";

/// How long to wait for the headless `--export-3mf` build before giving up and
/// falling back to opening the raw STLs in the GUI.
const BUILD_TIMEOUT: Duration = Duration::from_secs(180);

/// A selectable OrcaSlicer preset discovered on disk. `path` is the JSON file we
/// hand to the CLI; `name`/`vendor` are for display.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct OrcaPreset {
    pub name: String,
    pub vendor: String,
    pub path: String,
}

/// Presets available to choose as defaults, grouped by kind.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct OrcaPresets {
    pub machines: Vec<OrcaPreset>,
    pub filaments: Vec<OrcaPreset>,
}

/// The configured default printer/filament preset file paths (if any).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct OrcaDefaults {
    pub machine_path: Option<String>,
    pub filament_path: Option<String>,
}

/// Outcome of [`new_project`], surfaced to the frontend so it can tailor the
/// toast (a clean build vs. a degraded fallback). Serializes to a snake_case
/// string on the wire.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NewProjectOutcome {
    /// A `<name>.3mf` already existed in the object dir; opened it as-is.
    OpenedExisting,
    /// Built a fresh `<name>.3mf` headlessly and opened it.
    BuiltAndOpened,
    /// Headless build failed/timed out; loaded the raw STLs into a new GUI
    /// project instead (the user must Save Project themselves).
    FellBackToStls,
}

// --- configuration ---------------------------------------------------------

/// The path/command shown in Settings: the stored value if set, otherwise the
/// auto-detected install (so the field is pre-filled and the feature works
/// before the user touches anything).
pub async fn get_configured_path(pool: &SqlitePool) -> ServiceResult<Option<String>> {
    if let Some(v) = repository::settings::get(pool, ORCA_PATH_KEY).await? {
        if !v.trim().is_empty() {
            return Ok(Some(v));
        }
    }
    Ok(detect_orca().map(|p| p.to_string_lossy().into_owned()))
}

pub async fn set_configured_path(pool: &SqlitePool, value: Option<&str>) -> ServiceResult<()> {
    put_or_delete(pool, ORCA_PATH_KEY, value).await
}

// --- default printer / filament presets ------------------------------------

/// The configured default printer/filament preset paths.
pub async fn get_defaults(pool: &SqlitePool) -> ServiceResult<OrcaDefaults> {
    Ok(OrcaDefaults {
        machine_path: nonempty(repository::settings::get(pool, ORCA_MACHINE_KEY).await?),
        filament_path: nonempty(repository::settings::get(pool, ORCA_FILAMENT_KEY).await?),
    })
}

pub async fn set_defaults(
    pool: &SqlitePool,
    machine_path: Option<&str>,
    filament_path: Option<&str>,
) -> ServiceResult<()> {
    put_or_delete(pool, ORCA_MACHINE_KEY, machine_path).await?;
    put_or_delete(pool, ORCA_FILAMENT_KEY, filament_path).await?;
    Ok(())
}

/// Discover the printer (machine) and filament presets OrcaSlicer ships/stores
/// on disk, so the frontend can offer them as dropdowns. Both system (bundled
/// vendor) and user presets are included.
pub fn list_presets() -> OrcaPresets {
    let mut machines = Vec::new();
    let mut filaments = Vec::new();
    if let Some(cfg) = orca_config_dir() {
        for base in ["system", "user"] {
            collect_presets(&cfg.join(base), "machine", &mut machines);
            collect_presets(&cfg.join(base), "filament", &mut filaments);
        }
    }
    finalize_presets(&mut machines);
    finalize_presets(&mut filaments);
    OrcaPresets { machines, filaments }
}

/// Locate OrcaSlicer's config root (`…/OrcaSlicer`), checking the XDG location,
/// the default `~/.config`, and the Flatpak sandbox path.
fn orca_config_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        candidates.push(PathBuf::from(xdg).join("OrcaSlicer"));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        candidates.push(home.join(".config/OrcaSlicer"));
        candidates
            .push(home.join(".var/app/io.github.softfever.OrcaSlicer/config/OrcaSlicer"));
    }
    candidates.into_iter().find(|p| p.is_dir())
}

/// Presets live at `<root>/<vendor-or-user>/<kind>/<name>.json`.
fn collect_presets(root: &Path, kind: &str, out: &mut Vec<OrcaPreset>) {
    let Ok(vendors) = std::fs::read_dir(root) else { return };
    for vendor in vendors.flatten() {
        if !vendor.path().is_dir() {
            continue;
        }
        let vendor_name = vendor.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(vendor.path().join(kind)) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let name = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(OrcaPreset {
                name,
                vendor: vendor_name.clone(),
                path: p.to_string_lossy().into_owned(),
            });
        }
    }
}

fn finalize_presets(v: &mut Vec<OrcaPreset>) {
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v.dedup_by(|a, b| a.path == b.path);
}

fn nonempty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

async fn put_or_delete(pool: &SqlitePool, key: &str, value: Option<&str>) -> ServiceResult<()> {
    match value.map(str::trim).filter(|s| !s.is_empty()) {
        Some(v) => repository::settings::put(pool, key, v).await?,
        None => repository::settings::delete(pool, key).await?,
    }
    Ok(())
}

/// Resolve the launch spec (stored value or auto-detected), erroring if neither
/// is available so the frontend can prompt the user to configure it.
async fn resolve_spec(pool: &SqlitePool) -> ServiceResult<String> {
    if let Some(v) = repository::settings::get(pool, ORCA_PATH_KEY).await? {
        if !v.trim().is_empty() {
            return Ok(v.trim().to_string());
        }
    }
    detect_orca()
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| {
            ServiceError::Validation(
                "OrcaSlicer was not found. Set its path in Settings.".into(),
            )
        })
}

/// Best-effort discovery of a local OrcaSlicer: PATH names first, then
/// `*orca*.AppImage` files in the usual install dirs. Directories are tried in
/// priority order (a deliberate install under `~/Applications` beats a leftover
/// download), and within the first dir that has a match the newest wins. We
/// don't filter by the execute bit here — AppImages are often downloaded
/// without it, and the launcher chmods them just-in-time.
pub fn detect_orca() -> Option<PathBuf> {
    for name in ["orca-slicer", "OrcaSlicer", "orcaslicer", "orca_slicer"] {
        if let Some(p) = which(name) {
            return Some(p);
        }
    }

    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    for sub in ["Applications", "Apps", ".local/bin", "bin", "Downloads", "Desktop"] {
        let dir = home.join(sub);
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut matches: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.contains("orca") && name.ends_with(".appimage") {
                let mtime = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                matches.push((mtime, entry.path()));
            }
        }
        matches.sort_by(|a, b| b.0.cmp(&a.0));
        if let Some((_, p)) = matches.into_iter().next() {
            return Some(p);
        }
    }
    None
}

/// Minimal `which`: scan `$PATH` for an executable file named `name`.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

// --- launching -------------------------------------------------------------

/// Open one or more model files in the OrcaSlicer GUI. Fire-and-forget.
pub async fn open_asset(
    pool: &SqlitePool,
    library: &Library,
    asset_id: AssetId,
) -> ServiceResult<()> {
    let asset = repository::assets::get(pool, asset_id).await?;
    let abs = library.absolute(Path::new(&asset.relative_path));
    if !abs.exists() {
        return Err(ServiceError::NotFound(format!(
            "file is missing on disk: {}",
            asset.original_filename
        )));
    }
    let spec = resolve_spec(pool).await?;
    spawn_open(&spec, &[abs])?;
    Ok(())
}

/// Build (or reopen) `<object-name>.3mf` in the object's library dir from all of
/// its STL parts, then open it in the GUI. See [`NewProjectOutcome`].
pub async fn new_project(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
) -> ServiceResult<NewProjectOutcome> {
    let spec = resolve_spec(pool).await?;
    let object = repository::objects::get_basic(pool, object_id).await?;
    let dir = svc_objects::ensure_dir(pool, library, object_id).await?;

    let stls = stl_part_paths(pool, library, object_id).await?;
    if stls.is_empty() {
        return Err(ServiceError::Validation(
            "This object has no STL parts to load into Orca.".into(),
        ));
    }

    let target = dir.join(format!("{}.3mf", library::sanitize_segment(&object.name)));

    // Don't clobber an existing project the user may have edited — just reopen.
    if target.exists() {
        spawn_open(&spec, &[target])?;
        return Ok(NewProjectOutcome::OpenedExisting);
    }

    let defaults = get_defaults(pool).await?;

    // Headless build runs Orca to completion, so push it off the async runtime.
    let built = {
        let spec = spec.clone();
        let stls = stls.clone();
        let target = target.clone();
        let machine = defaults.machine_path.map(PathBuf::from);
        let filament = defaults.filament_path.map(PathBuf::from);
        tokio::task::spawn_blocking(move || {
            build_project(&spec, &stls, &target, machine, filament)
        })
        .await
        .unwrap_or(false)
    };

    if built && target.exists() {
        // The new file lands inside the watched library; the watcher will pick
        // it up and attach it to the object as an asset.
        spawn_open(&spec, &[target])?;
        Ok(NewProjectOutcome::BuiltAndOpened)
    } else {
        spawn_open(&spec, &stls)?;
        Ok(NewProjectOutcome::FellBackToStls)
    }
}

/// Absolute paths of an object's `.stl` parts that still exist on disk, in the
/// order they're shown in the UI.
async fn stl_part_paths(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
) -> ServiceResult<Vec<PathBuf>> {
    let rows = sqlx::query(
        "SELECT a.relative_path
           FROM object_assets oa
           JOIN assets a ON a.id = oa.asset_id
          WHERE oa.object_id = ? AND oa.role = 'stl_part' AND a.deleted_at IS NULL
          ORDER BY oa.sort_order, a.id",
    )
    .bind(object_id.0)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::new();
    for r in rows {
        let rel: String = r.get("relative_path");
        if !rel.to_lowercase().ends_with(".stl") {
            continue;
        }
        let abs = library.absolute(Path::new(&rel));
        if abs.exists() {
            out.push(abs);
        }
    }
    Ok(out)
}

/// Run Orca headlessly to load every STL as a separate object, arrange them on
/// the plate, apply the default printer/filament presets, and write the project
/// to `target`. Returns whether it succeeded.
fn build_project(
    spec: &str,
    stls: &[PathBuf],
    target: &Path,
    machine: Option<PathBuf>,
    filament: Option<PathBuf>,
) -> bool {
    let mut cmd = build_command(spec);

    // `--load-settings` wants machine + process. Orca rejects a machine without
    // a compatible process ("process not compatible with printer"), so we only
    // pass the printer if we can also resolve one of its processes; otherwise we
    // skip it and let Orca fall back to its built-in default.
    if let Some(m) = machine.as_ref().filter(|p| p.exists()) {
        if let Some(process) = find_process_for_machine(m) {
            cmd.arg("--load-settings").arg(format!(
                "{};{}",
                m.to_string_lossy(),
                process.to_string_lossy()
            ));
        }
    }
    if let Some(f) = filament.as_ref().filter(|p| p.exists()) {
        cmd.arg("--load-filaments").arg(f);
    }

    for s in stls {
        cmd.arg(s);
    }
    cmd.arg("--arrange").arg("1");
    cmd.arg("--export-3mf").arg(target);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    run_bounded(cmd, BUILD_TIMEOUT)
}

/// Find a process (print-settings) preset compatible with `machine`, preferring
/// a standard 0.20mm profile. System presets name their processes
/// `<quality> @<machine>`, and live in the machine's sibling `process/` dir.
fn find_process_for_machine(machine: &Path) -> Option<PathBuf> {
    let machine_name = machine.file_stem()?.to_string_lossy().into_owned();
    let process_dir = machine.parent()?.parent()?.join("process");

    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&process_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .filter(|p| {
            p.file_stem()
                .map(|s| s.to_string_lossy().contains(&machine_name))
                .unwrap_or(false)
        })
        .collect();

    candidates.sort_by_key(|p| {
        let s = p
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let rank = match (s.contains("0.20"), s.contains("standard")) {
            (true, true) => 0,
            (false, true) => 1,
            (true, false) => 2,
            _ => 3,
        };
        (rank, s)
    });
    candidates.into_iter().next()
}

/// Launch the GUI on the given files without waiting for it to exit.
fn spawn_open(spec: &str, files: &[PathBuf]) -> ServiceResult<()> {
    let mut cmd = build_command(spec);
    for f in files {
        cmd.arg(f);
    }
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd.spawn().map_err(ServiceError::Io)?;
    Ok(())
}

/// Turn a configured spec into a `Command`. If the whole string is an existing
/// path, it's the program (handles AppImage paths with spaces). Otherwise it's
/// treated as a `program arg arg…` command line (handles `flatpak run …`).
fn build_command(spec: &str) -> Command {
    let spec = spec.trim();
    if Path::new(spec).exists() {
        ensure_executable(Path::new(spec));
        let mut cmd = Command::new(spec);
        // AppImages mount themselves via FUSE, which is broken or absent on many
        // systems ("Cannot mount AppImage, please check your FUSE setup"). The
        // self-extracting fallback sidesteps FUSE entirely; it's slightly slower
        // to start but always works. Must be the first argument so the AppImage
        // runtime — not OrcaSlicer — consumes it.
        if spec.to_ascii_lowercase().ends_with(".appimage") {
            cmd.arg("--appimage-extract-and-run");
        }
        return cmd;
    }
    let mut parts = spec.split_whitespace();
    let prog = parts.next().unwrap_or(spec);
    let mut cmd = Command::new(prog);
    for arg in parts {
        cmd.arg(arg);
    }
    cmd
}

/// AppImages are routinely downloaded without the execute bit, which makes
/// `spawn()` fail with `EACCES`. When the configured/detected spec is a real
/// file we own, add the execute bit just-in-time. Best-effort: if we can't
/// (e.g. not the owner), the spawn error surfaces to the user as before.
#[cfg(unix)]
fn ensure_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mode = meta.permissions().mode();
        if mode & 0o111 == 0 {
            let mut perms = meta.permissions();
            perms.set_mode(mode | 0o755);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) {}

/// Spawn `cmd`, waiting up to `timeout` for it to finish; kill it if it runs
/// over. Returns whether it exited successfully within the budget.
fn run_bounded(mut cmd: Command, timeout: Duration) -> bool {
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait(); // reap so we don't leave a zombie
                    return false;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}
