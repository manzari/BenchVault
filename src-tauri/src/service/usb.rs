use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::SqlitePool;

use crate::db::now_ts;
use crate::domain::{
    AssetId, MountedDrive, SyncReport, UsbManifest, UsbManifestFile, UsbMarker, UsbTarget,
};
use crate::events::{self, AppEvent};
use crate::library::{self, Library};
use crate::repository;
use crate::repository::gcode_metadata::ExportableGcode;
use crate::repository::settings::keys as kv;

use super::files::sha256_file;
use super::{ServiceError, ServiceResult};

const MARKER_FILENAME: &str = ".benchvault.json";
const MARKER_TMP_FILENAME: &str = ".benchvault.json.tmp";
const PROBE_FILENAME: &str = ".benchvault-probe";

// ---------- mount enumeration ----------

pub fn list_mounts() -> std::io::Result<Vec<MountedDrive>> {
    list_mounts_inner()
}

#[cfg(target_os = "linux")]
fn list_mounts_inner() -> std::io::Result<Vec<MountedDrive>> {
    let content = std::fs::read_to_string("/proc/self/mountinfo")?;
    let mut out: Vec<MountedDrive> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.split(' ').collect();
        let dash = match parts.iter().position(|&p| p == "-") {
            Some(i) => i,
            None => continue,
        };
        if parts.len() < dash + 3 || parts.len() < 5 {
            continue;
        }
        let mount_point = unescape_mountinfo(parts[4]);
        let fs_type = parts[dash + 1];
        let source = parts[dash + 2];
        if !source.starts_with("/dev/") {
            continue;
        }
        let mp = Path::new(&mount_point);
        let parent_ok = mp.starts_with("/media")
            || mp.starts_with("/run/media")
            || mp.starts_with("/mnt");
        if !parent_ok {
            continue;
        }
        if !seen.insert(mount_point.clone()) {
            continue;
        }
        let label = mp.file_name().and_then(|n| n.to_str()).map(|s| s.to_string());
        out.push(MountedDrive {
            mount_path: mount_point,
            label,
            fs_kind: Some(fs_type.to_string()),
        });
    }
    Ok(out)
}

#[cfg(not(target_os = "linux"))]
fn list_mounts_inner() -> std::io::Result<Vec<MountedDrive>> {
    Ok(Vec::new())
}

fn unescape_mountinfo(s: &str) -> String {
    // mountinfo octal-escapes individual UTF-8 *bytes* (e.g. space, tab, and any
    // byte of a multibyte char), so decode into a byte buffer and interpret the
    // whole thing as UTF-8 at the end — pushing each byte as a `char` would
    // mojibake any non-ASCII mount path (a value ≥ 0o200 is one UTF-8 byte, not
    // a codepoint).
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            let d1 = bytes[i + 1];
            let d2 = bytes[i + 2];
            let d3 = bytes[i + 3];
            if (b'0'..=b'7').contains(&d1)
                && (b'0'..=b'7').contains(&d2)
                && (b'0'..=b'7').contains(&d3)
            {
                let val = ((d1 - b'0') as u32) * 64
                    + ((d2 - b'0') as u32) * 8
                    + ((d3 - b'0') as u32);
                if let Ok(byte) = u8::try_from(val) {
                    out.push(byte);
                    i += 4;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------- target / marker ----------

pub async fn current_target(pool: &SqlitePool) -> ServiceResult<Option<UsbTarget>> {
    let raw = repository::settings::get(pool, kv::USB_TARGET).await?;
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

pub async fn find_target_mount(pool: &SqlitePool) -> ServiceResult<Option<PathBuf>> {
    let target = match current_target(pool).await? {
        Some(t) => t,
        None => return Ok(None),
    };
    let mounts = list_mounts().map_err(ServiceError::Io)?;
    for m in mounts {
        let mp = PathBuf::from(&m.mount_path);
        if let Ok(marker) = read_marker(&mp) {
            if marker.uuid == target.uuid {
                return Ok(Some(mp));
            }
        }
    }
    Ok(None)
}

pub async fn register_usb(
    pool: &SqlitePool,
    library: &Library,
    mount_path: &Path,
    label: &str,
    force: bool,
) -> ServiceResult<UsbTarget> {
    let canon_mount = mount_path.canonicalize().map_err(ServiceError::Io)?;
    let canon_lib = library
        .root()
        .canonicalize()
        .unwrap_or_else(|_| library.root().to_path_buf());
    if canon_mount.starts_with(&canon_lib) {
        return Err(ServiceError::Other(
            "cannot register the library directory as a USB target".into(),
        ));
    }

    let probe = canon_mount.join(PROBE_FILENAME);
    std::fs::write(&probe, b"ok").map_err(ServiceError::Io)?;
    let _ = std::fs::remove_file(&probe);

    if let Ok(existing) = read_marker(&canon_mount) {
        let saved = current_target(pool).await?;
        let same = saved.as_ref().map(|t| t.uuid == existing.uuid).unwrap_or(false);
        if !same && !force {
            return Err(ServiceError::Other("foreign marker".into()));
        }
    }

    let uuid = uuid::Uuid::new_v4().to_string();
    let marker = UsbMarker {
        uuid: uuid.clone(),
        label: label.to_string(),
        created_at: now_ts(),
        schema_version: 1,
    };
    write_marker_atomic(&canon_mount, &marker).map_err(ServiceError::Io)?;

    let target = UsbTarget {
        uuid,
        label: label.to_string(),
        registered_at: now_ts(),
    };
    repository::settings::put(
        pool,
        kv::USB_TARGET,
        &serde_json::to_string(&target)?,
    )
    .await?;
    // Wipe any prior manifest — registering means we treat the USB's managed set as empty
    // until the first sync repopulates it. Strangers stay safe (we never delete them anyway).
    repository::settings::delete(pool, kv::USB_MANIFEST).await?;
    let _ = repository::settings::delete(pool, kv::USB_LAST_ERROR).await;

    Ok(target)
}

pub async fn unregister_usb(pool: &SqlitePool) -> ServiceResult<()> {
    repository::settings::delete(pool, kv::USB_TARGET).await?;
    repository::settings::delete(pool, kv::USB_MANIFEST).await?;
    let _ = repository::settings::delete(pool, kv::USB_LAST_ERROR).await;
    Ok(())
}

pub async fn set_export_flag(
    pool: &SqlitePool,
    asset_id: AssetId,
    flag: bool,
) -> ServiceResult<()> {
    repository::gcode_metadata::set_export_to_usb(pool, asset_id, flag).await?;
    Ok(())
}

fn read_marker(mount: &Path) -> std::io::Result<UsbMarker> {
    let path = mount.join(MARKER_FILENAME);
    let bytes = std::fs::read(&path)?;
    serde_json::from_slice::<UsbMarker>(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn write_marker_atomic(mount: &Path, m: &UsbMarker) -> std::io::Result<()> {
    let path = mount.join(MARKER_FILENAME);
    let tmp = mount.join(MARKER_TMP_FILENAME);
    let json = serde_json::to_vec_pretty(m)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

// ---------- manifest ----------

async fn load_manifest(pool: &SqlitePool) -> ServiceResult<UsbManifest> {
    let raw = repository::settings::get(pool, kv::USB_MANIFEST).await?;
    Ok(raw
        .and_then(|s| serde_json::from_str::<UsbManifest>(&s).ok())
        .unwrap_or_default())
}

async fn save_manifest(pool: &SqlitePool, m: &UsbManifest) -> ServiceResult<()> {
    let json = serde_json::to_string(m)?;
    repository::settings::put(pool, kv::USB_MANIFEST, &json).await?;
    Ok(())
}

// ---------- sync ----------

pub async fn sync_now(
    app: tauri::AppHandle,
    pool: SqlitePool,
    library: Arc<Library>,
    lock: Arc<tokio::sync::Mutex<()>>,
    deep: bool,
) -> ServiceResult<SyncReport> {
    let started_at = now_ts();
    let _guard = match lock.try_lock_owned() {
        Ok(g) => g,
        Err(_) => {
            return Ok(SyncReport {
                skipped: true,
                reason: Some("already syncing".into()),
                started_at,
                finished_at: now_ts(),
                ..Default::default()
            });
        }
    };

    if current_target(&pool).await?.is_none() {
        return Ok(SyncReport {
            skipped: true,
            reason: Some("no USB registered".into()),
            started_at,
            finished_at: now_ts(),
            ..Default::default()
        });
    }

    let mount = match find_target_mount(&pool).await? {
        Some(p) => p,
        None => {
            return Ok(SyncReport {
                skipped: true,
                reason: Some("USB not detected".into()),
                started_at,
                finished_at: now_ts(),
                ..Default::default()
            });
        }
    };

    let exportable = repository::gcode_metadata::list_exportable(&pool).await?;
    let prior_manifest = load_manifest(&pool).await?;
    let clean_kobra = super::settings::get_clean_kobra_gcode(&pool)
        .await
        .unwrap_or(false);

    let mut desired: HashMap<PathBuf, ExportableGcode> = HashMap::new();
    let mut total_bytes: u64 = 0;
    for g in exportable {
        let p = PathBuf::from(&g.relative_path);
        total_bytes = total_bytes.saturating_add(g.size_bytes.max(0) as u64);
        desired.insert(p, g);
    }

    // Prior per-file sync state, keyed by relative path. Copy decisions compare DB
    // timestamps against what the manifest says we last mirrored — USB mtimes are
    // useless on FAT/exFAT (2s granularity, timezone-dependent interpretation).
    let prior_by_path: HashMap<&str, &UsbManifestFile> = prior_manifest
        .files
        .iter()
        .map(|f| (f.relative_path.as_str(), f))
        .collect();

    let mut report = SyncReport {
        started_at,
        ..Default::default()
    };
    let mut next_files: Vec<UsbManifestFile> = Vec::with_capacity(desired.len());
    let mut next_dirs: HashSet<PathBuf> = HashSet::new();
    let mut bytes_done: u64 = 0;

    let emit_progress = |percent: u8, message: Option<String>| {
        events::emit(
            &app,
            AppEvent::JobProgress {
                job_id: "usb-sync".into(),
                percent,
                message,
            },
        );
    };
    emit_progress(0, Some("starting".into()));

    for (rel, g) in &desired {
        let dst = mount.join(rel);
        let src = library.absolute(rel);
        let mut g = g.clone();

        // With Kobra cleaning on, strip the offending line from the library copy
        // itself (the source of truth) before mirroring. Rewriting in place keeps
        // the USB a faithful mirror and the size check below idempotent — once
        // cleaned, later syncs find nothing to strip and don't re-copy.
        if clean_kobra {
            match super::files::clean_gcode_file_in_place(&library, &src) {
                Ok(true) => {
                    if let Ok(meta) = std::fs::metadata(&src) {
                        let size = meta.len() as i64;
                        let sha = sha256_file(&src).ok();
                        let _ = repository::assets::update_size_sha(
                            &pool,
                            g.asset_id,
                            size,
                            sha.as_deref(),
                        )
                        .await;
                        g.size_bytes = size;
                        g.sha256 = sha;
                    }
                }
                Ok(false) => {}
                Err(e) => report.errors.push(format!("clean {}: {e}", rel.display())),
            }
        }

        let rel_str = rel.to_string_lossy().to_string();
        let prior = prior_by_path.get(rel_str.as_str()).copied();
        // On success the manifest records the library copy's updated_at; on failure it
        // keeps the prior value so the next sync retries instead of assuming it's done.
        let mut synced_updated_at = Some(g.asset_updated_at);
        match should_copy(&dst, &g, deep, prior) {
            Ok(NeedsCopy::No) => {}
            Ok(NeedsCopy::Yes { existed }) => {
                match copy_atomic(&src, &dst) {
                    Ok(()) => {
                        if existed {
                            report.updated += 1;
                        } else {
                            report.copied += 1;
                        }
                    }
                    Err(e) => {
                        report.errors.push(format!("copy {}: {e}", rel.display()));
                        synced_updated_at = prior.and_then(|p| p.synced_updated_at);
                    }
                }
            }
            Err(e) => {
                report
                    .errors
                    .push(format!("inspect {}: {e}", rel.display()));
                synced_updated_at = prior.and_then(|p| p.synced_updated_at);
            }
        }

        bytes_done = bytes_done.saturating_add(g.size_bytes.max(0) as u64);
        let percent = if total_bytes == 0 {
            99
        } else {
            ((bytes_done * 100) / total_bytes).min(99) as u8
        };
        emit_progress(percent, Some(rel.to_string_lossy().to_string()));

        next_files.push(UsbManifestFile {
            relative_path: rel_str,
            size: g.size_bytes,
            asset_id: g.asset_id.0,
            synced_updated_at,
        });
        let mut p = rel.parent();
        while let Some(d) = p {
            if d.as_os_str().is_empty() {
                break;
            }
            next_dirs.insert(d.to_path_buf());
            p = d.parent();
        }
    }

    let desired_paths: HashSet<PathBuf> = desired.keys().cloned().collect();
    let mut prune_seeds: HashSet<PathBuf> = HashSet::new();
    for old in &prior_manifest.files {
        let p = PathBuf::from(&old.relative_path);
        if desired_paths.contains(&p) {
            continue;
        }
        let dst = mount.join(&p);
        if dst.exists() {
            match std::fs::remove_file(&dst) {
                Ok(()) => report.deleted += 1,
                Err(e) => report.errors.push(format!("delete {}: {e}", p.display())),
            }
        }
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                prune_seeds.insert(parent.to_path_buf());
            }
        }
    }
    for d in &prior_manifest.dirs {
        let dp = PathBuf::from(d);
        if !next_dirs.contains(&dp) {
            prune_seeds.insert(dp);
        }
    }
    for d in prune_seeds {
        let abs = mount.join(&d);
        library::prune_empty_ancestors(&abs, &mount);
    }

    let new_manifest = UsbManifest {
        files: next_files,
        dirs: next_dirs
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    };
    save_manifest(&pool, &new_manifest).await?;

    repository::settings::put(&pool, kv::USB_LAST_SYNC_AT, &now_ts().to_string()).await?;
    if report.errors.is_empty() {
        let _ = repository::settings::delete(&pool, kv::USB_LAST_ERROR).await;
    } else {
        let _ = repository::settings::put(&pool, kv::USB_LAST_ERROR, &report.errors[0]).await;
    }

    report.finished_at = now_ts();
    emit_progress(100, None);
    Ok(report)
}

enum NeedsCopy {
    Yes { existed: bool },
    No,
}

/// Decide via size + manifest state, never USB mtimes: FAT/exFAT timestamps have 2s
/// granularity and shift with mount timezone options, which made the old mtime check
/// re-copy everything on some sticks and miss updates on others.
fn should_copy(
    dst: &Path,
    g: &ExportableGcode,
    deep: bool,
    prior: Option<&UsbManifestFile>,
) -> std::io::Result<NeedsCopy> {
    let meta = match std::fs::metadata(dst) {
        Ok(m) => m,
        Err(_) => return Ok(NeedsCopy::Yes { existed: false }),
    };
    if meta.len() as i64 != g.size_bytes {
        return Ok(NeedsCopy::Yes { existed: true });
    }
    if let Some(synced_at) = prior.and_then(|p| p.synced_updated_at) {
        if synced_at < g.asset_updated_at {
            return Ok(NeedsCopy::Yes { existed: true });
        }
    }
    if deep {
        if let Some(known_sha) = &g.sha256 {
            let actual = sha256_file(dst)?;
            if &actual != known_sha {
                return Ok(NeedsCopy::Yes { existed: true });
            }
        }
    }
    Ok(NeedsCopy::No)
}

fn copy_atomic(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = {
        let mut s = dst.as_os_str().to_owned();
        s.push(".gcfsync.tmp");
        PathBuf::from(s)
    };
    let write = || -> std::io::Result<()> {
        use std::io::Write;
        let mut input = std::fs::File::open(src)?;
        let mut output = std::fs::File::create(&tmp)?;
        std::io::copy(&mut input, &mut output)?;
        output.flush()?;
        output.sync_all()?;
        Ok(())
    };
    if let Err(e) = write().and_then(|()| std::fs::rename(&tmp, dst)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // Best-effort: flush the directory entry so a prompt unplug keeps the rename.
    if let Some(parent) = dst.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}
