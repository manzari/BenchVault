use std::path::Path;

use tauri::State;

use crate::domain::AssetId;
use crate::repository;
use crate::service::AppError;
use crate::AppState;

/// Read raw bytes of an asset for the frontend (used by STLLoader, image preview, etc.).
/// STLs are bounded in size, so passing them across IPC is fine for v1.
#[tauri::command]
#[specta::specta]
pub async fn read_asset_bytes(state: State<'_, AppState>, id: AssetId) -> Result<Vec<u8>, AppError> {
    let asset = repository::assets::get(&state.db, id).await?;
    let abs = state.library.absolute(Path::new(&asset.relative_path));
    Ok(std::fs::read(abs)?)
}
