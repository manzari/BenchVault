use std::path::Path;

use tauri::{AppHandle, State};

use crate::domain::{AssetId, MountedDrive, SyncReport, UsbTarget};
use crate::service::{self, AppError};
use crate::AppState;

#[tauri::command]
#[specta::specta]
pub async fn list_mounted_drives() -> Result<Vec<MountedDrive>, AppError> {
    Ok(service::usb::list_mounts()?)
}

#[tauri::command]
#[specta::specta]
pub async fn register_usb_target(
    state: State<'_, AppState>,
    mount_path: String,
    label: String,
    force: Option<bool>,
) -> Result<UsbTarget, AppError> {
    Ok(service::usb::register_usb(
        &state.db,
        &state.library,
        Path::new(&mount_path),
        &label,
        force.unwrap_or(false),
    )
    .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn clear_usb_target(state: State<'_, AppState>) -> Result<(), AppError> {
    service::usb::unregister_usb(&state.db).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_usb_target(state: State<'_, AppState>) -> Result<Option<UsbTarget>, AppError> {
    Ok(service::usb::current_target(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_usb_now(
    state: State<'_, AppState>,
    app: AppHandle,
    deep: Option<bool>,
) -> Result<SyncReport, AppError> {
    Ok(service::usb::sync_now(
        app,
        state.db.clone(),
        state.library.clone(),
        state.usb_sync_lock.clone(),
        deep.unwrap_or(false),
    )
    .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_gcode_export(
    state: State<'_, AppState>,
    asset_id: AssetId,
    export_to_usb: bool,
) -> Result<(), AppError> {
    service::usb::set_export_flag(&state.db, asset_id, export_to_usb).await?;
    state.autosync.ping();
    Ok(())
}
