use std::path::{Path, PathBuf};

use tauri::{AppHandle, State};

use crate::domain::ObjectId;
use crate::events::{self, AppEvent};
use crate::jobs;
use crate::service::{self, AppError};
use crate::AppState;

#[tauri::command]
#[specta::specta]
pub async fn reveal_object_in_explorer(
    state: State<'_, AppState>,
    object_id: ObjectId,
) -> Result<(), AppError> {
    let target: PathBuf =
        service::objects::ensure_dir(&state.db, &state.library, object_id).await?;
    reveal(&target)?;
    Ok(())
}

/// Sweep the managed library for files that exist on disk but aren't tracked in the
/// DB yet (e.g. gcode a slicer just saved into an object's folder), ingesting each one.
/// Returns the number of files newly discovered. Emits `LibraryChanged` if any were.
#[tauri::command]
#[specta::specta]
pub async fn rescan_library(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<u32, AppError> {
    let ingested = jobs::watcher::reconcile(&state.db, &state.library).await?;
    if ingested > 0 {
        events::emit(&app, AppEvent::LibraryChanged);
        state.autosync.ping();
    }
    Ok(ingested as u32)
}

/// Classify a list of filesystem paths as `dir`, `file`, or `missing`. Used by the drag-and-drop
/// router to tell whether a dropped path should be ingested as a folder or a single file.
#[tauri::command]
#[specta::specta]
pub fn classify_paths(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .map(|p| {
            let path = Path::new(&p);
            match std::fs::symlink_metadata(path) {
                Ok(m) if m.is_dir() => "dir".to_string(),
                Ok(_) => "file".to_string(),
                Err(_) => "missing".to_string(),
            }
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn reveal(path: &Path) -> std::io::Result<()> {
    if path.is_file() {
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()?;
    } else {
        std::process::Command::new("explorer.exe").arg(path).spawn()?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn reveal(path: &Path) -> std::io::Result<()> {
    if path.is_file() {
        std::process::Command::new("open").arg("-R").arg(path).spawn()?;
    } else {
        std::process::Command::new("open").arg(path).spawn()?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn reveal(path: &Path) -> std::io::Result<()> {
    let dir = if path.is_file() {
        path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    };
    std::process::Command::new("xdg-open").arg(&dir).spawn()?;
    Ok(())
}
