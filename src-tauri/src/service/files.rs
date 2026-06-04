use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::db::now_ts;
use crate::domain::{
    Asset, AssetId, AssetKind, GcodeMetadata, ObjectAssetRole, ObjectId, ParsedGcode,
};
use crate::library::{self, Library};
use crate::parsers::gcode;
use crate::repository;

use super::objects as svc_objects;
use super::{ServiceError, ServiceResult};

/// Copy `source` into the managed library and create matching DB rows.
pub async fn add_stl(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
) -> ServiceResult<Asset> {
    let asset = ingest_file(pool, library, object_id, source, AssetKind::Stl, mime_for_stl()).await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::StlPart).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::StlPart, next_sort).await?;
    repository::objects::touch(pool, object_id).await?;
    Ok(asset)
}

/// Copy an OpenSCAD source into the managed library, attaching it with the `scad`
/// role so it gets its own section and preview action (mirrors [`add_stl`]).
pub async fn add_scad(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
) -> ServiceResult<Asset> {
    let asset =
        ingest_file(pool, library, object_id, source, AssetKind::Scad, mime_for_path(source)).await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::Scad).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Scad, next_sort).await?;
    repository::objects::touch(pool, object_id).await?;
    Ok(asset)
}

pub async fn add_gcode(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
) -> ServiceResult<(Asset, GcodeMetadata)> {
    let parsed: ParsedGcode = match gcode::parse_path(source) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[gcode] parse failed for {}: {e}", source.display());
            ParsedGcode::default()
        }
    };
    let extras_json = if parsed.raw_extras.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&parsed.raw_extras)?)
    };
    let asset = ingest_file_with_extras(
        pool,
        library,
        object_id,
        source,
        AssetKind::Gcode,
        mime_for_gcode(),
        extras_json.as_deref(),
    )
    .await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::Gcode).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Gcode, next_sort).await?;

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

    // Strip the OrcaSlicer 2.3.2 Kobra-breaking line if the user opted in. We do
    // this after ingest (the parse above is unaffected — the line carries no
    // metadata) and refresh the row's size/hash to match the rewritten file.
    let mut asset = asset;
    if super::settings::get_clean_kobra_gcode(pool).await? {
        let abs = library.absolute(Path::new(&asset.relative_path));
        if clean_gcode_file_in_place(library, &abs)? {
            let size = std::fs::metadata(&abs)?.len() as i64;
            let sha = sha256_file(&abs).ok();
            repository::assets::update_size_sha(pool, asset.id, size, sha.as_deref()).await?;
            asset = repository::assets::get(pool, asset.id).await?;
        }
    }

    repository::objects::touch(pool, object_id).await?;
    Ok((asset, metadata))
}

/// Strip OrcaSlicer 2.3.2's `; filament_colour_type = ...` lines from a text gcode
/// file in place, rewriting it atomically via a sibling tempfile + rename. No-op
/// (returns `Ok(false)`) for non-text gcode (`.bgcode` binary) or files that don't
/// contain the offending line. The watcher is told to ignore our own rename so it
/// doesn't re-process the file. Callers gate this on the "clean Kobra gcode" setting.
pub fn clean_gcode_file_in_place(library: &Library, abs: &Path) -> std::io::Result<bool> {
    let is_text_gcode = abs
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("gcode") || e.eq_ignore_ascii_case("gco"))
        .unwrap_or(false);
    if !is_text_gcode {
        return Ok(false);
    }

    let bytes = std::fs::read(abs)?;
    let cleaned = match gcode::strip_kobra_breakers(&bytes) {
        Some(c) => c,
        None => return Ok(false),
    };

    let dir = abs
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "gcode file has no parent dir"))?;
    let tmp = tempfile::Builder::new().prefix(".bv-clean-").tempfile_in(dir)?;
    std::fs::write(tmp.path(), &cleaned)?;
    // Suppress the watcher's reaction to the rename we're about to perform.
    library.ignore_set().mark(abs);
    tmp.into_temp_path().persist(abs).map_err(|e| e.error)?;
    Ok(true)
}

pub async fn set_cover_image(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
) -> ServiceResult<Asset> {
    let asset = ingest_file(pool, library, object_id, source, AssetKind::Image, mime_for_path(source)).await?;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Attachment, 0).await?;
    repository::objects::set_cover(pool, object_id, asset.id).await?;
    Ok(asset)
}

pub async fn add_attachment(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
) -> ServiceResult<Asset> {
    // A `.scad` picked through the generic attachment path still becomes a proper
    // scad asset (own role + preview action), not a CAD attachment.
    if source
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("scad"))
    {
        return add_scad(pool, library, object_id, source).await;
    }
    let (kind, mime) = classify_attachment(source);
    let asset = ingest_file(pool, library, object_id, source, kind, mime).await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::Attachment).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Attachment, next_sort).await?;
    repository::objects::touch(pool, object_id).await?;
    Ok(asset)
}

pub async fn set_cover_from_asset(
    pool: &SqlitePool,
    object_id: ObjectId,
    asset_id: AssetId,
) -> ServiceResult<()> {
    repository::objects::set_cover(pool, object_id, asset_id).await?;
    repository::objects::touch(pool, object_id).await?;
    Ok(())
}

/// Marker stored in `assets.metadata_json` for covers auto-captured from the STL
/// preview. Lets `set_cover_from_png` replace the previous render cover on each
/// capture (so they don't pile up) without ever deleting user-added photos, which
/// never carry this marker.
const RENDER_COVER_MARKER: &str = "{\"generated\":\"stl_render_cover\"}";

/// Ingest an in-memory PNG (e.g. a thumbnail rendered from an STL on the frontend)
/// as a new `Image` asset and make it the object's cover. Mirrors `set_cover_image`
/// but takes bytes rather than a path, since the image never exists on disk.
///
/// The new asset is tagged with `RENDER_COVER_MARKER`; any earlier render cover for
/// this object (asset row + file) is then deleted, so repeated captures replace
/// rather than accumulate.
pub async fn set_cover_from_png(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    png: &[u8],
    filename: &str,
) -> ServiceResult<Asset> {
    let asset = ingest_bytes_with_extras(
        pool,
        library,
        object_id,
        png,
        filename,
        Some("image/png"),
        Some(RENDER_COVER_MARKER),
    )
    .await?;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Attachment, 0).await?;
    repository::objects::set_cover(pool, object_id, asset.id).await?;

    // The new cover is in place; drop any previous auto-captured render covers. This
    // runs after `set_cover` so `objects.cover_asset_id` (ON DELETE SET NULL) already
    // points at the new asset. Cleanup is best-effort — a stale leftover is harmless
    // and shouldn't fail the capture the user just made.
    let stale =
        repository::objects::render_cover_asset_ids(pool, object_id, RENDER_COVER_MARKER, asset.id)
            .await?;
    for id in stale {
        if let Err(e) = remove_asset(pool, library, id).await {
            eprintln!("[cover] failed to remove superseded render cover {id:?}: {e}");
        }
    }
    Ok(asset)
}

pub async fn remove_asset(
    pool: &SqlitePool,
    library: &Library,
    asset_id: AssetId,
) -> ServiceResult<()> {
    let asset = repository::assets::get(pool, asset_id).await?;
    purge_asset_rows(pool, asset_id).await?;
    let abs = library.absolute(Path::new(&asset.relative_path));
    if abs.exists() {
        let _ = std::fs::remove_file(abs);
    }
    // Best-effort: drop any cached OpenSCAD render for this asset.
    super::openscad::clear_cache(library, asset_id);
    Ok(())
}

/// Tear down every DB row that references an asset (object_assets, entity_links,
/// entity_tags) and then the asset row itself. Leaves the on-disk file alone —
/// callers that own the file (e.g. `remove_asset`) delete it separately; the
/// watcher's delete/rename paths use this when the file is already gone or moved.
pub async fn purge_asset_rows(pool: &SqlitePool, asset_id: AssetId) -> ServiceResult<()> {
    repository::objects::detach_asset(pool, asset_id).await?;
    repository::links::delete_for_asset(pool, asset_id.0).await?;
    repository::tags::delete_for_entity(pool, "asset", asset_id.0).await?;
    repository::assets::delete_hard(pool, asset_id).await?;
    Ok(())
}

// --- internals ---

async fn ingest_file(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
    kind: AssetKind,
    mime: Option<&str>,
) -> ServiceResult<Asset> {
    ingest_file_with_extras(pool, library, object_id, source, kind, mime, None).await
}

/// Atomic ingest: copy `source` into the managed library and create the matching
/// DB row in a single SQLite transaction. The on-disk write goes via a sibling
/// tempfile + atomic rename, and is only performed inside the transaction so a
/// commit failure (or any earlier error) leaves *neither* a row nor a file
/// behind. The only window where things can drift is a process crash between
/// the atomic rename and the commit — in that case we may leave an orphan file
/// on disk (no DB row), which a future GC pass can detect.
async fn ingest_file_with_extras(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
    kind: AssetKind,
    mime: Option<&str>,
    metadata_json: Option<&str>,
) -> ServiceResult<Asset> {
    let original_filename = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ServiceError::Other("invalid source filename".into()))?
        .to_string();

    // Materialize the destination dir up-front (idempotent and outside the tx —
    // dir creation is harmless if a later step rolls back).
    let object_dir: PathBuf = svc_objects::ensure_dir(pool, library, object_id).await?;

    // Pre-compute size + hash from the *source* file so the DB row's metadata
    // matches what we're about to copy. Reading the source twice (here + during
    // copy) is cheaper than hashing the destination after the rename.
    let bytes = std::fs::metadata(source)?.len() as i64;
    let sha = sha256_file(source).ok();

    // Copy into a sibling tempfile *before* opening the write transaction, so the
    // SQLite write lock isn't held for the duration of a potentially multi-GB
    // copy (which would make concurrent writers fail with "database is locked").
    // The eventual rename is atomic on the same filesystem; if anything later
    // fails, the tempfile is deleted by `tempfile`'s Drop and the tx rolled back.
    let tmp = tempfile::Builder::new()
        .prefix(".bv-ingest-")
        .tempfile_in(&object_dir)?;
    std::fs::copy(source, tmp.path())?;

    let mut tx = pool.begin().await?;
    let now = now_ts();

    // INSERT to allocate the asset id; relative_path is filled in by the
    // matching UPDATE below once we know the final filename (which depends on
    // the id for collision-avoidance).
    let asset_id_raw: i64 = sqlx::query(
        "INSERT INTO assets (kind, relative_path, original_filename, mime_type, created_at, updated_at)
         VALUES (?, '', ?, ?, ?, ?)",
    )
    .bind(kind.as_str())
    .bind(&original_filename)
    .bind(mime)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let asset_id = AssetId(asset_id_raw);

    // Pick the final filename now that we have an id to disambiguate with.
    let filename = library::unique_filename(&object_dir, &original_filename, asset_id.0);
    let abs = object_dir.join(&filename);
    let rel = abs
        .strip_prefix(library.root())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| PathBuf::from(&filename));
    let rel_str = rel.to_string_lossy().to_string();

    // Finalize the row inside the same transaction.
    sqlx::query(
        "UPDATE assets SET relative_path = ?, size_bytes = ?, sha256 = ?, metadata_json = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&rel_str)
    .bind(bytes)
    .bind(sha.as_deref())
    .bind(metadata_json)
    .bind(now)
    .bind(asset_id.0)
    .execute(&mut *tx)
    .await?;

    // Tell the watcher to ignore the upcoming rename event we're about to fire.
    library.ignore_set().mark(&abs);

    // Atomic rename. On error, roll back the tx so the row vanishes too.
    let temp_path = tmp.into_temp_path();
    if let Err(e) = temp_path.persist(&abs) {
        // PersistError carries the original tempfile back so it gets cleaned up
        // when its TempPath is dropped at function exit.
        let _ = tx.rollback().await;
        return Err(ServiceError::Io(e.error));
    }

    // Commit: if this fails, undo the rename so we don't leave an orphan file.
    if let Err(e) = tx.commit().await {
        let _ = std::fs::remove_file(&abs);
        return Err(ServiceError::Db(e));
    }

    repository::assets::get(pool, asset_id).await.map_err(Into::into)
}

/// Atomic ingest of an in-memory buffer. Same allocate → write tempfile → atomic
/// rename → commit dance as `ingest_file_with_extras`, but the source is a byte
/// slice with a caller-supplied `original_filename`/`mime` rather than a file on disk.
async fn ingest_bytes_with_extras(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    bytes: &[u8],
    original_filename: &str,
    mime: Option<&str>,
    metadata_json: Option<&str>,
) -> ServiceResult<Asset> {
    let object_dir: PathBuf = svc_objects::ensure_dir(pool, library, object_id).await?;

    let size = bytes.len() as i64;
    let sha = Some(sha256_bytes(bytes));

    // Write the tempfile before opening the write transaction (see the note in
    // `ingest_file_with_extras`) to keep the SQLite write lock window short.
    let tmp = tempfile::Builder::new()
        .prefix(".bv-ingest-")
        .tempfile_in(&object_dir)?;
    std::fs::write(tmp.path(), bytes)?;

    let mut tx = pool.begin().await?;
    let now = now_ts();

    let asset_id_raw: i64 = sqlx::query(
        "INSERT INTO assets (kind, relative_path, original_filename, mime_type, created_at, updated_at)
         VALUES (?, '', ?, ?, ?, ?)",
    )
    .bind(AssetKind::Image.as_str())
    .bind(original_filename)
    .bind(mime)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let asset_id = AssetId(asset_id_raw);

    let filename = library::unique_filename(&object_dir, original_filename, asset_id.0);
    let abs = object_dir.join(&filename);
    let rel = abs
        .strip_prefix(library.root())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| PathBuf::from(&filename));
    let rel_str = rel.to_string_lossy().to_string();

    sqlx::query(
        "UPDATE assets SET relative_path = ?, size_bytes = ?, sha256 = ?, metadata_json = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&rel_str)
    .bind(size)
    .bind(sha.as_deref())
    .bind(metadata_json)
    .bind(now)
    .bind(asset_id.0)
    .execute(&mut *tx)
    .await?;

    library.ignore_set().mark(&abs);

    let temp_path = tmp.into_temp_path();
    if let Err(e) = temp_path.persist(&abs) {
        let _ = tx.rollback().await;
        return Err(ServiceError::Io(e.error));
    }

    if let Err(e) = tx.commit().await {
        let _ = std::fs::remove_file(&abs);
        return Err(ServiceError::Db(e));
    }

    repository::assets::get(pool, asset_id).await.map_err(Into::into)
}

/// Register a file that already lives under `library.root()` as an asset attached to an
/// object. Used by the file watcher when external tools drop files directly into the
/// managed library — the file is **not** copied or renamed; the slicer's chosen filename
/// is preserved as `original_filename`.
pub async fn attach_existing_gcode(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    abs: &Path,
) -> ServiceResult<(Asset, GcodeMetadata)> {
    let parsed: ParsedGcode = match gcode::parse_path(abs) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[gcode] parse failed for {}: {e}", abs.display());
            ParsedGcode::default()
        }
    };
    let extras_json = if parsed.raw_extras.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&parsed.raw_extras)?)
    };
    let asset = attach_existing_inner(
        pool,
        library,
        abs,
        AssetKind::Gcode,
        mime_for_gcode(),
        extras_json.as_deref(),
    )
    .await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::Gcode).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Gcode, next_sort).await?;

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
    repository::objects::touch(pool, object_id).await?;
    Ok((asset, metadata))
}

pub async fn attach_existing_stl(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    abs: &Path,
) -> ServiceResult<Asset> {
    let asset = attach_existing_inner(pool, library, abs, AssetKind::Stl, mime_for_stl(), None).await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::StlPart).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::StlPart, next_sort).await?;
    repository::objects::touch(pool, object_id).await?;
    Ok(asset)
}

pub async fn attach_existing_scad(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    abs: &Path,
) -> ServiceResult<Asset> {
    let asset =
        attach_existing_inner(pool, library, abs, AssetKind::Scad, mime_for_path(abs), None).await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::Scad).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Scad, next_sort).await?;
    repository::objects::touch(pool, object_id).await?;
    Ok(asset)
}

pub async fn attach_existing_attachment(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    abs: &Path,
    kind: AssetKind,
    role: ObjectAssetRole,
) -> ServiceResult<Asset> {
    let asset = attach_existing_inner(pool, library, abs, kind, mime_for_path(abs), None).await?;
    let next_sort = next_sort_order(pool, object_id, role).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, role, next_sort).await?;
    if matches!(kind, AssetKind::Image) {
        repository::objects::set_cover_if_missing(pool, object_id, asset.id).await?;
    }
    repository::objects::touch(pool, object_id).await?;
    Ok(asset)
}

/// Atomically register an already-on-disk file as an asset row. No filesystem
/// writes happen here — the file is at `abs` already — so we only need the DB
/// allocate+finalize pair to be transactional.
async fn attach_existing_inner(
    pool: &SqlitePool,
    library: &Library,
    abs: &Path,
    kind: AssetKind,
    mime: Option<&str>,
    metadata_json: Option<&str>,
) -> ServiceResult<Asset> {
    let original_filename = abs
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ServiceError::Other("invalid filename".into()))?
        .to_string();

    let bytes = std::fs::metadata(abs)?.len() as i64;
    let sha = sha256_file(abs).ok();
    let rel = abs
        .strip_prefix(library.root())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| PathBuf::from(&original_filename));
    let rel_str = rel.to_string_lossy().to_string();

    let mut tx = pool.begin().await?;
    let now = now_ts();

    let asset_id_raw: i64 = sqlx::query(
        "INSERT INTO assets (kind, relative_path, original_filename, mime_type, created_at, updated_at)
         VALUES (?, '', ?, ?, ?, ?)",
    )
    .bind(kind.as_str())
    .bind(&original_filename)
    .bind(mime)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let asset_id = AssetId(asset_id_raw);

    sqlx::query(
        "UPDATE assets SET relative_path = ?, size_bytes = ?, sha256 = ?, metadata_json = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&rel_str)
    .bind(bytes)
    .bind(sha.as_deref())
    .bind(metadata_json)
    .bind(now)
    .bind(asset_id.0)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    repository::assets::get(pool, asset_id).await.map_err(Into::into)
}

async fn next_sort_order(pool: &SqlitePool, object_id: ObjectId, role: ObjectAssetRole) -> sqlx::Result<i64> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT COALESCE(MAX(sort_order), -1) AS m FROM object_assets WHERE object_id = ? AND role = ?",
    )
    .bind(object_id.0)
    .bind(role.as_str())
    .fetch_one(pool)
    .await?;
    Ok(row.get::<i64, _>("m") + 1)
}

pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn mime_for_stl() -> Option<&'static str> { Some("model/stl") }
fn mime_for_gcode() -> Option<&'static str> { Some("text/x.gcode") }

fn mime_for_path(p: &Path) -> Option<&'static str> {
    match p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("webp") => Some("image/webp"),
        Some("gif") => Some("image/gif"),
        Some("bmp") => Some("image/bmp"),
        Some("svg") => Some("image/svg+xml"),
        Some("pdf") => Some("application/pdf"),
        Some("txt") => Some("text/plain"),
        Some("md") => Some("text/markdown"),
        Some("json") => Some("application/json"),
        Some("zip") => Some("application/zip"),
        Some("step") | Some("stp") => Some("model/step"),
        Some("scad") => Some("application/x-openscad"),
        Some("3mf") => Some("model/3mf"),
        Some("obj") => Some("model/obj"),
        _ => None,
    }
}

fn classify_attachment(p: &Path) -> (AssetKind, Option<&'static str>) {
    match p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("png") | Some("jpg") | Some("jpeg") | Some("webp") | Some("gif") | Some("bmp") | Some("svg") => {
            (AssetKind::Image, mime_for_path(p))
        }
        Some("step") | Some("stp") | Some("iges") | Some("igs") | Some("3mf") | Some("f3d") | Some("sldprt") | Some("obj") => {
            (AssetKind::Cad, mime_for_path(p))
        }
        _ => (AssetKind::Other, mime_for_path(p)),
    }
}
