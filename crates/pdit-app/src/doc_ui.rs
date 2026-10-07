//! Header & footer and watermark (D-042): right-click → Document → Header &
//! footer… / Watermark… opens a window (the signature window: Transitions.dev
//! "Modal open/close" on the menu's surface, "Tabs sliding" for two-way
//! choices) docked at the bottom, while the pages preview the choice live
//! (dashed outline). Apply writes it into the chosen pages with one Undo.
//!
//! While the window is open, an existing header & footer (or watermark) is
//! taken off the pages so the preview isn't doubled; Cancel puts the document
//! back as it was, and Apply's Undo returns to that same state.

use crate::page_tools::{PageTools, next_frame, set_timeout};
use dioxus::prelude::*;
use pdit_core::doc_content::{self as doc, HeaderFooter, Watermark};
use std::rc::Rc;
use wasm_bindgen::JsCast;

const DOC_CSS: Asset = asset!("/assets/css/doc.css");
const SIGNATURE_CSS: Asset = asset!("/assets/css/signature.css");
pub const ICON_HF: &str = include_str!("../assets/icons/cartoon-layout-template.svg");
pub const ICON_WM: &str = include_str!("../assets/icons/cartoon-watermark.svg");
/// The modal's close duration (--modal-close-dur).
const CLOSE_MS: i32 = 150;
/// The watermark colours (D-042, approved): grey, red, blue, green.
const WM_COLORS: [(&str, [u8; 3]); 4] = [
    ("Grey", [138, 141, 147]),
    ("Red", [211, 58, 44]),
    ("Blue", [29, 111, 216]),
    ("Green", [31, 157, 85]),
];
/// Must match the engine's layout (pdit-core doc_content).
const HF_EDGE: f32 = 0.045;
const HF_SIDE: f32 = 0.07;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    HeaderFooter,
    Watermark,
}

/// What the window edits: the settings plus the page choice.
#[derive(Clone, PartialEq)]
struct Draft {
    kind: Kind,
    hf: HeaderFooter,
    wm: Watermark,
    /// All pages, or `from`–`to` (1-based).
    all: bool,
    from: u16,
    to: u16,
    /// One existed before (offer Remove; messages say "updated").
    existed: bool,
}

#[derive(Clone)]
struct Window {
    draft: Draft,
    /// The document as it was before the window opened (Cancel / Undo).
    before: Option<Rc<Vec<u8>>>,
    shown: bool,
    closing: bool,
}

/// Shared state of the Document windows.
#[derive(Clone, Copy)]
pub struct DocTools {
    window: Signal<Option<Window>>,
    /// The text field a chip inserts into ("h0".."f2"), and the caret there.
    focus: Signal<(&'static str, Option<u32>)>,
}

impl DocTools {
    pub fn provide() -> Self {
        use_context_provider(|| DocTools {
            window: Signal::new(None),
            focus: Signal::new(("f1", None)),
        })
    }

    /// Right-click → Document → Header & footer… / Watermark….
    pub fn open(mut self, kind: Kind) {
        let tools = consume_context::<PageTools>();
        let pages = page_count();
        let (hf_now, wm_now) = (
            doc::header_footer().ok().flatten(),
            doc::watermark().ok().flatten(),
        );
        let existed = match kind {
            Kind::HeaderFooter => hf_now.is_some(),
            Kind::Watermark => wm_now.is_some(),
        };
        let range = match kind {
            Kind::HeaderFooter => hf_now.as_ref().and_then(|h| h.pages),
            Kind::Watermark => wm_now.as_ref().and_then(|w| w.pages),
        };
        let draft = Draft {
            kind,
            hf: hf_now.unwrap_or(HeaderFooter {
                header: Default::default(),
                footer: [
                    String::new(),
                    "Page {page} of {pages}".into(),
                    String::new(),
                ],
                size_pt: 9.0,
                pages: None,
            }),
            wm: wm_now.unwrap_or(Watermark {
                text: "CONFIDENTIAL".into(),
                color: WM_COLORS[0].1,
                opacity: 0.25,
                size_pt: 54.0,
                diagonal: true,
                pages: None,
            }),
            all: range.is_none(),
            from: range.map_or(1, |r| r.0),
            to: range.map_or(pages, |r| r.1),
            existed,
        };
        // Take the existing one off the pages while editing (no doubled preview).
        let before = tools.snapshot();
        if existed {
            let removed = match kind {
                Kind::HeaderFooter => doc::remove_header_footer(),
                Kind::Watermark => doc::remove_watermark(),
            };
            if let Err(error) = removed {
                crate::log(&format!(
                    "pdit: could not take the old one off for editing: {error}"
                ));
            }
            tools.refresh_pages();
        }
        self.focus.set(("f1", None));
        self.window.set(Some(Window {
            draft,
            before,
            shown: false,
            closing: false,
        }));
        let mut window = self.window;
        next_frame(move || {
            next_frame(move || {
                window.with_mut(|w| {
                    if let Some(w) = w.as_mut() {
                        w.shown = true;
                    }
                })
            })
        });
    }

    fn close(mut self) {
        self.window.with_mut(|w| {
            if let Some(w) = w.as_mut() {
                w.closing = true;
            }
        });
        let mut window = self.window;
        set_timeout(CLOSE_MS, move || {
            if window.peek().as_ref().is_some_and(|w| w.closing) {
                window.set(None);
            }
        });
    }

    /// Cancel / Escape: the document goes back to how it was.
    fn cancel(self) {
        let Some(w) = self.window.peek().clone().filter(|w| !w.closing) else {
            return;
        };
        if w.draft.existed
            && let Some(before) = &w.before
        {
            consume_context::<PageTools>().restore_snapshot(before);
        }
        self.close();
    }

    fn apply(self) {
        let Some(w) = self.window.peek().clone().filter(|w| !w.closing) else {
            return;
        };
        let Some(before) = w.before.clone() else {
            return self.close();
        };
        let d = w.draft;
        let pages = (!d.all).then_some((d.from.min(d.to), d.from.max(d.to)));
        let tools = consume_context::<PageTools>();
        match d.kind {
            Kind::HeaderFooter => {
                let hf = HeaderFooter { pages, ..d.hf };
                let message = if d.existed {
                    "Header & footer updated"
                } else {
                    "Header & footer added"
                };
                let date = crate::annotations_ui::today();
                tools.apply_since(before, message, ICON_HF, || {
                    doc::set_header_footer(&hf, &date)
                });
            }
            Kind::Watermark => {
                let wm = Watermark { pages, ..d.wm };
                let message = if d.existed {
                    "Watermark updated"
                } else {
                    "Watermark added"
                };
                tools.apply_since(before, message, ICON_WM, || doc::set_watermark(&wm));
            }
        }
        self.close();
    }

    /// Remove: the existing one is already off the pages; keep it that way.
    fn remove(self) {
        let Some(w) = self.window.peek().clone().filter(|w| !w.closing) else {
            return;
        };
        if let Some(before) = w.before.clone() {
            let (message, icon) = match w.draft.kind {
                Kind::HeaderFooter => ("Header & footer removed", ICON_HF),
                Kind::Watermark => ("Watermark removed", ICON_WM),
            };
            consume_context::<PageTools>().apply_since(before, message, icon, || Ok(()));
        }
        self.close();
    }

    fn edit(mut self, f: impl FnOnce(&mut Draft)) {
        self.window.with_mut(|w| {
            if let Some(w) = w.as_mut() {
                f(&mut w.draft);
            }
        });
    }

    /// Inserts `token` into the focused (or last typed-in) field at its caret.
    fn insert(self, token: &str) {
        let (field, caret) = *self.focus.peek();
        self.edit(|d| {
            let slot = text_slot(&mut d.hf, field);
            let at = caret.map_or(slot.encode_utf16().count(), |c| c as usize);
            let byte = char_index_to_byte(slot, at);
            slot.insert_str(byte, token);
        });
    }
}

fn text_slot<'a>(hf: &'a mut HeaderFooter, field: &str) -> &'a mut String {
    let i = (field.as_bytes()[1] - b'0') as usize;
    if field.starts_with('h') {
        &mut hf.header[i]
    } else {
        &mut hf.footer[i]
    }
}

/// The byte offset of the `utf16_index`-th UTF-16 unit (the caret the input
/// reports) in `s`.
fn char_index_to_byte(s: &str, utf16_index: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in s.char_indices() {
        if units >= utf16_index {
            return byte;
        }
        units += ch.len_utf16();
    }
    s.len()
}

fn page_count() -> u16 {
    pdit_core::page_ops::page_sizes().map_or(1, |s| s.len().max(1) as u16)
}

fn fill(template: &str, page: u16, pages: u16) -> String {
    template
        .replace("{page}", &page.to_string())
        .replace("{pages}", &pages.to_string())
        .replace("{date}", &crate::annotations_ui::today())
}

/// The window, rendered once at the app root.
#[component]
pub fn DocWindow() -> Element {
    let tools = use_context::<DocTools>();
    // Escape cancels.
    use_hook(move || {
        let on_key = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if event.key() == "Escape" && tools.window.peek().is_some() {
                    tools.cancel();
                }
            },
        );
        if let Some(window) = web_sys::window() {
            let _ =
                window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        }
        on_key.forget();
    });
    // Keep the tab pills under the selected tabs (hooks run on every render).
    let key = tools.window.read().as_ref().map(|w| {
        (
            w.draft.all,
            w.draft.wm.diagonal,
            w.shown,
            w.draft.kind == Kind::Watermark,
        )
    });
    use_effect(use_reactive!(|key| {
        if key.is_some() {
            place_pills_soon(".doc-box .t-tabs");
        }
    }));
    let Some(w) = tools.window.read().clone() else {
        return rsx! {
            document::Stylesheet { href: DOC_CSS }
        };
    };
    let d = w.draft.clone();
    let class = match (w.shown, w.closing) {
        (_, true) => "doc-box t-modal is-closing",
        (true, false) => "doc-box t-modal is-open",
        (false, false) => "doc-box t-modal",
    };
    let title = match d.kind {
        Kind::HeaderFooter => "Header & footer",
        Kind::Watermark => "Watermark",
    };
    let pages = page_count();
    rsx! {
        document::Stylesheet { href: SIGNATURE_CSS }
        document::Stylesheet { href: DOC_CSS }
        div { class: "doc-layer sa-root",
            div { class, role: "dialog", "aria-label": title,
                div { class: "doc-head", span { class: "title", "{title}" } }
                if d.kind == Kind::HeaderFooter {
                    for (row, label) in [("h", "Header — left · centre · right"), ("f", "Footer — left · centre · right")] {
                        div {
                            div { class: "doc-lbl", "{label}" }
                            div { class: "doc-grid3",
                                for (i, ph) in ["Left", "Centre", "Right"].into_iter().enumerate() {
                                    {
                                        let field: &'static str = match (row, i) {
                                            ("h", 0) => "h0", ("h", 1) => "h1", ("h", _) => "h2",
                                            (_, 0) => "f0", (_, 1) => "f1", _ => "f2",
                                        };
                                        let value = if row == "h" { d.hf.header[i].clone() } else { d.hf.footer[i].clone() };
                                        rsx! {
                                            input {
                                                class: "doc-field",
                                                id: "doc-{field}",
                                                placeholder: ph,
                                                value: "{value}",
                                                onfocus: move |_| tools.focus_on(field),
                                                oninput: move |e| {
                                                    let v = e.value();
                                                    tools.edit(|d| *text_slot(&mut d.hf, field) = v);
                                                    tools.focus_on(field);
                                                },
                                                onkeyup: move |_| tools.focus_on(field),
                                                onclick: move |_| tools.focus_on(field),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "doc-chips",
                        span { class: "doc-lbl", "Insert" }
                        for (token, label) in [("{page}", "Page number"), ("{pages}", "Total pages"), ("{date}", "Date")] {
                            button {
                                class: "sa-control",
                                r#type: "button",
                                // Keep the field's focus and caret.
                                onpointerdown: move |e| e.prevent_default(),
                                onmousedown: move |e| e.prevent_default(),
                                onclick: move |_| tools.insert(token),
                                "{label}"
                            }
                        }
                    }
                    div { class: "doc-row",
                        label { "Size" }
                        input {
                            r#type: "range", min: "6", max: "16", value: "{d.hf.size_pt}",
                            oninput: move |e| if let Ok(v) = e.value().parse::<f32>() { tools.edit(|d| d.hf.size_pt = v) },
                        }
                        span { class: "doc-wv", "{d.hf.size_pt} pt" }
                    }
                } else {
                    div {
                        div { class: "doc-lbl", "Text" }
                        input {
                            class: "doc-field",
                            value: "{d.wm.text}",
                            oninput: move |e| { let v = e.value(); tools.edit(|d| d.wm.text = v) },
                        }
                    }
                    div { class: "doc-row",
                        label { "Colour" }
                        for (name, rgb) in WM_COLORS {
                            button {
                                class: "pdit-swatch",
                                r#type: "button",
                                "aria-label": name,
                                "aria-pressed": if d.wm.color == rgb { "true" } else { "false" },
                                style: "background: rgb({rgb[0]}, {rgb[1]}, {rgb[2]});",
                                onclick: move |_| tools.edit(|d| d.wm.color = rgb),
                            }
                        }
                        label { class: "doc-gap", "Direction" }
                        Tabs {
                            options: vec![("Diagonal".into(), d.wm.diagonal), ("Horizontal".into(), !d.wm.diagonal)],
                            on_pick: move |i: usize| tools.edit(|d| d.wm.diagonal = i == 0),
                        }
                    }
                    div { class: "doc-row",
                        label { "Opacity" }
                        input {
                            r#type: "range", min: "5", max: "60", value: "{(d.wm.opacity * 100.0).round()}",
                            oninput: move |e| if let Ok(v) = e.value().parse::<f32>() { tools.edit(|d| d.wm.opacity = v / 100.0) },
                        }
                        span { class: "doc-wv", "{(d.wm.opacity * 100.0).round()} %" }
                        label { "Size" }
                        input {
                            r#type: "range", min: "24", max: "90", value: "{d.wm.size_pt}",
                            oninput: move |e| if let Ok(v) = e.value().parse::<f32>() { tools.edit(|d| d.wm.size_pt = v) },
                        }
                        span { class: "doc-wv", "{d.wm.size_pt} pt" }
                    }
                }
                div { class: "doc-row",
                    label { "Pages" }
                    Tabs {
                        options: vec![("All pages".into(), d.all), ("Some pages".into(), !d.all)],
                        on_pick: move |i: usize| tools.edit(|d| d.all = i == 0),
                    }
                    if !d.all {
                        input {
                            class: "doc-field doc-range", r#type: "number", min: "1", max: "{pages}", value: "{d.from}",
                            oninput: move |e| if let Ok(v) = e.value().parse::<u16>() { tools.edit(|d| d.from = v.clamp(1, pages)) },
                        }
                        label { "to" }
                        input {
                            class: "doc-field doc-range", r#type: "number", min: "1", max: "{pages}", value: "{d.to}",
                            oninput: move |e| if let Ok(v) = e.value().parse::<u16>() { tools.edit(|d| d.to = v.clamp(1, pages)) },
                        }
                    }
                }
                div { class: "doc-foot",
                    if d.existed {
                        button {
                            class: "sa-control", r#type: "button",
                            onclick: move |_| tools.remove(),
                            if d.kind == Kind::HeaderFooter { "Remove header & footer" } else { "Remove watermark" }
                        }
                    }
                    span { class: "grow" }
                    button { class: "sa-control", r#type: "button", onclick: move |_| tools.cancel(), "Cancel" }
                    button { class: "sa-primary", r#type: "button", onclick: move |_| tools.apply(), "Apply" }
                }
            }
        }
    }
}

impl DocTools {
    fn focus_on(mut self, field: &'static str) {
        let caret = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(&format!("doc-{field}")))
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
            .and_then(|i| i.selection_start().ok().flatten());
        self.focus.set((field, caret));
    }
}

/// Transitions.dev "Tabs sliding": a two-way choice.
#[component]
pub(crate) fn Tabs(options: Vec<(String, bool)>, on_pick: EventHandler<usize>) -> Element {
    rsx! {
        div { class: "t-tabs", role: "tablist",
            span { class: "t-tabs-pill", "aria-hidden": "true" }
            for (i, (label, selected)) in options.into_iter().enumerate() {
                button {
                    class: "t-tab",
                    r#type: "button",
                    role: "tab",
                    "aria-selected": if selected { "true" } else { "false" },
                    onclick: move |_| on_pick.call(i),
                    "{label}"
                }
            }
        }
    }
}

/// Places the pills in `selector` once the page shows the new selection: two
/// frames later, since an effect can run before Dioxus has applied the change
/// (one frame read the old selected tab).
pub(crate) fn place_pills_soon(selector: &'static str) {
    next_frame(move || next_frame(move || place_pills(selector)));
}

/// Moves each tab pill in `selector` under its selected tab (the snippet's wire-up).
fn place_pills(selector: &str) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(bars) = document.query_selector_all(selector) else {
        return;
    };
    for i in 0..bars.length() {
        let Some(bar) = bars
            .item(i)
            .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        else {
            continue;
        };
        let tab = bar
            .query_selector(".t-tab[aria-selected='true']")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
        let pill = bar
            .query_selector(".t-tabs-pill")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
        if let (Some(tab), Some(pill)) = (tab, pill) {
            let style = pill.style();
            let _ =
                style.set_property("transform", &format!("translateX({}px)", tab.offset_left()));
            let _ = style.set_property("width", &format!("{}px", tab.offset_width()));
        }
    }
}

/// Per page: the live preview of the window's choice (dashed outline), drawn
/// where the engine will put it. `scale` is CSS px per PDF point.
#[component]
pub fn DocPreview(page: u16, page_width_pt: f32, page_height_pt: f32, scale: f32) -> Element {
    let tools = use_context::<DocTools>();
    let Some(w) = tools.window.read().clone().filter(|w| !w.closing) else {
        return rsx! {};
    };
    let d = w.draft;
    let n = page + 1;
    if !d.all && (n < d.from.min(d.to) || n > d.from.max(d.to)) {
        return rsx! {};
    }
    let pages = page_count();
    match d.kind {
        Kind::HeaderFooter => {
            let size = d.hf.size_pt * scale;
            let side = page_width_pt * HF_SIDE * scale;
            let edge = page_height_pt * HF_EDGE * scale;
            let rows = [
                ("top", edge, d.hf.header.clone()),
                ("bottom", edge, d.hf.footer.clone()),
            ];
            rsx! {
                for (anchor, offset, texts) in rows {
                    div {
                        class: "doc-preview-hf",
                        style: "left: {side}px; right: {side}px; {anchor}: {offset}px; font-size: {size}px;",
                        for (i, t) in texts.iter().enumerate() {
                            span { key: "{i}", "{fill(t, n, pages)}" }
                        }
                    }
                }
            }
        }
        Kind::Watermark => {
            if d.wm.text.trim().is_empty() {
                return rsx! {};
            }
            let [r, g, b] = d.wm.color;
            let angle = if d.wm.diagonal { -45 } else { 0 };
            rsx! {
                div {
                    class: "doc-preview-wm",
                    style: "font-size: {d.wm.size_pt * scale}px; color: rgba({r}, {g}, {b}, {d.wm.opacity}); transform: translate(-50%, -50%) rotate({angle}deg);",
                    "{d.wm.text}"
                }
            }
        }
    }
}
