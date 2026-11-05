//! Redaction (D-086, D-087; approved mockup .claude/research/redact-ui/, option B): the rail's Redact opens a bar under
//! the centre bar; marks are only previews until Apply. Apply sends the document to MuPDF in the desktop app
//! (`redact_pdf`), hands the result to the user as `<name>-redacted.pdf` and opens that copy, which also ends undo.
//! Online the rail row says it's made in the desktop app.
//! Look: assets/css/redact.css (marks and black boxes: the mockup's own, no Devigner source).

use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;
use js_sys::{Function, Object, Reflect};
use pdit_core::annotations::Word;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const SIGNATURE_CSS: Asset = asset!("/assets/css/signature.css");
const DOC_CSS: Asset = asset!("/assets/css/doc.css");
const COMPRESS_CSS: Asset = asset!("/assets/css/compress.css");
pub const REDACT_CSS: Asset = asset!("/assets/css/redact.css");
pub const ICON_REDACT: &str = include_str!("../assets/icons/devigner/Eraser.svg");
const ICON_MARK_TEXT: &str = include_str!("../assets/icons/devigner/TextSelection.svg");
const ICON_MARK_AREA: &str = include_str!("../assets/icons/devigner/Crop.svg");
const ICON_REMOVE: &str = include_str!("../assets/icons/devigner/Close.svg");
const ICON_CHECK: &str = include_str!("../assets/icons/devigner/CheckSquare.svg");
/// Transitions.dev "Modal open/close" close time (ms).
const CLOSE_MS: i32 = 200;
/// A drawn area smaller than this (points) was a slip.
const SMALLEST_AREA: f32 = 4.0;
/// PDFium's word boxes reach into the next line, and MuPDF removes every glyph an area touches.
const LINE_INSET: f32 = 0.2;

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Text,
    Area,
}

/// Something marked on a page. All boxes are PDF points [left, bottom, right, top].
#[derive(Clone, PartialEq)]
struct Mark {
    id: u32,
    page: u16,
    area: bool,
    /// What goes to MuPDF: one box per line of a text mark, or the drawn area.
    rects: Vec<[f32; 4]>,
    /// The words it takes, by their place in the page's words.
    words: Vec<usize>,
    word_boxes: Vec<[f32; 4]>,
}

#[derive(Clone, PartialEq)]
struct Draft {
    page: u16,
    start: (f32, f32),
    now: (f32, f32),
    words: Rc<Vec<Word>>,
    first: Option<usize>,
    last: Option<usize>,
}

#[derive(Clone, PartialEq)]
enum Stage {
    Confirm,
    Done { name: String, hidden: bool },
}

#[derive(Clone, PartialEq)]
struct Dialog {
    stage: Stage,
    words: usize,
    picture: bool,
    fields: usize,
    status: Option<String>,
    busy: bool,
    shown: bool,
    closing: bool,
}

/// Shared Redact state.
#[derive(Clone, Copy)]
pub struct Redact {
    open: Signal<bool>,
    tool: Signal<Tool>,
    marks: Signal<Vec<Mark>>,
    draft: Signal<Option<Draft>>,
    dialog: Signal<Option<Dialog>>,
    hidden: Signal<bool>,
    next_id: Signal<u32>,
}

fn touches(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// One box per line for a run of words, thinner across the line. `sideways`: a page turned a quarter, its lines run
/// up or down.
fn line_boxes(boxes: &[[f32; 4]], sideways: bool) -> Vec<[f32; 4]> {
    let across = if sideways { 0 } else { 1 };
    let mut lines: Vec<[f32; 4]> = Vec::new();
    for b in boxes {
        let middle = (b[across] + b[across + 2]) / 2.0;
        match lines.last_mut() {
            Some(l) if middle > l[across] && middle < l[across + 2] => {
                *l = [
                    l[0].min(b[0]),
                    l[1].min(b[1]),
                    l[2].max(b[2]),
                    l[3].max(b[3]),
                ];
            }
            _ => lines.push(*b),
        }
    }
    for l in &mut lines {
        let inset = (l[across + 2] - l[across]) * LINE_INSET;
        l[across] += inset;
        l[across + 2] -= inset;
    }
    lines
}

/// `marks` as `redact_pdf`'s x-areas: MuPDF's page space starts at the corner of the page as shown.
fn engine_areas(marks: &[Mark]) -> String {
    let sizes = pdit_core::page_ops::page_sizes().unwrap_or_default();
    let mut areas = Vec::new();
    for mark in marks {
        let height = sizes.get(usize::from(mark.page)).map_or(0.0, |s| s.1);
        let (ox, oy) = pdit_core::page_ops::page_origin(mark.page).unwrap_or((0.0, 0.0));
        for r in &mark.rects {
            areas.push(format!(
                "{} {} {} {} {}",
                mark.page,
                r[0] - ox,
                height - (r[3] - oy),
                r[2] - ox,
                height - (r[1] - oy)
            ));
        }
    }
    areas.join(";")
}

/// PDFium's words, pictures and fields are placed on the page before its rotation; the page's presses and overlays
/// work on the page as shown.
// ponytail: a page both rotated and cropped is off by the crop for text marks; areas still land right.
#[derive(Clone, Copy)]
struct Shown {
    rotation: u32,
    width: f32,
    height: f32,
}

impl Shown {
    fn of(page: u16) -> Self {
        let size = pdit_core::page_ops::page_sizes()
            .ok()
            .and_then(|s| s.get(usize::from(page)).copied())
            .unwrap_or((0.0, 0.0));
        Shown {
            rotation: pdit_core::page_ops::page_rotation(page).unwrap_or(0),
            width: size.0,
            height: size.1,
        }
    }

    fn rect(self, [l, b, r, t]: [f32; 4]) -> [f32; 4] {
        let (w, h) = (self.width, self.height);
        match self.rotation {
            90 => [b, h - r, t, h - l],
            180 => [w - r, h - t, w - l, h - b],
            270 => [w - t, l, w - b, r],
            _ => [l, b, r, t],
        }
    }

    /// A point on the page as shown, back on the page before its rotation.
    fn point(self, x: f32, y: f32) -> (f32, f32) {
        let (w, h) = (self.width, self.height);
        match self.rotation {
            90 => (h - y, x),
            180 => (w - x, h - y),
            270 => (y, w - x),
            _ => (x, y),
        }
    }
}

/// The page's words, placed as shown.
fn shown_words(page: u16) -> Vec<Word> {
    let shown = Shown::of(page);
    let mut words = pdit_core::annotations::words(page).unwrap_or_default();
    for word in &mut words {
        word.bounds = shown.rect(word.bounds);
    }
    words
}

/// x-areas of a text mark over words `first..=last` of the open document's page (for the self-check).
#[cfg(any(debug_assertions, feature = "self-check"))]
pub fn text_mark_areas(page: u16, first: usize, last: usize) -> String {
    let words = shown_words(page);
    let boxes: Vec<_> = words[first..=last].iter().map(|w| w.bounds).collect();
    engine_areas(&[Mark {
        id: 0,
        page,
        area: false,
        rects: line_boxes(&boxes, !Shown::of(page).rotation.is_multiple_of(180)),
        words: Vec::new(),
        word_boxes: Vec::new(),
    }])
}

/// A picture under `rect`, by sampling it (PDFium has no list of a page's pictures here).
fn picture_under(page: u16, rect: [f32; 4]) -> bool {
    let shown = Shown::of(page);
    (0..=4).any(|i| {
        (0..=4).any(|j| {
            let x = rect[0] + (rect[2] - rect[0]) * i as f32 / 4.0;
            let y = rect[1] + (rect[3] - rect[1]) * j as f32 / 4.0;
            let (x, y) = shown.point(x, y);
            matches!(pdit_core::page_ops::image_at(page, x, y), Ok(Some(_)))
        })
    })
}

impl Redact {
    pub fn provide() -> Self {
        use_context_provider(|| Redact {
            open: Signal::new(false),
            tool: Signal::new(Tool::Text),
            marks: Signal::new(Vec::new()),
            draft: Signal::new(None),
            dialog: Signal::new(None),
            hidden: Signal::new(true),
            next_id: Signal::new(1),
        })
    }

    pub fn is_open(&self) -> bool {
        *self.open.read()
    }

    /// The page's presses mark instead of editing.
    pub fn armed(&self) -> bool {
        *self.open.read() && self.dialog.read().is_none()
    }

    /// The page's cursor while marking.
    pub fn page_mode(&self) -> Option<&'static str> {
        self.armed().then(|| match *self.tool.read() {
            Tool::Text => "text",
            Tool::Area => "area",
        })
    }

    pub fn dragging(&self) -> bool {
        self.draft.peek().is_some()
    }

    pub fn toggle(mut self) {
        let open = !*self.open.peek();
        if open {
            consume_context::<crate::editing::Editing>().keep_waiting();
            consume_context::<crate::page_tools::PageTools>().clear_image_selection();
        }
        self.open.set(open);
    }

    /// Another rail section took over; the marks wait.
    pub fn hide(mut self) {
        if *self.open.peek() {
            self.open.set(false);
        }
    }

    pub fn begin(mut self, page: u16, x: f32, y: f32) {
        let words = Rc::new(shown_words(page));
        let first = word_at(&words, x, y);
        if *self.tool.peek() == Tool::Text && first.is_none() {
            return;
        }
        self.draft.set(Some(Draft {
            page,
            start: (x, y),
            now: (x, y),
            words,
            first,
            last: first,
        }));
    }

    pub fn drag_to(mut self, x: f32, y: f32) {
        self.draft.with_mut(|d| {
            if let Some(d) = d.as_mut() {
                d.now = (x, y);
                if let Some(word) = word_at(&d.words, x, y) {
                    d.last = Some(word);
                }
            }
        });
    }

    pub fn release(mut self) {
        let Some(d) = self.draft.take() else { return };
        let tool = *self.tool.peek();
        let (words, rects): (Vec<usize>, Vec<[f32; 4]>) = match tool {
            Tool::Text => {
                let picked = text_run(&d);
                let boxes: Vec<_> = picked.iter().map(|&i| d.words[i].bounds).collect();
                let sideways = !Shown::of(d.page).rotation.is_multiple_of(180);
                (picked, line_boxes(&boxes, sideways))
            }
            Tool::Area => {
                let rect = area_of(&d);
                if rect[2] - rect[0] < SMALLEST_AREA || rect[3] - rect[1] < SMALLEST_AREA {
                    return;
                }
                (touched(&d.words, rect), vec![rect])
            }
        };
        if rects.is_empty() {
            return;
        }
        let id = *self.next_id.peek();
        self.next_id.set(id + 1);
        let word_boxes = words.iter().map(|&i| d.words[i].bounds).collect();
        self.marks.with_mut(|m| {
            m.push(Mark {
                id,
                page: d.page,
                area: tool == Tool::Area,
                rects,
                words,
                word_boxes,
            })
        });
    }

    /// A pressed mark's ×.
    fn remove(mut self, id: u32) {
        self.marks.with_mut(|m| m.retain(|mark| mark.id != id));
    }

    fn apply(mut self) {
        let marks = self.marks.peek().clone();
        let words: HashSet<(u16, usize)> = marks
            .iter()
            .flat_map(|m| m.words.iter().map(|&w| (m.page, w)))
            .collect();
        let picture = marks
            .iter()
            .any(|m| m.area && picture_under(m.page, m.rects[0]));
        let mut fields = 0;
        let mut pages: Vec<u16> = marks.iter().map(|m| m.page).collect();
        pages.sort_unstable();
        pages.dedup();
        for page in pages {
            let rects: Vec<_> = marks
                .iter()
                .filter(|m| m.page == page)
                .flat_map(|m| m.rects.iter().copied())
                .collect();
            let shown = Shown::of(page);
            fields += pdit_core::form_fields(page)
                .unwrap_or_default()
                .iter()
                .filter(|f| {
                    let r = shown.rect([f.rect.0, f.rect.1, f.rect.2, f.rect.3]);
                    rects.iter().any(|a| touches(*a, r))
                })
                .count();
        }
        self.dialog.set(Some(Dialog {
            stage: Stage::Confirm,
            words: words.len(),
            picture,
            fields,
            status: None,
            busy: false,
            shown: false,
            closing: false,
        }));
        self.show_soon();
    }

    fn show_soon(self) {
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

    fn close_dialog(self) {
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

    /// Save copy…: removes, asks where to save, then opens the saved copy.
    fn save(mut self) {
        let document = consume_context::<Signal<Option<crate::pages::OpenDocument>>>();
        let Some(original) = document.peek().as_ref().map(|d| d.name.clone()) else {
            return;
        };
        let pdf = match pdit_core::save_document() {
            Ok(bytes) => bytes,
            Err(error) => {
                crate::log(&format!("pdit: redact: {error}"));
                return self.edit(|d| d.status = Some("The PDF couldn't be read.".into()));
            }
        };
        let hidden = *self.hidden.peek();
        let headers = [
            ("x-areas", engine_areas(&self.marks.peek())),
            ("x-hidden", if hidden { "1" } else { "0" }.to_owned()),
        ];
        let name = crate::save::derived_name(&original, "redacted");
        self.edit(|d| {
            d.busy = true;
            d.status = Some("Removing…".into());
        });
        spawn(async move {
            let copy = match crate::ai_ui::desktop::invoke_raw("redact_pdf", &pdf, &headers).await {
                Ok(out) => js_sys::Uint8Array::new(&out).to_vec(),
                Err(error) => {
                    crate::log(&format!("pdit: redact: {error:?}"));
                    return self.edit(|d| {
                        d.busy = false;
                        d.status = Some("This couldn't be redacted. Nothing was saved.".into());
                    });
                }
            };
            match crate::save::save_file(&copy, &name).await {
                Ok(true) => {
                    self.marks.set(Vec::new());
                    self.open.set(false);
                    crate::show_saved_copy(copy, name.clone());
                    self.edit(|d| {
                        d.busy = false;
                        d.status = None;
                        d.stage = Stage::Done { name, hidden };
                    });
                }
                Ok(false) => self.edit(|d| {
                    d.busy = false;
                    d.status = None;
                }),
                Err(error) => {
                    crate::log(&format!("pdit: could not save the redacted PDF: {error:?}"));
                    self.edit(|d| {
                        d.busy = false;
                        d.status = Some("The copy couldn't be saved.".into());
                    });
                }
            }
        });
    }
}

fn word_at(words: &[Word], x: f32, y: f32) -> Option<usize> {
    words.iter().position(|w| {
        let b = w.bounds;
        x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3]
    })
}

fn area_of(d: &Draft) -> [f32; 4] {
    [
        d.start.0.min(d.now.0),
        d.start.1.min(d.now.1),
        d.start.0.max(d.now.0),
        d.start.1.max(d.now.1),
    ]
}

fn touched(words: &[Word], rect: [f32; 4]) -> Vec<usize> {
    (0..words.len())
        .filter(|&i| touches(rect, words[i].bounds))
        .collect()
}

fn text_run(d: &Draft) -> Vec<usize> {
    match (d.first, d.last) {
        (Some(a), Some(b)) => (a.min(b)..=a.max(b)).collect(),
        _ => Vec::new(),
    }
}

/// Marks on a page: inside the page's PDF-coordinate overlay.
#[component]
pub fn RedactOverlay(page: u16, page_height_pt: f32, scale: f32) -> Element {
    let redact = use_context::<Redact>();
    if !redact.is_open() {
        return rsx! {};
    }
    let place = move |r: [f32; 4]| {
        format!(
            "left: {}px; top: {}px; width: {}px; height: {}px;",
            r[0] * scale,
            (page_height_pt - r[3]) * scale,
            (r[2] - r[0]) * scale,
            (r[3] - r[1]) * scale
        )
    };
    let marks: Vec<Mark> = redact
        .marks
        .read()
        .iter()
        .filter(|m| m.page == page)
        .cloned()
        .collect();
    // What the mark being dragged will take.
    let (draft_words, draft_area) = match redact.draft.read().as_ref().filter(|d| d.page == page) {
        Some(d) if *redact.tool.read() == Tool::Area => {
            let rect = area_of(d);
            let words = touched(&d.words, rect);
            (
                words.iter().map(|&i| d.words[i].bounds).collect(),
                Some(rect),
            )
        }
        Some(d) => (
            text_run(d).iter().map(|&i| d.words[i].bounds).collect(),
            None,
        ),
        None => (Vec::new(), None),
    };
    rsx! {
        for mark in marks {
            for b in mark.word_boxes.iter().copied() {
                div { class: "rx-word", style: place(b) }
            }
            for (i, r) in mark.rects.iter().copied().enumerate() {
                div {
                    class: if mark.area { "rx-mark is-area" } else { "rx-mark" },
                    style: place(r),
                    if i == 0 {
                        button {
                            class: "rx-remove",
                            r#type: "button",
                            title: "Remove this mark",
                            "aria-label": "Remove this mark",
                            dangerous_inner_html: ICON_REMOVE,
                            onpointerdown: move |event| event.stop_propagation(),
                            onclick: {
                                let id = mark.id;
                                move |event: Event<MouseData>| {
                                    event.stop_propagation();
                                    redact.remove(id);
                                }
                            },
                        }
                    }
                }
            }
        }
        for b in draft_words {
            div { class: "rx-word", style: place(b) }
        }
        if let Some(rect) = draft_area {
            div { class: "rx-mark is-area", style: place(rect) }
        }
    }
}

/// The Devigner GooSwitch (an island, web/gooey-island `mountSwitch`).
#[component]
fn HiddenSwitch() -> Element {
    let mut redact = use_context::<Redact>();
    let held = use_hook(|| Rc::new(RefCell::new(None::<(JsValue, Closure<dyn Fn(JsValue)>)>)));
    let dropped = held.clone();
    use_drop(move || {
        if let Some((controller, _)) = dropped.borrow_mut().take()
            && let Ok(unmount) = Reflect::get(&controller, &"unmount".into())
            && let Ok(unmount) = unmount.dyn_into::<Function>()
        {
            let _ = unmount.call0(&JsValue::NULL);
        }
    });
    let changed = use_callback(move |on: bool| redact.hidden.set(on));
    rsx! {
        span {
            onmounted: move |event| {
                let held = held.clone();
                async move {
                    let Some(element) = event.data().downcast::<web_sys::Element>().cloned() else {
                        return;
                    };
                    let on_change = Closure::<dyn Fn(JsValue)>::new(move |on: JsValue| {
                        changed.call(on.as_bool().unwrap_or(true))
                    });
                    let options = Object::new();
                    let _ = Reflect::set(&options, &"onChange".into(), on_change.as_ref());
                    let _ = Reflect::set(&options, &"checked".into(), &(*redact.hidden.peek()).into());
                    match crate::island::mount_with(&element, "mountSwitch", &options).await {
                        Ok(controller) => *held.borrow_mut() = Some((controller, on_change)),
                        Err(error) => crate::log(&format!("pdit: could not load the switch: {error:?}")),
                    }
                }
            },
        }
    }
}

/// The bar under the centre bar, and the Apply and saved windows.
#[component]
pub fn RedactUi() -> Element {
    let mut redact = use_context::<Redact>();
    let document = use_context::<Signal<Option<crate::pages::OpenDocument>>>();
    // Marks belong to one document and its pages.
    // ponytail: pages moved while marks wait keep their marks on the old index; clear marks too if that matters.
    let shape = document.read().as_ref().map(|d| (d.id, d.page_sizes.len()));
    use_effect(use_reactive!(|shape| {
        let _ = shape;
        redact.marks.set(Vec::new());
    }));
    // Escape: the window first, then the bar.
    use_hook(move || {
        let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if event.key() != "Escape" {
                    return;
                }
                if redact.dialog.peek().is_some() {
                    redact.close_dialog();
                } else {
                    redact.hide();
                }
            },
        );
        if let Some(window) = web_sys::window() {
            let _ =
                window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        }
        on_key.forget();
    });
    let open = *redact.open.read();
    let tool = *redact.tool.read();
    let count = redact.marks.read().len();
    let label = match count {
        0 => "No marks yet".to_owned(),
        1 => "1 mark".to_owned(),
        n => format!("{n} marks"),
    };
    let dialog = redact.dialog.read().clone();
    let class = match dialog.as_ref().map(|d| (d.shown, d.closing)) {
        Some((_, true)) => "doc-box t-modal is-closing",
        Some((true, false)) => "doc-box t-modal is-open",
        _ => "doc-box t-modal",
    };
    rsx! {
        document::Stylesheet { href: SIGNATURE_CSS }
        document::Stylesheet { href: DOC_CSS }
        document::Stylesheet { href: COMPRESS_CSS }
        document::Stylesheet { href: REDACT_CSS }
        div {
            class: "pdit-sel-toolbar sa-root rx-bar",
            "data-open": if open { "true" } else { "false" },
            role: "toolbar",
            "aria-label": "Redact",
            button {
                class: "sa-control", r#type: "button",
                "aria-pressed": tool == Tool::Text,
                onclick: move |_| redact.tool.set(Tool::Text),
                span { dangerous_inner_html: ICON_MARK_TEXT, style: "display: contents" }
                "Mark text"
            }
            button {
                class: "sa-control", r#type: "button",
                "aria-pressed": tool == Tool::Area,
                onclick: move |_| redact.tool.set(Tool::Area),
                span { dangerous_inner_html: ICON_MARK_AREA, style: "display: contents" }
                "Mark area"
            }
            span { class: "pdit-sel-sep" }
            span { class: "rx-count", "{label}" }
            button {
                class: "sa-control", r#type: "button", disabled: count == 0,
                onclick: move |_| redact.marks.set(Vec::new()),
                "Clear"
            }
            button {
                class: "sa-primary", r#type: "button", disabled: count == 0,
                onclick: move |_| redact.apply(),
                "Apply…"
            }
        }
        if let Some(d) = dialog {
            div { class: "doc-layer sa-root pdit-compress",
                match d.stage.clone() {
                    Stage::Confirm => {
                        let mut parts = Vec::new();
                        match d.words {
                            0 => {}
                            1 => parts.push("1 word".to_owned()),
                            n => parts.push(format!("{n} words")),
                        }
                        if d.picture {
                            parts.push("part of a picture".to_owned());
                        }
                        match d.fields {
                            0 => {}
                            1 => parts.push("1 form field".to_owned()),
                            n => parts.push(format!("{n} form fields")),
                        }
                        let what = if parts.is_empty() {
                            "What's under the marks".to_owned()
                        } else {
                            parts.join(", ")
                        };
                        rsx! {
                            div { class, role: "dialog", "aria-label": "Redact",
                                div { class: "doc-head", span { class: "title", "Redact" } }
                                div { class: "cz-size", b { "{what}" } }
                                div { class: "cz-what", "Removed from the copy for good, not just covered. Black boxes show where." }
                                if d.picture {
                                    div { class: "cz-what rx-note", "If this picture is also on other pages, it stays there unless you mark it too." }
                                }
                                div { class: "rx-opt",
                                    div {
                                        div { class: "rx-opt-title", "Also remove hidden data" }
                                        div { class: "cz-what", "Title, author, bookmarks and attachments." }
                                    }
                                    HiddenSwitch {}
                                }
                                div { class: "doc-foot",
                                    span { class: "cz-status",
                                        {d.status.clone().unwrap_or_else(|| "Saved as a copy — the open PDF doesn’t change.".to_owned())}
                                    }
                                    span { class: "grow" }
                                    button {
                                        class: "sa-control", r#type: "button", disabled: d.busy,
                                        onclick: move |_| redact.close_dialog(),
                                        "Cancel"
                                    }
                                    button {
                                        class: "sa-primary", r#type: "button", disabled: d.busy,
                                        onclick: move |_| redact.save(),
                                        "Save copy…"
                                    }
                                }
                            }
                        }
                    }
                    Stage::Done { name, hidden } => rsx! {
                        div { class, role: "dialog", "aria-label": "Redacted copy saved",
                            div { class: "doc-head", span { class: "title", "Redacted copy saved" } }
                            div { class: "cz-what", "Now open: {name}. Your original is unchanged." }
                            div { class: "cz-what rx-check",
                                span { dangerous_inner_html: ICON_CHECK, style: "display: contents" }
                                span { "Read the saved file back: no text is left in the marked areas." }
                            }
                            if hidden {
                                div { class: "cz-what rx-check",
                                    span { dangerous_inner_html: ICON_CHECK, style: "display: contents" }
                                    span { "Hidden data removed: title, author, bookmarks and attachments." }
                                }
                            }
                            div { class: "doc-foot",
                                span { class: "grow" }
                                button {
                                    class: "sa-primary", r#type: "button",
                                    onclick: move |_| redact.close_dialog(),
                                    "Done"
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}
