//! pdit's desktop app (D-032, D-046): the unchanged pdit web build in a Tauri
//! window. PDFs stay on this computer; nothing is uploaded (D-002). Saving:
//! the macOS/Linux webview (WebKit) has no showSaveFilePicker, so pdit's Save
//! falls back to a browser download, which is turned into a native Save
//! dialog here. OCR (D-055): the web app sends scanned pages to `ocr_language` /
//! `ocr_read`, which run leafmind's Tesseract reader on this computer.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod qa;

use leafmind_ocr::{OcrEngine, OcrLanguage, OcrModels};
use std::sync::OnceLock;
use tauri::ipc::{InvokeBody, Request};
use tauri::webview::DownloadEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Tesseract, loaded on the first OCR call from the app's bundled files.
static OCR: OnceLock<Result<OcrEngine, String>> = OnceLock::new();

fn ocr(app: &AppHandle) -> Result<&'static OcrEngine, String> {
    OCR.get_or_init(|| {
        let dir = app
            .path()
            .resource_dir()
            .map_err(|e| e.to_string())?
            .join("ocr");
        OcrEngine::load(&OcrModels {
            tesseract: dir.join("lib/libtesseract.5.dylib"),
            tessdata: dir.join("tessdata"),
        })
        .map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// A page image sent by the web app: RGBA bytes as the raw body, its size and
/// resolution in the x-width / x-height / x-dpi headers.
fn page_image<'a>(request: &'a Request<'_>) -> Result<(&'a [u8], u32, u32, Option<u32>), String> {
    let InvokeBody::Raw(rgba) = request.body() else {
        return Err("expected the page image as raw bytes".into());
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok())
    };
    let (Some(width), Some(height)) = (header("x-width"), header("x-height")) else {
        return Err("missing x-width / x-height".into());
    };
    Ok((rgba, width, height, header("x-dpi")))
}

fn language(code: &str) -> Option<OcrLanguage> {
    [
        OcrLanguage::English,
        OcrLanguage::German,
        OcrLanguage::Persian,
        OcrLanguage::Arabic,
    ]
    .into_iter()
    .find(|l| l.code() == code)
}

/// Debug builds: the web app's log lines, printed here (its console can't be
/// read from outside the window).
#[cfg(debug_assertions)]
#[tauri::command]
fn debug_log(message: String) {
    println!("{message}");
}

/// The language of a scanned page ("eng", "deu", "fas", "ara"), if Tesseract can tell.
#[tauri::command]
async fn ocr_language(app: AppHandle, request: Request<'_>) -> Result<Option<String>, String> {
    let (rgba, width, height, dpi) = page_image(&request)?;
    let found = ocr(&app)?
        .detect_language(rgba, width, height, dpi)
        .map_err(|e| e.to_string())?;
    Ok(found.map(|l| l.code().to_owned()))
}

#[derive(serde::Serialize)]
struct Word {
    text: String,
    /// Left, top, right, bottom in pixels of the image sent.
    bounds: [f32; 4],
    confidence: f32,
    line: u32,
}

#[derive(serde::Serialize)]
struct Page {
    confidence: f32,
    words: Vec<Word>,
}

/// The words of a scanned page in the language of the x-lang header.
#[tauri::command]
async fn ocr_read(app: AppHandle, request: Request<'_>) -> Result<Page, String> {
    let (rgba, width, height, dpi) = page_image(&request)?;
    let lang = request
        .headers()
        .get("x-lang")
        .and_then(|v| v.to_str().ok())
        .and_then(language)
        .unwrap_or(OcrLanguage::English);
    let page = ocr(&app)?
        .read(rgba, width, height, &[lang], dpi)
        .map_err(|e| e.to_string())?;
    Ok(Page {
        confidence: page.confidence,
        words: page
            .words
            .into_iter()
            .map(|w| Word {
                text: w.text,
                bounds: w.bounds,
                confidence: w.confidence,
                line: w.line,
            })
            .collect(),
    })
}

#[cfg(debug_assertions)]
fn handlers() -> impl Fn(tauri::ipc::Invoke) -> bool {
    tauri::generate_handler![
        ocr_language,
        ocr_read,
        qa::qa_status,
        qa::qa_download,
        qa::qa_cancel,
        qa::qa_index,
        qa::qa_ask,
        debug_log
    ]
}

#[cfg(not(debug_assertions))]
fn handlers() -> impl Fn(tauri::ipc::Invoke) -> bool {
    tauri::generate_handler![
        ocr_language,
        ocr_read,
        qa::qa_status,
        qa::qa_download,
        qa::qa_cancel,
        qa::qa_index,
        qa::qa_ask
    ]
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(handlers())
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("pdit")
                .inner_size(1280.0, 860.0)
                .min_inner_size(640.0, 480.0)
                .on_download(|_webview, event| match event {
                    DownloadEvent::Requested { destination, .. } => {
                        let name = destination
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "document.pdf".into());
                        match rfd::FileDialog::new()
                            .set_file_name(&name)
                            .add_filter("PDF", &["pdf"])
                            .save_file()
                        {
                            Some(path) => {
                                *destination = path;
                                true
                            }
                            None => false, // cancelled
                        }
                    }
                    _ => true,
                })
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("pdit could not start");
}
