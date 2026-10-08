//! The analysis (D-055), run in AI mode (D-062, frame_ui.rs) when the PDF was
//! never analysed or changed since. The analysis decides by itself what each page needs —
//! its own text, or OCR for a scan (desktop app: leafmind's Tesseract through
//! the desktop commands) — looks for form fields with leafmind's finder, and
//! says whether questions can be asked. Its steps show in a panel on the right.
//! Look: assets/css/ai.css (+ context-menu.css, thumbnails.css).

use crate::page_tools::PageTools;
use dioxus::prelude::*;
use pdit_core::analysis::{PageKind, page_kind};
use std::rc::Rc;

/// Scans are read at this resolution.
const OCR_DPI: f32 = 300.0;

/// A step's state, shown in the AI chat (ask_ui.rs).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum State {
    Wait,
    Run,
    Done,
    Skip,
}

#[derive(Clone, PartialEq)]
pub(crate) struct Step {
    pub(crate) what: String,
    pub(crate) detail: String,
    pub(crate) state: State,
}

impl Step {
    fn new(what: &str, state: State, detail: impl Into<String>) -> Self {
        Step {
            what: what.into(),
            detail: detail.into(),
            state,
        }
    }
}

#[derive(Clone, PartialEq, Default)]
pub(crate) struct Analysis {
    pub(crate) steps: Vec<Step>,
    pub(crate) running: bool,
    done: bool,
    pub(crate) summary: String,
    /// The document before OCR's text layer, for "Undo OCR".
    before: Option<Rc<Vec<u8>>>,
}

/// Shared AI state.
#[derive(Clone, Copy)]
pub struct Ai {
    analysis: Signal<Analysis>,
    /// The document's id (it grows with every change) when it was analysed
    /// (D-060): another id means it changed since, and "Analyze again" is offered.
    analyzed: Signal<Option<u64>>,
    /// Which file opening this state belongs to (OPEN_SEQ).
    seen_open: Signal<u64>,
    changed: Signal<bool>,
    /// What the analysis found (D-060), kept for this file and its saves.
    record: Signal<Option<Rc<crate::analysis_cache::Record>>>,
}

thread_local! {
    /// The SHA-256 of the file as it was opened (D-060: the record's key).
    static OPENED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// Counts file openings (page refreshes after edits don't count).
    static OPEN_SEQ: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A file is being opened: its key for the analysis record.
pub fn opened_file(bytes: &[u8]) {
    let hash = crate::analysis_cache::file_hash(bytes);
    OPENED.with_borrow_mut(|h| *h = Some(hash));
    OPEN_SEQ.set(OPEN_SEQ.get() + 1);
}

/// How many files have been opened so far (edits don't count).
pub fn open_seq() -> u64 {
    OPEN_SEQ.get()
}

/// The open document's id (it grows with every change).
fn document_id() -> Option<u64> {
    consume_context::<Signal<Option<crate::pages::OpenDocument>>>()
        .peek()
        .as_ref()
        .map(|d| d.id)
}

/// The document was saved as `bytes`: its record goes under the saved file's
/// key too, so opening the saved file needs no new analysis.
pub fn saved_file(bytes: &[u8]) {
    let Some(record) = try_consume_context::<Ai>().and_then(|ai| ai.record.peek().clone()) else {
        return;
    };
    let hash = crate::analysis_cache::file_hash(bytes);
    spawn(async move { crate::analysis_cache::store(&hash, &record).await });
}

impl Ai {
    pub fn provide() -> Self {
        use_context_provider(|| Ai {
            analysis: Signal::new(Analysis::default()),
            analyzed: Signal::new(None),
            changed: Signal::new(false),
            record: Signal::new(None),
            seen_open: Signal::new(0),
        })
    }

    /// AI mode opened (D-062): fresh model status, and whether the PDF changed
    /// since its analysis.
    pub fn entered(mut self) {
        let ask = consume_context::<crate::ask_ui::Ask>();
        spawn(async move { ask.refresh().await });
        if let Some(at) = *self.analyzed.peek() {
            self.changed.set(document_id() != Some(at));
        }
    }

    /// In AI mode: analyse the PDF if it never was, or changed since.
    pub fn analyze_if_needed(self) {
        let a = self.analysis.peek();
        if a.running || (self.analyzed.peek().is_some() && !*self.changed.peek()) {
            return;
        }
        drop(a);
        self.analyze();
    }

    /// Opening a file analysed before (D-060): its record comes back — the
    /// unsaved OCR text goes on its scanned pages again, the field suggestions
    /// wait for review, and Analyze isn't offered.
    async fn restore(mut self) {
        let Some(hash) = OPENED.with_borrow(|h| h.clone()) else {
            return;
        };
        let Some(record) = crate::analysis_cache::load(&hash).await else {
            return;
        };
        if !record.ocr.is_empty() {
            // The fallback font loads at start; wait for it a little.
            let editing = consume_context::<crate::editing::Editing>();
            let mut noto = editing.fallback_font.peek().clone();
            for _ in 0..50 {
                if noto.is_some() {
                    break;
                }
                crate::print_ui::pause(100).await;
                noto = editing.fallback_font.peek().clone();
            }
            if let Some(noto) = noto {
                let mut laid = false;
                for p in &record.ocr {
                    if matches!(page_kind(p.page), Ok(PageKind::Scan)) {
                        let words: Vec<pdit_core::LayerWord> = p
                            .words
                            .iter()
                            .map(|(text, rect)| pdit_core::LayerWord {
                                text: text.clone(),
                                rect: *rect,
                            })
                            .collect();
                        laid |= pdit_core::add_text_layer(p.page, &words, &noto).is_ok();
                    }
                }
                if laid {
                    consume_context::<PageTools>().refresh_pages();
                }
            }
        }
        let found = record.found();
        if !found.is_empty() {
            consume_context::<crate::find_fields_ui::FindFields>().set_prepared(Some(found));
        }
        self.analyzed.set(document_id());
        self.record.set(Some(Rc::new(record)));
        crate::log("pdit: analysis remembered for this file");
    }

    /// The analysis, for the AI chat (ask_ui.rs).
    pub(crate) fn analysis(&self) -> Analysis {
        self.analysis.read().clone()
    }

    /// A new document: nothing analysed yet.
    fn reset(mut self) {
        self.analysis.set(Analysis::default());
        self.analyzed.set(None);
        self.changed.set(false);
        self.record.set(None);
        consume_context::<crate::find_fields_ui::FindFields>().set_prepared(None);
    }

    fn set_step(mut self, i: usize, step: Step) {
        self.analysis.with_mut(|a| {
            if let Some(s) = a.steps.get_mut(i) {
                *s = step;
            }
        });
    }

    /// Analyze PDF: every step in turn, each deciding what the document needs.
    fn analyze(mut self) {
        if self.analysis.peek().running {
            return;
        }
        self.analysis.set(Analysis {
            steps: vec![
                Step::new("Checking the pages", State::Run, ""),
                Step::new("Reading scanned pages (OCR)", State::Wait, ""),
                Step::new("Looking for form fields", State::Wait, ""),
                Step::new("Getting ready for questions", State::Wait, ""),
            ],
            running: true,
            ..Analysis::default()
        });
        spawn(async move {
            let desktop = desktop::available();
            // OCR comes with the macOS app; other systems' builds don't have it yet.
            let ocr = desktop
                && desktop::invoke("ocr_available", &js_sys::Object::new())
                    .await
                    .ok()
                    .and_then(|v| v.as_bool())
                    == Some(true);
            // 1. What each page is.
            let pages = pdit_core::page_ops::page_sizes().unwrap_or_default();
            let kinds: Vec<PageKind> = (0..pages.len() as u16)
                .map(|p| page_kind(p).unwrap_or(PageKind::Blank))
                .collect();
            let scans: Vec<u16> = (0..kinds.len() as u16)
                .filter(|&p| kinds[usize::from(p)] == PageKind::Scan)
                .collect();
            let texts = kinds.iter().filter(|k| **k == PageKind::Text).count();
            // The language, from the first scan (or text page) — desktop only.
            let probe = scans.first().copied().or_else(|| {
                kinds
                    .iter()
                    .position(|k| *k == PageKind::Text)
                    .map(|p| p as u16)
            });
            let language = match (ocr, probe) {
                (true, Some(page)) => desktop::language(page).await,
                _ => None,
            };
            let mut record = crate::analysis_cache::Record::new();
            record.kinds = kinds
                .iter()
                .map(|k| {
                    match k {
                        PageKind::Text => "text",
                        PageKind::Scan => "scan",
                        PageKind::Blank => "blank",
                    }
                    .to_owned()
                })
                .collect();
            record.language = language.map(str::to_owned);
            let mut found = format!(
                "{} page{}: {texts} with text",
                pages.len(),
                if pages.len() == 1 { "" } else { "s" }
            );
            if !scans.is_empty() {
                found += &format!(", {} scanned", scans.len());
            }
            if let Some(lang) = language {
                found += &format!(" · {}", language_name(lang));
            }
            self.set_step(0, Step::new("Checking the pages", State::Done, found));

            // 2. OCR, only where a page needs it.
            if scans.is_empty() {
                self.set_step(
                    1,
                    Step::new(
                        "OCR not needed",
                        State::Skip,
                        "Every page has its own text; it is read directly.",
                    ),
                );
            } else if desktop && !ocr {
                self.set_step(
                    1,
                    Step::new(
                        "OCR isn't in this version yet",
                        State::Skip,
                        format!(
                            "{} scanned pages can be read in the macOS app for now.",
                            scans.len()
                        ),
                    ),
                );
            } else if !desktop {
                self.set_step(
                    1,
                    Step::new(
                        "OCR needs the desktop app",
                        State::Skip,
                        format!(
                            "{} scanned pages can't be read in the browser.",
                            scans.len()
                        ),
                    ),
                );
            } else {
                let tools = consume_context::<PageTools>();
                let before = tools.snapshot();
                let lang = language.unwrap_or("eng");
                let mut made = 0;
                let mut skipped = 0;
                for (k, &page) in scans.iter().enumerate() {
                    self.set_step(
                        1,
                        Step::new(
                            "Reading scanned pages (OCR)",
                            State::Run,
                            format!("Page {} of {}…", k + 1, scans.len()),
                        ),
                    );
                    crate::print_ui::pause(30).await;
                    match desktop::read(page, lang).await {
                        Ok(words) => {
                            record.ocr.push(crate::analysis_cache::OcrPage {
                                page,
                                words: words.iter().map(|w| (w.text.clone(), w.rect)).collect(),
                            });
                            let total = words.len();
                            let noto = consume_context::<crate::editing::Editing>()
                                .fallback_font
                                .peek()
                                .clone();
                            match noto.map(|noto| pdit_core::add_text_layer(page, &words, &noto)) {
                                Some(Ok(n)) => {
                                    made += 1;
                                    skipped += total - n;
                                }
                                Some(Err(error)) => crate::log(&format!(
                                    "pdit: OCR text layer, page {}: {error}",
                                    page + 1
                                )),
                                None => crate::log("pdit: the fallback font is still loading"),
                            }
                        }
                        Err(error) => crate::log(&format!("pdit: OCR, page {}: {error}", page + 1)),
                    }
                }
                tools.refresh_pages();
                let mut detail = format!(
                    "{made} page{} now searchable ({})",
                    if made == 1 { " is" } else { "s are" },
                    language_name(lang)
                );
                if skipped > 0 {
                    detail += &format!(
                        " · {skipped} words in a script pdit can't write yet were left out"
                    );
                }
                self.set_step(
                    1,
                    Step::new("Read scanned pages (OCR)", State::Done, detail),
                );
                if made > 0 {
                    self.analysis.with_mut(|a| a.before = before);
                }
            }

            // 3. Form fields: those the PDF has, and the finder's suggestions.
            self.set_step(2, Step::new("Looking for form fields", State::Run, ""));
            crate::print_ui::pause(30).await;
            let existing: usize = (0..pages.len() as u16)
                .map(|p| pdit_core::form_fields(p).map_or(0, |f| f.len()))
                .sum();
            let mut suggestions = Vec::new();
            if let Some(finder) = crate::find_fields_ui::finder().await {
                for page in 0..pages.len() as u16 {
                    self.set_step(
                        2,
                        Step::new(
                            "Looking for form fields",
                            State::Run,
                            format!("Page {} of {}…", page + 1, pages.len()),
                        ),
                    );
                    crate::print_ui::pause(30).await;
                    let have = pdit_core::form_fields(page).unwrap_or_default();
                    for f in pdit_core::fields::find_fields(&finder, page).unwrap_or_default() {
                        let taken = have.iter().any(|e| {
                            crate::find_fields_ui::covers(
                                [e.rect.0, e.rect.1, e.rect.2, e.rect.3],
                                f.rect,
                            )
                        });
                        if !taken {
                            suggestions.push((page, f));
                        }
                    }
                }
            }
            let n = suggestions.len();
            let detail = match (existing, n) {
                (0, 0) => "No fields".to_owned(),
                (0, n) => format!("None fillable yet · {n} possible fields found"),
                (e, 0) => format!("{e} fillable fields already in the PDF"),
                (e, n) => format!("{e} fillable fields already in the PDF · {n} more possible"),
            };
            record.suggestions = crate::analysis_cache::Record::suggestions_of(&suggestions);
            consume_context::<crate::find_fields_ui::FindFields>()
                .set_prepared((n > 0).then_some(suggestions));
            self.set_step(2, Step::new("Form fields", State::Done, detail));

            // 4. Questions (asking comes next; desktop only).
            self.set_step(
                3,
                if desktop {
                    Step::new(
                        "Ready for questions",
                        State::Done,
                        "The question models (680 MB) download the first time you ask.",
                    )
                } else {
                    Step::new("Questions need the desktop app", State::Skip, "")
                },
            );
            let before = self.analysis.peek().before.clone();
            self.analysis.with_mut(|a| {
                a.running = false;
                a.done = true;
                a.summary = if a.before.is_some() {
                    "Done. The OCR text was added to the PDF.".into()
                } else {
                    "Done. Nothing in the PDF was changed.".into()
                };
            });
            self.analyzed.set(document_id());
            self.changed.set(false);
            // Remember it for this file (D-060).
            let record = Rc::new(record);
            self.record.set(Some(record.clone()));
            if let Some(hash) = OPENED.with_borrow(|h| h.clone()) {
                crate::analysis_cache::store(&hash, &record).await;
            }
            // OCR's Undo is in the usual toast, and found fields open for review.
            if let Some(before) = before {
                consume_context::<PageTools>().show(
                    "Scanned pages are now searchable".into(),
                    crate::form_edit_ui::ICON_FORM,
                    Some(before),
                );
            }
            consume_context::<crate::find_fields_ui::FindFields>().review_prepared_quietly();
        });
    }
}

fn language_name(code: &str) -> &'static str {
    match code {
        "deu" => "German",
        "fas" => "Persian",
        "ara" => "Arabic",
        _ => "English",
    }
}

/// The desktop app's OCR (desktop/src/main.rs), through Tauri's IPC.
pub(crate) mod desktop {
    use super::OCR_DPI;
    use js_sys::{Object, Promise, Reflect, Uint8Array};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    pub(crate) fn internals() -> Option<JsValue> {
        let window = web_sys::window()?;
        Reflect::get(&window, &"__TAURI_INTERNALS__".into())
            .ok()
            .filter(|v| !v.is_undefined())
    }

    /// Calls a desktop command with JSON arguments.
    pub(crate) async fn invoke(command: &str, args: &Object) -> Result<JsValue, JsValue> {
        let internals = internals().ok_or("not the desktop app")?;
        let invoke: js_sys::Function = Reflect::get(&internals, &"invoke".into())?.dyn_into()?;
        let promise: Promise = invoke
            .call2(&internals, &command.into(), args)?
            .dyn_into()?;
        wasm_bindgen_futures::JsFuture::from(promise).await
    }

    /// Calls a desktop command with `bytes` as the raw body and `headers`.
    pub(crate) async fn invoke_raw(
        command: &str,
        bytes: &[u8],
        headers: &[(&str, String)],
    ) -> Result<JsValue, JsValue> {
        let internals = internals().ok_or("not the desktop app")?;
        let invoke: js_sys::Function = Reflect::get(&internals, &"invoke".into())?.dyn_into()?;
        let body = Uint8Array::from(bytes);
        let head = Object::new();
        for (name, value) in headers {
            Reflect::set(&head, &(*name).into(), &value.into())?;
        }
        let options = Object::new();
        Reflect::set(&options, &"headers".into(), &head)?;
        let promise: Promise = invoke
            .call3(&internals, &command.into(), &body, &options)?
            .dyn_into()?;
        wasm_bindgen_futures::JsFuture::from(promise).await
    }

    /// Running inside the desktop app.
    pub fn available() -> bool {
        internals().is_some()
    }

    /// Debug builds: the message also goes to the desktop app's terminal
    /// (its window's console can't be read from outside).
    #[cfg(debug_assertions)]
    pub fn log(message: &str) {
        let Some(internals) = internals() else { return };
        let Ok(invoke) = Reflect::get(&internals, &"invoke".into())
            .and_then(|f| f.dyn_into::<js_sys::Function>())
        else {
            return;
        };
        let args = Object::new();
        let _ = Reflect::set(&args, &"message".into(), &message.into());
        let _ = invoke.call2(&internals, &"debug_log".into(), &args);
    }

    /// Page `page` drawn for OCR, as (RGBA, width, height).
    fn page_image(page: u16) -> Result<(Vec<u8>, u32, u32), JsValue> {
        let sizes =
            pdit_core::page_ops::page_sizes().map_err(|e| JsValue::from_str(&e.to_string()))?;
        let width_pt = sizes.get(usize::from(page)).map_or(595.0, |s| s.0);
        let width = (width_pt / 72.0 * OCR_DPI).round() as u32;
        let image =
            pdit_core::render_page(page, width).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok((image.data().to_vec(), image.width(), image.height()))
    }

    /// Calls a desktop command with the page image as the raw body.
    async fn call(
        command: &str,
        page: u16,
        lang: Option<&str>,
    ) -> Result<(JsValue, u32, u32), JsValue> {
        let internals = internals().ok_or("not the desktop app")?;
        let invoke: js_sys::Function = Reflect::get(&internals, &"invoke".into())?.dyn_into()?;
        let (rgba, width, height) = page_image(page)?;
        let headers = Object::new();
        Reflect::set(&headers, &"x-width".into(), &width.to_string().into())?;
        Reflect::set(&headers, &"x-height".into(), &height.to_string().into())?;
        Reflect::set(
            &headers,
            &"x-dpi".into(),
            &(OCR_DPI as u32).to_string().into(),
        )?;
        if let Some(lang) = lang {
            Reflect::set(&headers, &"x-lang".into(), &lang.into())?;
        }
        let options = Object::new();
        Reflect::set(&options, &"headers".into(), &headers)?;
        let body = Uint8Array::from(rgba.as_slice());
        let promise: Promise = invoke
            .call3(&internals, &command.into(), &body, &options)?
            .dyn_into()?;
        let value = wasm_bindgen_futures::JsFuture::from(promise).await?;
        Ok((value, width, height))
    }

    /// The language code of page `page` ("eng", "deu", "fas", "ara").
    pub(crate) async fn language(page: u16) -> Option<&'static str> {
        let (value, _, _) = call("ocr_language", page, None).await.ok()?;
        let code = value.as_string()?;
        ["eng", "deu", "fas", "ara"]
            .into_iter()
            .find(|c| *c == code)
    }

    /// The words of page `page`, in PDF points.
    pub(crate) async fn read(page: u16, lang: &str) -> Result<Vec<pdit_core::LayerWord>, String> {
        let (value, width, _) = call("ocr_read", page, Some(lang))
            .await
            .map_err(|e| format!("{e:?}"))?;
        let sizes = pdit_core::page_ops::page_sizes().map_err(|e| e.to_string())?;
        let (width_pt, height_pt) = sizes
            .get(usize::from(page))
            .copied()
            .unwrap_or((595.0, 842.0));
        let k = width_pt / width as f32;
        let words = Reflect::get(&value, &"words".into()).map_err(|e| format!("{e:?}"))?;
        let words: js_sys::Array = words.dyn_into().map_err(|_| "no words")?;
        Ok(words
            .iter()
            .filter_map(|w| {
                let text = Reflect::get(&w, &"text".into()).ok()?.as_string()?;
                let b: js_sys::Array = Reflect::get(&w, &"bounds".into()).ok()?.dyn_into().ok()?;
                let n = |i: u32| b.get(i).as_f64().map(|v| v as f32);
                let (l, t, r, bottom) = (n(0)?, n(1)?, n(2)?, n(3)?);
                Some(pdit_core::LayerWord {
                    text,
                    rect: [l * k, height_pt - bottom * k, r * k, height_pt - t * k],
                })
            })
            .collect())
    }
}

/// The analysis's lifecycle (a new file resets it and restores what is
/// remembered); its steps show in the AI chat (ask_ui.rs, D-062).
#[component]
pub fn AiUi() -> Element {
    let ai = use_context::<Ai>();
    let document = use_context::<Signal<Option<crate::pages::OpenDocument>>>();
    // A different document: start over.
    let doc_id = document.read().as_ref().map(|d| d.id);
    // A file was opened (not just refreshed after an edit): start over, and
    // bring back what's remembered about it (D-060).
    use_effect(use_reactive!(|doc_id| {
        let seq = OPEN_SEQ.get();
        if doc_id.is_some() && seq != *ai.seen_open.peek() {
            let mut ai = ai;
            ai.seen_open.set(seq);
            ai.reset();
            spawn(async move { ai.restore().await });
        }
    }));
    // Know the models' state before the AI button is first pressed.
    use_hook(|| {
        let ask = consume_context::<crate::ask_ui::Ask>();
        spawn(async move { ask.refresh().await });
    });
    rsx! {}
}
