//! Ask a question (D-056): AI menu → Ask a question opens Beautiful UI's
//! "Chat" in the right-hand panel. The first time, a card asks before the
//! question models are downloaded (desktop app: desktop/src/qa.rs). Answers are
//! the document's own sentences, picked by leafmind, with their page; "Show on
//! page" marks the sentence (search_ui's mark). Look: assets/css/ask.css.

use crate::ai_ui::desktop;
use dioxus::prelude::*;
use js_sys::{Object, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const ASK_CSS: Asset = asset!("/assets/css/ask.css");
const ICON_CLOSE: &str = r#"<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M18 6L6 18M6 6l12 12"/></svg>"#;
const ICON_SEND: &str = r#"<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7"/></svg>"#;
const INPUT_ID: &str = "pdit-ask-q";

#[derive(Clone, PartialEq)]
enum Models {
    /// Checking.
    Unknown,
    /// Not on this Mac (ONNX Runtime 1.30 is Apple-silicon only).
    Unsupported,
    /// Not downloaded; maybe downloading (done, total) or failed.
    Missing {
        progress: Option<(u64, u64)>,
        error: Option<String>,
    },
    Ready,
}

#[derive(Clone, PartialEq)]
enum Msg {
    Me(String),
    Wait(&'static str),
    Found(Vec<(u32, String)>),
    NotFound,
    WrongLanguage(String),
    Failed(String),
}

/// Shared Ask state.
#[derive(Clone, Copy)]
pub struct Ask {
    open: Signal<bool>,
    models: Signal<Models>,
    messages: Signal<Vec<Msg>>,
    draft: Signal<String>,
    busy: Signal<bool>,
    /// Which version of the document the engine has read (FNV of its bytes).
    indexed: Signal<Option<u64>>,
}

impl Ask {
    pub fn provide() -> Self {
        use_context_provider(|| Ask {
            open: Signal::new(false),
            models: Signal::new(Models::Unknown),
            messages: Signal::new(Vec::new()),
            draft: Signal::new(String::new()),
            busy: Signal::new(false),
            indexed: Signal::new(None),
        })
    }

    pub fn open(mut self) {
        self.open.set(true);
        spawn(async move { self.check().await });
    }

    pub fn close(mut self) {
        if *self.open.peek() {
            self.open.set(false);
        }
    }

    /// A different document: a fresh conversation.
    fn reset(mut self) {
        self.messages.set(Vec::new());
        self.indexed.set(None);
        self.open.set(false);
    }

    async fn check(mut self) {
        let Ok(status) = desktop::invoke("qa_status", &Object::new()).await else {
            return self.models.set(Models::Unsupported);
        };
        let get = |k: &str| Reflect::get(&status, &k.into()).unwrap_or(JsValue::NULL);
        let num = |k: &str| get(k).as_f64().unwrap_or(0.0) as u64;
        let models = if get("supported").as_bool() != Some(true) {
            Models::Unsupported
        } else if get("ready").as_bool() == Some(true) {
            Models::Ready
        } else {
            Models::Missing {
                progress: (get("downloading").as_bool() == Some(true))
                    .then(|| (num("done"), num("total"))),
                error: get("error").as_string().filter(|e| e != "cancelled"),
            }
        };
        let downloading = matches!(
            models,
            Models::Missing {
                progress: Some(_),
                ..
            }
        );
        self.models.set(models);
        if downloading {
            crate::print_ui::pause(400).await;
            Box::pin(self.check()).await;
        } else if *self.models.peek() == Models::Ready {
            next_focus();
        }
    }

    fn download(self) {
        spawn(async move {
            if let Err(error) = desktop::invoke("qa_download", &Object::new()).await {
                crate::log(&format!("pdit: question models: {error:?}"));
            }
            self.check().await;
        });
    }

    fn cancel(self) {
        spawn(async move {
            let _ = desktop::invoke("qa_cancel", &Object::new()).await;
        });
    }

    fn ask(mut self) {
        let question = self.draft.peek().trim().to_owned();
        if question.is_empty() || *self.busy.peek() || *self.models.peek() != Models::Ready {
            return;
        }
        self.draft.set(String::new());
        self.busy.set(true);
        self.messages
            .with_mut(|m| m.push(Msg::Me(question.clone())));
        spawn(async move {
            let reply = self.answer(&question).await;
            self.messages.with_mut(|m| {
                if matches!(m.last(), Some(Msg::Wait(_))) {
                    m.pop();
                }
                m.push(reply);
            });
            self.busy.set(false);
            next_focus();
        });
    }

    async fn answer(mut self, question: &str) -> Msg {
        // The engine reads the document again whenever it has changed (edits, OCR).
        let bytes = match pdit_core::save_document() {
            Ok(bytes) => bytes,
            Err(error) => return Msg::Failed(error.to_string()),
        };
        let version = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
        });
        if *self.indexed.peek() != Some(version) {
            self.messages
                .with_mut(|m| m.push(Msg::Wait("Reading the document…")));
            if let Err(error) = desktop::invoke_raw("qa_index", &bytes).await {
                return Msg::Failed(format!("{error:?}"));
            }
            self.indexed.set(Some(version));
            self.messages.with_mut(|m| {
                m.pop();
            });
        }
        self.messages
            .with_mut(|m| m.push(Msg::Wait("Looking through the document…")));
        let args = Object::new();
        let _ = Reflect::set(&args, &"question".into(), &question.into());
        let reply = match desktop::invoke("qa_ask", &args).await {
            Ok(reply) => reply,
            Err(error) => return Msg::Failed(format!("{error:?}")),
        };
        let get = |k: &str| Reflect::get(&reply, &k.into()).unwrap_or(JsValue::NULL);
        match get("kind").as_string().as_deref() {
            Some("found") => {
                let sentences = get("sentences")
                    .dyn_into::<js_sys::Array>()
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| {
                                let page = Reflect::get(&s, &"page".into()).ok()?.as_f64()? as u32;
                                let text = Reflect::get(&s, &"text".into()).ok()?.as_string()?;
                                Some((page, text))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Msg::Found(sentences)
            }
            Some("wrong_language") => {
                Msg::WrongLanguage(get("document").as_string().unwrap_or_default())
            }
            _ => Msg::NotFound,
        }
    }
}

fn next_focus() {
    crate::page_tools::next_frame(|| {
        let input = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(INPUT_ID))
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
        if let Some(input) = input {
            let _ = input.focus();
        }
    });
}

fn mb(bytes: u64) -> u64 {
    (bytes + 500_000) / 1_000_000
}

/// The Ask panel.
#[component]
pub fn AskUi() -> Element {
    let mut ask = use_context::<Ask>();
    let find = use_context::<crate::search_ui::Find>();
    let document = use_context::<Signal<Option<crate::pages::OpenDocument>>>();
    let doc_id = document.read().as_ref().map(|d| d.id);
    use_effect(use_reactive!(|doc_id| {
        let _ = doc_id;
        ask.reset();
    }));
    let models = (ask.models)();
    let messages = (ask.messages)();
    let draft = (ask.draft)();
    let can_send = models == Models::Ready && !draft.trim().is_empty() && !(ask.busy)();
    rsx! {
        document::Stylesheet { href: ASK_CSS }
        aside {
            class: "pdit-panel pdit-ask t-panel-slide",
            "data-open": if (ask.open)() { "true" } else { "false" },
            "aria-label": "Ask a question",
            div { class: "pdit-ask-head",
                span { class: "tab", "Ask" }
                button {
                    class: "icon",
                    r#type: "button",
                    title: "Close (Esc)",
                    "aria-label": "Close",
                    onclick: move |_| ask.close(),
                    span { dangerous_inner_html: ICON_CLOSE, style: "display: contents" }
                }
            }
            div { class: "pdit-ask-conv",
                match models.clone() {
                    Models::Unknown => rsx! {},
                    Models::Unsupported => rsx! {
                        div { class: "pdit-ask-card",
                            b { "Asking questions needs an Apple-silicon Mac" }
                            div { "leafmind's question models run on ONNX Runtime, which is published for Apple-silicon Macs only." }
                        }
                    },
                    Models::Missing { progress, error } => rsx! {
                        div { class: "pdit-ask-card",
                            b { "Asking needs leafmind's question models" }
                            div {
                                "About "
                                b { "760 MB" }
                                ", downloaded once to this computer from Hugging Face and the ONNX Runtime releases. Only the models are downloaded — this PDF never leaves your computer."
                            }
                            div { class: "small",
                                "Answers are sentences from the document itself, with their page. Nothing is made up; if no sentence answers, pdit says so."
                            }
                            if let Some((done, total)) = progress {
                                div { class: "bar",
                                    i { style: "width: {done as f64 / total.max(1) as f64 * 100.0}%;" }
                                }
                                div { class: "small", "Downloading… {mb(done)} of {mb(total)} MB" }
                            }
                            if let Some(error) = error {
                                div { class: "small err", "The download stopped: {error}. You can try again." }
                            }
                            div { class: "row",
                                if progress.is_some() {
                                    button { class: "btn", r#type: "button", onclick: move |_| ask.cancel(), "Cancel" }
                                } else {
                                    button { class: "btn", r#type: "button", onclick: move |_| ask.close(), "Not now" }
                                    button { class: "btn primary", r#type: "button", onclick: move |_| ask.download(), "Download" }
                                }
                            }
                        }
                    },
                    Models::Ready => rsx! {
                        if messages.is_empty() {
                            div { class: "pdit-ask-empty", "Ask anything about this document. Answers quote it, with the page." }
                        }
                        for (k, m) in messages.into_iter().enumerate() {
                            match m {
                                Msg::Me(q) => rsx! {
                                    div { key: "{k}", class: "pdit-ask-me", div { "{q}" } }
                                },
                                Msg::Wait(what) => rsx! {
                                    div { key: "{k}", class: "pdit-ask-sec is-wait",
                                        div { class: "lab", b { "Looking" } }
                                        p { "{what}" }
                                    }
                                },
                                Msg::Found(sentences) => rsx! {
                                    for (i, (page, text)) in sentences.into_iter().enumerate() {
                                        div { key: "{k}-{i}", class: "pdit-ask-sec",
                                            div { class: "lab",
                                                if i == 0 {
                                                    b { "Page {page}" }
                                                    span { "of the document" }
                                                } else {
                                                    b { "Changed by" }
                                                    span { "page {page}" }
                                                }
                                            }
                                            p { class: "quote", "{text}" }
                                            button {
                                                class: "go",
                                                r#type: "button",
                                                onclick: {
                                                    let text = text.clone();
                                                    move |_| find.mark(page.saturating_sub(1) as u16, &text)
                                                },
                                                "Show on page {page}"
                                            }
                                        }
                                    }
                                },
                                Msg::NotFound => rsx! {
                                    div { key: "{k}", class: "pdit-ask-sec",
                                        div { class: "lab", b { "Not in the document" } }
                                        p { "No sentence answers this closely enough, so nothing is guessed." }
                                    }
                                },
                                Msg::WrongLanguage(language) => rsx! {
                                    div { key: "{k}", class: "pdit-ask-sec",
                                        div { class: "lab",
                                            b { "Please ask in {language}" }
                                            span { "— the document is in {language}" }
                                        }
                                        p { "Questions are answered in the document's own language." }
                                    }
                                },
                                Msg::Failed(error) => rsx! {
                                    div { key: "{k}", class: "pdit-ask-sec",
                                        div { class: "lab", b { "Something went wrong" } }
                                        p { "{error}" }
                                    }
                                },
                            }
                        }
                    },
                }
            }
            div { class: "pdit-ask-composer",
                div {
                    class: "box",
                    onclick: move |_| next_focus(),
                    input {
                        id: INPUT_ID,
                        "aria-label": "Question",
                        disabled: models != Models::Ready,
                        placeholder: if models == Models::Ready { "Ask about this document…" } else { "Download the models first" },
                        value: "{draft}",
                        oninput: move |e| ask.draft.set(e.value()),
                        onkeydown: move |e| {
                            if e.key() == Key::Enter {
                                ask.ask();
                            }
                        },
                    }
                    button {
                        class: if can_send { "send is-on" } else { "send" },
                        r#type: "button",
                        "aria-label": "Send",
                        disabled: !can_send,
                        onclick: move |_| ask.ask(),
                        span { dangerous_inner_html: ICON_SEND, style: "display: contents" }
                    }
                }
            }
        }
    }
}
