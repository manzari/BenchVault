use tauri::State;

use crate::domain::{AssetId, ObjectId};
use crate::service::orca::{NewProjectOutcome, OrcaDefaults, OrcaPresets};
use crate::service::{self, AppError};
use crate::AppState;

/// The configured OrcaSlicer path/command, or the auto-detected install if none
/// is set. `None` means nothing was found and the user must configure it.
#[tauri::command]
#[specta::specta]
pub async fn get_orca_path(state: State<'_, AppState>) -> Result<Option<String>, AppError> {
    Ok(service::orca::get_configured_path(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_orca_path(
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<(), AppError> {
    service::orca::set_configured_path(&state.db, path.as_deref()).await?;
    Ok(())
}

/// List the printer (machine) and filament presets OrcaSlicer has on disk, for
/// the default-preset dropdowns in Settings.
#[tauri::command]
#[specta::specta]
pub async fn list_orca_presets() -> Result<OrcaPresets, AppError> {
    Ok(service::orca::list_presets())
}

/// The configured default printer/filament preset paths.
#[tauri::command]
#[specta::specta]
pub async fn get_orca_defaults(state: State<'_, AppState>) -> Result<OrcaDefaults, AppError> {
    Ok(service::orca::get_defaults(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_orca_defaults(
    state: State<'_, AppState>,
    machine_path: Option<String>,
    filament_path: Option<String>,
) -> Result<(), AppError> {
    service::orca::set_defaults(&state.db, machine_path.as_deref(), filament_path.as_deref())
        .await?;
    Ok(())
}

/// Open a single asset (e.g. a `.3mf`) in the OrcaSlicer GUI.
#[tauri::command]
#[specta::specta]
pub async fn open_asset_in_orca(
    state: State<'_, AppState>,
    asset_id: AssetId,
) -> Result<(), AppError> {
    service::orca::open_asset(&state.db, &state.library, asset_id).await?;
    Ok(())
}

/// Build a fresh Orca project (`<object-name>.3mf`) from all of the object's STL
/// parts and open it.
#[tauri::command]
#[specta::specta]
pub async fn new_orca_project(
    state: State<'_, AppState>,
    object_id: ObjectId,
) -> Result<NewProjectOutcome, AppError> {
    Ok(service::orca::new_project(&state.db, &state.library, object_id).await?)
}
