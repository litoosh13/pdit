//! Saving several files at once (Split into files, D-070): the user picks a folder once, then each file is
//! written into that folder only — a bare file name of a type pdit makes, never over an existing file.
//! Also the open document's place on disk: a PDF dropped onto the window (P-050 #1) is opened from its path.

use percent_encoding::percent_decode_str;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::ipc::{InvokeBody, Request};

/// The file types pdit writes into a folder (Split, Export as).
const TYPES: [&str; 7] = ["pdf", "png", "jpg", "jpeg", "txt", "docx", "odt"];

/// The folder the user picked last; files are only written there.
static FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The PDF last dropped onto the window, waiting for the app to take it.
static DROPPED: Mutex<Option<PathBuf>> = Mutex::new(None);
/// The open document's file (dropped onto the window).
static OPENED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// A file dropped onto the window: kept when it is a PDF; true then.
pub fn dropped(path: &std::path::Path) -> bool {
    let pdf = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    if pdf && let Ok(mut d) = DROPPED.lock() {
        *d = Some(path.to_path_buf());
    }
    pdf
}

/// The dropped PDF becomes the open document; returns its file name, or None when nothing waits.
#[tauri::command]
pub fn take_dropped() -> Option<String> {
    let path = DROPPED.lock().ok()?.take()?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    *OPENED.lock().ok()? = Some(path);
    Some(name)
}

/// Open (P-050 #10): the system's open window for a PDF; it becomes the open document. Returns its file name, or
/// None when cancelled.
#[tauri::command]
pub fn open_pdf() -> Option<String> {
    let path = rfd::FileDialog::new()
        .add_filter("PDF", &["pdf"])
        .pick_file()?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    *OPENED.lock().ok()? = Some(path);
    Some(name)
}

/// A new document has no file yet: its first Save asks where.
#[tauri::command]
pub fn forget_opened() {
    if let Ok(mut opened) = OPENED.lock() {
        *opened = None;
    }
}

/// Save (P-050 #10). Body: the PDF; header x-name: its percent-encoded name (suggested when asking). Writes it over
/// the open document's file (a new file next to it first, then swapped in, so a failed write leaves the old one);
/// a new document asks where first. Returns the file name, or None when that was cancelled.
#[tauri::command]
pub fn save_opened(request: Request<'_>) -> Result<Option<String>, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected the PDF as raw bytes".into());
    };
    let mut opened = OPENED.lock().map_err(|_| "busy")?;
    let path = match opened.clone() {
        Some(path) => path,
        None => {
            let name = request
                .headers()
                .get("x-name")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| percent_decode_str(v).decode_utf8().ok())
                .map(|n| n.into_owned())
                .unwrap_or_else(|| "document.pdf".into());
            let Some(path) = rfd::FileDialog::new()
                .set_file_name(&name)
                .add_filter("PDF", &["pdf"])
                .save_file()
            else {
                return Ok(None);
            };
            path
        }
    };
    write_over(&path, bytes).map_err(|e| e.to_string())?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    *opened = Some(path);
    Ok(name)
}

/// Writes `bytes` to `path` through a new file next to it, then swaps it in: a failed write leaves the old file.
fn write_over(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let part = path.with_extension("pdf.pdit-saving");
    std::fs::write(&part, bytes)?;
    std::fs::rename(&part, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })
}

/// The open document's bytes, read from its file.
#[tauri::command]
pub fn read_opened() -> Result<tauri::ipc::Response, String> {
    let path = OPENED.lock().map_err(|_| "busy")?.clone().ok_or("no file")?;
    std::fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|e| e.to_string())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_replaces_the_file_and_leaves_nothing_else() {
        let dir = std::env::temp_dir().join(format!("pdit-save-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("letter.pdf");
        std::fs::write(&path, b"old").unwrap();
        write_over(&path, b"new bytes").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new bytes");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["letter.pdf"], "no temporary file left");
        // A folder that can't be written: the old file stays as it was.
        assert!(write_over(&dir.join("missing").join("x.pdf"), b"new").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"new bytes");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

