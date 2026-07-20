//! OpenSCAD integration: locating the binary, opening `.scad` files in the GUI,
//! and rendering a `.scad` to an STL mesh for the three.js preview (with an
//! on-disk cache invalidated by dependency hashes).
//!
//! Modeled on [`super::orca`]: the binary location is user-configurable (settings
//! key `tools.openscad_path`) and best-effort auto-detected when unset. GUI opens
//! are fire-and-forget; the headless render is bounded by a timeout on a blocking
//! thread. Mesh export (`-o out.stl`) is fully headless — no GPU/X server needed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use sqlx::SqlitePool;

use crate::domain::{AssetId, AssetKind};
use crate::library::Library;
use crate::repository;

use super::files::sha256_file;
use super::{ServiceError, ServiceResult};

const OPENSCAD_PATH_KEY: &str = "tools.openscad_path";

/// Kill a full render that runs longer than this (complex CGAL models can take
/// minutes; we cap the foreground wait). Mirrors `orca::BUILD_TIMEOUT`.
const RENDER_TIMEOUT: Duration = Duration::from_secs(180);

/// Flatpak app id; used as a marker spec (`flatpak run <id>`) when the flatpak
/// build is the only install found.
const FLATPAK_ID: &str = "org.openscad.OpenSCAD";

// --- configuration ---------------------------------------------------------

/// The path/command shown in Settings: the stored value if set, otherwise the
/// auto-detected install (so the field is pre-filled and the feature works out
/// of the box).
pub async fn get_configured_path(pool: &SqlitePool) -> ServiceResult<Option<String>> {
    if let Some(v) = repository::settings::get(pool, OPENSCAD_PATH_KEY).await? {
        if !v.trim().is_empty() {
            return Ok(Some(v));
        }
    }
    Ok(detect_openscad())
}

pub async fn set_configured_path(pool: &SqlitePool, value: Option<&str>) -> ServiceResult<()> {
    match value.map(str::trim).filter(|s| !s.is_empty()) {
        Some(v) => repository::settings::put(pool, OPENSCAD_PATH_KEY, v).await?,
        None => repository::settings::delete(pool, OPENSCAD_PATH_KEY).await?,
    }
    Ok(())
}

/// Resolve the launch spec (stored value if it still exists, else auto-detected),
/// erroring if neither is available.
async fn resolve_spec(pool: &SqlitePool) -> ServiceResult<String> {
    if let Some(v) = repository::settings::get(pool, OPENSCAD_PATH_KEY).await? {
        let v = v.trim();
        if !v.is_empty() {
            // A stored plain path that no longer exists falls through to detection;
            // a command line (e.g. `flatpak run …`) is taken as-is.
            if !Path::new(v).is_absolute() || Path::new(v).exists() {
                return Ok(v.to_string());
            }
        }
    }
    detect_openscad().ok_or_else(|| {
        ServiceError::Validation("OpenSCAD not found — set the path in Settings".into())
    })
}

/// Best-effort discovery of a local OpenSCAD install. Returns a spec string
/// (an absolute path, or a `flatpak run …` command line).
pub fn detect_openscad() -> Option<String> {
    for name in ["openscad", "openscad-nightly"] {
        if let Some(p) = which(name) {
            return Some(p.to_string_lossy().into_owned());
        }
    }

    #[cfg(target_os = "linux")]
    {
        if flatpak_installed() {
            return Some(format!("flatpak run {FLATPAK_ID}"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        let app = Path::new("/Applications/OpenSCAD.app/Contents/MacOS/OpenSCAD");
        if app.exists() {
            return Some(app.to_string_lossy().into_owned());
        }
    }

    #[cfg(target_os = "windows")]
    {
        for c in [
            r"C:\Program Files\OpenSCAD\openscad.com",
            r"C:\Program Files\OpenSCAD\openscad.exe",
        ] {
            if Path::new(c).exists() {
                return Some(c.to_string());
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn flatpak_installed() -> bool {
    Command::new("flatpak")
        .args(["info", FLATPAK_ID])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Minimal `which`: scan `$PATH` for an executable file named `name`.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

// --- version detection -----------------------------------------------------

/// Cache of parsed `--version` output keyed by resolved spec, so we don't shell
/// out on every render.
fn version_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Run `--version` and parse `OpenSCAD version <ver>` from stderr (falling back
/// to stdout). Old builds return a non-zero exit for `--version`, so we parse the
/// output regardless of the exit code. Result is cached per spec.
fn openscad_version(spec: &str) -> Option<String> {
    if let Some(v) = version_cache().lock().unwrap().get(spec) {
        return v.clone();
    }
    let mut cmd = build_command(spec);
    cmd.arg("--version");
    let parsed = cmd
        .output()
        .ok()
        .and_then(|out| {
            parse_version(&String::from_utf8_lossy(&out.stderr))
                .or_else(|| parse_version(&String::from_utf8_lossy(&out.stdout)))
        });
    version_cache()
        .lock()
        .unwrap()
        .insert(spec.to_string(), parsed.clone());
    parsed
}

fn parse_version(text: &str) -> Option<String> {
    let idx = text.find("OpenSCAD version ")?;
    let rest = &text[idx + "OpenSCAD version ".len()..];
    let token = rest.split_whitespace().next()?;
    (!token.is_empty()).then(|| token.to_string())
}

/// The Manifold backend exists only in nightlies (non-experimental since
/// `2024.09.28`). True iff `version` parses as a `YYYY.MM.DD` date at or after that.
fn supports_manifold(version: &str) -> bool {
    let mut parts = version.split('.');
    let y: u32 = match parts.next().and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return false,
    };
    let m: u32 = match parts.next().and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return false,
    };
    // A dated nightly has a day component; the stable "2021.01" does not.
    let d: u32 = match parts.next().and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => return false,
    };
    (y, m, d) >= (2024, 9, 28)
}

// --- opening in the GUI ----------------------------------------------------

/// Open a `.scad` asset in the OpenSCAD GUI. Fire-and-forget.
pub async fn open_asset(
    pool: &SqlitePool,
    library: &Library,
    asset_id: AssetId,
) -> ServiceResult<()> {
    let asset = repository::assets::get(pool, asset_id).await?;
    if asset.kind != AssetKind::Scad {
        return Err(ServiceError::Validation("not an OpenSCAD asset".into()));
    }
    let abs = library.absolute(Path::new(&asset.relative_path));
    if !abs.exists() {
        return Err(ServiceError::NotFound(format!(
            "file is missing on disk: {}",
            asset.original_filename
        )));
    }
    let spec = resolve_spec(pool).await?;
    let mut cmd = build_command(&spec);
    cmd.arg(&abs);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd.spawn().map_err(ServiceError::Io)?;
    Ok(())
}

// --- preview render --------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
struct Dep {
    path: String,
    sha256: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Manifest {
    deps: Vec<Dep>,
    openscad_version: Option<String>,
    rendered_at: i64,
}

fn cache_paths(library: &Library, asset_id: AssetId) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let dir = library.scad_cache_dir();
    let id = asset_id.0;
    (
        dir.join(format!("{id}.stl")),
        dir.join(format!("{id}.json")),
        dir.join(format!("{id}.deps")),
        dir.join(format!("{id}.tmp.stl")),
    )
}

/// Best-effort removal of every cache file for an asset (called on asset delete).
pub fn clear_cache(library: &Library, asset_id: AssetId) {
    let (stl, manifest, deps, tmp) = cache_paths(library, asset_id);
    for p in [stl, manifest, deps, tmp] {
        let _ = std::fs::remove_file(p);
    }
}

/// Render a `.scad` asset to STL bytes, using the on-disk cache when every
/// recorded dependency still hashes to its stored value. Bounded by a timeout.
///
/// Callers should serialize concurrent invocations (a render mutex) so a
/// double-click doesn't spawn two renders; the second caller then hits the cache.
pub async fn render_preview(
    pool: &SqlitePool,
    library: &Library,
    asset_id: AssetId,
) -> ServiceResult<Vec<u8>> {
    let asset = repository::assets::get(pool, asset_id).await?;
    if asset.kind != AssetKind::Scad {
        return Err(ServiceError::Validation("not an OpenSCAD asset".into()));
    }
    let source = library.absolute(Path::new(&asset.relative_path));
    if !source.exists() {
        return Err(ServiceError::NotFound(format!(
            "file is missing on disk: {}",
            asset.original_filename
        )));
    }

    let (stl_path, manifest_path, deps_path, tmp_path) = cache_paths(library, asset_id);

    // Cache hit: STL + manifest present and every dep unchanged.
    if let Some(bytes) = try_cache_hit(&stl_path, &manifest_path) {
        store_geometry(pool, asset_id, &bytes).await;
        return Ok(bytes);
    }

    let spec = resolve_spec(pool).await?;
    let version = openscad_version(&spec);
    let manifold = version.as_deref().map(supports_manifold).unwrap_or(false);

    std::fs::create_dir_all(library.scad_cache_dir())?;
    let _ = std::fs::remove_file(&tmp_path);
    let _ = std::fs::remove_file(&deps_path);

    let outcome = {
        let spec = spec.clone();
        let source = source.clone();
        let tmp_path = tmp_path.clone();
        let deps_path = deps_path.clone();
        tokio::task::spawn_blocking(move || {
            run_render(&spec, &source, &tmp_path, &deps_path, manifold)
        })
        .await
        .map_err(|e| ServiceError::Other(format!("render task failed: {e}")))?
    };

    match outcome {
        RenderRun::Timeout => {
            let _ = std::fs::remove_file(&tmp_path);
            let hint = if version.as_deref() == Some("2021.01") {
                " (try a nightly build with the faster Manifold backend)"
            } else {
                ""
            };
            Err(ServiceError::Other(format!(
                "OpenSCAD render timed out after {}s{hint}",
                RENDER_TIMEOUT.as_secs()
            )))
        }
        RenderRun::Failed(stderr) => {
            let _ = std::fs::remove_file(&tmp_path);
            Err(ServiceError::Other(format!(
                "OpenSCAD render failed: {}",
                stderr_tail(&stderr)
            )))
        }
        RenderRun::Ok => {
            // Parse deps, hash each, write the manifest, then atomically publish.
            let mut dep_files = parse_deps_file(&deps_path);
            if !dep_files.iter().any(|p| p == &source) {
                dep_files.push(source.clone());
            }
            let deps: Vec<Dep> = dep_files
                .into_iter()
                .filter_map(|p| {
                    sha256_file(&p).ok().map(|sha256| Dep {
                        path: p.to_string_lossy().into_owned(),
                        sha256,
                    })
                })
                .collect();
            let manifest = Manifest {
                deps,
                openscad_version: version,
                rendered_at: crate::db::now_ts(),
            };
            std::fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
            std::fs::rename(&tmp_path, &stl_path)?;
            let bytes = std::fs::read(&stl_path)?;
            store_geometry(pool, asset_id, &bytes).await;
            Ok(bytes)
        }
    }
}

/// Best-effort: derive geometry from a freshly rendered `.scad` STL and store it
/// in `mesh_metadata` (tagged `source_format = "scad"`) so it shows up in the same
/// dimensions/volume UI as STL/OBJ. Runs on every render (cache hit or miss); a
/// parse or DB failure is logged and ignored — it must never fail a preview.
async fn store_geometry(pool: &SqlitePool, asset_id: AssetId, stl_bytes: &[u8]) {
    let stats = match crate::parsers::mesh::stl::parse_bytes(stl_bytes) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[scad] geometry parse failed for {asset_id:?}: {e}");
            return;
        }
    };
    let meta = stats.to_metadata(asset_id, "scad");
    if let Err(e) = repository::mesh_metadata::upsert(pool, &meta).await {
        eprintln!("[scad] geometry store failed for {asset_id:?}: {e}");
    }
}

/// Return the cached STL bytes iff the manifest exists and every recorded dep
/// still hashes to its stored value (a missing dep counts as invalid).
fn try_cache_hit(stl_path: &Path, manifest_path: &Path) -> Option<Vec<u8>> {
    if !stl_path.exists() {
        return None;
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(manifest_path).ok()?).ok()?;
    for dep in &manifest.deps {
        match sha256_file(Path::new(&dep.path)) {
            Ok(sha) if sha == dep.sha256 => {}
            _ => return None,
        }
    }
    std::fs::read(stl_path).ok()
}

enum RenderRun {
    Ok,
    Failed(String),
    Timeout,
}

/// Spawn the headless render, capturing stderr, killing it on timeout.
fn run_render(
    spec: &str,
    source: &Path,
    out_tmp: &Path,
    deps: &Path,
    manifold: bool,
) -> RenderRun {
    let mut cmd = build_command(spec);
    cmd.arg("-o").arg(out_tmp);
    cmd.arg("-d").arg(deps);
    if manifold {
        cmd.arg("--backend=manifold");
    }
    cmd.arg(source);
    cmd.stdout(Stdio::null()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return RenderRun::Failed(e.to_string()),
    };

    // Drain stderr on a helper thread so a full pipe can't block the child.
    let drain = child.stderr.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut s, &mut buf);
            buf
        })
    });
    let collect = |drain: Option<std::thread::JoinHandle<Vec<u8>>>| -> String {
        drain
            .and_then(|h| h.join().ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    };

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let err = collect(drain);
                return if status.success() {
                    RenderRun::Ok
                } else {
                    RenderRun::Failed(err)
                };
            }
            Ok(None) => {
                if start.elapsed() > RENDER_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = collect(drain);
                    return RenderRun::Timeout;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return RenderRun::Failed(format!("{}: {e}", collect(drain)));
            }
        }
    }
}

/// Keep the last ~2000 chars of stderr — OpenSCAD's `ERROR:` lines are at the end.
fn stderr_tail(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return "no diagnostics".into();
    }
    if s.len() <= 2000 {
        return s.to_string();
    }
    let start = s.len() - 2000;
    // Snap to a char boundary so we don't slice mid-UTF-8.
    let start = (start..s.len()).find(|&i| s.is_char_boundary(i)).unwrap_or(0);
    format!("…{}", &s[start..])
}

/// Parse a make-style dependency file into absolute paths, ignoring the
/// `target:` prefix. Handles `\`-continued lines and `\`-escaped spaces.
fn parse_deps_file(deps: &Path) -> Vec<PathBuf> {
    let content = match std::fs::read_to_string(deps) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    // Join line continuations (`\` at end of line).
    let joined = content.replace("\\\r\n", " ").replace("\\\n", " ");
    let mut out = Vec::new();
    for line in joined.lines() {
        let rhs = strip_target(line);
        let mut cur = String::new();
        let mut chars = rhs.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(&n) = chars.peek() {
                        cur.push(n);
                        chars.next();
                    }
                }
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        out.push(PathBuf::from(std::mem::take(&mut cur)));
                    }
                }
                c => cur.push(c),
            }
        }
        if !cur.is_empty() {
            out.push(PathBuf::from(cur));
        }
    }
    out
}

/// Drop the make `target:` prefix, tolerating a Windows drive letter (`C:\…`) in
/// the target by only splitting on a colon followed by whitespace/end.
fn strip_target(line: &str) -> &str {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b':' && bytes.get(i + 1).map_or(true, |c| c.is_ascii_whitespace()) {
            return &line[i + 1..];
        }
    }
    line
}

// --- command construction --------------------------------------------------

/// Turn a configured spec into a `Command` (copied from `orca::build_command`):
/// a whole-string existing path is the program; otherwise it's a `program arg…`
/// command line (handles `flatpak run …`).
fn build_command(spec: &str) -> Command {
    let spec = spec.trim();
    if Path::new(spec).exists() {
        ensure_executable(Path::new(spec));
        let mut cmd = Command::new(spec);
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
