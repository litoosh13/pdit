//! The analysis records (D-060): one small JSON file per PDF in the app's data
//! folder (`analysis/<sha256>.json`), written and read by the web app
//! (pdit-app/src/analysis_cache.rs). Only the newest KEEP files stay.

use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const KEEP: usize = 300;

/// The record's file; `None` unless `hash` is a SHA-256 in hex (the key comes
/// from the web side, so it must not be able to name any other file).
fn path(app: &AppHandle, hash: &str) -> Option<PathBuf> {
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(
        app.path()
            .app_data_dir()
            .ok()?
            .join("analysis")
            .join(format!("{hash}.json")),
    )
}

#[tauri::command]
pub fn analysis_get(app: AppHandle, hash: String) -> Option<String> {
    std::fs::read_to_string(path(&app, &hash)?).ok()
}

#[tauri::command]
pub fn analysis_put(app: AppHandle, hash: String, json: String) -> Result<(), String> {
    let file = path(&app, &hash).ok_or("not a file hash")?;
    let dir = file.parent().ok_or("no folder")?.to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&file, json).map_err(|e| e.to_string())?;
    // Oldest first out, past KEEP records.
    let mut all: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if all.len() > KEEP {
        all.sort();
        for (_, old) in &all[..all.len() - KEEP] {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(())
}
