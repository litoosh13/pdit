//! Filling a form from a sentence (D-079): leafmind-fill proposes changes to the form's fields ("my phone number
//! is …", "tick I agree"); the web app shows them and writes only after the user confirms. The model and Needle's
//! engine come with the question models (qa.rs). leafmind writes nothing and nothing goes online.

use crate::qa::{NEEDLE_LIB, ORT_LIB, dir, ready};
use leafmind_fill::{
    Change, FieldKind, FillEngine, FillModels, Form, FormField, PageWords, Proposal, Word,
};
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;
use tauri::ipc::{InvokeBody, Request};

static ENGINE: OnceLock<Result<FillEngine, String>> = OnceLock::new();
static FORM: Mutex<Option<Form>> = Mutex::new(None);

fn engine(app: &AppHandle) -> Result<&'static FillEngine, String> {
    let dir = dir(app)?;
    if !ready(&dir) {
        return Err("the AI models are not downloaded".into());
    }
    ENGINE
        .get_or_init(|| {
            FillEngine::load(&FillModels {
                onnxruntime: dir.join(ORT_LIB),
                embedder: dir.join("gte-embed"),
                needle_library: dir.join(NEEDLE_LIB),
                needle_model: dir.join("fill").join("fill.cact"),
            })
            .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// The form as the web app sends it: boxes `[left, top, right, bottom]`, y downward, in points.
#[derive(serde::Deserialize)]
struct FormIn {
    fields: Vec<FieldIn>,
    pages: Vec<PageIn>,
}

#[derive(serde::Deserialize)]
struct FieldIn {
    id: u32,
    page: u32,
    bounds: [f32; 4],
    /// "text", "checkbox" or "choice".
    kind: String,
    #[serde(default)]
    options: Vec<String>,
    tooltip: Option<String>,
    #[serde(default)]
    read_only: bool,
}

#[derive(serde::Deserialize)]
struct PageIn {
    page: u32,
    words: Vec<(String, [f32; 4])>,
}

/// Reads the open document's fields and words (JSON as the raw body; once per document, again after changes).
#[tauri::command]
pub async fn fill_prepare(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let InvokeBody::Raw(body) = request.body() else {
        return Err("expected the form as raw bytes".into());
    };
    let form: FormIn = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    let fields: Vec<FormField> = form
        .fields
        .into_iter()
        .map(|f| FormField {
            id: f.id,
            page: f.page,
            bounds: f.bounds,
            kind: match f.kind.as_str() {
                "checkbox" => FieldKind::Checkbox,
                "choice" => FieldKind::Choice { options: f.options },
                _ => FieldKind::Text,
            },
            tooltip: f.tooltip,
            read_only: f.read_only,
        })
        .collect();
    let pages: Vec<PageWords> = form
        .pages
        .into_iter()
        .map(|p| PageWords {
            page: p.page,
            words: p
                .words
                .into_iter()
                .map(|(text, bounds)| Word { text, bounds })
                .collect(),
        })
        .collect();
    let prepared = engine(&app)?
        .prepare(&fields, &pages)
        .map_err(|e| e.to_string())?;
    *FORM.lock().map_err(|e| e.to_string())? = Some(prepared);
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ActionOut {
    field: u32,
    /// "fill", "check", "choose", "clear" or "copy_from".
    change: &'static str,
    value: Option<String>,
    checked: Option<bool>,
    from: Option<u32>,
    alternatives: Vec<u32>,
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Actions { actions: Vec<ActionOut> },
    NoValue { candidates: Vec<u32> },
    NoSuchField,
    NotSupported { reason: String },
}

/// What `sentence` would change; `today` is today's date as the form should get it.
#[tauri::command]
pub async fn fill_propose(
    app: AppHandle,
    sentence: String,
    today: String,
) -> Result<Reply, String> {
    let engine = engine(&app)?;
    let form = FORM.lock().map_err(|e| e.to_string())?;
    let form = form.as_ref().ok_or("the form has not been read yet")?;
    Ok(
        match engine
            .propose(form, &sentence, &today)
            .map_err(|e| e.to_string())?
        {
            Proposal::Actions(actions) => Reply::Actions {
                actions: actions
                    .into_iter()
                    .map(|a| {
                        let (change, value, checked, from) = match a.change {
                            Change::Fill(v) => ("fill", Some(v), None, None),
                            Change::Check(c) => ("check", None, Some(c), None),
                            Change::Choose(o) => ("choose", Some(o), None, None),
                            Change::Clear => ("clear", None, None, None),
                            Change::CopyFrom(f) => ("copy_from", None, None, Some(f)),
                        };
                        ActionOut {
                            field: a.field,
                            change,
                            value,
                            checked,
                            from,
                            alternatives: a.alternatives,
                        }
                    })
                    .collect(),
            },
            Proposal::NoValue { candidates } => Reply::NoValue { candidates },
            Proposal::NoSuchField => Reply::NoSuchField,
            Proposal::NotSupported { reason } => Reply::NotSupported {
                reason: reason.into(),
            },
        },
    )
}
