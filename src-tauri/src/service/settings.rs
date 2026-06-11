//! Generic, user-tunable app settings surfaced to the frontend. Each value is
//! stored as a string in `app_settings` (see `repository::settings`) and parsed
//! back here, falling back to a sane default when unset or unparseable.

use sqlx::SqlitePool;

use crate::repository::settings::{self, keys};

use super::{ServiceError, ServiceResult};

/// Default cap on the STL file size the preview will read into the webview.
/// Bigger meshes blow the WebGL renderer's memory and the tab OOMs anyway, so
/// past this we refuse to load rather than crash. 100 MiB.
pub const DEFAULT_STL_PREVIEW_MAX_BYTES: i64 = 100 * 1024 * 1024;

/// Largest STL the preview will attempt to read, in bytes. The stored value, or
/// [`DEFAULT_STL_PREVIEW_MAX_BYTES`] when unset/invalid.
pub async fn get_stl_preview_max_bytes(pool: &SqlitePool) -> ServiceResult<i64> {
    let stored = settings::get(pool, keys::STL_PREVIEW_MAX_BYTES).await?;
    Ok(stored
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_STL_PREVIEW_MAX_BYTES))
}

pub async fn set_stl_preview_max_bytes(pool: &SqlitePool, bytes: i64) -> ServiceResult<()> {
    if bytes <= 0 {
        return Err(ServiceError::Validation(
            "maximum preview size must be greater than zero".into(),
        ));
    }
    settings::put(pool, keys::STL_PREVIEW_MAX_BYTES, &bytes.to_string()).await?;
    Ok(())
}

/// Whether incoming gcode should have OrcaSlicer 2.3.2's Kobra-breaking
/// `; filament_colour_type = ...` line stripped (on discovery and on USB export).
/// Off by default.
pub async fn get_clean_kobra_gcode(pool: &SqlitePool) -> ServiceResult<bool> {
    let stored = settings::get(pool, keys::GCODE_CLEAN_KOBRA).await?;
    Ok(stored.as_deref() == Some("1"))
}

pub async fn set_clean_kobra_gcode(pool: &SqlitePool, enabled: bool) -> ServiceResult<()> {
    settings::put(pool, keys::GCODE_CLEAN_KOBRA, if enabled { "1" } else { "0" }).await?;
    Ok(())
}

/// Whether the registered USB is mirrored automatically when the library changes
/// (and shortly after the stick is plugged in). On by default.
pub async fn get_usb_auto_sync(pool: &SqlitePool) -> ServiceResult<bool> {
    let stored = settings::get(pool, keys::USB_AUTO_SYNC).await?;
    Ok(stored.as_deref() != Some("0"))
}

pub async fn set_usb_auto_sync(pool: &SqlitePool, enabled: bool) -> ServiceResult<()> {
    settings::put(pool, keys::USB_AUTO_SYNC, if enabled { "1" } else { "0" }).await?;
    Ok(())
}
