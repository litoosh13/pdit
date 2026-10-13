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

/// (folder, file name, url, size, sha256). The question models are leafmind v0.5.0's vocabulary-pruned gte models
/// (English, German, Persian, Arabic; scripts/fetch-qa-models.sh there).
const FILES: [(&str, &str, &str, u64, &str); 5] = [
    (
        "gte-embed",
        "model_int8.onnx",
        "https://huggingface.co/litoo13/leafmind-gte-multilingual-4lang/resolve/466fd26d9e751de3a57adc3ae222115865ca8cc4/gte-embed/model_int8.onnx",
        171_260_482,
        "5668afdae8b748de54ef85b7a13a711005ccf2a7a08806159f78b30c5f722cbf",
    ),
    (
        "gte-embed",
        "tokenizer.json",
        "https://huggingface.co/litoo13/leafmind-gte-multilingual-4lang/resolve/466fd26d9e751de3a57adc3ae222115865ca8cc4/gte-embed/tokenizer.json",
        2_788_722,
        "c1bafc0e9c74e6cefcd032df267ce8603c7a288a771e517d9c8823e0a7de8a37",
    ),
    (
        "gte-reranker",
        "model_int8.onnx",
        "https://huggingface.co/litoo13/leafmind-gte-multilingual-4lang/resolve/466fd26d9e751de3a57adc3ae222115865ca8cc4/gte-reranker/model_int8.onnx",
        171_799_885,
        "4e58a5fd59990fd78afc8a509f27d4f799d8b8723ceea00e0a07fd6f316fb542",
    ),
    (
        "gte-reranker",
        "tokenizer.json",
        "https://huggingface.co/litoo13/leafmind-gte-multilingual-4lang/resolve/466fd26d9e751de3a57adc3ae222115865ca8cc4/gte-reranker/tokenizer.json",
        2_788_927,
        "e26ac9905f59291e2540981b82ca2e1022e9699b2456324f66ddba4804272847",
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

/// The models are there at their expected size (older models, e.g. before leafmind v0.5.0, differ in size and are
/// downloaded again).
fn ready(dir: &Path) -> bool {
    FILES
        .iter()
        .filter(|f| f.0 != "onnxruntime")
        .all(|(folder, name, _, size, _)| {
            std::fs::metadata(dir.join(folder).join(name)).is_ok_and(|m| m.len() == *size)
        })
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
