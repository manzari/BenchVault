use sqlx::{Row, SqlitePool};

use crate::db::now_ts;
use crate::domain::{AssetId, MeshMetadata};

pub async fn upsert(pool: &SqlitePool, m: &MeshMetadata) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO mesh_metadata
            (asset_id, source_format, source_units, triangle_count, vertex_count,
             bbox_x_mm, bbox_y_mm, bbox_z_mm, volume_mm3, surface_area_mm2,
             is_watertight, parsed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(asset_id) DO UPDATE SET
            source_format = excluded.source_format,
            source_units = excluded.source_units,
            triangle_count = excluded.triangle_count,
            vertex_count = excluded.vertex_count,
            bbox_x_mm = excluded.bbox_x_mm,
            bbox_y_mm = excluded.bbox_y_mm,
            bbox_z_mm = excluded.bbox_z_mm,
            volume_mm3 = excluded.volume_mm3,
            surface_area_mm2 = excluded.surface_area_mm2,
            is_watertight = excluded.is_watertight,
            parsed_at = excluded.parsed_at",
    )
    .bind(m.asset_id.0)
    .bind(&m.source_format)
    .bind(&m.source_units)
    .bind(m.triangle_count)
    .bind(m.vertex_count)
    .bind(m.bbox_x_mm)
    .bind(m.bbox_y_mm)
    .bind(m.bbox_z_mm)
    .bind(m.volume_mm3)
    .bind(m.surface_area_mm2)
    .bind(m.is_watertight.map(|b| if b { 1i64 } else { 0i64 }))
    .bind(m.parsed_at.unwrap_or_else(now_ts))
    .execute(pool)
    .await?;
    Ok(())
}

/// Parseable mesh assets (STL/OBJ) with no `mesh_metadata` row yet, as
/// (id, relative_path, kind) tuples, for backfill.
pub async fn mesh_assets_missing(pool: &SqlitePool) -> sqlx::Result<Vec<(AssetId, String, String)>> {
    let rows = sqlx::query(
        "SELECT a.id, a.relative_path, a.kind
           FROM assets a
           LEFT JOIN mesh_metadata mm ON mm.asset_id = a.id
          WHERE a.kind IN ('stl', 'obj') AND a.deleted_at IS NULL AND mm.asset_id IS NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                AssetId(r.get::<i64, _>("id")),
                r.get::<String, _>("relative_path"),
                r.get::<String, _>("kind"),
            )
        })
        .collect())
}

pub async fn get(pool: &SqlitePool, asset_id: AssetId) -> sqlx::Result<Option<MeshMetadata>> {
    let row = sqlx::query("SELECT * FROM mesh_metadata WHERE asset_id = ?")
        .bind(asset_id.0)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| MeshMetadata {
        asset_id,
        source_format: r.try_get("source_format").ok(),
        source_units: r.try_get("source_units").ok(),
        triangle_count: r.try_get("triangle_count").ok(),
        vertex_count: r.try_get("vertex_count").ok(),
        bbox_x_mm: r.try_get("bbox_x_mm").ok(),
        bbox_y_mm: r.try_get("bbox_y_mm").ok(),
        bbox_z_mm: r.try_get("bbox_z_mm").ok(),
        volume_mm3: r.try_get("volume_mm3").ok(),
        surface_area_mm2: r.try_get("surface_area_mm2").ok(),
        is_watertight: r.try_get::<Option<i64>, _>("is_watertight").ok().flatten().map(|v| v != 0),
        parsed_at: r.try_get("parsed_at").ok(),
    }))
}
