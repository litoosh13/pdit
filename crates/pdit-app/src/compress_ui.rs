//! Compress PDF (M-009, D-069; approved mockup .claude/research/compress-ui/): the
//! Print window's look (doc.css), opened from the rail's Document → Compress PDF…
//! (desktop app). All three levels are worked out by MuPDF (desktop
//! `compress_pdf`) as the window opens; Save copy… hands the chosen one to the
//! user as `<name>-compressed.pdf`, and the open document doesn't change.
//! Look: assets/css/compress.css.

use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;
use std::rc::Rc;

const SIGNATURE_CSS: Asset = asset!("/assets/css/signature.css");
const DOC_CSS: Asset = asset!("/assets/css/doc.css");
const COMPRESS_CSS: Asset = asset!("/assets/css/compress.css");
/// The rail row's icon (Devigner Box, picked by the user).
pub const ICON_BOX: &str = include_str!("../assets/icons/devigner/Box.svg");
/// Transitions.dev "Modal open/close" close time (ms).
const CLOSE_MS: i32 = 200;
/// Less than this much smaller: "already about as small as it gets".
const WORTH: f64 = 0.05;
/// The levels: tab, what it does.
const LEVELS: [(&str, &str); 3] = [
    (
        "Best quality",
        "Nothing in the pictures changes; the file is tidied and packed.",
    ),
    (
        "Medium",
        "Pictures at 150 dpi: sharp on screen and in print. Recommended.",
    ),
    (
        "Smallest",
        "Pictures at 100 dpi: fine on screen, softer in print.",
    ),
];

#[derive(Clone, PartialEq)]
enum Outcome {
    Working,
    Done(Rc<Vec<u8>>),
    Failed(String),
}

#[derive(Clone, PartialEq)]
struct Dialog {
    level: usize,
    name: String,
    before: usize,
    copies: [Outcome; 3],
    status: Option<String>,
    busy: bool,
    shown: bool,
    closing: bool,
}

/// Shared Compress state.
#[derive(Clone, Copy)]
pub struct Compress {
    dialog: Signal<Option<Dialog>>,
}

impl Compress {
    pub fn provide() -> Self {
        use_context_provider(|| Compress {
            dialog: Signal::new(None),
        })
    }

    /// Opens the window and works out the three sizes (Medium first: it's chosen).
    pub fn open(mut self) {
        if self.dialog.peek().as_ref().is_some_and(|d| !d.closing) {
            return;
        }
        let document = consume_context::<Signal<Option<crate::pages::OpenDocument>>>();
        let Some(name) = document.peek().as_ref().map(|d| d.name.clone()) else {
            return;
        };
        consume_context::<crate::editing::Editing>().keep_waiting();
        let pdf = match pdit_core::save_document() {
            Ok(bytes) => bytes,
            Err(error) => return crate::log(&format!("pdit: compress: {error}")),
        };
        self.dialog.set(Some(Dialog {
            level: 1,
            name,
            before: pdf.len(),
            copies: [Outcome::Working, Outcome::Working, Outcome::Working],
            status: None,
            busy: false,
            shown: false,
            closing: false,
        }));
        let mut dialog = self.dialog;
        next_frame(move || {
            next_frame(move || {
                dialog.with_mut(|d| {
                    if let Some(d) = d.as_mut() {
                        d.shown = true;
                    }
                })
            })
        });
        crate::doc_ui::place_pills_soon(".pdit-compress .t-tabs");
        spawn(async move {
            for level in [1usize, 0, 2] {
                let copy = match crate::ai_ui::desktop::invoke_raw(
                    "compress_pdf",
                    &pdf,
                    &[("x-level", level.to_string())],
                )
                .await
                {
                    Ok(out) => Outcome::Done(Rc::new(js_sys::Uint8Array::new(&out).to_vec())),
                    Err(e) => Outcome::Failed(e.as_string().unwrap_or_else(|| format!("{e:?}"))),
                };
                if let Outcome::Failed(error) = &copy {
                    crate::log(&format!("pdit: compress level {level}: {error}"));
                }
                // The window may have been closed meanwhile.
                if self.dialog.peek().is_none() {
                    return;
                }
                self.edit(|d| d.copies[level] = copy);
            }
        });
    }

    pub fn close(self) {
        if self
            .dialog
            .peek()
            .as_ref()
            .is_none_or(|d| d.closing || d.busy)
        {
            return;
        }
        self.edit(|d| d.closing = true);
        let mut dialog = self.dialog;
        set_timeout(CLOSE_MS, move || {
            if dialog.peek().as_ref().is_some_and(|d| d.closing) {
                dialog.set(None);
            }
        });
    }

    fn edit(mut self, f: impl FnOnce(&mut Dialog)) {
        self.dialog.with_mut(|d| {
            if let Some(d) = d.as_mut() {
                f(d);
            }
        });
    }

    /// Save copy…: the chosen level's file, through the system Save window.
    fn save(self) {
        let Some(d) = self.dialog.peek().clone() else {
            return;
        };
        let Outcome::Done(bytes) = d.copies[d.level].clone() else {
            return;
        };
        let name = crate::save::derived_name(&d.name, "compressed");
        self.edit(|d| {
            d.busy = true;
            d.status = Some(format!("Saving “{name}”…"));
        });
        spawn(async move {
            let result = crate::save::save_file(&bytes, &name).await;
            self.edit(|d| d.busy = false);
            match result {
                Ok(true) => {
                    self.edit(|d| d.status = Some(format!("Saved — {}.", size(bytes.len()))));
                    set_timeout(1400, move || self.close());
                }
                Ok(false) => self.edit(|d| d.status = None),
                Err(error) => {
                    crate::log(&format!(
                        "pdit: could not save the compressed PDF: {error:?}"
                    ));
                    self.edit(|d| d.status = Some("The copy couldn't be saved.".into()));
                }
            }
        });
    }
}

/// 1.2 MB, 504 KB.
fn size(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{} KB", (bytes as f64 / 1024.0).round().max(1.0))
    }
}

#[component]
pub fn CompressUi() -> Element {
    let compress = use_context::<Compress>();
    // Keep the tab pill under the chosen level.
    let pill_key = compress.dialog.read().as_ref().map(|d| (d.shown, d.level));
    use_effect(use_reactive!(|pill_key| {
        if pill_key.is_some() {
            crate::doc_ui::place_pills_soon(".pdit-compress .t-tabs");
        }
    }));
    let Some(d) = compress.dialog.read().clone() else {
        return rsx! {};
    };
    let class = match (d.shown, d.closing) {
        (_, true) => "doc-box t-modal is-closing",
        (true, false) => "doc-box t-modal is-open",
        (false, false) => "doc-box t-modal",
    };
    let worth = match &d.copies[d.level] {
        Outcome::Done(out) => (out.len() as f64) < d.before as f64 * (1.0 - WORTH),
        _ => false,
    };
    rsx! {
        document::Stylesheet { href: SIGNATURE_CSS }
        document::Stylesheet { href: DOC_CSS }
        document::Stylesheet { href: COMPRESS_CSS }
        div { class: "doc-layer sa-root pdit-compress",
            div { class, role: "dialog", "aria-label": "Compress",
                div { class: "doc-head", span { class: "title", "Compress" } }
                div {
                    div { class: "doc-lbl", "Size" }
                    crate::doc_ui::Tabs {
                        options: LEVELS.iter().enumerate().map(|(i, (tab, _))| ((*tab).into(), i == d.level)).collect(),
                        on_pick: move |i| compress.edit(|d| d.level = i),
                    }
                }
                div { class: "cz-size",
                    match &d.copies[d.level] {
                        Outcome::Working => rsx! { span { class: "from", "Working out the sizes…" } },
                        Outcome::Failed(_) => rsx! { span { class: "from", "This level couldn't be worked out for this PDF." } },
                        Outcome::Done(out) if !worth => rsx! {
                            b { {size(d.before)} }
                            span { class: "from", "— already about as small as it gets" }
                            span { class: "from", {format!("({})", size(out.len()))} }
                        },
                        Outcome::Done(out) => rsx! {
                            span { class: "from", {format!("{} →", size(d.before))} }
                            b { {size(out.len())} }
                            span { class: "less",
                                {format!("{}% smaller", ((1.0 - out.len() as f64 / d.before as f64) * 100.0).round())}
                            }
                        },
                    }
                }
                div { class: "cz-what", {format!("{} Text and pages stay as they are.", LEVELS[d.level].1)} }
                div { class: "doc-foot",
                    span { class: "cz-status",
                        {d.status.clone().unwrap_or_else(|| "Saved as a copy — the open PDF doesn’t change.".into())}
                    }
                    span { class: "grow" }
                    button {
                        class: "sa-control", r#type: "button", disabled: d.busy,
                        onclick: move |_| compress.close(),
                        "Cancel"
                    }
                    button {
                        class: "sa-primary", r#type: "button", disabled: d.busy || !worth,
                        onclick: move |_| compress.save(),
                        "Save copy…"
                    }
                }
            }
        }
    }
}
