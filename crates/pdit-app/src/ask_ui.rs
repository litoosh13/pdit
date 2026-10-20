//! The AI chat (D-056; redesign D-062, approved mockup
//! .claude/research/canva-ui/): AI mode's right half. The first time, a card
//! asks before the question models are downloaded (desktop app:
//! desktop/src/qa.rs); then the analysis's steps (ai_ui.rs) show as a message;
//! then questions. Answers are the document's own sentences, picked by
//! leafmind, with their page; "Show on page" marks the sentence (search_ui's
//! mark). Look: assets/css/ask.css (Devigner UI tokens).

use crate::ai_ui::desktop;
use dioxus::prelude::*;
use js_sys::{Object, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const ASK_CSS: Asset = asset!("/assets/css/ask.css");
const ICON_SEND: &str = include_str!("../assets/icons/devigner/ArrowUp.svg");
const ICON_SHOW: &str = include_str!("../assets/icons/devigner/Eye.svg");
const INPUT_ID: &str = "pdit-ask-q";

#[derive(Clone, PartialEq)]
pub(crate) enum Models {
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
    /// `change all "old" to "new"` done: (old, new, how many).
    Changed(String, String, usize),
    /// `change all "old" …`: no "old" in the document.
    NothingToChange(String),
    /// A plain reply (why a question can't be answered here).
    Info(&'static str),
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

    pub(crate) fn models(&self) -> Models {
        self.models.read().clone()
    }

    /// leafmind reads the document as it is now (edits, OCR), unless it already has.
    pub(crate) async fn prepare(mut self) -> Result<(), String> {
        // A scan's OCR text goes first (x-ocr: its length): leafmind's PDF reader skips invisible text.
        let ocr = consume_context::<crate::ai_ui::Ai>().ocr_text();
        let mut bytes = match ocr.is_empty() {
            true => Vec::new(),
            false => serde_json::to_vec(&ocr).map_err(|e| e.to_string())?,
        };
        let ocr_len = bytes.len();
        bytes.extend(pdit_core::save_document().map_err(|e| e.to_string())?);
        let version = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
        });
        if *self.indexed.peek() != Some(version) {
            desktop::invoke_raw("qa_index", &bytes, &[("x-ocr", ocr_len.to_string())])
                .await
                .map_err(|e| format!("{e:?}"))?;
            self.indexed.set(Some(version));
        }
        Ok(())
    }

    pub async fn refresh(self) {
        self.check().await;
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
            // The analysis waited for the models (P-050 #3): it starts now, in AI mode.
            if consume_context::<crate::frame_ui::Frame>().is_ai() {
                consume_context::<crate::ai_ui::Ai>().analyze_if_needed();
            }
        }
    }

    pub(crate) fn download(self) {
        spawn(async move {
            if let Err(error) = desktop::invoke("qa_download", &Object::new()).await {
                crate::log(&format!("pdit: question models: {error:?}"));
            }
            self.check().await;
        });
    }

    pub(crate) fn cancel(self) {
        spawn(async move {
            let _ = desktop::invoke("qa_cancel", &Object::new()).await;
        });
    }

    fn ask(mut self) {
        let question = self.draft.peek().trim().to_owned();
        if question.is_empty() || *self.busy.peek() {
            return;
        }
        // An edit asked for in words (D-062): done here, no models needed.
        if let Some((old, new)) = pdit_core::search::parse_change_command(&question) {
            self.draft.set(String::new());
            let reply = match crate::search_ui::replace_everywhere(&old, &new) {
                Ok(0) => Msg::NothingToChange(old),
                Ok(n) => Msg::Changed(old, new, n),
                Err(error) => Msg::Failed(error),
            };
            self.messages.with_mut(|m| {
                m.push(Msg::Me(question));
                m.push(reply);
            });
            next_focus();
            return;
        }
        if *self.models.peek() != Models::Ready {
            let why = if *self.models.peek() == Models::Unsupported {
                "Questions work on Apple-silicon Macs for now. Edits like change all \"old\" to \"new\" work here."
            } else {
                "Questions need the models first (the card above). Edits like change all \"old\" to \"new\" work now."
            };
            self.draft.set(String::new());
            self.messages.with_mut(|m| {
                m.push(Msg::Me(question));
                m.push(Msg::Info(why));
            });
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
        self.messages
            .with_mut(|m| m.push(Msg::Wait("Looking through the document…")));
        // Read again if the document changed since (edits, OCR).
        if let Err(error) = self.prepare().await {
            return Msg::Failed(error);
        }
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
    // A new file (not an edit of this one): a fresh conversation.
    let doc_id = document.read().as_ref().map(|d| d.id);
    let mut seen = use_signal(|| 0u64);
    use_effect(use_reactive!(|doc_id| {
        let seq = crate::ai_ui::open_seq();
        if doc_id.is_some() && seq != *seen.peek() {
            seen.set(seq);
            ask.reset();
        }
    }));
    let ai = use_context::<crate::ai_ui::Ai>();
    let frame = use_context::<crate::frame_ui::Frame>();
    let analysis = ai.analysis();
    let tools = use_context::<crate::page_tools::PageTools>();
    let models = (ask.models)();
    let messages = (ask.messages)();
    let draft = (ask.draft)();
    let waiting_for_models = matches!(models, Models::Missing { .. } | Models::Unknown);
    let can_send = !draft.trim().is_empty() && !(ask.busy)() && !waiting_for_models;
    rsx! {
        document::Stylesheet { href: ASK_CSS }
        aside {
            class: "pdit-panel pdit-ask t-panel-slide",
            "data-open": if (ask.open)() { "true" } else { "false" },
            "aria-label": "AI chat",
            div { class: "pdit-ask-conv",
                if let Models::Missing { progress, error } = models.clone() {
                    div { class: "ac-card",
                        b { "One last step: the question models" }
                        span {
                            "About "
                            b { "390 MB" }
                            ", downloaded once to this computer. Only the models — your PDFs never leave your computer."
                        }
                        span { class: "small",
                            "Answers are sentences from the document itself, with their page. Nothing is made up; if no sentence answers, pdit says so."
                        }
                        if let Some((done, total)) = progress {
                            span { class: "small", "Downloading… {mb(done)} of {mb(total)} MB" }
                            span { class: "ac-bar",
                                i { style: "width: {done as f64 / total.max(1) as f64 * 100.0}%;" }
                            }
                        }
                        if let Some(error) = error {
                            span { class: "small err", "The download stopped: {error}. You can try again." }
                        }
                        div { class: "ac-row",
                            if progress.is_some() {
                                button { class: "ac-btn", r#type: "button", onclick: move |_| ask.cancel(), "Cancel" }
                            } else {
                                button { class: "ac-btn", r#type: "button", onclick: move |_| frame.leave_ai(), "Not now" }
                                button { class: "ac-btn primary", r#type: "button", onclick: move |_| ask.download(), "Download" }
                            }
                        }
                    }
                }
                // The analysis (D-055), as a message.
                if !analysis.steps.is_empty() {
                    div { class: "ac-msg ai ac-steps",
                        for (i, s) in analysis.steps.iter().enumerate() {
                            div {
                                key: "{i}",
                                class: match s.state {
                                    crate::ai_ui::State::Wait => "ac-step",
                                    crate::ai_ui::State::Run => "ac-step is-run",
                                    crate::ai_ui::State::Done => "ac-step is-done",
                                    crate::ai_ui::State::Skip => "ac-step is-skip",
                                },
                                span { class: "st",
                                    match s.state {
                                        crate::ai_ui::State::Wait => "○",
                                        crate::ai_ui::State::Run => "…",
                                        crate::ai_ui::State::Done => "✓",
                                        crate::ai_ui::State::Skip => "–",
                                    }
                                }
                                div {
                                    b { "{s.what}" }
                                    if !s.detail.is_empty() {
                                        small { "{s.detail}" }
                                    }
                                }
                            }
                        }
                        if !analysis.summary.is_empty() {
                            div { class: "ac-sum", "{analysis.summary}" }
                        }
                        if let Some(pct) = analysis.percent() {
                            div { class: "ac-row start",
                                span { class: "ac-badge", if pct >= 100 { "Ready" } else { "{pct}%" } }
                                span { class: "small", "{analysis.now()}" }
                            }
                        }
                    }
                }
                if models == Models::Unsupported {
                    div { class: "ac-msg ai",
                        b { "Asking questions works on Apple-silicon Macs for now" }
                        div { class: "small", "leafmind's question models need ONNX Runtime, which pdit downloads for Apple-silicon Macs only so far." }
                    }
                }
                if models == Models::Ready && messages.is_empty() && !analysis.running {
                    div { class: "ac-msg ai", "Ask me anything about this document — I answer with the sentence and its page." }
                }
                for (k, m) in messages.into_iter().enumerate() {
                    match m {
                        Msg::Me(q) => rsx! {
                            div { key: "{k}", class: "ac-msg me", "{q}" }
                        },
                        Msg::Wait(what) => rsx! {
                            div { key: "{k}", class: "ac-msg ai is-wait", "{what}" }
                        },
                        Msg::Found(sentences) => rsx! {
                            for (i, (page, text)) in sentences.into_iter().enumerate() {
                                div { key: "{k}-{i}", class: "ac-msg ai",
                                    if i > 0 {
                                        div { class: "small", "Changed by page {page}:" }
                                    }
                                    blockquote { "“{text}”" }
                                    div { class: "ac-row start",
                                        span { class: "ac-badge", "Page {page}" }
                                        button {
                                            class: "ac-chip",
                                            r#type: "button",
                                            onclick: {
                                                let text = text.clone();
                                                move |_| find.mark(page.saturating_sub(1) as u16, &text)
                                            },
                                            span { dangerous_inner_html: ICON_SHOW, style: "display: contents" }
                                            "Show on page"
                                        }
                                    }
                                }
                            }
                        },
                        Msg::NotFound => rsx! {
                            div { key: "{k}", class: "ac-msg ai",
                                b { "Not in the document" }
                                div { class: "small", "No sentence answers this closely enough, so nothing is guessed." }
                            }
                        },
                        Msg::WrongLanguage(language) => rsx! {
                            div { key: "{k}", class: "ac-msg ai",
                                b { "Please ask in {language}" }
                                div { class: "small", "The document is in {language}; questions are answered in its own language." }
                            }
                        },
                        Msg::Failed(error) => rsx! {
                            div { key: "{k}", class: "ac-msg ai",
                                b { "Something went wrong" }
                                div { class: "small", "{error}" }
                            }
                        },
                        Msg::Changed(old, new, n) => rsx! {
                            div { key: "{k}", class: "ac-msg ai",
                                b {
                                    if n == 1 { "Changed 1 place" } else { "Changed {n} places" }
                                }
                                div { class: "small", "“{old}” → “{new}”" }
                                div { class: "ac-row start",
                                    button {
                                        class: "ac-chip",
                                        r#type: "button",
                                        disabled: !tools.can_undo(),
                                        onclick: move |_| tools.undo_last(),
                                        "Undo"
                                    }
                                }
                            }
                        },
                        Msg::Info(text) => rsx! {
                            div { key: "{k}", class: "ac-msg ai", "{text}" }
                        },
                        Msg::NothingToChange(old) => rsx! {
                            div { key: "{k}", class: "ac-msg ai",
                                b { "“{old}” isn't in the document" }
                                div { class: "small", "Nothing was changed. If the PDF splits the words into pieces, try a shorter part." }
                            }
                        },
                    }
                }
            }
            div {
                class: "ac-input",
                onclick: move |_| next_focus(),
                input {
                    id: INPUT_ID,
                    "aria-label": "Question",
                    // Nothing AI until the models are there (P-050 #3); where none can be downloaded (online),
                    // edits in words still work.
                    disabled: waiting_for_models,
                    placeholder: if models == Models::Ready {
                        "Ask about this document, or: change all \"old\" to \"new\""
                    } else if waiting_for_models {
                        "Download the AI models above to start"
                    } else {
                        "Edit in words: change all \"old\" to \"new\""
                    },
                    value: "{draft}",
                    oninput: move |e| ask.draft.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            ask.ask();
                        }
                    },
                }
                button {
                    class: "ac-send",
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

/// The first-launch prompt (D-066, approved mockup canva-ui `#first-run`): the
/// question models, offered once per start until they are downloaded, in the
/// update prompt's look (D-058, update.css). Hidden in AI mode (its card is there).
#[component]
pub fn ModelsPrompt() -> Element {
    let ask = use_context::<Ask>();
    let updates = use_context::<crate::update_ui::Updates>();
    let frame = use_context::<crate::frame_ui::Frame>();
    let mut closed = use_signal(|| false);
    let mut open = use_signal(|| false);
    use_hook(move || {
        if desktop::available() {
            spawn(async move {
                ask.refresh().await;
                crate::print_ui::pause(50).await;
                open.set(true);
            });
        }
    });
    let Models::Missing { progress, error } = (ask.models)() else {
        return rsx! {};
    };
    if closed() || frame.is_ai() || updates.on_screen() {
        return rsx! {};
    }
    rsx! {
        div { class: "pdit-toast-host pdit-update-host",
            div {
                class: if open() { "pdit-toast t-toast sa-root pdit-update is-open" } else { "pdit-toast t-toast sa-root pdit-update" },
                role: "status",
                "aria-live": "polite",
                if let Some((done, total)) = progress {
                    span {
                        "Downloading the AI models… "
                        span { class: "pdit-update-pct", "{done * 100 / total.max(1)}" }
                        " %"
                    }
                } else {
                    if let Some(error) = &error {
                        span { "The download stopped: {error}" }
                    } else {
                        span {
                            "pdit's AI needs its models: "
                            b { "390 MB" }
                            ", downloaded once "
                            span { class: "pdit-update-dim", "· your PDFs never leave this computer" }
                        }
                    }
                    button { class: "sa-control", r#type: "button", onclick: move |_| closed.set(true), "Not now" }
                    button { class: "sa-primary", r#type: "button", onclick: move |_| ask.download(),
                        if error.is_some() { "Try again" } else { "Download" }
                    }
                }
            }
        }
    }
}
