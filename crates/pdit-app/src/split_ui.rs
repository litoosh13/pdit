//! Split into files (D-070; approved mockup .claude/research/page-tools-ui/):
//! the rail's Pages → Split into files… opens a window in the Print look:
//! Every page | Every … pages | After pages…, the files it makes, and Save n
//! files… — one folder is picked, then each file is written into it (save.rs
//! `Folder`). The open document doesn't change. Look: assets/css/split.css.

use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;
use pdit_core::split::{Split, describe, groups};

const SIGNATURE_CSS: Asset = asset!("/assets/css/signature.css");
const DOC_CSS: Asset = asset!("/assets/css/doc.css");
const SPLIT_CSS: Asset = asset!("/assets/css/split.css");
/// Devigner Scissor, picked by the user.
pub const ICON_SCISSOR: &str = include_str!("../assets/icons/devigner/Scissor.svg");
/// Transitions.dev "Modal open/close" close time (ms).
const CLOSE_MS: i32 = 200;

#[derive(Clone, PartialEq)]
struct Dialog {
    /// 0 every page, 1 every n pages, 2 after pages.
    how: usize,
    every: u16,
    after: String,
    pages: u16,
    name: String,
    status: Option<String>,
    busy: bool,
    shown: bool,
    closing: bool,
}

impl Dialog {
    fn split(&self) -> Split {
        match self.how {
            0 => Split::EveryPage,
            1 => Split::Every(self.every),
            _ => Split::After(self.after.clone()),
        }
    }
}

/// Shared Split state.
#[derive(Clone, Copy)]
pub struct SplitFiles {
    dialog: Signal<Option<Dialog>>,
}

impl SplitFiles {
    pub fn provide() -> Self {
        use_context_provider(|| SplitFiles {
            dialog: Signal::new(None),
        })
    }

    pub fn open(mut self) {
        if self.dialog.peek().as_ref().is_some_and(|d| !d.closing) {
            return;
        }
        let document = consume_context::<Signal<Option<crate::pages::OpenDocument>>>();
        let Some((name, pages)) = document
            .peek()
            .as_ref()
            .map(|d| (d.name.clone(), d.page_sizes.len() as u16))
        else {
            return;
        };
        consume_context::<crate::editing::Editing>().keep_waiting();
        self.dialog.set(Some(Dialog {
            // Every 2 pages, unless that makes one file (a 2-page PDF): every page then.
            how: if pages > 2 { 1 } else { 0 },
            every: 2,
            after: String::new(),
            pages,
            name,
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
        crate::doc_ui::place_pills_soon(".pdit-split .t-tabs");
    }

    fn edit(mut self, f: impl FnOnce(&mut Dialog)) {
        self.dialog.with_mut(|d| {
            if let Some(d) = d.as_mut() {
                f(d);
            }
        });
    }

    fn close(self) {
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

    /// Save n files…: one folder, then each file.
    fn save(self) {
        let Some(d) = self.dialog.peek().clone() else {
            return;
        };
        let files = groups(d.pages, &d.split());
        let stem = d
            .name
            .strip_suffix(".pdf")
            .or_else(|| d.name.strip_suffix(".PDF"))
            .unwrap_or(&d.name)
            .to_owned();
        self.edit(|d| d.busy = true);
        spawn(async move {
            let folder = match crate::save::Folder::pick().await {
                Ok(Some(folder)) => folder,
                Ok(None) => return self.edit(|d| d.busy = false),
                Err(error) => {
                    crate::log(&format!("pdit: split: {error:?}"));
                    return self.edit(|d| {
                        d.busy = false;
                        d.status = Some("No folder could be opened.".into());
                    });
                }
            };
            let total = files.len();
            for (k, &(first, last)) in files.iter().enumerate() {
                self.edit(|d| d.status = Some(format!("Saving file {} of {total}…", k + 1)));
                crate::print_ui::pause(30).await;
                let name = format!("{stem} {}.pdf", describe((first, last)));
                let saved = match pdit_core::page_ops::extract_pages(first, last) {
                    Ok(bytes) => folder
                        .save(&bytes, &name)
                        .await
                        .map_err(|e| format!("{e:?}")),
                    Err(error) => Err(error.to_string()),
                };
                if let Err(error) = saved {
                    crate::log(&format!("pdit: split, {name}: {error}"));
                    return self.edit(|d| {
                        d.busy = false;
                        d.status = Some(format!(
                            "Saved {k} of {total} files; “{name}” couldn't be saved."
                        ));
                    });
                }
            }
            self.edit(|d| {
                d.busy = false;
                d.status = Some(format!("Saved {total} files in “{}”.", folder.name()));
            });
            set_timeout(1600, move || self.close());
        });
    }
}

#[component]
pub fn SplitUi() -> Element {
    let split = use_context::<SplitFiles>();
    let pill_key = split.dialog.read().as_ref().map(|d| (d.shown, d.how));
    use_effect(use_reactive!(|pill_key| {
        if pill_key.is_some() {
            crate::doc_ui::place_pills_soon(".pdit-split .t-tabs");
        }
    }));
    let Some(d) = split.dialog.read().clone() else {
        return rsx! {};
    };
    let class = match (d.shown, d.closing) {
        (_, true) => "doc-box t-modal is-closing",
        (true, false) => "doc-box t-modal is-open",
        (false, false) => "doc-box t-modal",
    };
    let files = groups(d.pages, &d.split());
    let list = files
        .iter()
        .map(|&g| describe(g))
        .collect::<Vec<_>>()
        .join(", ");
    let n = files.len();
    rsx! {
        document::Stylesheet { href: SIGNATURE_CSS }
        document::Stylesheet { href: DOC_CSS }
        document::Stylesheet { href: SPLIT_CSS }
        div { class: "doc-layer sa-root pdit-split",
            div { class, role: "dialog", "aria-label": "Split into files",
                div { class: "doc-head", span { class: "title", "Split into files" } }
                div {
                    div { class: "doc-lbl", "Split" }
                    crate::doc_ui::Tabs {
                        options: vec![
                            ("Every page".into(), d.how == 0),
                            ("Every … pages".into(), d.how == 1),
                            ("After pages…".into(), d.how == 2),
                        ],
                        on_pick: move |i| split.edit(|d| d.how = i),
                    }
                }
                if d.how == 1 {
                    div { class: "doc-row",
                        label { "Pages per file" }
                        input {
                            class: "doc-field doc-range", r#type: "number", min: "1", max: "{d.pages}", value: "{d.every}",
                            "aria-label": "Pages per file",
                            oninput: move |e| {
                                let v = e.value().parse::<u16>().unwrap_or(1).max(1);
                                split.edit(|d| d.every = v.min(d.pages.max(1)));
                            },
                        }
                    }
                }
                if d.how == 2 {
                    div { class: "doc-row",
                        label { "Split after pages" }
                        input {
                            class: "doc-field split-after", r#type: "text", value: "{d.after}", placeholder: "e.g. 3, 5",
                            "aria-label": "Split after pages",
                            oninput: move |e| split.edit(|d| d.after = e.value()),
                        }
                    }
                }
                div { class: "split-files",
                    "Makes "
                    b { if n == 1 { "1 file" } else { "{n} files" } }
                    ": {list}"
                }
                div { class: "doc-foot",
                    span { class: "split-status",
                        {d.status.clone().unwrap_or_else(|| "Saved as new files in a folder you choose — the open PDF doesn’t change.".into())}
                    }
                    span { class: "grow" }
                    button { class: "sa-control", r#type: "button", disabled: d.busy, onclick: move |_| split.close(), "Cancel" }
                    button {
                        class: "sa-primary", r#type: "button", disabled: d.busy || n < 2,
                        onclick: move |_| split.save(),
                        if n == 1 { "Save 1 file…" } else { "Save {n} files…" }
                    }
                }
            }
        }
    }
}
