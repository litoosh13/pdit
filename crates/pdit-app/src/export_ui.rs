//! Export as (D-071; approved mockup .claude/research/convert-ui/): the rail's Document → Export as… opens a
//! window in the Print look: Format (Word | OpenDocument | Images | Text), Pages (All | This page | Pages…), and
//! for images the type (PNG | JPEG) and sharpness (96 | 200 | 300 dpi). Word and OpenDocument are made by MuPDF
//! in the desktop app (`to_office`); pictures go into one picked folder (save.rs `Folder`, as Split); the others
//! are one file. The open document doesn't change. Look: assets/css/export.css.

use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;

const SIGNATURE_CSS: Asset = asset!("/assets/css/signature.css");
const DOC_CSS: Asset = asset!("/assets/css/doc.css");
const EXPORT_CSS: Asset = asset!("/assets/css/export.css");
/// Devigner Export2, picked by the user.
pub const ICON_EXPORT: &str = include_str!("../assets/icons/devigner/Export2.svg");
/// Transitions.dev "Modal open/close" close time (ms).
const CLOSE_MS: i32 = 200;
const DPI: [(f32, &str); 3] = [
    (96.0, "Screen (96 dpi)"),
    (200.0, "Print (200 dpi)"),
    (300.0, "High (300 dpi)"),
];

/// What Export as makes.
#[derive(Clone, Copy, PartialEq)]
enum Format {
    Word,
    OpenDocument,
    Images,
    Text,
}

impl Format {
    const ALL: [Format; 4] = [
        Format::Word,
        Format::OpenDocument,
        Format::Images,
        Format::Text,
    ];

    fn tab(self) -> &'static str {
        match self {
            Format::Word => "Word",
            Format::OpenDocument => "OpenDocument",
            Format::Images => "Images",
            Format::Text => "Text",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Format::Word => {
                "Text as paragraphs you can edit in Word, with the pictures. Complex layouts may shift."
            }
            Format::OpenDocument => "The same, for LibreOffice and other open office apps.",
            Format::Images => "One picture per page, into a folder you choose.",
            Format::Text => "All the text, page after page, as a plain .txt file.",
        }
    }

    /// Word and OpenDocument are MuPDF's, in the desktop app.
    fn office(self) -> Option<&'static str> {
        match self {
            Format::Word => Some("docx"),
            Format::OpenDocument => Some("odt"),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq)]
struct Dialog {
    format: Format,
    /// 0 all pages, 1 this page, 2 a range.
    which: usize,
    current: u16,
    from: u16,
    to: u16,
    pages: u16,
    jpeg: bool,
    dpi: usize,
    name: String,
    status: Option<String>,
    busy: bool,
    shown: bool,
    closing: bool,
}

impl Dialog {
    /// The chosen pages, from 0.
    fn range(&self) -> (u16, u16) {
        match self.which {
            0 => (0, self.pages.saturating_sub(1)),
            1 => (self.current - 1, self.current - 1),
            _ => (self.from.min(self.to) - 1, self.from.max(self.to) - 1),
        }
    }
}

/// Shared Export as state.
#[derive(Clone, Copy)]
pub struct Export {
    dialog: Signal<Option<Dialog>>,
}

impl Export {
    pub fn provide() -> Self {
        use_context_provider(|| Export {
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
            format: if crate::ai_ui::desktop::available() {
                Format::Word
            } else {
                Format::Images
            },
            which: 0,
            current: (crate::bookmarks_ui::current_page() + 1).min(pages.max(1)),
            from: 1,
            to: pages.max(1),
            pages,
            jpeg: false,
            dpi: 1,
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
        crate::doc_ui::place_pills_soon(".pdit-export .t-tabs");
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

    fn save(self) {
        let Some(d) = self.dialog.peek().clone() else {
            return;
        };
        let stem = d
            .name
            .rsplit_once('.')
            .map_or(d.name.as_str(), |(s, _)| s)
            .to_owned();
        let (first, last) = d.range();
        self.edit(|d| d.busy = true);
        spawn(async move {
            let done = match d.format {
                Format::Text => save_text(&stem, first, last).await,
                Format::Images => {
                    self.save_pictures(&stem, first, last, d.jpeg, DPI[d.dpi].0)
                        .await
                }
                Format::Word | Format::OpenDocument => {
                    self.edit(|d| d.status = Some("Making the file…".into()));
                    save_office(&stem, first, last, d.format.office().unwrap_or("docx")).await
                }
            };
            self.edit(|d| {
                d.busy = false;
                d.status = done
                    .as_ref()
                    .ok()
                    .cloned()
                    .flatten()
                    .or_else(|| done.as_ref().err().cloned());
            });
            if matches!(done, Ok(Some(_))) {
                set_timeout(1600, move || self.close());
            }
        });
    }

    /// Each page as a picture into one folder; Ok(None) when the user cancelled.
    async fn save_pictures(
        self,
        stem: &str,
        first: u16,
        last: u16,
        jpeg: bool,
        dpi: f32,
    ) -> Result<Option<String>, String> {
        let folder = match crate::save::Folder::pick().await {
            Ok(Some(folder)) => folder,
            Ok(None) => return Ok(None),
            Err(error) => {
                crate::log(&format!("pdit: export: {error:?}"));
                return Err("No folder could be opened.".into());
            }
        };
        let total = last - first + 1;
        for (k, page) in (first..=last).enumerate() {
            self.edit(|d| d.status = Some(format!("Saving picture {} of {total}…", k + 1)));
            crate::print_ui::pause(30).await;
            let name = format!(
                "{stem} page {}.{}",
                page + 1,
                if jpeg { "jpg" } else { "png" }
            );
            let saved = match pdit_core::page_ops::page_picture(page, dpi, jpeg) {
                Ok(bytes) => folder
                    .save(&bytes, &name)
                    .await
                    .map_err(|e| format!("{e:?}")),
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = saved {
                crate::log(&format!("pdit: export, {name}: {error}"));
                return Err(format!(
                    "Saved {k} of {total} pictures; “{name}” couldn't be saved."
                ));
            }
        }
        Ok(Some(format!(
            "Saved {total} picture{} in “{}”.",
            if total == 1 { "" } else { "s" },
            folder.name()
        )))
    }
}

/// The pages as a Word / OpenDocument file (desktop `to_office`); Ok(None) when the user cancelled.
async fn save_office(
    stem: &str,
    first: u16,
    last: u16,
    ext: &str,
) -> Result<Option<String>, String> {
    let pdf = pdit_core::save_document().map_err(|e| e.to_string())?;
    let headers = [
        ("x-first", first.to_string()),
        ("x-last", last.to_string()),
        ("x-format", ext.to_owned()),
    ];
    let file = match crate::ai_ui::desktop::invoke_raw("to_office", &pdf, &headers).await {
        Ok(value) => js_sys::Uint8Array::new(&value).to_vec(),
        Err(error) => {
            crate::log(&format!("pdit: export {ext}: {error:?}"));
            return Err("This PDF couldn't be converted.".into());
        }
    };
    let name = format!("{stem}.{ext}");
    match crate::save::save_file(&file, &name).await {
        Ok(true) => Ok(Some(format!("Saved “{name}”."))),
        Ok(false) => Ok(None),
        Err(error) => {
            crate::log(&format!("pdit: export {ext}: {error:?}"));
            Err("The file couldn't be saved.".into())
        }
    }
}

/// The pages' text as one .txt file; Ok(None) when the user cancelled.
async fn save_text(stem: &str, first: u16, last: u16) -> Result<Option<String>, String> {
    let text = pdit_core::page_ops::pages_text(first, last).map_err(|e| e.to_string())?;
    let name = format!("{stem}.txt");
    match crate::save::save_file(text.as_bytes(), &name).await {
        Ok(true) => Ok(Some(format!("Saved “{name}”."))),
        Ok(false) => Ok(None),
        Err(error) => {
            crate::log(&format!("pdit: export text: {error:?}"));
            Err("The text couldn't be saved.".into())
        }
    }
}

#[component]
pub fn ExportUi() -> Element {
    let export = use_context::<Export>();
    let pill_key = export
        .dialog
        .read()
        .as_ref()
        .map(|d| (d.shown, d.format == Format::Images, d.which, d.jpeg, d.dpi));
    use_effect(use_reactive!(|pill_key| {
        if pill_key.is_some() {
            crate::doc_ui::place_pills_soon(".pdit-export .t-tabs");
        }
    }));
    let Some(d) = export.dialog.read().clone() else {
        return rsx! {};
    };
    let class = match (d.shown, d.closing) {
        (_, true) => "doc-box t-modal is-closing",
        (true, false) => "doc-box t-modal is-open",
        (false, false) => "doc-box t-modal",
    };
    let (first, last) = d.range();
    let count = last - first + 1;
    let pages = d.pages;
    let go = match d.format {
        Format::Images if count == 1 => "Save 1 picture…".to_owned(),
        Format::Images => format!("Save {count} pictures…"),
        _ => "Save…".to_owned(),
    };
    // Word and OpenDocument need the desktop app.
    let unavailable = d.format.office().is_some() && !crate::ai_ui::desktop::available();
    let this_page = format!("This page ({})", d.current);
    rsx! {
        document::Stylesheet { href: SIGNATURE_CSS }
        document::Stylesheet { href: DOC_CSS }
        document::Stylesheet { href: EXPORT_CSS }
        div { class: "doc-layer sa-root pdit-export",
            div { class, role: "dialog", "aria-label": "Export as",
                div { class: "doc-head", span { class: "title", "Export as" } }
                div {
                    div { class: "doc-lbl", "Format" }
                    crate::doc_ui::Tabs {
                        options: Format::ALL.iter().map(|f| (f.tab().into(), *f == d.format)).collect(),
                        on_pick: move |i| export.edit(|d| d.format = Format::ALL[i]),
                    }
                }
                if unavailable {
                    div { class: "ex-note", "Word and OpenDocument are made in the desktop app." }
                }
                div {
                    div { class: "doc-lbl", "Pages" }
                    crate::doc_ui::Tabs {
                        options: vec![("All pages".into(), d.which == 0), (this_page, d.which == 1), ("Pages…".into(), d.which == 2)],
                        on_pick: move |i| export.edit(|d| d.which = i),
                    }
                }
                if d.which == 2 {
                    div { class: "doc-row",
                        label { "From" }
                        input {
                            class: "doc-field doc-range", r#type: "number", min: "1", max: "{pages}", value: "{d.from}",
                            "aria-label": "From page",
                            oninput: move |e| {
                                let v = e.value().parse::<u16>().unwrap_or(1).clamp(1, pages.max(1));
                                export.edit(|d| d.from = v);
                            },
                        }
                        label { "to" }
                        input {
                            class: "doc-field doc-range", r#type: "number", min: "1", max: "{pages}", value: "{d.to}",
                            "aria-label": "To page",
                            oninput: move |e| {
                                let v = e.value().parse::<u16>().unwrap_or(pages).clamp(1, pages.max(1));
                                export.edit(|d| d.to = v);
                            },
                        }
                        span { class: "doc-wv", "of {pages}" }
                    }
                }
                if d.format == Format::Images {
                    div {
                        div { class: "doc-lbl", "Picture type" }
                        crate::doc_ui::Tabs {
                            options: vec![("PNG".into(), !d.jpeg), ("JPEG".into(), d.jpeg)],
                            on_pick: move |i| export.edit(|d| d.jpeg = i == 1),
                        }
                    }
                    div {
                        div { class: "doc-lbl", "Sharpness" }
                        crate::doc_ui::Tabs {
                            options: DPI.iter().enumerate().map(|(i, (_, t))| ((*t).into(), i == d.dpi)).collect(),
                            on_pick: move |i| export.edit(|d| d.dpi = i),
                        }
                    }
                }
                div { class: "ex-note", "{d.format.note()}" }
                div { class: "doc-foot",
                    span { class: "ex-status",
                        {d.status.clone().unwrap_or_else(|| match d.format {
                            Format::Images => "Saved as new pictures — the open PDF doesn’t change.".into(),
                            f => format!("Saved as a new .{} file — the open PDF doesn’t change.", f.office().unwrap_or("txt")),
                        })}
                    }
                    span { class: "grow" }
                    button { class: "sa-control", r#type: "button", disabled: d.busy, onclick: move |_| export.close(), "Cancel" }
                    button { class: "sa-primary", r#type: "button", disabled: d.busy || unavailable, onclick: move |_| export.save(), "{go}" }
                }
            }
        }
    }
}
