use tauri::State;

use crate::service::{self, AppError};
use crate::AppState;

/// Largest STL the preview will attempt to read, in bytes (default 100 MiB).
#[tauri::command]
#[specta::specta]
pub async fn get_stl_preview_max_bytes(state: State<'_, AppState>) -> Result<i64, AppError> {
    Ok(service::settings::get_stl_preview_max_bytes(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_stl_preview_max_bytes(
    state: State<'_, AppState>,
    bytes: i64,
) -> Result<(), AppError> {
    service::settings::set_stl_preview_max_bytes(&state.db, bytes).await?;
    Ok(())
}

/// Whether incoming gcode is stripped of OrcaSlicer 2.3.2's Kobra-breaking line.
#[tauri::command]
#[specta::specta]
pub async fn get_clean_kobra_gcode(state: State<'_, AppState>) -> Result<bool, AppError> {
    Ok(service::settings::get_clean_kobra_gcode(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_clean_kobra_gcode(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), AppError> {
    service::settings::set_clean_kobra_gcode(&state.db, enabled).await?;
    Ok(())
}

/// Whether the registered USB is mirrored automatically on library changes.
#[tauri::command]
#[specta::specta]
pub async fn get_usb_auto_sync(state: State<'_, AppState>) -> Result<bool, AppError> {
    Ok(service::settings::get_usb_auto_sync(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_usb_auto_sync(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), AppError> {
    service::settings::set_usb_auto_sync(&state.db, enabled).await?;
    if enabled {
        state.autosync.ping();
    }
    Ok(())
}
