use std::path::Path;

use tauri::State;

use crate::domain::{FolderId, ObjectId};
use crate::service::folder_import::{self, FolderImportReport};
use crate::service::imports::{self, UrlImportReport};
use crate::service::AppError;
use crate::AppState;

#[tauri::command]
#[specta::specta]
pub async fn import_from_url(
    state: State<'_, AppState>,
    url: String,
    folder_id: Option<FolderId>,
) -> Result<UrlImportReport, AppError> {
    Ok(imports::import_from_url(&state.db, &state.library, &url, folder_id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_thingiverse_token(state: State<'_, AppState>) -> Result<Option<String>, AppError> {
    Ok(imports::get_thingiverse_token(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_thingiverse_token(
    state: State<'_, AppState>,
    token: Option<String>,
) -> Result<(), AppError> {
    imports::set_thingiverse_token(&state.db, token.as_deref()).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn import_folder_as_object(
    state: State<'_, AppState>,
    source_path: String,
    folder_id: Option<FolderId>,
) -> Result<FolderImportReport, AppError> {
    Ok(folder_import::import_folder_as_object(
        &state.db,
        &state.library,
        Path::new(&source_path),
        folder_id,
    )
    .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn import_folder_into_object(
    state: State<'_, AppState>,
    object_id: ObjectId,
    source_path: String,
) -> Result<FolderImportReport, AppError> {
    Ok(folder_import::import_folder_into_object(
        &state.db,
        &state.library,
        object_id,
        Path::new(&source_path),
    )
    .await?)
}
