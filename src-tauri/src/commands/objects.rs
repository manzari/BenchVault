use tauri::State;

use crate::domain::{
    AssetId, CreateObjectInput, FolderId, Object, ObjectFull, ObjectId, ObjectSummary,
    UpdateObjectPatch,
};
use crate::repository;
use crate::service::{self, AppError};
use crate::AppState;

#[tauri::command]
#[specta::specta]
pub async fn list_objects(
    state: State<'_, AppState>,
    folder_id: Option<FolderId>,
) -> Result<Vec<ObjectSummary>, AppError> {
    Ok(repository::objects::list_in_folder(&state.db, folder_id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn list_all_objects(state: State<'_, AppState>) -> Result<Vec<ObjectSummary>, AppError> {
    Ok(repository::objects::list_all(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn get_object(state: State<'_, AppState>, id: ObjectId) -> Result<ObjectFull, AppError> {
    Ok(service::objects::full(&state.db, id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn create_object(
    state: State<'_, AppState>,
    input: CreateObjectInput,
) -> Result<Object, AppError> {
    Ok(service::objects::create_with_files(&state.db, &state.library, input).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn update_object(
    state: State<'_, AppState>,
    id: ObjectId,
    patch: UpdateObjectPatch,
) -> Result<Object, AppError> {
    Ok(service::objects::update(&state.db, &state.library, id, patch).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_object(state: State<'_, AppState>, id: ObjectId) -> Result<(), AppError> {
    service::objects::delete(&state.db, &state.library, id).await?;
    state.autosync.ping();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn set_main_project(
    state: State<'_, AppState>,
    object_id: ObjectId,
    asset_id: AssetId,
) -> Result<(), AppError> {
    service::objects::set_main_project(&state.db, object_id, asset_id).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn move_object(
    state: State<'_, AppState>,
    id: ObjectId,
    folder_id: Option<FolderId>,
) -> Result<(), AppError> {
    service::objects::move_to(&state.db, &state.library, id, folder_id).await?;
    // Moving changes the gcodes' relative paths, which the USB mirror tracks.
    state.autosync.ping();
    Ok(())
}
