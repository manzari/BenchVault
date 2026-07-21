use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::db::now_ts;
use crate::domain::{
    Asset, AssetId, AssetKind, GcodeMetadata, ObjectAssetRole, ObjectId, ParsedGcode,
};
use crate::library::{self, Library};
use crate::parsers::{gcode, mesh, scad, threemf};
use crate::repository;

use super::objects as svc_objects;
use super::{ServiceError, ServiceResult};

/// Copy a 3D model (`.stl`, `.obj`, `.step`, `.3mf`, …) into the managed library
/// and create matching DB rows. All model files share the `StlPart` role (one
/// "Models" section) but carry a per-format [`AssetKind`] so the frontend can pick
/// the right preview path (only STL/OBJ render in-app).
pub async fn add_stl(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
) -> ServiceResult<Asset> {
    let kind = classify_model_kind(source);
    let mime = mime_for_path(source).or_else(|| mime_for_stl());
    let asset = ingest_file(pool, library, object_id, source, kind, mime).await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::StlPart).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::StlPart, next_sort).await?;
    store_model_derived(pool, library, object_id, asset.id, source, kind).await;
    repository::objects::touch(pool, object_id).await?;
    Ok(asset)
}

/// Best-effort dispatch for a freshly-attached model asset: 3MF carries units,
/// document metadata, and an embedded thumbnail on top of geometry, so it goes
/// through [`store_threemf`]; STL/OBJ only need geometry. Never fatal.
async fn store_model_derived(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    asset_id: AssetId,
    path: &Path,
    kind: AssetKind,
) -> bool {
    if matches!(kind, AssetKind::ThreeMf) {
        store_threemf(pool, library, object_id, asset_id, path).await
    } else {
        store_mesh_metadata(pool, asset_id, path, kind).await
    }
}

/// Best-effort: parse a 3MF once and persist everything it yields — geometry (in
/// mm) and declared units into `mesh_metadata`, loose document metadata into
/// `assets.metadata_json`, and an embedded thumbnail as the object cover when the
/// object has none. A failure at any step logs and moves on rather than failing
/// the ingest, mirroring the other parsers' tolerance.
async fn store_threemf(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    asset_id: AssetId,
    path: &Path,
) -> bool {
    let parsed = match threemf::parse_path(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[3mf] parse failed for {}: {e}", path.display());
            return false;
        }
    };

    // Geometry + units. `to_metadata` leaves `source_units` NULL; 3MF is the one
    // format that fills it, so set it on the returned row.
    let mut meta = parsed.stats.to_metadata(asset_id, "threemf");
    meta.source_units = parsed.units.clone();
    if let Err(e) = repository::mesh_metadata::upsert(pool, &meta).await {
        eprintln!("[3mf] mesh store failed for {asset_id:?}: {e}");
    }

    // Loose document metadata (Title/Designer/…/object names/slicer settings).
    let doc = serde_json::json!({
        "format": "threemf",
        "units": parsed.units,
        "metadata": parsed.metadata,
        "object_names": parsed.object_names,
        "slicer_config": parsed.slicer_config,
    });
    match serde_json::to_string(&doc) {
        Ok(json) => {
            if let Err(e) = repository::assets::update_metadata_json(pool, asset_id, Some(&json)).await {
                eprintln!("[3mf] metadata_json store failed for {asset_id:?}: {e}");
            }
        }
        Err(e) => eprintln!("[3mf] metadata serialize failed for {asset_id:?}: {e}"),
    }

    // Embedded preview → cover, only when the object has none (don't override a
    // user-chosen cover or leave an orphan image if one already exists).
    if let Some(png) = parsed.thumbnail {
        if let Err(e) = set_cover_from_threemf_thumbnail(pool, library, object_id, &png).await {
            eprintln!("[3mf] cover-from-thumbnail failed for {object_id:?}: {e}");
        }
    }
    true
}

/// Marker stored on thumbnail covers auto-captured from a 3MF's embedded preview.
/// Distinct from the STL render marker so the two auto-cover sources stay separable.
const THREEMF_COVER_MARKER: &str = "{\"generated\":\"3mf_thumbnail\"}";

/// Ingest a 3MF's embedded PNG as the object cover, but only if the object has no
/// cover yet. No-op otherwise — avoids overriding a user's cover and avoids leaving
/// an unused image asset behind.
async fn set_cover_from_threemf_thumbnail(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    png: &[u8],
) -> ServiceResult<()> {
    if repository::objects::has_cover(pool, object_id).await? {
        return Ok(());
    }
    let asset = ingest_bytes_with_extras(
        pool,
        library,
        object_id,
        png,
        "thumbnail.png",
        Some("image/png"),
        Some(THREEMF_COVER_MARKER),
    )
    .await?;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Attachment, 0).await?;
    repository::objects::set_cover_if_missing(pool, object_id, asset.id).await?;
    Ok(())
}

/// Best-effort: compute and store geometry for a mesh asset (STL/OBJ). 3MF is
/// handled by [`store_threemf`] instead. A parse or DB failure logs and leaves the
/// asset without a `mesh_metadata` sidecar rather than failing the ingest —
/// mirrors the gcode parser's tolerance.
async fn store_mesh_metadata(pool: &SqlitePool, asset_id: AssetId, path: &Path, kind: AssetKind) -> bool {
    let (parsed, format) = match kind {
        AssetKind::Stl => (mesh::stl::parse_path(path), "stl"),
        AssetKind::Obj => (mesh::obj::parse_path(path), "obj"),
        _ => return false,
    };
    let stats = match parsed {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[mesh] parse failed for {}: {e}", path.display());
            return false;
        }
    };
    let meta = stats.to_metadata(asset_id, format);
    if let Err(e) = repository::mesh_metadata::upsert(pool, &meta).await {
        eprintln!("[mesh] store failed for {asset_id:?}: {e}");
        return false;
    }
    true
}

/// One-shot: compute geometry for every parseable mesh asset (STL/OBJ) that has
/// no `mesh_metadata` row yet — files added before mesh extraction existed, or
/// added for a format the parser only learned later. Idempotent (subsequent
/// runs find nothing) and best-effort per file. Returns the number of assets
/// successfully populated. Intended to run detached in the background at startup.
pub async fn backfill_mesh_metadata(pool: &SqlitePool, library: &Library) -> ServiceResult<usize> {
    let missing = repository::mesh_metadata::mesh_assets_missing(pool).await?;
    let mut done = 0usize;
    for (asset_id, rel, kind_str) in missing {
        let abs = library.absolute(Path::new(&rel));
        if !abs.exists() {
            continue;
        }
        if store_mesh_metadata(pool, asset_id, &abs, AssetKind::from_str(&kind_str)).await {
            done += 1;
        }
    }
    Ok(done)
}

/// One-shot: (re)derive geometry, units, metadata_json, and cover for every 3MF
/// asset with no `mesh_metadata` row yet — files added before 3MF extraction
/// existed. 3MF needs the owning object to attach a thumbnail cover, so it can't
/// piggyback on [`backfill_mesh_metadata`] (which is object-agnostic). Idempotent
/// and best-effort per file; runs detached at startup. Returns the count populated.
pub async fn backfill_threemf(pool: &SqlitePool, library: &Library) -> ServiceResult<usize> {
    let missing = repository::mesh_metadata::threemf_assets_missing(pool).await?;
    let mut done = 0usize;
    for (asset_id, rel) in missing {
        let abs = library.absolute(Path::new(&rel));
        if !abs.exists() {
            continue;
        }
        let object_id = match repository::objects::object_id_for_asset(pool, asset_id).await? {
            Some(id) => id,
            None => continue,
        };
        if store_threemf(pool, library, object_id, asset_id, &abs).await {
            done += 1;
        }
    }
    Ok(done)
}

/// Best-effort: statically analyse a `.scad` source and store the result as the
/// asset's `metadata_json`. Never fails the ingest — a read/parse error logs and
/// leaves the asset without the sidecar. Stored even when empty so backfill (which
/// keys on a NULL `metadata_json`) doesn't re-scan geometry-only sources.
async fn store_scad_metadata(pool: &SqlitePool, asset_id: AssetId, path: &Path) -> bool {
    let meta = match scad::parse_path(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[scad] parse failed for {}: {e}", path.display());
            return false;
        }
    };
    let json = match serde_json::to_string(&meta) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("[scad] serialize failed for {asset_id:?}: {e}");
            return false;
        }
    };
    if let Err(e) = repository::assets::update_metadata_json(pool, asset_id, Some(&json)).await {
        eprintln!("[scad] store failed for {asset_id:?}: {e}");
        return false;
    }
    true
}

/// One-shot: render every `.scad` asset that has no `mesh_metadata` row yet and
/// store the resulting geometry (dimensions/volume/…), so SCAD sources join STL
/// and OBJ in the geometry UI. Rendering is expensive and needs the OpenSCAD
/// binary, so this is skipped entirely when it isn't configured, and runs
/// detached at startup. The render itself upserts the sidecar (see
/// [`super::openscad::render_preview`]); we only drive it and count successes.
pub async fn backfill_scad_geometry(pool: &SqlitePool, library: &Library) -> ServiceResult<usize> {
    if super::openscad::get_configured_path(pool).await?.is_none() {
        return Ok(0);
    }
    let missing = repository::mesh_metadata::scad_geometry_missing(pool).await?;
    let mut done = 0usize;
    for asset_id in missing {
        match super::openscad::render_preview(pool, library, asset_id).await {
            Ok(_) => done += 1,
            Err(e) => eprintln!("[scad] geometry backfill render failed for {asset_id:?}: {e}"),
        }
    }
    Ok(done)
}

/// One-shot: statically analyse every `.scad` asset with no `metadata_json` yet.
/// Idempotent and best-effort per file; runs detached at startup like the mesh
/// backfill. Returns the number of assets populated.
pub async fn backfill_scad_metadata(pool: &SqlitePool, library: &Library) -> ServiceResult<usize> {
    let missing = repository::assets::scad_assets_missing(pool).await?;
    let mut done = 0usize;
    for (asset_id, rel) in missing {
        let abs = library.absolute(Path::new(&rel));
        if !abs.exists() {
            continue;
        }
        if store_scad_metadata(pool, asset_id, &abs).await {
            done += 1;
        }
    }
    Ok(done)
}

/// Copy a CadQuery Python script into the managed library, attaching it with the
/// `scad` role so it shares the "CAD Scripts" section with OpenSCAD sources
/// (mirrors [`add_scad`]).
pub async fn add_cadquery(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
) -> ServiceResult<Asset> {
    let asset =
        ingest_file(pool, library, object_id, source, AssetKind::CadQuery, mime_for_path(source)).await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::Scad).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Scad, next_sort).await?;
    repository::objects::touch(pool, object_id).await?;
    Ok(asset)
}

/// Copy a text document (Markdown or plain `.txt`) into the managed library,
/// attaching it with the `document` role so it renders in the object's
/// "Docs / Notes" section. The kind drives whether the frontend renders it as
/// Markdown or verbatim.
pub async fn add_doc(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    source: &Path,
) -> ServiceResult<Asset> {
    let kind = match source.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("txt") => AssetKind::Text,
        _ => AssetKind::Markdown,
    };
    let asset =
        ingest_file(pool, library, object_id, source, kind, mime_for_path(source)).await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::Document).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::Document, next_sort).await?;
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
    store_scad_metadata(pool, asset.id, source).await;
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
    // Files picked through the generic attachment path are re-routed to their
    // proper section when the extension (or content) identifies them: models to
    // "Models", OpenSCAD/CadQuery to "CAD Scripts", Markdown to "Docs / Notes".
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "scad" => return add_scad(pool, library, object_id, source).await,
        "stl" | "obj" | "step" | "stp" | "3mf" => {
            return add_stl(pool, library, object_id, source).await
        }
        "md" | "markdown" | "txt" => return add_doc(pool, library, object_id, source).await,
        "py" if is_cadquery_py(source) => {
            return add_cadquery(pool, library, object_id, source).await
        }
        _ => {}
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

/// Register an already-on-disk model file (`.stl`, `.obj`, `.step`, `.3mf`) as a
/// `StlPart`. Geometry (and, for 3MF, units/metadata/thumbnail) is computed for
/// the formats the parser understands; other kinds are attached without it.
pub async fn attach_existing_model(
    pool: &SqlitePool,
    library: &Library,
    object_id: ObjectId,
    abs: &Path,
    kind: AssetKind,
) -> ServiceResult<Asset> {
    let mime = mime_for_path(abs).or_else(mime_for_stl);
    let asset = attach_existing_inner(pool, library, abs, kind, mime, None).await?;
    let next_sort = next_sort_order(pool, object_id, ObjectAssetRole::StlPart).await? as i64;
    repository::objects::attach_asset(pool, object_id, asset.id, ObjectAssetRole::StlPart, next_sort).await?;
    store_model_derived(pool, library, object_id, asset.id, abs, kind).await;
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
    store_scad_metadata(pool, asset.id, abs).await;
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
        Some("md") | Some("markdown") => Some("text/markdown"),
        Some("json") => Some("application/json"),
        Some("zip") => Some("application/zip"),
        Some("step") | Some("stp") => Some("model/step"),
        Some("scad") => Some("application/x-openscad"),
        Some("py") => Some("text/x-python"),
        Some("3mf") => Some("model/3mf"),
        Some("obj") => Some("model/obj"),
        _ => None,
    }
}

/// Map a 3D-model file extension to its [`AssetKind`]. All of these share the
/// `StlPart` role; the kind only drives which preview path the frontend uses.
fn classify_model_kind(p: &Path) -> AssetKind {
    match p.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("obj") => AssetKind::Obj,
        Some("step") | Some("stp") => AssetKind::Step,
        Some("3mf") => AssetKind::ThreeMf,
        _ => AssetKind::Stl,
    }
}

/// Sniff a `.py` file to decide whether it is a CadQuery script. Reads at most the
/// first 64 KiB and looks for a `cadquery` import (also matches the common
/// `import cadquery as cq` alias). Mirrors the slicer-sniffing done for gcode.
pub(crate) fn is_cadquery_py(p: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(p) else { return false };
    let mut buf = vec![0u8; 64 * 1024];
    let n = f.read(&mut buf).unwrap_or(0);
    let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
    head.lines().any(|line| {
        let l = line.trim_start();
        (l.starts_with("import ") || l.starts_with("from ")) && l.contains("cadquery")
    })
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
