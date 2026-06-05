use std::path::Path;

use tauri::State;

use crate::domain::{Asset, AssetId, GcodeMetadata, ObjectId, ParsedGcode};
use crate::parsers;
use crate::service::{self, AppError};
use crate::AppState;

#[tauri::command]
#[specta::specta]
pub async fn add_stl(
    state: State<'_, AppState>,
    object_id: ObjectId,
    source_path: String,
) -> Result<Asset, AppError> {
    Ok(service::files::add_stl(&state.db, &state.library, object_id, Path::new(&source_path)).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn add_scad(
    state: State<'_, AppState>,
    object_id: ObjectId,
    source_path: String,
) -> Result<Asset, AppError> {
    Ok(service::files::add_scad(&state.db, &state.library, object_id, Path::new(&source_path)).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn add_gcode(
    state: State<'_, AppState>,
    object_id: ObjectId,
    source_path: String,
) -> Result<(Asset, GcodeMetadata), AppError> {
    let res =
        service::files::add_gcode(&state.db, &state.library, object_id, Path::new(&source_path))
            .await?;
    state.autosync.ping();
    Ok(res)
}

#[tauri::command]
#[specta::specta]
pub async fn remove_asset(state: State<'_, AppState>, id: AssetId) -> Result<(), AppError> {
    service::files::remove_asset(&state.db, &state.library, id).await?;
    state.autosync.ping();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn set_cover_image(
    state: State<'_, AppState>,
    object_id: ObjectId,
    source_path: String,
) -> Result<Asset, AppError> {
    Ok(
        service::files::set_cover_image(&state.db, &state.library, object_id, Path::new(&source_path))
            .await?,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn add_attachment(
    state: State<'_, AppState>,
    object_id: ObjectId,
    source_path: String,
) -> Result<Asset, AppError> {
    Ok(
        service::files::add_attachment(&state.db, &state.library, object_id, Path::new(&source_path))
            .await?,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn set_cover_from_render(
    state: State<'_, AppState>,
    object_id: ObjectId,
    png: Vec<u8>,
    filename: String,
) -> Result<Asset, AppError> {
    Ok(
        service::files::set_cover_from_png(&state.db, &state.library, object_id, &png, &filename)
            .await?,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn set_cover_from_asset(
    state: State<'_, AppState>,
    object_id: ObjectId,
    asset_id: AssetId,
) -> Result<(), AppError> {
    service::files::set_cover_from_asset(&state.db, object_id, asset_id).await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn parse_gcode_file(path: String) -> Result<ParsedGcode, AppError> {
    parsers::gcode::parse_path(Path::new(&path))
        .map_err(|e| AppError::new("parser", format!("gcode parser: {e}")))
}
