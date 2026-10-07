//! Asking questions (D-056): leafmind-qa's models, downloaded once (after the
//! user agrees in the Ask panel) into the app's data folder — only model files,
//! from fixed addresses with SHA-256 checks (as leafmind's
//! scripts/fetch-qa-models.sh pins them) — then the engine reads the open
//! document and picks the sentence that answers. Uses the system's curl, tar
//! and shasum (macOS has them); nothing else goes online.

use leafmind_qa::{Answer, Document, QaEngine, QaModels, QaOptions};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager};

/// (folder, file name, url, size, sha256).
const FILES: [(&str, &str, &str, u64, &str); 5] = [
    (
        "gte-embed",
        "model_int8.onnx",
        "https://huggingface.co/onnx-community/gte-multilingual-base/resolve/2edbf5e672aab465f9ed4c154a8b61791c082c69/onnx/model_int8.onnx",
        340_318_797,
        "ab2bd164ebd8ca9003dc49a981b611e849b5d326f504c8873ba76e07fa6c0082",
    ),
    (
        "gte-embed",
        "tokenizer.json",
        "https://huggingface.co/onnx-community/gte-multilingual-base/resolve/2edbf5e672aab465f9ed4c154a8b61791c082c69/tokenizer.json",
        17_082_734,
        "3a56def25aa40facc030ea8b0b87f3688e4b3c39eb8b45d5702b3a1300fe2a20",
    ),
    (
        "gte-reranker",
        "model_int8.onnx",
        "https://huggingface.co/onnx-community/gte-multilingual-reranker-base/resolve/ee64367e35a2db0da46bb6497e13a18f8bd585cb/onnx/model_int8.onnx",
        340_858_200,
        "ccf51dba7f8aa9205753761cfaa68c55f741792501463a3bf25d7e5bcdac7c35",
    ),
    (
        "gte-reranker",
        "tokenizer.json",
        "https://huggingface.co/onnx-community/gte-multilingual-reranker-base/resolve/ee64367e35a2db0da46bb6497e13a18f8bd585cb/tokenizer.json",
        17_082_999,
        "3ffb37461c391f096759f4a9bbbc329da0f36952f88bab061fcf84940c022e98",
    ),
    (
        "onnxruntime",
        "onnxruntime-osx-arm64-1.30.0.tgz",
        "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-osx-arm64-1.30.0.tgz",
        42_373_116,
        "6ebb5062a934537c352937821f9fe9718e7de1a2db1122a93dd363ffd53a7012",
    ),
];
/// The ONNX Runtime library inside the unpacked archive.
const ORT_LIB: &str = "onnxruntime/onnxruntime-osx-arm64-1.30.0/lib/libonnxruntime.1.30.0.dylib";

/// ONNX Runtime 1.30 is published for Apple-silicon Macs only.
fn supported() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("qa"))
}

fn ready(dir: &Path) -> bool {
    FILES
        .iter()
        .filter(|f| f.0 != "onnxruntime")
        .all(|(folder, name, ..)| dir.join(folder).join(name).is_file())
        && dir.join(ORT_LIB).is_file()
}

static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
static DONE: AtomicU64 = AtomicU64::new(0);
static ERROR: Mutex<Option<String>> = Mutex::new(None);

#[derive(serde::Serialize)]
pub struct Status {
    supported: bool,
    ready: bool,
    downloading: bool,
    done: u64,
    total: u64,
    error: Option<String>,
}

#[tauri::command]
pub fn qa_status(app: AppHandle) -> Result<Status, String> {
    Ok(Status {
        supported: supported(),
        ready: ready(&dir(&app)?),
        downloading: DOWNLOADING.load(Ordering::SeqCst),
        done: DONE.load(Ordering::SeqCst),
        total: FILES.iter().map(|f| f.3).sum(),
        error: ERROR.lock().ok().and_then(|e| e.clone()),
    })
}

/// Starts the download (the user clicked Download); progress via qa_status.
#[tauri::command]
pub fn qa_download(app: AppHandle) -> Result<(), String> {
    if !supported() {
        return Err("asking questions needs an Apple-silicon Mac".into());
    }
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let dir = dir(&app)?;
    CANCEL.store(false, Ordering::SeqCst);
    DONE.store(0, Ordering::SeqCst);
    if let Ok(mut e) = ERROR.lock() {
        *e = None;
    }
    std::thread::spawn(move || {
        let result = download_all(&dir);
        if let (Err(error), Ok(mut e)) = (result, ERROR.lock()) {
            *e = Some(error);
        }
        DOWNLOADING.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
pub fn qa_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn sha256(path: &Path) -> Option<String> {
    let out = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .ok()?;
    String::from_utf8(out.stdout)
        .ok()?
        .split_whitespace()
        .next()
        .map(str::to_owned)
}

fn download_all(dir: &Path) -> Result<(), String> {
    let mut finished = 0;
    for (folder, name, url, size, sum) in FILES {
        let folder = dir.join(folder);
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let out = folder.join(name);
        if out.is_file() && sha256(&out).as_deref() == Some(sum) {
            finished += size;
            DONE.store(finished, Ordering::SeqCst);
            continue;
        }
        let part = folder.join(format!("{name}.part"));
        let mut curl = Command::new("/usr/bin/curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .arg(&part)
            .arg(url)
            .spawn()
            .map_err(|e| format!("could not start the download: {e}"))?;
        let status = loop {
            if CANCEL.load(Ordering::SeqCst) {
                let _ = curl.kill();
                let _ = std::fs::remove_file(&part);
                return Err("cancelled".into());
            }
            let now = std::fs::metadata(&part).map_or(0, |m| m.len());
            DONE.store(finished + now, Ordering::SeqCst);
            if let Some(status) = curl.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        };
        if !status.success() {
            let _ = std::fs::remove_file(&part);
            return Err(format!("could not download {name}"));
        }
        if sha256(&part).as_deref() != Some(sum) {
            let _ = std::fs::remove_file(&part);
            return Err(format!("{name} did not match its checksum"));
        }
        std::fs::rename(&part, &out).map_err(|e| e.to_string())?;
        if name.ends_with(".tgz") {
            let unpacked = Command::new("/usr/bin/tar")
                .arg("xzf")
                .arg(&out)
                .arg("-C")
                .arg(&folder)
                .status()
                .map_err(|e| e.to_string())?;
            if !unpacked.success() {
                return Err("could not unpack ONNX Runtime".into());
            }
        }
        finished += size;
        DONE.store(finished, Ordering::SeqCst);
    }
    Ok(())
}

static ENGINE: OnceLock<Result<QaEngine, String>> = OnceLock::new();
static DOC: Mutex<Option<Document>> = Mutex::new(None);

fn engine(app: &AppHandle) -> Result<&'static QaEngine, String> {
    let dir = dir(app)?;
    if !ready(&dir) {
        return Err("the question models are not downloaded".into());
    }
    ENGINE
        .get_or_init(|| {
            QaEngine::load(
                &QaModels {
                    onnxruntime: dir.join(ORT_LIB),
                    embedder: dir.join("gte-embed"),
                    reranker: dir.join("gte-reranker"),
                    accurate_reranker: None,
                },
                QaOptions::default(),
            )
            .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Reads the open document (its PDF bytes as the raw body) for questions.
#[tauri::command]
pub async fn qa_index(app: AppHandle, request: Request<'_>) -> Result<Option<String>, String> {
    let InvokeBody::Raw(pdf) = request.body() else {
        return Err("expected the PDF as raw bytes".into());
    };
    let engine = engine(&app)?;
    // The index is kept per PDF (D-060, leafmind 0.3 save/load), so a PDF read
    // before isn't read again; a kept index from other models is refused and
    // the PDF read again.
    let kept = app.path().app_data_dir().ok().map(|d| d.join("qa-index"));
    let file = kept.as_ref().map(|d| {
        let hash: String = Sha256::digest(pdf)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        d.join(format!("{hash}.lmqa"))
    });
    let loaded = file
        .as_ref()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|bytes| engine.load_document(&bytes).ok());
    let doc = match loaded {
        Some(doc) => doc,
        None => {
            let doc = engine.index_pdf(pdf).map_err(|e| e.to_string())?;
            if let (Some(dir), Some(file)) = (&kept, &file)
                && std::fs::create_dir_all(dir).is_ok()
                && std::fs::write(file, engine.save_document(&doc)).is_ok()
            {
                crate::cache::keep_newest(dir);
            }
            doc
        }
    };
    let language = doc.language().map(|l| format!("{l:?}"));
    *DOC.lock().map_err(|e| e.to_string())? = Some(doc);
    Ok(language)
}

#[derive(serde::Serialize)]
pub struct Sentence {
    page: u32,
    text: String,
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Found {
        sentences: Vec<Sentence>,
        confidence: f64,
    },
    NotFound,
    WrongLanguage {
        document: String,
    },
}

#[tauri::command]
pub async fn qa_ask(app: AppHandle, question: String) -> Result<Reply, String> {
    let engine = engine(&app)?;
    let doc = DOC.lock().map_err(|e| e.to_string())?;
    let doc = doc.as_ref().ok_or("the document has not been read yet")?;
    Ok(
        match engine.ask(doc, &question).map_err(|e| e.to_string())? {
            Answer::Found {
                sentences,
                confidence,
            } => Reply::Found {
                sentences: sentences
                    .into_iter()
                    .map(|s| Sentence {
                        page: s.page,
                        text: s.text,
                    })
                    .collect(),
                confidence,
            },
            Answer::NotFound { .. } => Reply::NotFound,
            Answer::WrongLanguage { document, .. } => Reply::WrongLanguage {
                document: format!("{document:?}"),
            },
        },
    )
}
