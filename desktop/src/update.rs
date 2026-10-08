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

/// The running app's version (set from the release tag when it is built).
#[tauri::command]
pub fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Opens one of pdit's own pages on GitHub (the About panel's links) in the
/// system browser. Only these fixed addresses: the web side names a page, not
/// a URL.
#[tauri::command]
pub fn open_project_page(page: String) -> Result<(), String> {
    let url = match page.as_str() {
        "source" => "https://github.com/litoosh13/pdit",
        "licence" => "https://github.com/litoosh13/pdit/blob/main/LICENSE",
        "notices" => "https://github.com/litoosh13/pdit/blob/main/THIRD_PARTY_NOTICES.md",
        _ => return Err("unknown page".into()),
    };
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    command
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
