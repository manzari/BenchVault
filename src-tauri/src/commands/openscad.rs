use tauri::State;

use crate::domain::AssetId;
use crate::service::{self, AppError};
use crate::AppState;

/// The configured OpenSCAD path/command, or the auto-detected install if none is
/// set. `None` means nothing was found and the user must configure it.
#[tauri::command]
#[specta::specta]
pub async fn get_openscad_path(state: State<'_, AppState>) -> Result<Option<String>, AppError> {
    Ok(service::openscad::get_configured_path(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_openscad_path(
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<(), AppError> {
    service::openscad::set_configured_path(&state.db, path.as_deref()).await?;
    Ok(())
}

/// Open a `.scad` asset in the OpenSCAD GUI (fire-and-forget).
#[tauri::command]
#[specta::specta]
pub async fn open_asset_in_openscad(
    state: State<'_, AppState>,
    asset_id: AssetId,
) -> Result<(), AppError> {
    service::openscad::open_asset(&state.db, &state.library, asset_id).await?;
    Ok(())
}

/// Render a `.scad` asset to STL bytes for the three.js preview. Serialized by a
/// render lock so a double-click doesn't spawn two renders — the second call then
/// hits the on-disk cache.
#[tauri::command]
#[specta::specta]
pub async fn render_scad_preview(
    state: State<'_, AppState>,
    asset_id: AssetId,
) -> Result<Vec<u8>, AppError> {
    let _guard = state.scad_render_lock.lock().await;
    Ok(service::openscad::render_preview(&state.db, &state.library, asset_id).await?)
}
