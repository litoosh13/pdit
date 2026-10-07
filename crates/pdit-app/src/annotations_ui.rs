//! Annotations, part 1 (D-040). Drag across words → the Selection Actions
//! tint + a markup bar (Highlight / Underline / Strikeout, four colours); a
//! plain click on text still edits it. Click a mark → its colours + Delete.
//! Right-click → Comment → Add note → a note box (the menu's surface and
//! Transitions.dev "Menu dropdown" motion); click a note to open it again.
//! Every change goes through the page tools' snapshot + Undo toast.

use crate::page_tools::{PageTools, next_frame, set_timeout};
use dioxus::prelude::*;
use pdit_core::annotations::{self as annots, Annotation, AnnotationKind, MarkupKind, Word};

const ANNOTATIONS_CSS: Asset = asset!("/assets/css/annotations.css");
/// Transitions.dev "Tabs sliding" lives there; the link box uses it (D-043).
const TABS_CSS: Asset = asset!("/assets/css/signature.css");
const ICON_HIGHLIGHT: &str = include_str!("../assets/icons/highlighter.svg");
const ICON_UNDERLINE: &str = include_str!("../assets/icons/underline-letter.svg");
const ICON_STRIKEOUT: &str = include_str!("../assets/icons/strikethrough-2.svg");
pub const ICON_NOTE: &str = include_str!("../assets/icons/comment-bubble.svg");
const ICON_TRASH: &str = include_str!("../assets/icons/cartoon-trash.svg");
const ICON_PEN: &str = include_str!("../assets/icons/cartoon-pen.svg");
const ICON_STAMP: &str = include_str!("../assets/icons/cartoon-stamp.svg");
const THUMBS_CSS: Asset = asset!("/assets/css/thumbnails.css");

/// The stamps (D-041, approved): label and colour.
pub const STAMPS: [(&str, [u8; 3]); 5] = [
    ("Approved", [31, 157, 85]),
    ("Rejected", [211, 58, 44]),
    ("Draft", [29, 111, 216]),
    ("Confidential", [211, 58, 44]),
    ("Final", [31, 157, 85]),
];
/// How long a comment flashes after its row is clicked, ms.
const FLASH_MS: i32 = 1200;

/// (name, highlight fill, line ink for underline / strikeout) — approved set.
const COLORS: [(&str, [u8; 3], [u8; 3]); 4] = [
    ("Yellow", [255, 225, 77], [212, 163, 0]),
    ("Green", [126, 224, 138], [31, 157, 85]),
    ("Pink", [255, 143, 199], [214, 51, 127]),
    ("Blue", [124, 196, 255], [29, 111, 216]),
];
/// The note box's close duration (--dropdown-close-dur).
const CLOSE_MS: i32 = 150;
/// The note box's width, CSS px (it opens to the left near the page edge).
const NOTE_BOX_PX: f32 = 250.0;

/// A drag across words that started on `page`.
#[derive(Clone)]
struct WordDrag {
    page: u16,
    words: Vec<Word>,
    start: usize,
    end: usize,
}

/// The note box: a new note (no index) or an existing one.
#[derive(Clone, PartialEq)]
struct NoteBox {
    page: u16,
    index: Option<usize>,
    x: f32,
    top: f32,
    original: String,
    draft: String,
    shown: bool,
    closing: bool,
}

/// Shared annotation state.
#[derive(Clone, Copy)]
pub struct Annotate {
    drag: Signal<Option<WordDrag>>,
    /// Chosen words, one box per line: (page, boxes in PDF points).
    selection: Signal<Option<(u16, Vec<[f32; 4]>)>>,
    /// A mark that was clicked.
    mark: Signal<Option<(u16, Annotation)>>,
    note: Signal<Option<NoteBox>>,
    /// The colour the next mark gets (index into COLORS).
    color: Signal<usize>,
    /// Swallow the click that ends a word drag (it would edit the text).
    swallow_click: Signal<bool>,
    /// The comments panel is open (D-041).
    panel: Signal<bool>,
    /// A comment to flash after its row was clicked: (page, index).
    flash: Signal<Option<(u16, usize)>>,
}

impl Annotate {
    pub fn provide() -> Self {
        use_context_provider(|| Annotate {
            drag: Signal::new(None),
            selection: Signal::new(None),
            mark: Signal::new(None),
            note: Signal::new(None),
            color: Signal::new(0),
            swallow_click: Signal::new(false),
            panel: Signal::new(false),
            flash: Signal::new(None),
        })
    }

    /// A press on text at (x, y) PDF points: may start a word drag.
    pub fn press(mut self, page: u16, x: f32, y: f32) {
        let words = annots::words(page).unwrap_or_default();
        if let Some(i) = word_at(&words, x, y) {
            self.drag.set(Some(WordDrag {
                page,
                words,
                start: i,
                end: i,
            }));
        }
    }

    pub fn dragging(&self) -> bool {
        self.drag.peek().is_some()
    }

    /// The pointer moved during a word drag: select up to the word under it.
    pub fn drag_to(mut self, x: f32, y: f32) {
        let Some(mut d) = self.drag.peek().clone() else {
            return;
        };
        let Some(end) = word_at(&d.words, x, y) else {
            return;
        };
        if end == d.end && self.selection.peek().is_some() {
            return;
        }
        d.end = end;
        if d.start != d.end {
            self.selection
                .set(Some((d.page, line_boxes(&d.words, d.start, d.end))));
        }
        self.drag.set(Some(d));
    }

    /// The press ended. After a drag across words the bar shows and the click
    /// that follows is swallowed; a plain click is left to text editing.
    pub fn release(mut self) {
        if self.drag.take().is_some() && self.selection.peek().is_some() {
            self.swallow_click.set(true);
        }
    }

    /// For the page's click handler: true if this click ended a word drag.
    pub fn take_click(mut self) -> bool {
        let swallow = *self.swallow_click.peek();
        if swallow {
            self.swallow_click.set(false);
        }
        swallow
    }

    /// A press anywhere on a page: closes the bar / note box (the note saves).
    pub fn dismiss(mut self) {
        if self.selection.peek().is_some() {
            self.selection.set(None);
        }
        if self.mark.peek().is_some() {
            self.mark.set(None);
        }
        self.close_note();
        if let Some(links) = try_consume_context::<crate::links_ui::LinkUi>() {
            links.dismiss();
        }
    }

    /// Right-click → Comment → Add note: a new note at (x, top) on `page`.
    pub fn add_note(self, page: u16, x: f32, top: f32) {
        self.dismiss();
        self.open(NoteBox {
            page,
            index: None,
            x,
            top,
            original: String::new(),
            draft: String::new(),
            shown: false,
            closing: false,
        });
    }

    /// Right-click → Open note (D-050).
    pub(crate) fn open_note_at(self, page: u16, index: usize) {
        if let Some(note) = annotation_at(page, index) {
            self.open_note(page, &note);
        }
    }

    /// Right-click → Delete (a note or a mark), with Undo.
    pub(crate) fn delete_at(mut self, page: u16, index: usize) {
        if let Some(a) = annotation_at(page, index) {
            self.dismiss();
            self.mark.set(Some((page, a)));
            self.delete_mark();
        }
    }

    fn open_note(self, page: u16, note: &Annotation) {
        self.dismiss();
        let [left, _, _, top] = note.bounds;
        self.open(NoteBox {
            page,
            index: Some(note.index),
            x: left,
            top,
            original: note.text.clone(),
            draft: note.text.clone(),
            shown: false,
            closing: false,
        });
    }

    fn open(mut self, note: NoteBox) {
        self.note.set(Some(note));
        let mut signal = self.note;
        next_frame(move || {
            next_frame(move || {
                signal.with_mut(|n| {
                    if let Some(n) = n.as_mut() {
                        n.shown = true;
                    }
                })
            })
        });
    }

    /// Closes the note box, saving a new non-empty note or a changed text.
    pub fn close_note(mut self) {
        let Some(n) = self.note.peek().clone().filter(|n| !n.closing) else {
            return;
        };
        self.note.with_mut(|b| {
            if let Some(b) = b.as_mut() {
                b.closing = true;
            }
        });
        let mut signal = self.note;
        // Only the box that is closing goes; a note opened meanwhile stays.
        set_timeout(CLOSE_MS, move || {
            if signal.peek().as_ref().is_some_and(|n| n.closing) {
                signal.set(None);
            }
        });
        let tools = consume_context::<PageTools>();
        match n.index {
            None if !n.draft.trim().is_empty() => {
                tools.apply("Note added", ICON_NOTE, || {
                    annots::add_note(n.page, n.x, n.top, &n.draft).map(|_| ())
                });
            }
            Some(index) if n.draft != n.original => {
                tools.apply("Note changed", ICON_NOTE, || {
                    annots::set_note_text(n.page, index, &n.draft)
                });
            }
            _ => {}
        }
    }

    fn delete_note(mut self) {
        let Some(n) = self.note.take() else { return };
        if let Some(index) = n.index {
            consume_context::<PageTools>().apply("Note deleted", ICON_NOTE, || {
                annots::delete_annotation(n.page, index)
            });
        }
    }

    fn add_markup(mut self, kind: MarkupKind) {
        let Some((page, boxes)) = self.selection.take() else {
            return;
        };
        let (_, fill, ink) = COLORS[*self.color.peek()];
        let (message, icon, color) = match kind {
            MarkupKind::Highlight => ("Highlight added", ICON_HIGHLIGHT, fill),
            MarkupKind::Underline => ("Underline added", ICON_UNDERLINE, ink),
            MarkupKind::Strikeout => ("Strikeout added", ICON_STRIKEOUT, ink),
        };
        consume_context::<PageTools>().apply(message, icon, || {
            annots::add_markup(page, kind, &boxes, color).map(|_| ())
        });
    }

    /// The bar's Link (D-043): the chosen words go to the link box.
    fn link_selection(mut self) {
        if let Some((page, boxes)) = self.selection.take() {
            consume_context::<crate::links_ui::LinkUi>().link_words(page, boxes);
        }
    }

    fn recolor_mark(mut self, color: usize) {
        let Some((page, mark)) = self.mark.take() else {
            return;
        };
        let (_, fill, ink) = COLORS[color];
        let new = if mark.kind == AnnotationKind::Markup(MarkupKind::Highlight) {
            fill
        } else {
            ink
        };
        consume_context::<PageTools>().apply("Colour changed", icon_of(mark.kind), || {
            annots::set_markup_color(page, mark.index, new)
        });
    }

    /// Places stamp `i` with today's date at (x, top) on `page` (D-041).
    pub fn add_stamp(self, page: u16, x: f32, top: f32, i: usize) {
        self.dismiss();
        let (label, color) = STAMPS[i];
        let date = today();
        consume_context::<PageTools>().apply(&format!("Stamp added: {label}"), ICON_STAMP, || {
            annots::add_stamp(page, x, top, label, &date, color).map(|_| ())
        });
    }

    /// All comments (D-041): opens the list on the right.
    pub fn open_panel(mut self) {
        self.panel.set(true);
    }

    fn delete_mark(mut self) {
        let Some((page, mark)) = self.mark.take() else {
            return;
        };
        let message = match mark.kind {
            AnnotationKind::Ink => "Drawing deleted",
            AnnotationKind::Stamp => "Stamp deleted",
            AnnotationKind::Note => "Note deleted",
            _ => "Mark deleted",
        };
        consume_context::<PageTools>().apply(message, icon_of(mark.kind), || {
            annots::delete_annotation(page, mark.index)
        });
    }
}

fn annotation_at(page: u16, index: usize) -> Option<Annotation> {
    annots::annotations(page)
        .ok()?
        .into_iter()
        .find(|a| a.index == index)
}

fn icon_of(kind: AnnotationKind) -> &'static str {
    match kind {
        AnnotationKind::Note => ICON_NOTE,
        AnnotationKind::Ink => ICON_PEN,
        AnnotationKind::Stamp => ICON_STAMP,
        AnnotationKind::Markup(MarkupKind::Highlight) => ICON_HIGHLIGHT,
        AnnotationKind::Markup(MarkupKind::Underline) => ICON_UNDERLINE,
        AnnotationKind::Markup(MarkupKind::Strikeout) => ICON_STRIKEOUT,
    }
}

fn word_at(words: &[Word], x: f32, y: f32) -> Option<usize> {
    let slack = 1.5;
    words.iter().position(|w| {
        let [l, b, r, t] = w.bounds;
        x >= l - slack && x <= r + slack && y >= b - slack && y <= t + slack
    })
}

/// One box per line for the words between `a` and `b` (reading order).
fn line_boxes(words: &[Word], a: usize, b: usize) -> Vec<[f32; 4]> {
    let mut boxes: Vec<[f32; 4]> = Vec::new();
    for w in &words[a.min(b)..=a.max(b)] {
        let [l, bo, r, t] = w.bounds;
        let centre = (bo + t) / 2.0;
        match boxes.last_mut() {
            Some(line) if (centre - (line[1] + line[3]) / 2.0).abs() < (t - bo) / 2.0 => {
                *line = [
                    line[0].min(l),
                    line[1].min(bo),
                    line[2].max(r),
                    line[3].max(t),
                ];
            }
            _ => boxes.push(w.bounds),
        }
    }
    boxes
}

/// Escape closes the note box (it saves) and the bars.
#[component]
pub fn AnnotationKeys() -> Element {
    let annotate = use_context::<Annotate>();
    // The window listener runs outside Dioxus, where looking up a context
    // panics ("RefCell already borrowed"); it only counts the key, and the
    // effect does the closing inside Dioxus.
    let mut escapes = use_signal(|| 0u32);
    use_effect(move || {
        if escapes() > 0 {
            annotate.dismiss();
            if let Some(forms) = try_consume_context::<crate::form_edit_ui::FormEdit>() {
                forms.escape();
            }
            if let Some(tools) = try_consume_context::<crate::tools_ui::Tools>() {
                tools.escape();
            }
            if let Some(find) = try_consume_context::<crate::search_ui::Find>() {
                find.close();
            }
            if let Some(print) = try_consume_context::<crate::print_ui::Print>() {
                print.close();
            }
            if let Some(find) = try_consume_context::<crate::find_fields_ui::FindFields>() {
                find.cancel();
            }
            if let Some(ai) = try_consume_context::<crate::ai_ui::Ai>() {
                ai.escape();
            }
            if let Some(ask) = try_consume_context::<crate::ask_ui::Ask>() {
                ask.close();
            }
        }
    });
    use_hook(move || {
        use wasm_bindgen::JsCast;
        let on_key = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if event.key() == "Escape" {
                    escapes += 1;
                }
            },
        );
        if let Some(window) = web_sys::window() {
            let _ =
                window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        }
        // The app lives as long as the page.
        on_key.forget();
    });
    rsx! {
        document::Stylesheet { href: ANNOTATIONS_CSS }
        document::Stylesheet { href: TABS_CSS }
    }
}

/// Per page: the drag tint, clickable areas over notes and marks, the markup
/// bar and the note box. `scale` is CSS px per PDF point.
#[component]
pub fn AnnotationOverlay(
    page: u16,
    page_width_pt: f32,
    page_height_pt: f32,
    scale: f32,
) -> Element {
    let mut annotate = use_context::<Annotate>();
    // Re-read when the page is redrawn after a change (pages remount then).
    let existing = annots::annotations(page).unwrap_or_default();
    let flash = *annotate.flash.read();
    let px = move |[l, b, r, t]: [f32; 4]| {
        (
            l * scale,
            (page_height_pt - t) * scale,
            (r - l) * scale,
            (t - b) * scale,
        )
    };
    let selection = annotate
        .selection
        .read()
        .clone()
        .filter(|(p, _)| *p == page)
        .map(|(_, s)| s);
    let mark = annotate
        .mark
        .read()
        .clone()
        .filter(|(p, _)| *p == page)
        .map(|(_, m)| m);
    let note = annotate.note.read().clone().filter(|n| n.page == page);
    let current = *annotate.color.read();
    // The bar sits under the boxes it belongs to, centred.
    let bar_at = move |boxes: &[[f32; 4]]| {
        let l = boxes.iter().map(|b| b[0]).fold(f32::MAX, f32::min);
        let r = boxes.iter().map(|b| b[2]).fold(f32::MIN, f32::max);
        let bottom = boxes.iter().map(|b| b[1]).fold(f32::MAX, f32::min);
        (
            (l + r) / 2.0 * scale,
            (page_height_pt - bottom) * scale + 10.0,
        )
    };
    rsx! {
        if let Some(boxes) = selection.clone() {
            div { class: "sa-root",
                for (i, (x, y, w, h)) in boxes.iter().map(|b| px(*b)).enumerate() {
                    div {
                        key: "sel-{i}",
                        class: "sa-highlight",
                        style: "left: {x - 1.0}px; top: {y - 1.0}px; width: {w + 2.0}px; height: {h + 2.0}px;",
                    }
                }
            }
        }
        for a in existing.iter().cloned() {
            {
                let boxes = if a.rects.is_empty() { vec![a.bounds] } else { a.rects.clone() };
                rsx! {
                    for (i, (x, y, w, h)) in boxes.iter().map(|b| px(*b)).enumerate() {
                        div {
                            key: "an-{a.index}-{i}",
                            class: match (a.kind == AnnotationKind::Note, flash == Some((page, a.index))) {
                                (true, true) => "pdit-annot-hit is-note pdit-flash",
                                (true, false) => "pdit-annot-hit is-note",
                                (false, true) => "pdit-annot-hit pdit-flash",
                                (false, false) => "pdit-annot-hit",
                            },
                            "data-annot": "{page}-{a.index}",
                            title: if a.kind == AnnotationKind::Note { a.text.clone() } else { String::new() },
                            style: "left: {x}px; top: {y}px; width: {w}px; height: {h}px;",
                            onpointerdown: move |event| event.stop_propagation(),
                            onclick: {
                                let a = a.clone();
                                move |event: Event<MouseData>| {
                                    event.stop_propagation();
                                    if a.kind == AnnotationKind::Note {
                                        annotate.open_note(page, &a);
                                    } else {
                                        annotate.dismiss();
                                        annotate.mark.set(Some((page, a.clone())));
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
        if let Some(boxes) = selection {
            {
                let (cx, cy) = bar_at(&boxes);
                rsx! {
                    div {
                        class: "sa-root sa-anchor pdit-markup-anchor",
                        style: "transform: translate({cx}px, {cy}px) translateX(-50%);",
                        onpointerdown: move |event| event.stop_propagation(),
                        onclick: move |event| event.stop_propagation(),
                        div { class: "sa-bar",
                            for (kind, icon, label) in [
                                (MarkupKind::Highlight, ICON_HIGHLIGHT, "Highlight"),
                                (MarkupKind::Underline, ICON_UNDERLINE, "Underline"),
                                (MarkupKind::Strikeout, ICON_STRIKEOUT, "Strikeout"),
                            ] {
                                button {
                                    class: "sa-control",
                                    r#type: "button",
                                    onclick: move |_| annotate.add_markup(kind),
                                    span { dangerous_inner_html: icon, style: "display: contents" }
                                    "{label}"
                                }
                            }
                            span { class: "pdit-bar-sep" }
                            for (i, (name, fill, _)) in COLORS.into_iter().enumerate() {
                                button {
                                    class: "pdit-swatch",
                                    r#type: "button",
                                    "aria-label": name,
                                    "aria-pressed": if i == current { "true" } else { "false" },
                                    style: "background: rgb({fill[0]}, {fill[1]}, {fill[2]});",
                                    onclick: move |_| annotate.color.set(i),
                                }
                            }
                            span { class: "pdit-bar-sep" }
                            button {
                                class: "sa-control",
                                r#type: "button",
                                onclick: move |_| annotate.link_selection(),
                                span { dangerous_inner_html: crate::links_ui::ICON_LINK, style: "display: contents" }
                                "Link"
                            }
                        }
                    }
                }
            }
        }
        if let Some(m) = mark {
            {
                let boxes = if m.rects.is_empty() { vec![m.bounds] } else { m.rects.clone() };
                let (cx, cy) = bar_at(&boxes);
                let highlight = m.kind == AnnotationKind::Markup(MarkupKind::Highlight);
                let markup = matches!(m.kind, AnnotationKind::Markup(_));
                let (rx, ry, rw, rh) = px(m.bounds);
                rsx! {
                    if !markup {
                        div {
                            class: "pdit-annot-ring",
                            style: "left: {rx - 3.0}px; top: {ry - 3.0}px; width: {rw + 6.0}px; height: {rh + 6.0}px;",
                        }
                    }
                    div {
                        class: "sa-root sa-anchor pdit-markup-anchor",
                        style: "transform: translate({cx}px, {cy}px) translateX(-50%);",
                        onpointerdown: move |event| event.stop_propagation(),
                        onclick: move |event| event.stop_propagation(),
                        div { class: "sa-bar",
                            for (i, (name, fill, ink)) in COLORS.into_iter().enumerate().filter(|_| markup) {
                                {
                                    let c = if highlight { fill } else { ink };
                                    rsx! {
                                        button {
                                            class: "pdit-swatch",
                                            r#type: "button",
                                            "aria-label": name,
                                            "aria-pressed": if c == m.color { "true" } else { "false" },
                                            style: "background: rgb({fill[0]}, {fill[1]}, {fill[2]});",
                                            onclick: move |_| annotate.recolor_mark(i),
                                        }
                                    }
                                }
                            }
                            if markup {
                                span { class: "pdit-bar-sep" }
                            }
                            button {
                                class: "sa-control",
                                r#type: "button",
                                onclick: move |_| annotate.delete_mark(),
                                span { dangerous_inner_html: ICON_TRASH, style: "display: contents" }
                                "Delete"
                            }
                        }
                    }
                }
            }
        }
        if let Some(n) = note {
            {
                let icon_left = n.x * scale;
                let top = (page_height_pt - n.top) * scale;
                let flip = icon_left + 26.0 + NOTE_BOX_PX > page_width_pt * scale;
                let left = if flip { icon_left - NOTE_BOX_PX - 6.0 } else { icon_left + 26.0 };
                let class = match (n.shown, n.closing) {
                    (_, true) => "cm-menu t-dropdown pdit-note-box is-closing",
                    (true, false) => "cm-menu t-dropdown pdit-note-box is-open",
                    (false, false) => "cm-menu t-dropdown pdit-note-box",
                };
                rsx! {
                    div {
                        class,
                        "data-origin": if flip { "top-right" } else { "top-left" },
                        style: "left: {left}px; top: {top}px;",
                        onpointerdown: move |event| event.stop_propagation(),
                        onclick: move |event| event.stop_propagation(),
                        textarea {
                            placeholder: "Write a note…",
                            "aria-label": "Note",
                            value: "{n.draft}",
                            onmounted: move |event| {
                                spawn(async move {
                                    let _ = event.data().set_focus(true).await;
                                });
                            },
                            oninput: move |event| {
                                annotate.note.with_mut(|b| {
                                    if let Some(b) = b.as_mut() {
                                        b.draft = event.value();
                                    }
                                })
                            },
                        }
                        div { class: "row sa-root",
                            button {
                                class: "sa-control",
                                r#type: "button",
                                onclick: move |_| annotate.delete_note(),
                                "Delete"
                            }
                            span { class: "grow" }
                            button {
                                class: "sa-primary",
                                r#type: "button",
                                onclick: move |_| annotate.close_note(),
                                "Done"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Today as "2 Oct 2026" (stamps, D-041) — always Latin letters, which the
/// stamp's standard Helvetica can show.
pub(crate) fn today() -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let now = js_sys::Date::new_0();
    format!(
        "{} {} {}",
        now.get_date(),
        MONTHS[now.get_month() as usize % 12],
        now.get_full_year()
    )
}

/// One row of the comment list.
struct Row {
    page: u16,
    index: usize,
    kind: AnnotationKind,
    text: String,
    top: f32,
}

/// All comments (D-041): the Pages panel's look on the right; one row per
/// note, mark, drawing and stamp (page order, top to bottom). A row scrolls
/// to its comment and flashes it.
#[component]
pub fn CommentsPanel() -> Element {
    let mut annotate = use_context::<Annotate>();
    let document = use_context::<Signal<Option<crate::pages::OpenDocument>>>();
    let open = *annotate.panel.read();
    let pages = document.read().as_ref().map_or(0, |d| d.page_sizes.len());
    let rows: Vec<Row> = if open {
        (0..pages as u16).flat_map(rows_on).collect()
    } else {
        Vec::new()
    };
    let count = rows.len();
    if pages == 0 {
        return rsx! {};
    }
    rsx! {
        document::Stylesheet { href: THUMBS_CSS }
        aside {
            class: "pdit-panel t-panel-slide pdit-comments",
            "data-open": if open { "true" } else { "false" },
            "aria-label": "Comments",
            div { class: "pdit-panel-head",
                span { class: "title", "Comments" }
                span { class: "count", "{count}" }
                span { class: "grow" }
                button {
                    class: "pdit-cmt-close",
                    r#type: "button",
                    onclick: move |_| annotate.panel.set(false),
                    "Close"
                }
            }
            div { class: "pdit-cmt-list",
                if rows.is_empty() {
                    div { class: "pdit-cmt-empty", "No comments on this document." }
                }
                for row in rows {
                    button {
                        key: "{row.page}-{row.index}",
                        class: "pdit-cmt-row",
                        r#type: "button",
                        onclick: move |_| {
                            scroll_to(row.page, row.index);
                            annotate.flash.set(Some((row.page, row.index)));
                            let mut flash = annotate.flash;
                            set_timeout(FLASH_MS, move || flash.set(None));
                        },
                        span { dangerous_inner_html: icon_of(row.kind), style: "display: contents" }
                        span { class: "what",
                            b { "{kind_name(row.kind)}" }
                            span { "{row.text}" }
                        }
                        span { class: "pg", "p. {row.page + 1}" }
                    }
                }
            }
        }
    }
}

fn rows_on(page: u16) -> Vec<Row> {
    let all = annots::annotations(page).unwrap_or_default();
    let words = if all
        .iter()
        .any(|a| matches!(a.kind, AnnotationKind::Markup(_)))
    {
        annots::words(page).unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut rows: Vec<Row> = all
        .into_iter()
        .map(|a| {
            let text = match a.kind {
                AnnotationKind::Note | AnnotationKind::Stamp => a.text.clone(),
                AnnotationKind::Ink => "Freehand drawing".to_owned(),
                AnnotationKind::Markup(_) => marked_words(&words, &a.rects),
            };
            Row {
                page,
                index: a.index,
                kind: a.kind,
                text,
                top: a.bounds[3],
            }
        })
        .collect();
    rows.sort_by(|a, b| b.top.total_cmp(&a.top));
    rows
}

/// The words a mark covers (their centres inside its line boxes).
fn marked_words(words: &[Word], rects: &[[f32; 4]]) -> String {
    words
        .iter()
        .filter(|w| {
            let (cx, cy) = (
                (w.bounds[0] + w.bounds[2]) / 2.0,
                (w.bounds[1] + w.bounds[3]) / 2.0,
            );
            rects
                .iter()
                .any(|r| cx >= r[0] && cx <= r[2] && cy >= r[1] && cy <= r[3])
        })
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn kind_name(kind: AnnotationKind) -> &'static str {
    match kind {
        AnnotationKind::Note => "Note",
        AnnotationKind::Markup(MarkupKind::Highlight) => "Highlight",
        AnnotationKind::Markup(MarkupKind::Underline) => "Underline",
        AnnotationKind::Markup(MarkupKind::Strikeout) => "Strikeout",
        AnnotationKind::Ink => "Drawing",
        AnnotationKind::Stamp => "Stamp",
    }
}

/// Scrolls the comment's clickable area into the middle of the window.
fn scroll_to(page: u16, index: usize) {
    use wasm_bindgen::JsCast;
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let selector = format!("[data-annot='{page}-{index}']");
    let target = document
        .query_selector(&selector)
        .ok()
        .flatten()
        .or_else(|| {
            // Pages far away render lazily: bring the page in first.
            document
                .query_selector(&format!(".page-list .page[data-page='{page}']"))
                .ok()
                .flatten()
        });
    let Some(target) = target else { return };
    let options = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&options, &"block".into(), &"center".into());
    let _ = js_sys::Reflect::set(&options, &"behavior".into(), &"smooth".into());
    if let Ok(f) = js_sys::Reflect::get(&target, &"scrollIntoView".into())
        && let Ok(f) = f.dyn_into::<js_sys::Function>()
    {
        let _ = f.call1(&target, &options);
    }
}
