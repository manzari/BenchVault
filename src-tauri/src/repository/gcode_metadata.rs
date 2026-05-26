use sqlx::{Row, SqlitePool};

use crate::db::now_ts;
use crate::domain::{AssetId, GcodeMetadata};

#[derive(Clone)]
pub struct ExportableGcode {
    pub asset_id: AssetId,
    pub relative_path: String,
    pub size_bytes: i64,
    pub asset_updated_at: i64,
    pub sha256: Option<String>,
}

pub async fn upsert(pool: &SqlitePool, m: &GcodeMetadata) -> sqlx::Result<()> {
    let now = now_ts();
    // On conflict, preserve the user's export_to_usb flag — re-parsing should never reset it.
    sqlx::query(
        "INSERT INTO gcode_metadata
            (asset_id, slicer_name, slicer_version, profile_name, layer_height_mm,
             nozzle_temp_c, bed_temp_c, estimated_time_seconds,
             filament_length_mm, filament_weight_g, filament_type, parsed_at, export_to_usb)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(asset_id) DO UPDATE SET
            slicer_name = excluded.slicer_name,
            slicer_version = excluded.slicer_version,
            profile_name = excluded.profile_name,
            layer_height_mm = excluded.layer_height_mm,
            nozzle_temp_c = excluded.nozzle_temp_c,
            bed_temp_c = excluded.bed_temp_c,
            estimated_time_seconds = excluded.estimated_time_seconds,
            filament_length_mm = excluded.filament_length_mm,
            filament_weight_g = excluded.filament_weight_g,
            filament_type = excluded.filament_type,
            parsed_at = excluded.parsed_at",
    )
    .bind(m.asset_id.0)
    .bind(&m.slicer_name)
    .bind(&m.slicer_version)
    .bind(&m.profile_name)
    .bind(m.layer_height_mm)
    .bind(m.nozzle_temp_c)
    .bind(m.bed_temp_c)
    .bind(m.estimated_time_seconds)
    .bind(m.filament_length_mm)
    .bind(m.filament_weight_g)
    .bind(&m.filament_type)
    .bind(m.parsed_at.unwrap_or(now))
    .bind(if m.export_to_usb { 1i64 } else { 0i64 })
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &SqlitePool, asset_id: AssetId) -> sqlx::Result<Option<GcodeMetadata>> {
    let row = sqlx::query("SELECT * FROM gcode_metadata WHERE asset_id = ?")
        .bind(asset_id.0)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| GcodeMetadata {
        asset_id,
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
    }))
}

pub async fn set_export_to_usb(
    pool: &SqlitePool,
    asset_id: AssetId,
    flag: bool,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE gcode_metadata SET export_to_usb = ? WHERE asset_id = ?")
        .bind(if flag { 1i64 } else { 0i64 })
        .bind(asset_id.0)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_exportable(pool: &SqlitePool) -> sqlx::Result<Vec<ExportableGcode>> {
    let rows = sqlx::query(
        "SELECT a.id, a.relative_path, a.size_bytes, a.updated_at, a.sha256
           FROM gcode_metadata gm
           JOIN assets a         ON a.id       = gm.asset_id
           JOIN object_assets oa ON oa.asset_id = a.id AND oa.role = 'gcode'
           JOIN objects o        ON o.id       = oa.object_id
          WHERE gm.export_to_usb = 1
            AND a.deleted_at IS NULL
            AND o.deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| ExportableGcode {
            asset_id: AssetId(r.get::<i64, _>("id")),
            relative_path: r.get("relative_path"),
            size_bytes: r.try_get::<Option<i64>, _>("size_bytes").ok().flatten().unwrap_or(0),
            asset_updated_at: r.get::<i64, _>("updated_at"),
            sha256: r.try_get("sha256").ok(),
        })
        .collect())
}
