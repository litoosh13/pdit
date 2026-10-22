//! Asking questions (D-056): leafmind-qa's models and ONNX Runtime for this system (Apple-silicon Macs, Windows
//! x64, Linux x64; P-050 #6), downloaded once (after the user agrees in the Ask panel) into the app's data folder —
//! only these files, from fixed addresses with SHA-256 checks (as leafmind's scripts/fetch-qa-models.sh pins the
//! models) — then the engine reads the open document and picks the sentence that answers. Downloads with the
//! system's curl (Linux: or wget) and unpacks with its tar (Windows 10+ ships both); nothing else goes online.

use leafmind_qa::{Answer, Document, QaEngine, QaModels, QaOptions};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager};

/// (folder, file name, url, size, sha256). The question models are leafmind v0.5.0's vocabulary-pruned gte models
/// (English, German, Persian, Arabic; scripts/fetch-qa-models.sh there), then ONNX Runtime ([`ORT`]).
type File = (&'static str, &'static str, &'static str, u64, &'static str);
const FILES: [File; 5] = [
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
    ORT.0,
];

/// ONNX Runtime 1.30.0 for this system (Microsoft's release; sizes and SHA-256 as GitHub lists them), and its
/// library inside the unpacked archive. There is no Intel-Mac build of 1.30.
#[cfg(target_os = "windows")]
const ORT: (File, &str) = (
    (
        "onnxruntime",
        "onnxruntime-win-x64-1.30.0.zip",
        "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-1.30.0.zip",
        82_645_522,
        "c6ba983baf5681af108599675d2a89c2d145512d02de28aed0bff177cd0ba949",
    ),
    "onnxruntime/onnxruntime-win-x64-1.30.0/lib/onnxruntime.dll",
);
#[cfg(target_os = "linux")]
const ORT: (File, &str) = (
    (
        "onnxruntime",
        "onnxruntime-linux-x64-1.30.0.tgz",
        "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-x64-1.30.0.tgz",
        11_306_877,
        "a5ed5a3cac51fbb2e90da632ae43d19212faaa20e76484e62bcb7c23ddb3b3fd",
    ),
    "onnxruntime/onnxruntime-linux-x64-1.30.0/lib/libonnxruntime.so.1.30.0",
);
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const ORT: (File, &str) = (
    (
        "onnxruntime",
        "onnxruntime-osx-arm64-1.30.0.tgz",
        "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-osx-arm64-1.30.0.tgz",
        42_373_116,
        "6ebb5062a934537c352937821f9fe9718e7de1a2db1122a93dd363ffd53a7012",
    ),
    "onnxruntime/onnxruntime-osx-arm64-1.30.0/lib/libonnxruntime.1.30.0.dylib",
);
const ORT_LIB: &str = ORT.1;

/// Systems ONNX Runtime 1.30 is published for (and pdit is released for): not Intel Macs.
fn supported() -> bool {
    cfg!(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(
            any(target_os = "windows", target_os = "linux"),
            target_arch = "x86_64"
        ),
    ))
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
        return Err("asking questions doesn't work on this computer yet".into());
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
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        match file.read(&mut buffer).ok()? {
            0 => break,
            n => hasher.update(&buffer[..n]),
        }
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

/// A system program by its full path where it has a fixed one (macOS, Windows), not whatever is first on PATH.
fn system_tool(name: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        return Path::new("/usr/bin").join(name);
    }
    if cfg!(target_os = "windows") {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        return Path::new(&root)
            .join("System32")
            .join(format!("{name}.exe"));
    }
    PathBuf::from(name)
}

/// Starts downloading `url` to `out`: curl, or on Linux wget when curl isn't installed.
fn start_download(url: &str, out: &Path) -> std::io::Result<std::process::Child> {
    let curl = Command::new(system_tool("curl"))
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(out)
        .arg(url)
        .spawn();
    match curl {
        Err(e) if cfg!(target_os = "linux") && e.kind() == std::io::ErrorKind::NotFound => {
            Command::new("wget")
                .args(["-q", "-O"])
                .arg(out)
                .arg(url)
                .spawn()
        }
        other => other,
    }
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
        let mut curl =
            start_download(url, &part).map_err(|e| format!("could not start the download: {e}"))?;
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
        if name.ends_with(".tgz") || name.ends_with(".zip") {
            // tar reads both (Windows' tar is bsdtar).
            let unpacked = Command::new(system_tool("tar"))
                .arg("-xf")
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
    let InvokeBody::Raw(body) = request.body() else {
        return Err("expected the PDF as raw bytes".into());
    };
    // With x-ocr (its length): the body starts with the OCR text of scanned pages as JSON [[page, text], …]
    // (pages from 1), then the PDF. leafmind's PDF reader skips invisible text, so a scan's OCR layer isn't read.
    let ocr_len = request
        .headers()
        .get("x-ocr")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(body.len());
    let ocr: Vec<(u32, String)> = match ocr_len {
        0 => Vec::new(),
        n => serde_json::from_slice(&body[..n]).map_err(|e| e.to_string())?,
    };
    let pdf = &body[ocr_len..];
    let engine = engine(&app)?;
    // The index is kept per PDF (D-060, leafmind 0.3 save/load), so a PDF read
    // before isn't read again; a kept index from other models is refused and
    // the PDF read again.
    let kept = app.path().app_data_dir().ok().map(|d| d.join("qa-index"));
    let file = kept.as_ref().map(|d| {
        let hash: String = Sha256::digest(body)
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
            let doc = match ocr.is_empty() {
                true => engine.index_pdf(pdf),
                false => {
                    let mut pages = leafmind_qa::text::pdf_pages(pdf).map_err(|e| e.to_string())?;
                    pages.retain(|(n, _)| !ocr.iter().any(|(o, _)| o == n));
                    pages.extend(ocr);
                    pages.sort_by_key(|(n, _)| *n);
                    let pages: Vec<(u32, &str)> =
                        pages.iter().map(|(n, t)| (*n, t.as_str())).collect();
                    engine.index_pages(&pages)
                }
            }
            .map_err(|e| e.to_string())?;
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
