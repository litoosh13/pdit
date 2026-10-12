//! Saving several files at once (Split into files, D-070): the user picks a folder once, then each file is
//! written into that folder only — a bare file name of a type pdit makes, never over an existing file.

use percent_encoding::percent_decode_str;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::ipc::{InvokeBody, Request};

/// The file types pdit writes into a folder (Split, Export as).
const TYPES: [&str; 7] = ["pdf", "png", "jpg", "jpeg", "txt", "docx", "odt"];

/// The folder the user picked last; files are only written there.
static FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Opens the system folder picker; returns the folder's name, or None when cancelled.
#[tauri::command]
pub fn pick_folder() -> Option<String> {
    let folder = rfd::FileDialog::new().pick_folder()?;
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned());
    *FOLDER.lock().ok()? = Some(folder);
    name.or(Some(String::new()))
}

/// Body: the file; header x-name: its percent-encoded file name. Writes it into the picked folder (as
/// "name (2).png" when "name.png" exists) and returns the name used.
#[tauri::command]
pub fn save_into_folder(request: Request<'_>) -> Result<String, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected the file as raw bytes".into());
    };
    let name = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| percent_decode_str(v).decode_utf8().ok())
        .ok_or("missing x-name")?;
    // Only a plain file name: no folders, nothing hidden.
    let name = std::path::Path::new(name.as_ref())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| {
            let ext = n.rsplit_once('.').map(|(_, e)| e.to_lowercase());
            !n.starts_with('.') && ext.is_some_and(|e| TYPES.contains(&e.as_str()))
        })
        .ok_or("bad file name")?;
    let folder = FOLDER
        .lock()
        .map_err(|_| "busy")?
        .clone()
        .ok_or("no folder picked")?;
    let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, "pdf"));
    let path = (1..1000)
        .map(|k| folder.join(if k == 1 { name.clone() } else { format!("{stem} ({k}).{ext}") }))
        .find(|p| !p.exists())
        .ok_or("too many files with that name")?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}
