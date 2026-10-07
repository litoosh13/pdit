//! Updates (D-058): the app asks GitHub for the newest release's latest.json
//! (tauri-plugin-updater; releases are signed, the public key is in
//! tauri.conf.json), and the web app offers it to the user. Downloading and
//! installing happen only when the user says so.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

static PENDING: Mutex<Option<Update>> = Mutex::new(None);
static DONE: AtomicU64 = AtomicU64::new(0);
static TOTAL: AtomicU64 = AtomicU64::new(0);

#[derive(serde::Serialize)]
pub struct Available {
    version: String,
    current: String,
    notes: Option<String>,
}

/// A newer version, if there is one (and keeps it for update_install).
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<Available>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let found = update.as_ref().map(|u| Available {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone(),
    });
    *PENDING.lock().map_err(|e| e.to_string())? = update;
    Ok(found)
}

/// Downloads and installs the update found by update_check; progress via
/// update_progress. The new version runs after update_restart.
#[tauri::command]
pub async fn update_install() -> Result<(), String> {
    let update = PENDING
        .lock()
        .map_err(|e| e.to_string())?
        .take()
        .ok_or("no update to install")?;
    DONE.store(0, Ordering::SeqCst);
    TOTAL.store(0, Ordering::SeqCst);
    update
        .download_and_install(
            |chunk, total| {
                DONE.fetch_add(chunk as u64, Ordering::SeqCst);
                if let Some(total) = total {
                    TOTAL.store(total, Ordering::SeqCst);
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct Progress {
    done: u64,
    total: u64,
}

#[tauri::command]
pub fn update_progress() -> Progress {
    Progress {
        done: DONE.load(Ordering::SeqCst),
        total: TOTAL.load(Ordering::SeqCst),
    }
}

#[tauri::command]
pub fn update_restart(app: AppHandle) {
    app.restart();
}
