// Typed event bus wrapper. Future background jobs (re-parse, thumbnails, file watchers)
// emit through here so the frontend can subscribe by name.

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter};

use crate::domain::SyncReport;

#[derive(Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppEvent {
    LibraryChanged,
    JobProgress { job_id: String, percent: u8, message: Option<String> },
    /// A background (auto) USB sync finished; manual syncs report via the command result.
    UsbSyncFinished { report: SyncReport },
}

pub fn emit(app: &AppHandle, event: AppEvent) {
    let _ = app.emit("app-event", event);
}
