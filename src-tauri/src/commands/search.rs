use tauri::State;

use crate::domain::SearchHit;
use crate::repository;
use crate::service::AppError;
use crate::AppState;

#[tauri::command]
#[specta::specta]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    kinds: Option<Vec<String>>,
) -> Result<Vec<SearchHit>, AppError> {
    Ok(repository::search::search(&state.db, &query, kinds.as_deref()).await?)
}
