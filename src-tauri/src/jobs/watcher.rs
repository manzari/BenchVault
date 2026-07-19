use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebouncedEvent};
use sqlx::{Row, SqlitePool};
use tauri::AppHandle;

use crate::domain::{AssetKind, GcodeMetadata, ObjectAssetRole, ObjectId};
use crate::events::{self, AppEvent};
use crate::jobs::autosync::AutoSync;
use crate::library::{self, Library};
use crate::parsers;
use crate::repository;
use crate::service;
use crate::service::paths::IngestMode;
use crate::service::ServiceResult;

const DEBOUNCE: Duration = Duration::from_millis(750);

/// Spawn the library file-watcher on its own OS thread. The thread owns the `notify`
/// debouncer and dispatches each batched event into a tokio runtime via
/// `tauri::async_runtime::block_on` so it can call into the existing async services.
pub fn spawn(pool: SqlitePool, library: Arc<Library>, app: AppHandle, autosync: AutoSync) {
    let _ = std::thread::Builder::new()
        .name("library-watcher".into())
        .spawn(move || run(pool, library, app, autosync));
}

fn run(pool: SqlitePool, library: Arc<Library>, app: AppHandle, autosync: AutoSync) {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = match new_debouncer(DEBOUNCE, None, tx) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[watcher] failed to start debouncer: {e}");
            return;
        }
    };
    if let Err(e) = debouncer.watch(library.root(), RecursiveMode::Recursive) {
        eprintln!("[watcher] failed to watch {:?}: {e}", library.root());
        return;
    }
    eprintln!("[watcher] watching {:?}", library.root());

    while let Ok(result) = rx.recv() {
        tauri::async_runtime::block_on(handle_batch(&pool, &library, &app, &autosync, result));
    }

    drop(debouncer);
}

async fn handle_batch(
    pool: &SqlitePool,
    library: &Library,
    app: &AppHandle,
    autosync: &AutoSync,
    result: notify_debouncer_full::DebounceEventResult,
) {
    let events = match result {
        Ok(ev) => ev,
        Err(errs) => {
            for e in errs {
                eprintln!("[watcher] notify error: {e}");
            }
            return;
        }
    };

    let mut changed = false;
    for ev in events {
        match dispatch_event(pool, library, ev).await {
            Ok(true) => changed = true,
            Ok(false) => {}
            Err(e) => eprintln!("[watcher] handler error: {e}"),
        }
    }
    if changed {
        events::emit(app, AppEvent::LibraryChanged);
        autosync.ping();
    }
}

async fn dispatch_event(
    pool: &SqlitePool,
    library: &Library,
    ev: DebouncedEvent,
) -> ServiceResult<bool> {
    let kind = ev.kind;
    let paths = ev.paths.clone();

    let thumbnails = library.root().join("thumbnails");
    if paths.iter().any(|p| p.starts_with(&thumbnails)) {
        return Ok(false);
    }
    if paths.is_empty() {
        return Ok(false);
    }

    match kind {
        EventKind::Create(_) => on_create(pool, library, &paths[0]).await,
        EventKind::Modify(ModifyKind::Data(_)) => on_modify(pool, library, &paths[0]).await,
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if paths.len() >= 2 => {
            on_rename(pool, library, &paths[0], &paths[1]).await
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            on_delete(pool, library, &paths[0]).await
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            on_create(pool, library, &paths[0]).await
        }
        EventKind::Remove(_) => on_delete(pool, library, &paths[0]).await,
        _ => Ok(false),
    }
}

/// Walk the whole library tree and ingest any on-disk file that isn't tracked
/// yet, returning the number of files newly ingested.
///
/// The live watcher only sees filesystem events while the app is running, and
/// even then inotify can drop them (queue overflow, or the lost-event race when
/// a file is written into a directory the instant it's created). So anything a
/// slicer or the user dropped into the library while we weren't looking —
/// notably gcode a slicer saved back into an object's folder between sessions —
/// stays invisible, and therefore never syncs to USB. This sweep closes that
/// gap. Unlike live events it runs in mirror mode, so directories holding asset
/// files that aren't objects yet are materialized too (ancestors become folders)
/// — new object dirs dropped on disk show up, not just files in known objects.
/// Classification, path→object resolution and the relpath/sha dedupe otherwise
/// match a live event; already-tracked files are skipped (by relative path)
/// before they're hashed, so re-running over a mostly-tracked library is cheap.
pub async fn reconcile(pool: &SqlitePool, library: &Library) -> ServiceResult<usize> {
    let root = library.root();
    // The thumbnails cache and the `_orphans` parking lot (migrate_layout) aren't
    // part of the user-visible tree and must never become objects.
    let skip = [root.join("thumbnails"), root.join("_orphans")];
    let mut files = Vec::new();
    collect_files(root, &skip, &mut files);

    let mut ingested = 0usize;
    for abs in files {
        // Mirror mode: an asset-bearing directory that isn't a known object becomes
        // one (ancestors become folders), so new object dirs dropped on disk show up
        // — not just files landing in objects we already track.
        match ingest_new_file(pool, library, &abs, IngestMode::AutoCreateMirror).await {
            Ok(true) => ingested += 1,
            Ok(false) => {}
            Err(e) => eprintln!("[watcher] reconcile error for {}: {e}", abs.display()),
        }
    }
    if ingested > 0 {
        eprintln!("[watcher] reconcile ingested {ingested} untracked file(s)");
    }
    Ok(ingested)
}

/// Depth-first collect of regular files under `root`, skipping any directory in
/// `skip_dirs`, hidden entries (including our own `.bv-*` tempfiles), macOS junk,
/// and symlinks — mirroring the folder-import walker.
fn collect_files(root: &Path, skip_dirs: &[PathBuf], out: &mut Vec<PathBuf>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_s = name.to_string_lossy();
            if name_s.starts_with('.') || name_s == "__MACOSX" {
                continue;
            }
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if ft.is_symlink() {
                continue;
            }
            let path = entry.path();
            if skip_dirs.iter().any(|s| *s == path) {
                continue;
            }
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                out.push(path);
            }
        }
    }
}

/// Live watcher entrypoint for a created/renamed-in path. Uses the conservative
/// marker mode so transient activity from other tools doesn't spawn objects.
async fn on_create(pool: &SqlitePool, library: &Library, abs: &Path) -> ServiceResult<bool> {
    ingest_new_file(pool, library, abs, IngestMode::AutoCreateMarked).await
}

/// Ingest a single on-disk file as a new asset under its owning object, creating the
/// object/folders per `mode` if needed. Shared by live events (`on_create`) and the
/// reconcile sweep. Idempotent: skips files already tracked by relative path or sha.
async fn ingest_new_file(
    pool: &SqlitePool,
    library: &Library,
    abs: &Path,
    mode: IngestMode,
) -> ServiceResult<bool> {
    if !abs.is_file() {
        return Ok(false);
    }
    if library.ignore_set().check(abs) {
        return Ok(false);
    }
    let (kind, role) = match classify(abs) {
        Some(c) => c,
        None => return Ok(false),
    };

    // Clean the OrcaSlicer 2.3.2 Kobra-breaking line on first discovery if the
    // user opted in, before the dedupe/attach below stat & hash the file — so the
    // tracked size/hash reflect the cleaned bytes.
    if matches!(kind, AssetKind::Gcode)
        && service::settings::get_clean_kobra_gcode(pool).await.unwrap_or(false)
    {
        if let Err(e) = service::files::clean_gcode_file_in_place(library, abs) {
            eprintln!("[watcher] kobra clean failed for {}: {e}", abs.display());
        }
    }

    let target = match service::paths::resolve_for_ingest(pool, library, abs, mode).await? {
        Some(t) => t,
        None => return Ok(false),
    };

    if repository::assets::find_by_relative_path(pool, &target.rel_path)
        .await?
        .is_some()
    {
        return Ok(false);
    }

    if let Ok(sha) = service::files::sha256_file(abs) {
        if let Some(existing) = repository::assets::find_by_sha256(pool, &sha).await? {
            eprintln!(
                "[watcher] sha256 dedupe: skipping {} (already tracked at {})",
                target.rel_path, existing.relative_path
            );
            return Ok(false);
        }
    }

    match kind {
        AssetKind::Gcode => {
            service::files::attach_existing_gcode(pool, library, target.object_id, abs).await?;
        }
        AssetKind::Stl | AssetKind::Obj | AssetKind::Step => {
            service::files::attach_existing_model(pool, library, target.object_id, abs, kind).await?;
        }
        AssetKind::Scad => {
            service::files::attach_existing_scad(pool, library, target.object_id, abs).await?;
        }
        _ => {
            service::files::attach_existing_attachment(
                pool,
                library,
                target.object_id,
                abs,
                kind,
                role,
            )
            .await?;
        }
    }
    eprintln!("[watcher] ingested {} ({:?})", target.rel_path, kind);
    Ok(true)
}

async fn on_modify(pool: &SqlitePool, library: &Library, abs: &Path) -> ServiceResult<bool> {
    if !abs.is_file() {
        return Ok(false);
    }
    if library.ignore_set().check(abs) {
        return Ok(false);
    }

    let rel = match abs.strip_prefix(library.root()) {
        Ok(r) => r.to_string_lossy().into_owned(),
        Err(_) => return Ok(false),
    };
    let asset = match repository::assets::find_by_relative_path(pool, &rel).await? {
        Some(a) => a,
        None => return on_create(pool, library, abs).await,
    };

    let bytes = std::fs::metadata(abs)?.len() as i64;
    let sha = service::files::sha256_file(abs).ok();
    repository::assets::update_path_and_size(
        pool,
        asset.id,
        &rel,
        &asset.original_filename,
        bytes,
        sha.as_deref(),
    )
    .await?;

    if matches!(asset.kind, AssetKind::Gcode) {
        let parsed = parsers::gcode::parse_path(abs).unwrap_or_default();
        let metadata = GcodeMetadata {
            asset_id: asset.id,
            slicer_name: parsed.slicer_name,
            slicer_version: parsed.slicer_version,
            profile_name: parsed.profile_name,
            layer_height_mm: parsed.layer_height_mm,
            nozzle_temp_c: parsed.nozzle_temp_c,
            bed_temp_c: parsed.bed_temp_c,
            estimated_time_seconds: parsed.estimated_time_seconds,
            filament_length_mm: parsed.filament_length_mm,
            filament_weight_g: parsed.filament_weight_g,
            filament_type: parsed.filament_type,
            parsed_at: Some(crate::db::now_ts()),
            export_to_usb: true,
        };
        repository::gcode_metadata::upsert(pool, &metadata).await?;
    }
    eprintln!("[watcher] modified {}", rel);
    Ok(true)
}

async fn on_rename(
    pool: &SqlitePool,
    library: &Library,
    from: &Path,
    to: &Path,
) -> ServiceResult<bool> {
    let rel_from = match from.strip_prefix(library.root()) {
        Ok(r) => r.to_string_lossy().into_owned(),
        Err(_) => return Ok(false),
    };
    let rel_to = match to.strip_prefix(library.root()) {
        Ok(r) => r.to_string_lossy().into_owned(),
        Err(_) => return Ok(false),
    };

    let asset = match repository::assets::find_by_relative_path(pool, &rel_from).await? {
        Some(a) => a,
        None => return on_create(pool, library, to).await,
    };

    let target = match service::paths::resolve_for_ingest(pool, library, to, IngestMode::AutoCreateMarked).await? {
        Some(t) => t,
        None => {
            eprintln!(
                "[watcher] rename target unresolvable, deleting tracked asset: {}",
                rel_to
            );
            service::files::purge_asset_rows(pool, asset.id).await?;
            return Ok(true);
        }
    };

    let bytes = std::fs::metadata(to).map(|m| m.len() as i64).ok();
    let sha = service::files::sha256_file(to).ok();
    let prior_size = asset.size_bytes.unwrap_or(0);
    repository::assets::update_path_and_size(
        pool,
        asset.id,
        &target.rel_path,
        &target.file_name,
        bytes.unwrap_or(prior_size),
        sha.as_deref(),
    )
    .await?;

    let old_object_id: Option<i64> = sqlx::query(
        "SELECT object_id FROM object_assets WHERE asset_id = ? LIMIT 1",
    )
    .bind(asset.id.0)
    .fetch_optional(pool)
    .await?
    .map(|r| r.get::<i64, _>("object_id"));

    if old_object_id != Some(target.object_id.0) {
        repository::assets::move_to_object(pool, asset.id, target.object_id.0).await?;
        repository::objects::touch(pool, target.object_id).await?;
        if let Some(oid) = old_object_id {
            repository::objects::touch(pool, ObjectId(oid)).await?;
        }
    } else {
        repository::objects::touch(pool, target.object_id).await?;
    }

    if let Some(parent) = from.parent() {
        library::prune_empty_ancestors(parent, library.root());
    }

    eprintln!("[watcher] renamed {} → {}", rel_from, rel_to);
    Ok(true)
}

async fn on_delete(pool: &SqlitePool, library: &Library, abs: &Path) -> ServiceResult<bool> {
    let rel = match abs.strip_prefix(library.root()) {
        Ok(r) => r.to_string_lossy().into_owned(),
        Err(_) => return Ok(false),
    };
    let asset = match repository::assets::find_by_relative_path(pool, &rel).await? {
        Some(a) => a,
        None => return Ok(false),
    };

    let object_id: Option<i64> = sqlx::query(
        "SELECT object_id FROM object_assets WHERE asset_id = ? LIMIT 1",
    )
    .bind(asset.id.0)
    .fetch_optional(pool)
    .await?
    .map(|r| r.get::<i64, _>("object_id"));

    service::files::purge_asset_rows(pool, asset.id).await?;
    if let Some(oid) = object_id {
        repository::objects::touch(pool, ObjectId(oid)).await?;
    }
    if let Some(parent) = abs.parent() {
        library::prune_empty_ancestors(parent, library.root());
    }
    eprintln!("[watcher] deleted {}", rel);
    Ok(true)
}

fn classify(abs: &Path) -> Option<(AssetKind, ObjectAssetRole)> {
    let ext = abs
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("gcode") | Some("bgcode") | Some("gco") | Some("g") => {
            Some((AssetKind::Gcode, ObjectAssetRole::Gcode))
        }
        Some("stl") => Some((AssetKind::Stl, ObjectAssetRole::StlPart)),
        Some("obj") => Some((AssetKind::Obj, ObjectAssetRole::StlPart)),
        Some("step") | Some("stp") => Some((AssetKind::Step, ObjectAssetRole::StlPart)),
        Some("scad") => Some((AssetKind::Scad, ObjectAssetRole::Scad)),
        // A `.py` is only auto-ingested when it sniffs as a CadQuery script;
        // unrelated Python is ignored by the watcher.
        Some("py") if crate::service::files::is_cadquery_py(abs) => {
            Some((AssetKind::CadQuery, ObjectAssetRole::Scad))
        }
        Some("md") | Some("markdown") => Some((AssetKind::Markdown, ObjectAssetRole::Document)),
        Some("3mf") | Some("iges") | Some("igs") | Some("f3d") | Some("sldprt") => {
            Some((AssetKind::Cad, ObjectAssetRole::Attachment))
        }
        Some("png") | Some("jpg") | Some("jpeg") | Some("webp") | Some("gif") | Some("bmp") => {
            Some((AssetKind::Image, ObjectAssetRole::Attachment))
        }
        _ => None,
    }
}

