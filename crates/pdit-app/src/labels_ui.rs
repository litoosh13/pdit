//! Page numbers (page labels, D-070; approved mockup .claude/research/page-tools-ui/): the rail's Pages →
//! Page numbers… (desktop app: MuPDF writes them, desktop `set_page_labels`) opens a window in the Print look
//! with ranges — from page, style (pdit's dropdown: a Selection Actions button opening the right-click menu's
//! list, as the format bar's Font menu), prefix, start at — and a preview. Done: one Undo. The pages' rows then
//! show "Page iii (3)". Look: assets/css/labels.css.

use crate::page_tools::{PageTools, next_frame, set_timeout};
use dioxus::prelude::*;
use pdit_core::labels::{Range, Style, is_plain, label, ranges_from_labels};

const SIGNATURE_CSS: Asset = asset!("/assets/css/signature.css");
const DOC_CSS: Asset = asset!("/assets/css/doc.css");
const CONTEXT_MENU_CSS: Asset = asset!("/assets/css/context-menu.css");
const LABELS_CSS: Asset = asset!("/assets/css/labels.css");
/// Devigner Tag, picked by the user.
pub const ICON_TAG: &str = include_str!("../assets/icons/devigner/Tag.svg");
const ICON_REMOVE: &str = include_str!("../assets/icons/devigner/TrashBinMinimalistic.svg");
/// Transitions.dev "Modal open/close" close time (ms).
const CLOSE_MS: i32 = 200;
/// Labels shown in the preview before "…".
const PREVIEW: u16 = 12;

#[derive(Clone, PartialEq)]
struct Dialog {
    ranges: Vec<Range>,
    pages: u16,
    /// The range whose style menu is open.
    menu: Option<usize>,
    status: Option<String>,
    busy: bool,
    shown: bool,
    closing: bool,
}

/// Shared Page numbers state.
#[derive(Clone, Copy)]
pub struct PageNumbers {
    dialog: Signal<Option<Dialog>>,
}

impl PageNumbers {
    pub fn provide() -> Self {
        use_context_provider(|| PageNumbers {
            dialog: Signal::new(None),
        })
    }

    /// Opens the window with the PDF's own ranges (read back from its labels).
    pub fn open(mut self) {
        if self.dialog.peek().as_ref().is_some_and(|d| !d.closing) {
            return;
        }
        let Ok(labels) = pdit_core::page_ops::page_labels() else {
            return;
        };
        self.dialog.set(Some(Dialog {
            ranges: ranges_from_labels(&labels),
            pages: labels.len() as u16,
            menu: None,
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

    /// Done: MuPDF writes the ranges (none when they're the plain 1, 2, 3); one Undo.
    fn done(self) {
        let Some(d) = self.dialog.peek().clone() else {
            return;
        };
        let ranges = tidy(&d.ranges);
        let spec = spec(&ranges);
        let tools = consume_context::<PageTools>();
        consume_context::<crate::editing::Editing>().keep_waiting();
        let Some(before) = tools.snapshot() else {
            return;
        };
        self.edit(|d| d.busy = true);
        spawn(async move {
            let headers = [(
                "x-labels",
                String::from(js_sys::encode_uri_component(&spec)),
            )];
            match crate::ai_ui::desktop::invoke_raw("set_page_labels", &before, &headers).await {
                Ok(value) => {
                    let bytes = js_sys::Uint8Array::new(&value).to_vec();
                    self.edit(|d| d.busy = false);
                    tools.apply_since(before, "Page numbers changed", ICON_TAG, move || {
                        pdit_core::history::replace_document(bytes).map(|_| ())
                    });
                    self.close();
                }
                Err(error) => {
                    crate::log(&format!("pdit: page numbers: {error:?}"));
                    self.edit(|d| {
                        d.busy = false;
                        d.status = Some("The page numbers couldn't be changed.".into());
                    });
                }
            }
        });
    }
}

/// The ranges for the desktop's `set_page_labels` (x-labels): one per line, "first TAB style TAB start TAB
/// prefix"; empty for the plain 1, 2, 3 (no labels).
pub(crate) fn spec(ranges: &[Range]) -> String {
    if is_plain(ranges) {
        return String::new();
    }
    ranges
        .iter()
        .map(|r| {
            let code = r.style.code();
            let code = if code == 0 {
                String::new()
            } else {
                char::from(code).to_string()
            };
            format!(
                "{}\t{code}\t{}\t{}\n",
                r.first,
                r.start,
                r.prefix.replace(['\t', '\n'], " ")
            )
        })
        .collect()
}

/// Ranges in page order, one per first page; the first starts at page 1.
fn tidy(ranges: &[Range]) -> Vec<Range> {
    let mut out = ranges.to_vec();
    out.sort_by_key(|r| r.first);
    out.dedup_by_key(|r| r.first);
    if let Some(first) = out.first_mut() {
        first.first = 0;
    }
    out
}

#[component]
pub fn LabelsUi() -> Element {
    let numbers = use_context::<PageNumbers>();
    let Some(d) = numbers.dialog.read().clone() else {
        return rsx! {};
    };
    let class = match (d.shown, d.closing) {
        (_, true) => "doc-box t-modal is-closing",
        (true, false) => "doc-box t-modal is-open",
        (false, false) => "doc-box t-modal",
    };
    let ranges = tidy(&d.ranges);
    let mut preview: Vec<String> = (0..d.pages.min(PREVIEW))
        .map(|p| label(&ranges, p))
        .map(|l| if l.is_empty() { "–".into() } else { l })
        .collect();
    if d.pages > PREVIEW {
        preview.push("…".into());
    }
    let preview = preview.join(", ");
    let pages = d.pages;
    rsx! {
        document::Stylesheet { href: SIGNATURE_CSS }
        document::Stylesheet { href: DOC_CSS }
        document::Stylesheet { href: CONTEXT_MENU_CSS }
        document::Stylesheet { href: LABELS_CSS }
        div { class: "doc-layer sa-root pdit-labels",
            div { class, role: "dialog", "aria-label": "Page numbers",
                div { class: "doc-head", span { class: "title", "Page numbers" } }
                div { class: "lb-ranges",
                    for (k, r) in d.ranges.iter().cloned().enumerate() {
                        div { key: "{k}", class: "lb-range",
                            label { "From page" }
                            input {
                                class: "doc-field lb-num", r#type: "number", min: "1", max: "{pages}",
                                value: "{r.first + 1}", disabled: k == 0, "aria-label": "From page",
                                oninput: move |e| {
                                    let v = e.value().parse::<u16>().unwrap_or(1).clamp(1, pages.max(1));
                                    numbers.edit(|d| d.ranges[k].first = v - 1);
                                },
                            }
                            div { class: "lb-style",
                                button {
                                    class: "sa-control", r#type: "button", "aria-haspopup": "menu",
                                    onclick: move |_| numbers.edit(|d| d.menu = if d.menu == Some(k) { None } else { Some(k) }),
                                    "{r.style.name()} ▾"
                                }
                                if d.menu == Some(k) {
                                    div { class: "cm-menu t-dropdown is-open", role: "menu",
                                        div { class: "cm-label", "Numbers" }
                                        for style in Style::ALL {
                                            button {
                                                r#type: "button", role: "menuitem",
                                                onclick: move |_| numbers.edit(|d| {
                                                    d.ranges[k].style = style;
                                                    d.menu = None;
                                                }),
                                                "{style.name()}"
                                            }
                                        }
                                    }
                                }
                            }
                            label { "Prefix" }
                            input {
                                class: "doc-field lb-prefix", r#type: "text", value: "{r.prefix}", placeholder: "none",
                                "aria-label": "Prefix",
                                oninput: move |e| numbers.edit(|d| d.ranges[k].prefix = e.value()),
                            }
                            label { "Start at" }
                            input {
                                class: "doc-field lb-num", r#type: "number", min: "1", value: "{r.start}",
                                "aria-label": "Start at",
                                oninput: move |e| {
                                    let v = e.value().parse::<u32>().unwrap_or(1).max(1);
                                    numbers.edit(|d| d.ranges[k].start = v);
                                },
                            }
                            if k > 0 {
                                button {
                                    class: "sa-control lb-remove", r#type: "button", "aria-label": "Remove this range",
                                    onclick: move |_| numbers.edit(|d| {
                                        d.ranges.remove(k);
                                        d.menu = None;
                                    }),
                                    span { dangerous_inner_html: ICON_REMOVE, style: "display: contents" }
                                }
                            }
                        }
                    }
                    if d.ranges.len() < usize::from(pages) {
                        button {
                            class: "sa-control lb-add", r#type: "button",
                            onclick: move |_| numbers.edit(|d| {
                                let last = d.ranges.iter().map(|r| r.first).max().unwrap_or(0);
                                d.ranges.push(Range { first: (last + 1).min(d.pages.saturating_sub(1)), ..Range::plain() });
                            }),
                            "+ Add a range"
                        }
                    }
                }
                div { class: "lb-preview", "Pages read: " b { "{preview}" } }
                div { class: "doc-foot",
                    span { class: "lb-status",
                        {d.status.clone().unwrap_or_else(|| "The numbers PDF readers show in their page box; the printed pages stay the same.".into())}
                    }
                    span { class: "grow" }
                    button { class: "sa-control", r#type: "button", disabled: d.busy, onclick: move |_| numbers.close(), "Cancel" }
                    button { class: "sa-primary", r#type: "button", disabled: d.busy, onclick: move |_| numbers.done(), "Done" }
                }
            }
        }
    }
}
