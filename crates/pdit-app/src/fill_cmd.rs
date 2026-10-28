//! Filling a form from a sentence (D-079), the app's side: the open document's fields and words go to leafmind
//! (desktop/src/fill.rs), its proposal comes back, the chat shows it (ask_ui.rs) and pdit writes only after the
//! user confirms — one Undo step. Radio buttons with one name are one field, their choices its options. Online
//! (D-080), leafmind runs in the browser instead, through assets/ai-fill/ai-fill.js ([`online`]).

use crate::ai_ui::desktop;
use dioxus::prelude::*;
use js_sys::{Array, Object, Reflect};
use pdit_core::FieldKind;
use wasm_bindgen::prelude::*;

/// How a field is written.
#[derive(Clone, Debug, PartialEq)]
enum Target {
    Text(u16, usize),
    Check(u16, usize),
    /// A dropdown or list: its options.
    Choice(u16, usize, Vec<String>),
    /// A radio group: (page, annotation, choice) per button.
    Radio(Vec<(u16, usize, String)>),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Field {
    target: Target,
    /// The words that name it on the page, for the chat.
    pub label: String,
    /// From 1.
    pub page: u32,
}

/// The document's fields, by leafmind's id (their position here).
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Fields(pub Vec<Field>);

/// One proposed change.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Change {
    Fill(String),
    Check(bool),
    Choose(String),
    Clear,
    CopyFrom(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Action {
    pub field: usize,
    pub change: Change,
    pub alternatives: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Proposal {
    Actions(Vec<Action>),
    NoValue(Vec<usize>),
    NoSuchField,
    NotSupported(String),
}

/// A question rather than a command: leafmind-qa answers it.
pub(crate) fn is_question(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    lower.ends_with('?')
        || [
            "what ", "who ", "whom ", "whose ", "where ", "when ", "which ", "why ", "how ", "is ",
            "are ", "does ", "do ", "did ", "can ", "was ", "were ", "has ", "have ",
        ]
        .iter()
        .any(|w| lower.starts_with(w))
}

/// The open document has form fields (quick: no words read).
pub(crate) fn has_fields() -> bool {
    let pages = pdit_core::page_ops::page_sizes().map_or(0, |s| s.len());
    (0..pages as u16).any(|p| !pdit_core::form_fields(p).unwrap_or_default().is_empty())
}

/// The open document's fillable fields and the JSON leafmind reads (boxes y downward, pages from 1); `None`
/// without such fields.
pub(crate) fn read_form() -> Option<(Fields, Vec<u8>)> {
    let sizes = pdit_core::page_ops::page_sizes().ok()?;
    let mut fields: Vec<Field> = Vec::new();
    let mut json_fields = Vec::new();
    let mut json_pages = Vec::new();
    let mut radios: Vec<(String, usize)> = Vec::new();
    for (page, (_, height)) in sizes.iter().enumerate() {
        let page = page as u16;
        let flip = |[l, b, r, t]: [f32; 4]| [l, height - t, r, height - b];
        let words = pdit_core::annotations::words(page).unwrap_or_default();
        let found = pdit_core::form_fields(page).unwrap_or_default();
        if found.is_empty() {
            continue;
        }
        for f in found {
            let (l, b, r, t) = f.rect;
            let label = label(&words, [l, b, r, t], &f);
            let target = match f.kind {
                FieldKind::Text => Target::Text(page, f.annotation),
                FieldKind::Checkbox => Target::Check(page, f.annotation),
                FieldKind::ComboBox | FieldKind::ListBox => {
                    Target::Choice(page, f.annotation, f.options.clone())
                }
                FieldKind::Radio => {
                    let name = f.name.clone().unwrap_or_default();
                    let choice = f.choice.clone().unwrap_or_default();
                    if let Some((_, id)) = radios.iter().find(|(n, _)| *n == name) {
                        if let Target::Radio(buttons) = &mut fields[*id].target {
                            buttons.push((page, f.annotation, choice));
                        }
                        continue;
                    }
                    radios.push((name, fields.len()));
                    Target::Radio(vec![(page, f.annotation, choice)])
                }
                _ => continue,
            };
            json_fields.push(serde_json::json!({
                "id": fields.len(),
                "page": page + 1,
                "bounds": flip([l, b, r, t]),
                "kind": match target { Target::Text(..) => "text", Target::Check(..) => "checkbox", _ => "choice" },
                "options": f.options,
                "tooltip": serde_json::Value::Null,
                "read_only": f.read_only,
            }));
            fields.push(Field {
                target,
                label,
                page: u32::from(page) + 1,
            });
        }
        let words: Vec<_> = words
            .iter()
            .map(|w| (w.text.as_str(), flip(w.bounds)))
            .collect();
        json_pages.push(serde_json::json!({ "page": page + 1, "words": words }));
    }
    // A radio group's options: its buttons' choices.
    for (_, id) in &radios {
        if let Target::Radio(buttons) = &fields[*id].target {
            let options: Vec<&str> = buttons.iter().map(|(_, _, c)| c.as_str()).collect();
            json_fields[*id]["options"] = serde_json::json!(options);
        }
    }
    if fields.is_empty() {
        return None;
    }
    let json =
        serde_json::to_vec(&serde_json::json!({ "fields": json_fields, "pages": json_pages }))
            .ok()?;
    Some((Fields(fields), json))
}

/// The words naming a field: on its line to the left (a checkbox: to the right), else just above; else its name.
fn label(
    words: &[pdit_core::annotations::Word],
    [l, b, r, t]: [f32; 4],
    field: &pdit_core::FormField,
) -> String {
    let middle = |w: &pdit_core::annotations::Word| (w.bounds[1] + w.bounds[3]) / 2.0;
    let on_line = |w: &&pdit_core::annotations::Word| middle(w) >= b - 2.0 && middle(w) <= t + 2.0;
    let pick = |mut found: Vec<&pdit_core::annotations::Word>, rightward: bool| {
        found.sort_by(|a, b| a.bounds[0].total_cmp(&b.bounds[0]));
        // The run of words next to the field (a gap of more than 30 pt ends it).
        let mut run: Vec<&pdit_core::annotations::Word> = Vec::new();
        let ordered: Vec<_> = match rightward {
            true => found,
            false => found.into_iter().rev().collect(),
        };
        for w in ordered {
            let gap = match (run.last(), rightward) {
                (None, _) => 0.0,
                (Some(p), true) => w.bounds[0] - p.bounds[2],
                (Some(p), false) => p.bounds[0] - w.bounds[2],
            };
            if gap > 30.0 || run.len() >= 8 {
                break;
            }
            run.push(w);
        }
        if !rightward {
            run.reverse();
        }
        run.iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let left: Vec<_> = words
        .iter()
        .filter(on_line)
        .filter(|w| w.bounds[2] <= l + 2.0 && l - w.bounds[2] < 200.0)
        .collect();
    let right: Vec<_> = words
        .iter()
        .filter(on_line)
        .filter(|w| w.bounds[0] >= r - 2.0 && w.bounds[0] - r < 200.0)
        .collect();
    let above: Vec<_> = words
        .iter()
        .filter(|w| {
            w.bounds[1] >= t - 2.0 && w.bounds[1] - t < 16.0 && w.bounds[0] < r && w.bounds[2] > l
        })
        .collect();
    // A checkbox's words are usually to its right; a text field's to its left or above.
    let tries = match matches!(field.kind, FieldKind::Checkbox | FieldKind::Radio) {
        true => [(right, true), (left, false), (above, true)],
        false => [(left, false), (above, true), (right, true)],
    };
    let text = tries
        .into_iter()
        .map(|(found, rightward)| pick(found, rightward))
        .find(|t| !t.trim().is_empty())
        .unwrap_or_default();
    let text = text.trim().trim_end_matches(':').trim().to_owned();
    let text = match text.is_empty() {
        true => field.name.clone().unwrap_or_else(|| "a field".into()),
        false => text,
    };
    match text.chars().count() > 40 {
        true => text.chars().take(39).collect::<String>() + "…",
        false => text,
    }
}

/// Today's date as the reader's locale writes it (e.g. 10/28/2026, 28.10.2026).
fn today() -> String {
    let navigator = Reflect::get(&js_sys::global(), &"navigator".into()).unwrap_or(JsValue::NULL);
    let locale = Reflect::get(&navigator, &"language".into())
        .ok()
        .and_then(|l| l.as_string())
        .unwrap_or_else(|| "en-US".into());
    js_sys::Date::new_0()
        .to_locale_date_string(&locale, &JsValue::UNDEFINED)
        .into()
}

/// leafmind reads the form (once per version of it; `json` from [`read_form`]).
pub(crate) async fn prepare(json: &[u8]) -> Result<(), String> {
    if !desktop::available() {
        let json = String::from_utf8_lossy(json).into_owned();
        return online("prepare", &[json.into()]).await.map(|_| ());
    }
    desktop::invoke_raw("fill_prepare", json, &[])
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// The online bridge and leafmind's browser package (scripts/fetch-leafmind-web.sh), served side by side in assets/
/// under their own names and unminified: they import each other by name, and dx's minifying breaks leafmind's
/// worker.
const AI_FILL: Asset = asset!(
    "/assets/ai-fill/ai-fill.js",
    AssetOptions::js()
        .with_minify(false)
        .with_hash_suffix(false)
);
const LEAFMIND_FILL: [Asset; 4] = [
    asset!(
        "/assets/ai-fill/leafmind-fill/leafmind-fill-web.js",
        AssetOptions::js()
            .with_minify(false)
            .with_hash_suffix(false)
    ),
    asset!(
        "/assets/ai-fill/leafmind-fill/leafmind-fill-worker.js",
        AssetOptions::js()
            .with_minify(false)
            .with_hash_suffix(false)
    ),
    asset!(
        "/assets/ai-fill/leafmind-fill/leafmind_fill.js",
        AssetOptions::js()
            .with_minify(false)
            .with_hash_suffix(false)
    ),
    asset!(
        "/assets/ai-fill/leafmind-fill/leafmind_fill_bg.wasm",
        AssetOptions::builder().with_hash_suffix(false)
    ),
];

/// Calls `name` of the online version's bridge (ai-fill.js; imported the first time).
pub(crate) async fn online(name: &str, args: &[JsValue]) -> Result<JsValue, String> {
    use wasm_bindgen_futures::JsFuture;
    thread_local!(static MODULE: std::cell::RefCell<Option<JsValue>> = const { std::cell::RefCell::new(None) });
    let error = |e: JsValue| e.as_string().unwrap_or_else(|| format!("{e:?}"));
    let module = match MODULE.with_borrow(Clone::clone) {
        Some(module) => module,
        None => {
            // Kept in the build (an asset nothing reads would be left out).
            std::hint::black_box(LEAFMIND_FILL.map(|a| a.to_string()));
            let import = js_sys::Function::new_with_args(
                "u",
                "return import(new URL(u, document.baseURI).href)",
            );
            let promise: js_sys::Promise = import
                .call1(&JsValue::NULL, &AI_FILL.to_string().into())
                .map_err(error)?
                .unchecked_into();
            let module = JsFuture::from(promise).await.map_err(error)?;
            MODULE.with_borrow_mut(|m| *m = Some(module.clone()));
            module
        }
    };
    let function: js_sys::Function = Reflect::get(&module, &name.into())
        .map_err(error)?
        .unchecked_into();
    let result = function
        .apply(&JsValue::NULL, &args.iter().collect::<Array>())
        .map_err(error)?;
    match result.dyn_into::<js_sys::Promise>() {
        Ok(promise) => JsFuture::from(promise).await.map_err(error),
        Err(value) => Ok(value),
    }
}

/// What `sentence` would change.
pub(crate) async fn propose(sentence: &str) -> Result<Proposal, String> {
    let args = Object::new();
    let _ = Reflect::set(&args, &"sentence".into(), &sentence.into());
    let _ = Reflect::set(&args, &"today".into(), &today().into());
    let reply = match desktop::available() {
        true => desktop::invoke("fill_propose", &args)
            .await
            .map_err(|e| format!("{e:?}"))?,
        false => online("propose", &[sentence.into(), today().into()]).await?,
    };
    let get = |o: &JsValue, k: &str| Reflect::get(o, &k.into()).unwrap_or(JsValue::NULL);
    let ids = |v: JsValue| -> Vec<usize> {
        v.dyn_into::<Array>()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_f64())
                    .map(|x| x as usize)
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(match get(&reply, "kind").as_string().as_deref() {
        Some("actions") => Proposal::Actions(
            get(&reply, "actions")
                .dyn_into::<Array>()
                .map(|a| {
                    a.iter()
                        .filter_map(|a| {
                            let text = || get(&a, "value").as_string().unwrap_or_default();
                            let change = match get(&a, "change").as_string()?.as_str() {
                                "fill" => Change::Fill(text()),
                                "check" => Change::Check(get(&a, "checked").as_bool()?),
                                "choose" => Change::Choose(text()),
                                "clear" => Change::Clear,
                                "copy_from" => Change::CopyFrom(get(&a, "from").as_f64()? as usize),
                                _ => return None,
                            };
                            Some(Action {
                                field: get(&a, "field").as_f64()? as usize,
                                change,
                                alternatives: ids(get(&a, "alternatives")),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        Some("no_value") => Proposal::NoValue(ids(get(&reply, "candidates"))),
        Some("not_supported") => {
            Proposal::NotSupported(get(&reply, "reason").as_string().unwrap_or_default())
        }
        _ => Proposal::NoSuchField,
    })
}

impl Fields {
    pub(crate) fn label(&self, id: usize) -> String {
        self.0
            .get(id)
            .map_or_else(|| "a field".into(), |f| f.label.clone())
    }

    /// How the change reads in the chat: "“0151 2345678”", "tick", "same as Home address".
    pub(crate) fn describe(&self, change: &Change) -> String {
        match change {
            Change::Fill(v) | Change::Choose(v) => format!("“{v}”"),
            Change::Check(true) => "tick".into(),
            Change::Check(false) => "untick".into(),
            Change::Clear => "clear".into(),
            Change::CopyFrom(from) => format!("same as {}", self.label(*from)),
        }
    }

    /// A field's value now (text, chosen option or radio choice; empty when none).
    fn value(&self, id: usize) -> String {
        let read = |page: u16, annotation: usize| {
            pdit_core::form_fields(page)
                .unwrap_or_default()
                .into_iter()
                .find(|f| f.annotation == annotation)
        };
        match self.0.get(id).map(|f| &f.target) {
            Some(Target::Text(p, a) | Target::Choice(p, a, _)) => {
                read(*p, *a).and_then(|f| f.value)
            }
            Some(Target::Radio(buttons)) => buttons
                .iter()
                .find(|(p, a, _)| read(*p, *a).is_some_and(|f| f.checked))
                .map(|(_, _, c)| c.clone()),
            _ => None,
        }
        .unwrap_or_default()
    }

    /// Writes the actions; returns the pages changed.
    pub(crate) fn apply(&self, actions: &[Action]) -> Result<Vec<u16>, String> {
        let mut pages = Vec::new();
        for action in actions {
            let field = self.0.get(action.field).ok_or("no such field")?;
            let change = match &action.change {
                Change::CopyFrom(from) => Change::Fill(self.value(*from)),
                c => c.clone(),
            };
            let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
            let result = match (&field.target, &change) {
                (Target::Text(p, a), Change::Fill(v)) => {
                    pdit_core::fill_text(*p, *a, v).map(|()| *p)
                }
                (Target::Text(p, a), Change::Clear) => {
                    pdit_core::fill_text(*p, *a, "").map(|()| *p)
                }
                (Target::Check(p, a), Change::Check(_) | Change::Clear) => {
                    let on = matches!(change, Change::Check(true));
                    let now = pdit_core::form_fields(*p)
                        .unwrap_or_default()
                        .into_iter()
                        .find(|f| f.annotation == *a)
                        .is_some_and(|f| f.checked);
                    match now == on {
                        true => Ok(*p),
                        false => pdit_core::toggle_choice(*p, *a).map(|()| *p),
                    }
                }
                (Target::Choice(p, a, options), Change::Fill(v) | Change::Choose(v)) => {
                    match options.iter().position(|o| same(o, v)) {
                        Some(i) => pdit_core::select_option(*p, *a, i).map(|()| *p),
                        None => pdit_core::fill_text(*p, *a, v).map(|()| *p),
                    }
                }
                (Target::Radio(buttons), Change::Fill(v) | Change::Choose(v)) => {
                    let (p, a, _) = buttons
                        .iter()
                        .find(|(_, _, c)| same(c, v))
                        .ok_or_else(|| format!("“{v}” isn't one of {}'s choices", field.label))?;
                    pdit_core::toggle_choice(*p, *a).map(|()| *p)
                }
                _ => return Err(format!("{} can't be changed that way", field.label)),
            };
            pages.push(result.map_err(|e| e.to_string())?);
        }
        Ok(pages)
    }
}
