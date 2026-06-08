use tauri::State;

use crate::domain::{Tag, TagId};
use crate::repository;
use crate::service::AppError;
use crate::AppState;

#[tauri::command]
#[specta::specta]
pub async fn list_tags(state: State<'_, AppState>) -> Result<Vec<Tag>, AppError> {
    Ok(repository::tags::list(&state.db).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn create_tag(
    state: State<'_, AppState>,
    name: String,
    color: Option<String>,
) -> Result<Tag, AppError> {
    Ok(repository::tags::create(&state.db, name.trim(), color.as_deref()).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn attach_tag(
    state: State<'_, AppState>,
    entity_kind: String,
    entity_id: i64,
    tag_id: TagId,
) -> Result<(), AppError> {
    repository::tags::attach(&state.db, &entity_kind, entity_id, tag_id).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn detach_tag(
    state: State<'_, AppState>,
    entity_kind: String,
    entity_id: i64,
    tag_id: TagId,
) -> Result<(), AppError> {
    repository::tags::detach(&state.db, &entity_kind, entity_id, tag_id).await?;
    Ok(())
}
