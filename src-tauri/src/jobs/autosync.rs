//! Background USB auto-sync. Listens for "the exportable set may have changed" pings
//! (file watcher batches, gcode adds/removals, export-flag flips) and mirrors to the
//! registered USB after a short quiet period. A slow periodic tick additionally picks
//! up a freshly plugged-in stick and any mutation that doesn't ping explicitly.

use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;
use tauri::AppHandle;
use tokio::sync::mpsc;

use crate::events::{self, AppEvent};
use crate::library::Library;
use crate::service;

/// Let a burst of changes (a slicer exporting several files) settle before syncing.
const QUIET: Duration = Duration::from_secs(3);
/// Fallback cadence; also how quickly a plugged-in stick gets picked up.
const TICK: Duration = Duration::from_secs(30);

/// Cheap cloneable handle for signalling the auto-sync task.
#[derive(Clone)]
pub struct AutoSync {
    tx: mpsc::UnboundedSender<()>,
}

impl AutoSync {
    /// Signal that the exportable gcode set may have changed. Never blocks.
    pub fn ping(&self) {
        let _ = self.tx.send(());
    }
}

pub fn spawn(
    app: AppHandle,
    pool: SqlitePool,
    library: Arc<Library>,
    lock: Arc<tokio::sync::Mutex<()>>,
) -> AutoSync {
    let (tx, mut rx) = mpsc::unbounded_channel::<()>();
    tauri::async_runtime::spawn(async move {
        loop {
            let pinged = tokio::select! {
                r = rx.recv() => match r {
                    Some(()) => true,
                    None => return,
                },
                _ = tokio::time::sleep(TICK) => false,
            };
            if pinged {
                // Debounce: restart the quiet timer on every further ping.
                loop {
                    tokio::select! {
                        r = rx.recv() => match r {
                            Some(()) => continue,
                            None => return,
                        },
                        _ = tokio::time::sleep(QUIET) => break,
                    }
                }
            }

            if !service::settings::get_usb_auto_sync(&pool).await.unwrap_or(true) {
                continue;
            }
            match service::usb::sync_now(
                app.clone(),
                pool.clone(),
                library.clone(),
                lock.clone(),
                false,
            )
            .await
            {
                Ok(report) => {
                    // "no USB registered" every tick is pure noise; everything else
                    // (including "USB not detected") drives the connected badge.
                    if report.reason.as_deref() != Some("no USB registered") {
                        events::emit(&app, AppEvent::UsbSyncFinished { report });
                    }
                }
                Err(e) => eprintln!("[autosync] sync failed: {e}"),
            }
        }
    });
    AutoSync { tx }
}
