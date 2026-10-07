//! The AI button (D-055): the round metal button in the top bar opens the
//! app's menu under it. "Analyze PDF" comes first; the other entries turn on
//! once it has run. The analysis decides by itself what each page needs —
//! its own text, or OCR for a scan (desktop app: leafmind's Tesseract through
//! the desktop commands) — looks for form fields with leafmind's finder, and
//! says whether questions can be asked. Its steps show in a panel on the right.
//! Look: assets/css/ai.css (+ context-menu.css, thumbnails.css).

use crate::page_tools::{PageTools, next_frame, set_timeout};
use dioxus::prelude::*;
use pdit_core::analysis::{PageKind, page_kind};
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const AI_CSS: Asset = asset!("/assets/css/ai.css");
/// Scans are read at this resolution.
const OCR_DPI: f32 = 300.0;
/// The menu's close time (Transitions.dev "Menu dropdown").
const CLOSE_MS: i32 = 150;

#[derive(Clone, Copy, PartialEq)]
enum State {
    Wait,
    Run,
    Done,
    Skip,
}

#[derive(Clone, PartialEq)]
struct Step {
    what: String,
    detail: String,
    state: State,
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
struct Analysis {
    steps: Vec<Step>,
    running: bool,
    done: bool,
    summary: String,
    /// The document before OCR's text layer, for "Undo OCR".
    before: Option<Rc<Vec<u8>>>,
}

/// Shared AI state.
#[derive(Clone, Copy)]
pub struct Ai {
    /// The menu is open (left, top of the button's bottom edge), and closing.
    menu: Signal<Option<(f64, f64)>>,
    closing: Signal<bool>,
    shown: Signal<bool>,
    panel: Signal<bool>,
    analysis: Signal<Analysis>,
}

impl Ai {
    pub fn provide() -> Self {
        use_context_provider(|| Ai {
            menu: Signal::new(None),
            closing: Signal::new(false),
            shown: Signal::new(false),
            panel: Signal::new(false),
            analysis: Signal::new(Analysis::default()),
        })
    }

    /// The round button: opens the menu under it, or closes it.
    pub fn toggle_menu(mut self) {
        if self.menu.peek().is_some() {
            return self.close_menu();
        }
        let rect = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.query_selector(".tb-ai").ok().flatten())
            .map(|b| b.get_bounding_client_rect());
        let Some(rect) = rect else { return };
        self.closing.set(false);
        self.shown.set(false);
        self.menu.set(Some((rect.left(), rect.bottom() + 10.0)));
        let mut shown = self.shown;
        next_frame(move || next_frame(move || shown.set(true)));
    }

    pub fn close_menu(mut self) {
        if self.menu.peek().is_none() || *self.closing.peek() {
            return;
        }
        self.closing.set(true);
        let (mut menu, mut closing) = (self.menu, self.closing);
        set_timeout(CLOSE_MS, move || {
            if *closing.peek() {
                menu.set(None);
                closing.set(false);
            }
        });
    }

    /// Esc: the menu, then the panel.
    pub fn escape(mut self) {
        if self.menu.peek().is_some() {
            self.close_menu();
        } else if *self.panel.peek() && !self.analysis.peek().running {
            self.panel.set(false);
        }
    }

    /// A new document: nothing analysed yet.
    fn reset(mut self) {
        self.analysis.set(Analysis::default());
        self.panel.set(false);
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
        self.panel.set(true);
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
            self.analysis.with_mut(|a| {
                a.running = false;
                a.done = true;
                a.summary = if a.before.is_some() {
                    "Done. The OCR text was added to the PDF".into()
                } else {
                    "Done. Nothing in the PDF was changed.".into()
                };
            });
        });
    }

    fn undo_ocr(mut self) {
        let Some(before) = self.analysis.peek().before.clone() else {
            return;
        };
        consume_context::<PageTools>().restore_snapshot(&before);
        self.analysis.with_mut(|a| {
            a.before = None;
            a.summary = "The OCR text was taken out again.".into();
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

    /// Calls a desktop command with `bytes` as the raw body.
    pub(crate) async fn invoke_raw(command: &str, bytes: &[u8]) -> Result<JsValue, JsValue> {
        let internals = internals().ok_or("not the desktop app")?;
        let invoke: js_sys::Function = Reflect::get(&internals, &"invoke".into())?.dyn_into()?;
        let body = Uint8Array::from(bytes);
        let promise: Promise = invoke
            .call3(&internals, &command.into(), &body, &Object::new())?
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

/// The menu under the AI button, and the Analysis panel.
#[component]
pub fn AiUi() -> Element {
    let ai = use_context::<Ai>();
    let document = use_context::<Signal<Option<crate::pages::OpenDocument>>>();
    // A different document: start over.
    let doc_id = document.read().as_ref().map(|d| d.id);
    use_effect(use_reactive!(|doc_id| {
        let _ = doc_id;
        ai.reset();
    }));
    use_outside_close(ai);
    let a = (ai.analysis)();
    let desktop = desktop::available();
    rsx! {
        document::Stylesheet { href: AI_CSS }
        if let Some((left, top)) = (ai.menu)() {
            div {
                class: match ((ai.closing)(), (ai.shown)()) {
                    (true, _) => "cm-menu t-dropdown pdit-ai-menu is-closing",
                    (false, true) => "cm-menu t-dropdown pdit-ai-menu is-open",
                    _ => "cm-menu t-dropdown pdit-ai-menu",
                },
                role: "menu",
                "data-origin": "top-left",
                style: "left: {left}px; top: {top}px;",
                button {
                    r#type: "button",
                    role: "menuitem",
                    disabled: a.running,
                    onclick: move |_| {
                        ai.close_menu();
                        ai.analyze();
                    },
                    span { class: "pdit-ai-primary", if a.done { "Analyze again" } else { "Analyze PDF" } }
                }
                div { class: "cm-label", "With the analysis" }
                MenuRow {
                    label: "Ask a question",
                    off: !a.done || !desktop,
                    why: (if desktop { "after analysis" } else { "desktop app" }).to_owned(),
                    onpick: move |_| {
                        let mut ai = ai;
                        ai.close_menu();
                        ai.panel.set(false);
                        consume_context::<crate::ask_ui::Ask>().open();
                    },
                }
                MenuRow {
                    label: "Find form fields",
                    off: !a.done,
                    why: "after analysis",
                    onpick: move |_| {
                        ai.close_menu();
                        consume_context::<crate::find_fields_ui::FindFields>().review_prepared();
                    },
                }
                MenuRow {
                    label: "Show analysis",
                    off: !a.done,
                    why: "after analysis",
                    onpick: move |_| {
                        let mut ai = ai;
                        ai.close_menu();
                        consume_context::<crate::ask_ui::Ask>().close();
                        ai.panel.set(true);
                    },
                }
            }
        }
        aside {
            class: "pdit-panel pdit-ai t-panel-slide",
            "data-open": if (ai.panel)() { "true" } else { "false" },
            "aria-label": "Analysis",
            div { class: "pdit-panel-head",
                span { class: "title", "Analysis" }
                span { class: "grow" }
                button {
                    class: "pdit-ai-x",
                    r#type: "button",
                    title: "Close",
                    disabled: a.running,
                    onclick: move |_| {
                        let mut ai = ai;
                        ai.panel.set(false);
                    },
                    "✕"
                }
            }
            for (i, s) in a.steps.iter().enumerate() {
                div {
                    key: "{i}",
                    class: match s.state {
                        State::Wait => "pdit-ai-step",
                        State::Run => "pdit-ai-step is-run",
                        State::Done => "pdit-ai-step is-done",
                        State::Skip => "pdit-ai-step is-skip",
                    },
                    span { class: "st",
                        match s.state {
                            State::Wait => "○",
                            State::Run => "…",
                            State::Done => "✓",
                            State::Skip => "–",
                        }
                    }
                    div {
                        div { class: "what", "{s.what}" }
                        if !s.detail.is_empty() {
                            div { class: "detail", "{s.detail}" }
                        }
                    }
                }
            }
            if !a.summary.is_empty() {
                div { class: "pdit-ai-sum",
                    "{a.summary}"
                    if a.before.is_some() {
                        " · "
                        button { r#type: "button", onclick: move |_| ai.undo_ocr(), "Undo OCR" }
                    }
                }
            }
        }
    }
}

/// A menu row that waits for the analysis (greyed, with why).
#[component]
fn MenuRow(label: String, off: bool, why: String, onpick: EventHandler<()>) -> Element {
    rsx! {
        button {
            r#type: "button",
            role: "menuitem",
            disabled: off,
            onclick: move |_| onpick.call(()),
            span { "{label}" }
            if off {
                span { class: "pdit-ai-why", "{why}" }
            }
        }
    }
}

/// A press outside the menu (and not on the AI button) closes it.
fn use_outside_close(ai: Ai) {
    use_hook(move || {
        let Some(window) = web_sys::window() else {
            return;
        };
        let (mut menu, mut closing) = (ai.menu, ai.closing);
        let on_pointer = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(
            move |event: web_sys::PointerEvent| {
                if menu.peek().is_none() {
                    return;
                }
                let inside = event
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .and_then(|el| el.closest(".pdit-ai-menu, .tb-ai").ok().flatten())
                    .is_some();
                if !inside {
                    // As close_menu, without looking up a context outside Dioxus.
                    closing.set(true);
                    set_timeout(CLOSE_MS, move || {
                        if *closing.peek() {
                            menu.set(None);
                            closing.set(false);
                        }
                    });
                }
            },
        );
        let capture = web_sys::AddEventListenerOptions::new();
        capture.set_capture(true);
        let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
            "pointerdown",
            on_pointer.as_ref().unchecked_ref(),
            &capture,
        );
        // The app lives as long as the page.
        on_pointer.forget();
    });
}
