use tauri::State;

use crate::domain::{Folder, FolderId};
use crate::repository;
use crate::service::{self, AppError};
use crate::AppState;

#[tauri::command]
#[specta::specta]
pub async fn list_folders(state: State<'_, AppState>) -> Result<Vec<Folder>, AppError> {
    Ok(repository::folders::list_active(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn create_folder(
    state: State<'_, AppState>,
    parent_id: Option<FolderId>,
    name: String,
) -> Result<Folder, AppError> {
    Ok(service::folders::create(&state.db, &state.library, parent_id, name.trim()).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn rename_folder(
    state: State<'_, AppState>,
    id: FolderId,
    name: String,
) -> Result<Folder, AppError> {
    Ok(service::folders::rename(&state.db, &state.library, id, name.trim()).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn move_folder(
    state: State<'_, AppState>,
    id: FolderId,
    new_parent: Option<FolderId>,
) -> Result<(), AppError> {
    service::folders::move_to(&state.db, &state.library, id, new_parent).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_folder(state: State<'_, AppState>, id: FolderId) -> Result<(), AppError> {
    service::folders::delete(&state.db, &state.library, id).await?;
    Ok(())
}
