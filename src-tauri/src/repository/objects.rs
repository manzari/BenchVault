use std::collections::HashSet;

use sqlx::{Row, SqlitePool};

use crate::db::now_ts;
use crate::domain::{
    Asset, AssetId, AssetKind, AttachedAsset, FolderId, GcodeMetadata, MeshMetadata, Object,
    ObjectAssetRole, ObjectFull, ObjectId, ObjectSummary, Tag, TagId, UpdateObjectPatch,
};

fn map_object(r: sqlx::sqlite::SqliteRow) -> Object {
    Object {
        id: ObjectId(r.get::<i64, _>("id")),
        folder_id: r.try_get::<Option<i64>, _>("folder_id").unwrap_or(None).map(FolderId),
        name: r.get("name"),
        notes: r.try_get("notes").ok(),
        source_url: r.try_get("source_url").ok(),
        author: r.try_get("author").ok(),
        license: r.try_get("license").ok(),
        cover_asset_id: r.try_get::<Option<i64>, _>("cover_asset_id").unwrap_or(None).map(AssetId),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

pub async fn create(
    pool: &SqlitePool,
    folder_id: Option<FolderId>,
    name: &str,
    dir_name: &str,
) -> sqlx::Result<Object> {
    let now = now_ts();
    let id = sqlx::query(
        "INSERT INTO objects (folder_id, name, dir_name, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(folder_id.map(|x| x.0))
    .bind(name)
    .bind(dir_name)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?
    .last_insert_rowid();
    get_basic(pool, ObjectId(id)).await
}

pub async fn sibling_object_dir_names(
    pool: &SqlitePool,
    folder_id: Option<FolderId>,
    exclude_id: Option<ObjectId>,
) -> sqlx::Result<HashSet<String>> {
    let rows = if let Some(f) = folder_id {
        sqlx::query(
            "SELECT dir_name FROM objects
             WHERE folder_id = ? AND deleted_at IS NULL AND dir_name IS NOT NULL
             AND (? IS NULL OR id != ?)",
        )
        .bind(f.0)
        .bind(exclude_id.map(|x| x.0))
        .bind(exclude_id.map(|x| x.0))
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT dir_name FROM objects
             WHERE folder_id IS NULL AND deleted_at IS NULL AND dir_name IS NOT NULL
             AND (? IS NULL OR id != ?)",
        )
        .bind(exclude_id.map(|x| x.0))
        .bind(exclude_id.map(|x| x.0))
        .fetch_all(pool)
        .await?
    };
    Ok(rows.into_iter().map(|r| r.get::<String, _>("dir_name")).collect())
}

pub async fn find_by_folder_and_dir(
    pool: &SqlitePool,
    folder_id: Option<FolderId>,
    dir_name: &str,
) -> sqlx::Result<Option<Object>> {
    let row = if let Some(f) = folder_id {
        sqlx::query(
            "SELECT * FROM objects
             WHERE folder_id = ? AND dir_name = ? AND deleted_at IS NULL",
        )
        .bind(f.0)
        .bind(dir_name)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT * FROM objects
             WHERE folder_id IS NULL AND dir_name = ? AND deleted_at IS NULL",
        )
        .bind(dir_name)
        .fetch_optional(pool)
        .await?
    };
    Ok(row.map(map_object))
}

pub async fn get_dir_name(pool: &SqlitePool, id: ObjectId) -> sqlx::Result<Option<String>> {
    let row = sqlx::query("SELECT dir_name FROM objects WHERE id = ?")
        .bind(id.0)
        .fetch_one(pool)
        .await?;
    Ok(row.try_get::<Option<String>, _>("dir_name").unwrap_or(None))
}

pub async fn set_dir_name(pool: &SqlitePool, id: ObjectId, dir_name: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE objects SET dir_name = ? WHERE id = ?")
        .bind(dir_name)
        .bind(id.0)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_basic(pool: &SqlitePool, id: ObjectId) -> sqlx::Result<Object> {
    let row = sqlx::query("SELECT * FROM objects WHERE id = ? AND deleted_at IS NULL")
        .bind(id.0)
        .fetch_one(pool)
        .await?;
    Ok(map_object(row))
}

pub async fn list_in_folder(pool: &SqlitePool, folder_id: Option<FolderId>) -> sqlx::Result<Vec<ObjectSummary>> {
    // Folder filter: NULL parent ⇒ root, otherwise specific folder.
    let rows = if let Some(f) = folder_id {
        sqlx::query(
            "SELECT o.id, o.folder_id, o.name, o.cover_asset_id, o.updated_at,
                    SUM(CASE WHEN oa.role = 'stl_part' THEN 1 ELSE 0 END) AS stl_count,
                    SUM(CASE WHEN oa.role = 'gcode' THEN 1 ELSE 0 END) AS gcode_count
             FROM objects o
             LEFT JOIN object_assets oa ON oa.object_id = o.id
             WHERE o.deleted_at IS NULL AND o.folder_id = ?
             GROUP BY o.id
             ORDER BY o.updated_at DESC",
        )
        .bind(f.0)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT o.id, o.folder_id, o.name, o.cover_asset_id, o.updated_at,
                    SUM(CASE WHEN oa.role = 'stl_part' THEN 1 ELSE 0 END) AS stl_count,
                    SUM(CASE WHEN oa.role = 'gcode' THEN 1 ELSE 0 END) AS gcode_count
             FROM objects o
             LEFT JOIN object_assets oa ON oa.object_id = o.id
             WHERE o.deleted_at IS NULL AND o.folder_id IS NULL
             GROUP BY o.id
             ORDER BY o.updated_at DESC",
        )
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(|r| ObjectSummary {
            id: ObjectId(r.get::<i64, _>("id")),
            folder_id: r.try_get::<Option<i64>, _>("folder_id").unwrap_or(None).map(FolderId),
            name: r.get("name"),
            cover_asset_id: r.try_get::<Option<i64>, _>("cover_asset_id").unwrap_or(None).map(AssetId),
            stl_count: r.try_get::<Option<i64>, _>("stl_count").ok().flatten().unwrap_or(0),
            gcode_count: r.try_get::<Option<i64>, _>("gcode_count").ok().flatten().unwrap_or(0),
            updated_at: r.get("updated_at"),
        })
        .collect())
}

pub async fn list_all(pool: &SqlitePool) -> sqlx::Result<Vec<ObjectSummary>> {
    let rows = sqlx::query(
        "SELECT o.id, o.folder_id, o.name, o.cover_asset_id, o.updated_at,
                SUM(CASE WHEN oa.role = 'stl_part' THEN 1 ELSE 0 END) AS stl_count,
                SUM(CASE WHEN oa.role = 'gcode' THEN 1 ELSE 0 END) AS gcode_count
         FROM objects o
         LEFT JOIN object_assets oa ON oa.object_id = o.id
         WHERE o.deleted_at IS NULL
         GROUP BY o.id
         ORDER BY o.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| ObjectSummary {
            id: ObjectId(r.get::<i64, _>("id")),
            folder_id: r.try_get::<Option<i64>, _>("folder_id").unwrap_or(None).map(FolderId),
            name: r.get("name"),
            cover_asset_id: r.try_get::<Option<i64>, _>("cover_asset_id").unwrap_or(None).map(AssetId),
            stl_count: r.try_get::<Option<i64>, _>("stl_count").ok().flatten().unwrap_or(0),
            gcode_count: r.try_get::<Option<i64>, _>("gcode_count").ok().flatten().unwrap_or(0),
            updated_at: r.get("updated_at"),
        })
        .collect())
}

pub async fn get_full(pool: &SqlitePool, id: ObjectId) -> sqlx::Result<ObjectFull> {
    let object = get_basic(pool, id).await?;

    let asset_rows = sqlx::query(
        "SELECT a.*, oa.role AS oa_role, oa.sort_order AS oa_sort,
                gm.slicer_name, gm.slicer_version, gm.profile_name, gm.layer_height_mm,
                gm.nozzle_temp_c, gm.bed_temp_c, gm.estimated_time_seconds,
                gm.filament_length_mm, gm.filament_weight_g, gm.filament_type, gm.parsed_at,
                gm.export_to_usb,
                mm.source_format AS mesh_source_format, mm.source_units AS mesh_source_units,
                mm.triangle_count AS mesh_triangle_count, mm.vertex_count AS mesh_vertex_count,
                mm.bbox_x_mm AS mesh_bbox_x, mm.bbox_y_mm AS mesh_bbox_y, mm.bbox_z_mm AS mesh_bbox_z,
                mm.volume_mm3 AS mesh_volume, mm.surface_area_mm2 AS mesh_area,
                mm.is_watertight AS mesh_watertight, mm.parsed_at AS mesh_parsed_at
         FROM object_assets oa
         JOIN assets a ON a.id = oa.asset_id
         LEFT JOIN gcode_metadata gm ON gm.asset_id = a.id
         LEFT JOIN mesh_metadata mm ON mm.asset_id = a.id
         WHERE oa.object_id = ?
         ORDER BY oa.role, oa.sort_order, a.id",
    )
    .bind(id.0)
    .fetch_all(pool)
    .await?;

    let mut assets = Vec::with_capacity(asset_rows.len());
    for r in asset_rows {
        let asset = Asset {
            id: AssetId(r.get::<i64, _>("id")),
            kind: AssetKind::from_str(r.get::<&str, _>("kind")),
            relative_path: r.get("relative_path"),
            original_filename: r.get("original_filename"),
            mime_type: r.try_get("mime_type").ok(),
            size_bytes: r.try_get("size_bytes").ok(),
            sha256: r.try_get("sha256").ok(),
            metadata_json: r.try_get("metadata_json").ok(),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
            deleted_at: r.try_get("deleted_at").ok(),
        };
        let role = ObjectAssetRole::from_str(r.get::<&str, _>("oa_role"));
        let gm = if matches!(role, ObjectAssetRole::Gcode) {
            Some(GcodeMetadata {
                asset_id: asset.id,
                slicer_name: r.try_get("slicer_name").ok(),
                slicer_version: r.try_get("slicer_version").ok(),
                profile_name: r.try_get("profile_name").ok(),
                layer_height_mm: r.try_get("layer_height_mm").ok(),
                nozzle_temp_c: r.try_get("nozzle_temp_c").ok(),
                bed_temp_c: r.try_get("bed_temp_c").ok(),
                estimated_time_seconds: r.try_get("estimated_time_seconds").ok(),
                filament_length_mm: r.try_get("filament_length_mm").ok(),
                filament_weight_g: r.try_get("filament_weight_g").ok(),
                filament_type: r.try_get("filament_type").ok(),
                parsed_at: r.try_get("parsed_at").ok(),
                export_to_usb: r
                    .try_get::<i64, _>("export_to_usb")
                    .map(|v| v != 0)
                    .unwrap_or(true),
            })
        } else {
            None
        };
        let mesh = if r.try_get::<Option<i64>, _>("mesh_parsed_at").ok().flatten().is_some() {
            Some(MeshMetadata {
                asset_id: asset.id,
                source_format: r.try_get("mesh_source_format").ok(),
                source_units: r.try_get("mesh_source_units").ok(),
                triangle_count: r.try_get("mesh_triangle_count").ok(),
                vertex_count: r.try_get("mesh_vertex_count").ok(),
                bbox_x_mm: r.try_get("mesh_bbox_x").ok(),
                bbox_y_mm: r.try_get("mesh_bbox_y").ok(),
                bbox_z_mm: r.try_get("mesh_bbox_z").ok(),
                volume_mm3: r.try_get("mesh_volume").ok(),
                surface_area_mm2: r.try_get("mesh_area").ok(),
                is_watertight: r
                    .try_get::<Option<i64>, _>("mesh_watertight")
                    .ok()
                    .flatten()
                    .map(|v| v != 0),
                parsed_at: r.try_get("mesh_parsed_at").ok(),
            })
        } else {
            None
        };
        assets.push(AttachedAsset {
            asset,
            role,
            sort_order: r.get("oa_sort"),
            gcode_metadata: gm,
            mesh_metadata: mesh,
        });
    }

    let tag_rows = sqlx::query(
        "SELECT t.id, t.name, t.color, t.created_at
         FROM entity_tags et
         JOIN tags t ON t.id = et.tag_id
         WHERE et.entity_kind = 'object' AND et.entity_id = ?
         ORDER BY t.name",
    )
    .bind(id.0)
    .fetch_all(pool)
    .await?;

    let tags = tag_rows
        .into_iter()
        .map(|r| Tag {
            id: TagId(r.get::<i64, _>("id")),
            name: r.get("name"),
            color: r.try_get("color").ok(),
            created_at: r.get("created_at"),
        })
        .collect();

    let main_project_asset_id = main_project(pool, id, &assets).await?;

    Ok(ObjectFull { object, assets, tags, main_project_asset_id })
}

fn is_3mf(a: &AttachedAsset) -> bool {
    a.asset.original_filename.to_lowercase().ends_with(".3mf")
}

/// Resolve the object's main `.3mf`: the user's explicit `main_project` link if
/// it still points at one of the object's `.3mf` assets, otherwise the most
/// recently updated `.3mf` (so it defaults to the latest project).
async fn main_project(
    pool: &SqlitePool,
    id: ObjectId,
    assets: &[AttachedAsset],
) -> sqlx::Result<Option<AssetId>> {
    let pinned: Option<i64> = sqlx::query(
        "SELECT to_id FROM entity_links
          WHERE from_kind = 'object' AND from_id = ? AND to_kind = 'asset'
            AND relation = 'main_project'
          LIMIT 1",
    )
    .bind(id.0)
    .fetch_optional(pool)
    .await?
    .map(|r| r.get::<i64, _>("to_id"));

    if let Some(tid) = pinned {
        if assets.iter().any(|a| a.asset.id.0 == tid && is_3mf(a)) {
            return Ok(Some(AssetId(tid)));
        }
    }

    Ok(assets
        .iter()
        .filter(|a| is_3mf(a))
        .max_by(|x, y| {
            x.asset
                .updated_at
                .cmp(&y.asset.updated_at)
                .then(x.asset.id.0.cmp(&y.asset.id.0))
        })
        .map(|a| a.asset.id))
}

pub async fn attach_asset(
    pool: &SqlitePool,
    object_id: ObjectId,
    asset_id: AssetId,
    role: ObjectAssetRole,
    sort_order: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO object_assets (object_id, asset_id, role, sort_order) VALUES (?, ?, ?, ?)",
    )
    .bind(object_id.0)
    .bind(asset_id.0)
    .bind(role.as_str())
    .bind(sort_order)
    .execute(pool)
    .await?;
    Ok(())
}

/// Asset ids of auto-captured STL render covers attached to `object_id`, identified
/// by `marker` stored in `assets.metadata_json`. Used to replace the previous render
/// cover on each capture; `except` (the freshly-created one) is never returned, and
/// user-added photos — which lack the marker — are never matched.
pub async fn render_cover_asset_ids(
    pool: &SqlitePool,
    object_id: ObjectId,
    marker: &str,
    except: AssetId,
) -> sqlx::Result<Vec<AssetId>> {
    let rows = sqlx::query(
        "SELECT a.id AS id
         FROM object_assets oa
         JOIN assets a ON a.id = oa.asset_id
         WHERE oa.object_id = ?
           AND oa.role = 'attachment'
           AND a.kind = 'image'
           AND a.metadata_json = ?
           AND a.id != ?",
    )
    .bind(object_id.0)
    .bind(marker)
    .bind(except.0)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| AssetId(r.get::<i64, _>("id"))).collect())
}

pub async fn asset_ids_for_object(pool: &SqlitePool, object_id: ObjectId) -> sqlx::Result<Vec<AssetId>> {
    let rows = sqlx::query("SELECT asset_id FROM object_assets WHERE object_id = ?")
        .bind(object_id.0)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| AssetId(r.get::<i64, _>("asset_id"))).collect())
}

/// The object an asset is attached to, if any (first match). Used by the 3MF
/// backfill to resolve the owner when re-deriving cover/metadata off an asset id.
pub async fn object_id_for_asset(pool: &SqlitePool, asset_id: AssetId) -> sqlx::Result<Option<ObjectId>> {
    let row = sqlx::query("SELECT object_id FROM object_assets WHERE asset_id = ? LIMIT 1")
        .bind(asset_id.0)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| ObjectId(r.get::<i64, _>("object_id"))))
}

/// Whether the object already has a cover image set.
pub async fn has_cover(pool: &SqlitePool, object_id: ObjectId) -> sqlx::Result<bool> {
    let row = sqlx::query("SELECT cover_asset_id FROM objects WHERE id = ?")
        .bind(object_id.0)
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|r| r.try_get::<Option<i64>, _>("cover_asset_id").ok().flatten()).is_some())
}

pub async fn detach_asset(pool: &SqlitePool, asset_id: AssetId) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM object_assets WHERE asset_id = ?")
        .bind(asset_id.0)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_cover(pool: &SqlitePool, object_id: ObjectId, asset_id: AssetId) -> sqlx::Result<()> {
    let now = now_ts();
    sqlx::query("UPDATE objects SET cover_asset_id = ?, updated_at = ? WHERE id = ?")
        .bind(asset_id.0)
        .bind(now)
        .bind(object_id.0)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_cover_if_missing(
    pool: &SqlitePool,
    object_id: ObjectId,
    asset_id: AssetId,
) -> sqlx::Result<()> {
    let now = now_ts();
    sqlx::query(
        "UPDATE objects SET cover_asset_id = ?, updated_at = ?
         WHERE id = ? AND cover_asset_id IS NULL",
    )
    .bind(asset_id.0)
    .bind(now)
    .bind(object_id.0)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn touch(pool: &SqlitePool, object_id: ObjectId) -> sqlx::Result<()> {
    let now = now_ts();
    sqlx::query("UPDATE objects SET updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(object_id.0)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update(pool: &SqlitePool, id: ObjectId, patch: UpdateObjectPatch) -> sqlx::Result<Object> {
    // Pure dynamic update: build SET list for fields that the caller actually provided.
    // Empty strings are written as-is and treated as "cleared" by the UI.
    let mut sets: Vec<&str> = Vec::new();
    if patch.name.is_some() { sets.push("name = ?"); }
    if patch.notes.is_some() { sets.push("notes = ?"); }
    if patch.source_url.is_some() { sets.push("source_url = ?"); }
    if patch.author.is_some() { sets.push("author = ?"); }
    if patch.license.is_some() { sets.push("license = ?"); }

    if sets.is_empty() {
        return get_basic(pool, id).await;
    }

    sets.push("updated_at = ?");

    let sql = format!("UPDATE objects SET {} WHERE id = ?", sets.join(", "));
    let mut q = sqlx::query(&sql);
    if let Some(v) = patch.name { q = q.bind(v); }
    if let Some(v) = patch.notes { q = q.bind(v); }
    if let Some(v) = patch.source_url { q = q.bind(v); }
    if let Some(v) = patch.author { q = q.bind(v); }
    if let Some(v) = patch.license { q = q.bind(v); }
    q = q.bind(now_ts()).bind(id.0);
    q.execute(pool).await?;
    get_basic(pool, id).await
}

pub async fn move_to(pool: &SqlitePool, id: ObjectId, folder_id: Option<FolderId>) -> sqlx::Result<()> {
    let now = now_ts();
    sqlx::query("UPDATE objects SET folder_id = ?, updated_at = ? WHERE id = ?")
        .bind(folder_id.map(|x| x.0))
        .bind(now)
        .bind(id.0)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn soft_delete(pool: &SqlitePool, id: ObjectId) -> sqlx::Result<()> {
    let now = now_ts();
    sqlx::query("UPDATE objects SET deleted_at = ?, updated_at = ? WHERE id = ?")
        .bind(now)
        .bind(now)
        .bind(id.0)
        .execute(pool)
        .await?;
    Ok(())
}
