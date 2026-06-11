use sqlx::{Row, SqlitePool};

use crate::db::now_ts;

pub mod keys {
    pub const USB_TARGET: &str = "usb.target";
    pub const USB_MANIFEST: &str = "usb.manifest";
    pub const USB_LAST_SYNC_AT: &str = "usb.last_sync_at";
    pub const USB_LAST_ERROR: &str = "usb.last_error";
    pub const USB_AUTO_SYNC: &str = "usb.auto_sync";
    pub const STL_PREVIEW_MAX_BYTES: &str = "viewer.stl_preview_max_bytes";
    pub const GCODE_CLEAN_KOBRA: &str = "gcode.clean_kobra";
}

pub async fn get(pool: &SqlitePool, key: &str) -> sqlx::Result<Option<String>> {
    let row = sqlx::query("SELECT value FROM app_settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get::<String, _>("value")))
}

pub async fn put(pool: &SqlitePool, key: &str, value: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET
             value = excluded.value,
             updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(now_ts())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, key: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM app_settings WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}
