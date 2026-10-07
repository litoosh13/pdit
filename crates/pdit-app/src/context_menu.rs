//! Right-click menu (D-023, D-025, D-050): only the options of the item under
//! the pointer — a link, a form field, a comment, a text line, an image — or,
//! on empty space, the page's own actions. Every tool lives in the tools rail
//! (tools_ui.rs), which runs the same [`Action`]s through [`run`].
//! Look and motion: assets/css/context-menu.css.

use crate::editing::Editing;
use crate::page_tools::{PageAction, PageTools};
use crate::shapes_ui::ShapeDraw;
use dioxus::prelude::*;
use pdit_core::ShapeKind;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const CONTEXT_MENU_CSS: Asset = asset!("/assets/css/context-menu.css");
const ICON_EDIT_TEXT: &str = include_str!("../assets/icons/cartoon-pencil.svg");
const ICON_ADD_TEXT: &str = include_str!("../assets/icons/cartoon-type.svg");
const ICON_ROTATE: &str = include_str!("../assets/icons/cartoon-rotate-ccw.svg");
const ICON_BLANK: &str = include_str!("../assets/icons/cartoon-blank-page.svg");
const ICON_COPY: &str = include_str!("../assets/icons/cartoon-copy.svg");
const ICON_EXTRACT: &str = include_str!("../assets/icons/cartoon-file-output.svg");
const ICON_IMPORT: &str = include_str!("../assets/icons/cartoon-import.svg");
const ICON_TRASH: &str = include_str!("../assets/icons/cartoon-trash.svg");
const ICON_ADD_IMAGE: &str = include_str!("../assets/icons/cartoon-image-plus.svg");
const ICON_SELECT_IMAGE: &str = include_str!("../assets/icons/person-selecting-note.svg");
const ICON_RECT: &str = include_str!("../assets/icons/cartoon-rectangle-sides.svg");
const ICON_ELLIPSE: &str = include_str!("../assets/icons/circle-small.svg");
const ICON_LINE: &str = include_str!("../assets/icons/pen-line.svg");
const ICON_ARROW: &str = include_str!("../assets/icons/arrow-up-right.svg");
const ICON_TABLE: &str = include_str!("../assets/icons/fact-table.svg");
const ICON_SIGNATURE: &str = include_str!("../assets/icons/signature.svg");
const ICON_DRAW: &str = include_str!("../assets/icons/cartoon-pen.svg");
const ICON_STAMP: &str = include_str!("../assets/icons/cartoon-stamp.svg");
const ICON_COMMENTS: &str = include_str!("../assets/icons/cartoon-message-square.svg");
const ICON_SETTINGS: &str = include_str!("../assets/icons/cartoon-settings.svg");

/// The Transitions.dev dropdown's close duration (--duration-quick).
const CLOSE_MS: i32 = 150;
/// Keeps the menu this far from the window edges when it flips.
const EDGE_GAP: f64 = 8.0;

/// What a menu entry (or a tools rail row) does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    /// A link's own entries, by its index on the page (D-050).
    OpenLink(usize),
    GoToLink(usize),
    EditLink(usize),
    RemoveLink(usize),
    /// A link on the text line under the pointer.
    AddLinkToLine,
    /// A form field's entries, by its annotation index.
    FieldSettings(usize),
    DeleteField(usize),
    /// A comment's entries, by its annotation index.
    OpenNote(usize),
    DeleteNote(usize),
    DeleteMark(usize),
    /// The image under the pointer: rotate (true = right) or delete.
    RotateImage(bool),
    DeleteImage,
    EditText,
    AddText,
    SelectImage,
    AddImage,
    AddSignature,
    AddNote,
    Draw,
    /// Re-opens the menu as the list of stamps (D-041).
    StampMenu,
    Stamp(usize),
    AllComments,
    HeaderFooter,
    Watermark,
    LinkWebAddresses,
    Bookmarks,
    /// Form edit mode (D-047), with a field type armed or none.
    EditForm(Option<crate::form_edit_ui::Tool>),
    DoneEditingForm,
    AddShape(ShapeKind),
    AddTable,
    Page(PageAction),
    ExtractPage,
    InsertPdf,
    /// The print window (D-052).
    Print,
    /// leafmind's field finder (D-053).
    FindFields,
}

impl Action {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Action::OpenLink(_) => "Open link",
            Action::GoToLink(_) => "Go to linked page",
            Action::EditLink(_) => "Edit link…",
            Action::RemoveLink(_) => "Remove link",
            Action::AddLinkToLine => "Add link…",
            Action::FieldSettings(_) => "Field settings…",
            Action::DeleteField(_) => "Delete field",
            Action::OpenNote(_) => "Open note",
            Action::DeleteNote(_) => "Delete note",
            Action::DeleteMark(_) => "Delete",
            Action::RotateImage(false) => "Rotate left",
            Action::RotateImage(true) => "Rotate right",
            Action::DeleteImage => "Delete image",
            Action::EditText => "Edit text",
            Action::AddText => "Add text",
            Action::SelectImage => "Select image",
            Action::AddImage => "Add image",
            Action::AddSignature => "Add signature",
            Action::AddNote => "Add note",
            Action::Draw => "Draw",
            Action::StampMenu => "Stamp…",
            Action::Stamp(i) => crate::annotations_ui::STAMPS[i].0,
            Action::AllComments => "All comments",
            Action::HeaderFooter => "Header & footer…",
            Action::Watermark => "Watermark…",
            Action::LinkWebAddresses => "Link web addresses",
            Action::Bookmarks => "Bookmarks",
            Action::EditForm(None) => "Edit form",
            Action::EditForm(Some(tool)) => tool.label(),
            Action::DoneEditingForm => "Done editing form",
            Action::AddShape(ShapeKind::Rectangle) => "Rectangle",
            Action::AddShape(ShapeKind::Ellipse) => "Ellipse",
            Action::AddShape(ShapeKind::Line) => "Line",
            Action::AddShape(ShapeKind::Arrow) => "Arrow",
            Action::AddTable => "Table",
            Action::Page(PageAction::RotateLeft) => "Rotate left",
            Action::Page(PageAction::RotateRight) => "Rotate right",
            Action::Page(PageAction::InsertBlank) => "Insert blank page",
            Action::Page(PageAction::Duplicate) => "Duplicate page",
            Action::Page(PageAction::Delete) => "Delete page",
            Action::ExtractPage => "Extract page as PDF",
            Action::InsertPdf => "Insert pages from PDF…",
            Action::Print => "Print…",
            Action::FindFields => "Find fields",
        }
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Action::OpenLink(_) | Action::GoToLink(_) => ICON_ARROW,
            Action::EditLink(_) => ICON_EDIT_TEXT,
            Action::AddLinkToLine => crate::links_ui::ICON_LINK,
            Action::FieldSettings(_) => ICON_SETTINGS,
            Action::OpenNote(_) => crate::annotations_ui::ICON_NOTE,
            Action::RotateImage(_) => ICON_ROTATE,
            Action::RemoveLink(_)
            | Action::DeleteField(_)
            | Action::DeleteNote(_)
            | Action::DeleteMark(_)
            | Action::DeleteImage => ICON_TRASH,
            Action::EditText => ICON_EDIT_TEXT,
            Action::AddText => ICON_ADD_TEXT,
            Action::SelectImage => ICON_SELECT_IMAGE,
            Action::AddImage => ICON_ADD_IMAGE,
            Action::AddSignature => ICON_SIGNATURE,
            Action::AddNote => crate::annotations_ui::ICON_NOTE,
            Action::Draw => ICON_DRAW,
            Action::StampMenu | Action::Stamp(_) => ICON_STAMP,
            Action::AllComments => ICON_COMMENTS,
            Action::HeaderFooter => crate::doc_ui::ICON_HF,
            Action::Watermark => crate::doc_ui::ICON_WM,
            Action::LinkWebAddresses => crate::links_ui::ICON_LINK,
            Action::Bookmarks => crate::bookmarks_ui::ICON_BOOKMARK,
            Action::EditForm(None) | Action::DoneEditingForm => crate::form_edit_ui::ICON_FORM,
            Action::EditForm(Some(tool)) => tool.icon(),
            Action::AddShape(ShapeKind::Rectangle) => ICON_RECT,
            Action::AddShape(ShapeKind::Ellipse) => ICON_ELLIPSE,
            Action::AddShape(ShapeKind::Line) => ICON_LINE,
            Action::AddShape(ShapeKind::Arrow) => ICON_ARROW,
            Action::AddTable => ICON_TABLE,
            Action::Page(PageAction::RotateLeft | PageAction::RotateRight) => ICON_ROTATE,
            Action::Page(PageAction::InsertBlank) => ICON_BLANK,
            Action::Page(PageAction::Duplicate) => ICON_COPY,
            Action::Page(PageAction::Delete) => ICON_TRASH,
            Action::ExtractPage => ICON_EXTRACT,
            Action::InsertPdf => ICON_IMPORT,
            // ponytail: no icon until the user picks a Koboyo one.
            Action::Print | Action::FindFields => "",
        }
    }

    /// A stamp's row shows its label in the stamp's colour (D-041).
    pub(crate) fn label_style(self) -> String {
        match self {
            Action::Stamp(i) => {
                let [r, g, b] = crate::annotations_ui::STAMPS[i].1;
                format!(
                    "color: rgb({r}, {g}, {b}); font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; font-size: 12px;"
                )
            }
            _ => String::new(),
        }
    }

    /// Rotate right shows the Rotate left icon mirrored (context-menu.css).
    pub(crate) fn mirrored(self) -> bool {
        matches!(
            self,
            Action::Page(PageAction::RotateRight) | Action::RotateImage(true)
        )
    }
}

/// An open menu: where it was asked for, and its sections.
#[derive(Clone, PartialEq, Debug)]
pub struct OpenMenu {
    /// Changes for every right-click, so each menu starts fresh.
    pub id: u64,
    /// Pointer position in CSS px (viewport).
    pub x: f64,
    pub y: f64,
    /// The page and the point on it in PDF points, for the actions.
    pub page: u16,
    pub point: (f32, f32),
    pub sections: Vec<(&'static str, Vec<Action>)>,
}

/// The menu's shared state.
#[derive(Clone, Copy)]
pub struct ContextMenuState {
    pub open: Signal<Option<OpenMenu>>,
    closing: Signal<bool>,
}

impl ContextMenuState {
    pub fn provide() -> Self {
        use_context_provider(|| Self {
            open: Signal::new(None),
            closing: Signal::new(false),
        })
    }

    /// Opens the menu of the item at this point (D-050). Returns false when
    /// nothing applies, so the browser's own menu can show instead.
    pub fn open_at(mut self, x: f64, y: f64, page: u16, point: (f32, f32)) -> bool {
        let sections = item_sections(page, point);
        let id = self.open.peek().as_ref().map_or(0, |m| m.id + 1);
        self.closing.set(false);
        self.open.set(Some(OpenMenu {
            id,
            x,
            y,
            page,
            point,
            sections,
        }));
        true
    }

    /// Opens a menu with `sections` at the same place as the menu that was
    /// just picked from (Stamp… → the stamp list, D-041).
    pub fn reopen_with(
        mut self,
        (x, y): (f64, f64),
        page: u16,
        point: (f32, f32),
        sections: Vec<(&'static str, Vec<Action>)>,
    ) {
        let id = self.open.peek().as_ref().map_or(0, |m| m.id + 1);
        self.closing.set(false);
        self.open.set(Some(OpenMenu {
            id,
            x,
            y,
            page,
            point,
            sections,
        }));
    }

    /// Plays the dropdown's closing transition, then removes the menu.
    pub fn close(mut self) {
        let Some(id) = self.open.peek().as_ref().map(|m| m.id) else {
            return;
        };
        if *self.closing.peek() {
            return;
        }
        self.closing.set(true);
        let mut open = self.open;
        let mut closing = self.closing;
        set_timeout(CLOSE_MS, move || {
            // Only the menu that was closing goes away, never one opened since.
            if open.peek().as_ref().map(|m| m.id) == Some(id) {
                open.set(None);
                closing.set(false);
            }
        });
    }
}

#[component]
pub fn ContextMenu() -> Element {
    let menu = use_context::<ContextMenuState>();
    use_dismiss(menu);
    rsx! {
        document::Stylesheet { href: CONTEXT_MENU_CSS }
        if let Some(open) = menu.open.read().clone() {
            MenuPanel { key: "{open.id}", open, closing: (menu.closing)() }
        }
    }
}

#[component]
fn MenuPanel(open: OpenMenu, closing: bool) -> Element {
    let menu = use_context::<ContextMenuState>();
    // Placed after measuring: (left, top, data-origin).
    let mut placement = use_signal(|| None::<(f64, f64, &'static str)>);
    let mut shown = use_signal(|| false);

    let (left, top, origin) = placement().unwrap_or((open.x, open.y, "top-left"));
    let state = if closing {
        " is-closing"
    } else if shown() {
        " is-open"
    } else {
        ""
    };
    let (page, (x, y)) = (open.page, open.point);
    let at = (open.x, open.y);
    rsx! {
        div {
            class: "cm-menu t-dropdown{state}",
            role: "menu",
            "data-origin": origin,
            style: "left: {left}px; top: {top}px;",
            // Stops the browser's menu on a right-click over the menu itself.
            oncontextmenu: move |event| event.prevent_default(),
            onmounted: move |event| {
                let Some(element) = event.data().downcast::<web_sys::Element>().cloned() else {
                    return;
                };
                placement.set(Some(place(&element, open.x, open.y)));
                // One frame in the start state, so the opening transition runs.
                // The menu may already be gone by then (an entry picked at
                // once); then there is nothing to show.
                next_frame(move || {
                    next_frame(move || {
                        if let Ok(mut shown) = shown.try_write() {
                            *shown = true;
                        }
                    })
                });
            },
            for (label, actions) in open.sections.iter().cloned() {
                div { class: "cm-label", "{label}" }
                for action in actions {
                    button {
                        r#type: "button",
                        role: "menuitem",
                        onclick: move |_| {
                            menu.close();
                            match action {
                                // After this menu has closed: replacing it inside its own
                                // click leaves the new rows without listeners.
                                Action::StampMenu => set_timeout(CLOSE_MS, move || {
                                    let stamps = (0..crate::annotations_ui::STAMPS.len()).map(Action::Stamp);
                                    menu.reopen_with(at, page, (x, y), vec![("Stamp", stamps.collect())]);
                                }),
                                _ => run(action, page, x, y),
                            }
                        },
                        span {
                            class: if action.mirrored() { "cm-mirror" } else { "" },
                            dangerous_inner_html: action.icon(),
                            style: "display: contents",
                        }
                        span { style: action.label_style(), "{action.label()}" }
                    }
                }
            }
        }
    }
}

/// The menu for what is under `(x, y)` on `page` (PDF points), most specific
/// first: a link, a form field, a comment, a text line, an image, else the page.
pub(crate) fn item_sections(page: u16, (x, y): (f32, f32)) -> Vec<(&'static str, Vec<Action>)> {
    use pdit_core::links::LinkTarget;
    let inside = |[l, b, r, t]: [f32; 4]| x >= l && x <= r && y >= b && y <= t;
    let links = pdit_core::links::links(page).unwrap_or_default();
    if let Some(link) = links.iter().rev().find(|l| {
        if l.rects.is_empty() {
            inside(l.bounds)
        } else {
            l.rects.iter().any(|r| inside(*r))
        }
    }) {
        let i = link.index;
        let mut row = match link.target {
            LinkTarget::Web(_) => vec![Action::OpenLink(i), Action::EditLink(i)],
            LinkTarget::Page(_) => vec![Action::GoToLink(i), Action::EditLink(i)],
            LinkTarget::Other => vec![],
        };
        row.push(Action::RemoveLink(i));
        return vec![("Link", row)];
    }
    let fields = pdit_core::form_fields(page).unwrap_or_default();
    if let Some(f) = fields.iter().rev().find(|f| {
        let (l, b, r, t) = f.rect;
        inside([l, b, r, t])
    }) {
        let a = f.annotation;
        return vec![(
            "Form field",
            vec![Action::FieldSettings(a), Action::DeleteField(a)],
        )];
    }
    let comments = pdit_core::annotations::annotations(page).unwrap_or_default();
    if let Some(c) = comments.iter().rev().find(|c| {
        if c.rects.is_empty() {
            inside(c.bounds)
        } else {
            c.rects.iter().any(|r| inside(*r))
        }
    }) {
        return if c.kind == pdit_core::annotations::AnnotationKind::Note {
            vec![(
                "Note",
                vec![Action::OpenNote(c.index), Action::DeleteNote(c.index)],
            )]
        } else {
            vec![("Comment", vec![Action::DeleteMark(c.index)])]
        };
    }
    if crate::editing::line_at(page, x, y).is_some() {
        return vec![("Text", vec![Action::EditText, Action::AddLinkToLine])];
    }
    if pdit_core::page_ops::image_at(page, x, y).is_ok_and(|hit| hit.is_some()) {
        return vec![(
            "Image",
            vec![
                Action::SelectImage,
                Action::RotateImage(false),
                Action::RotateImage(true),
                Action::DeleteImage,
            ],
        )];
    }
    vec![("Page", page_actions())]
}

/// The page's own actions (right-click on empty space, the rail's Page).
pub(crate) fn page_actions() -> Vec<Action> {
    let mut actions = vec![
        Action::Page(PageAction::RotateLeft),
        Action::Page(PageAction::RotateRight),
        Action::Page(PageAction::InsertBlank),
        Action::Page(PageAction::Duplicate),
        Action::ExtractPage,
    ];
    // The last page can't be deleted, so Delete isn't offered then.
    if pdit_core::page_ops::page_sizes().is_ok_and(|sizes| sizes.len() > 1) {
        actions.push(Action::Page(PageAction::Delete));
    }
    actions
}

/// Runs `action` at `(x, y)` on `page` (PDF points); actions that don't need
/// a point ignore it. `StampMenu` is handled by its caller (it opens a list).
pub(crate) fn run(action: Action, page: u16, x: f32, y: f32) {
    let editing = consume_context::<Editing>();
    let tools = consume_context::<PageTools>();
    let shapes = consume_context::<ShapeDraw>();
    let annotate = consume_context::<crate::annotations_ui::Annotate>();
    let links = consume_context::<crate::links_ui::LinkUi>();
    let forms = consume_context::<crate::form_edit_ui::FormEdit>();
    match action {
        Action::OpenLink(i) | Action::GoToLink(i) => links.follow(page, i),
        Action::EditLink(i) => links.edit_at(page, i),
        Action::RemoveLink(i) => links.remove(page, i),
        Action::AddLinkToLine => {
            if let Some(line) = crate::editing::line_at(page, x, y) {
                links.link_words(page, vec![line.bounds]);
            }
        }
        Action::FieldSettings(a) => forms.settings_for(page, a),
        Action::DeleteField(a) => forms.delete(page, a),
        Action::OpenNote(i) => annotate.open_note_at(page, i),
        Action::DeleteNote(i) | Action::DeleteMark(i) => annotate.delete_at(page, i),
        Action::RotateImage(clockwise) => {
            tools.select_image(page, x, y);
            tools.rotate_image_selection(clockwise);
        }
        Action::DeleteImage => {
            tools.select_image(page, x, y);
            tools.delete_image_selection();
        }
        Action::EditText => editing.select_at(page, x, y),
        Action::AddText => {
            // In a table cell, snap into the cell (D-038).
            let (x, y) = tools.snap_to_cell(page, x, y);
            editing.start_add(page, x, y)
        }
        Action::SelectImage => tools.select_image(page, x, y),
        Action::AddImage => tools.pick_image_at(page, x, y),
        Action::AddSignature => {
            consume_context::<crate::signature_ui::Signature>().open_at(page, x, y)
        }
        Action::AddNote => annotate.add_note(page, x, y),
        Action::Draw => shapes.arm_pen(),
        Action::StampMenu => {}
        Action::Stamp(i) => annotate.add_stamp(page, x, y, i),
        Action::AllComments => annotate.open_panel(),
        Action::HeaderFooter => {
            consume_context::<crate::doc_ui::DocTools>().open(crate::doc_ui::Kind::HeaderFooter)
        }
        Action::Watermark => {
            consume_context::<crate::doc_ui::DocTools>().open(crate::doc_ui::Kind::Watermark)
        }
        Action::LinkWebAddresses => links.link_web_addresses(),
        Action::EditForm(tool) => forms.start(tool),
        Action::DoneEditingForm => forms.finish(),
        Action::Bookmarks => consume_context::<crate::bookmarks_ui::BookmarksUi>().open(),
        Action::AddShape(kind) => shapes.arm(kind),
        Action::AddTable => shapes.arm_table(),
        Action::Page(page_action) => tools.run(page_action, page),
        Action::ExtractPage => tools.extract(page),
        Action::InsertPdf => tools.pick_pdf_to_insert(page),
        Action::Print => consume_context::<crate::print_ui::Print>().open(),
        Action::FindFields => consume_context::<crate::find_fields_ui::FindFields>().start(),
    }
}

/// Opens down-right from the pointer, flipping left or up where the menu would
/// leave the window; the origin follows so it grows from the pointer.
fn place(element: &web_sys::Element, x: f64, y: f64) -> (f64, f64, &'static str) {
    let rect = element.get_bounding_client_rect();
    let (width, height) = (rect.width(), rect.height());
    let (view_w, view_h) = web_sys::window()
        .map(|w| {
            (
                w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(0.0),
                w.inner_height()
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            )
        })
        .unwrap_or_default();
    let flip_x = x + width > view_w - EDGE_GAP && x - width >= EDGE_GAP;
    let flip_y = y + height > view_h - EDGE_GAP && y - height >= EDGE_GAP;
    let left = if flip_x { x - width } else { x };
    let top = if flip_y { y - height } else { y };
    let origin = match (flip_y, flip_x) {
        (false, false) => "top-left",
        (false, true) => "top-right",
        (true, false) => "bottom-left",
        (true, true) => "bottom-right",
    };
    (left, top, origin)
}

/// Closes the menu on a press outside it, on Escape, or when the page scrolls.
/// A left press that closes the menu does only that: its click does not reach
/// the page (as with the Gooey menus).
fn use_dismiss(menu: ContextMenuState) {
    use_hook(|| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let on_pointer = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(
            move |event: web_sys::PointerEvent| {
                if menu.open.peek().is_none() {
                    return;
                }
                let inside = event
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .and_then(|el| el.closest(".cm-menu").ok().flatten())
                    .is_some();
                if inside {
                    return;
                }
                if event.button() == 0 {
                    swallow_next_click();
                }
                menu.close();
            },
        );
        let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if event.key() == "Escape" {
                    menu.close();
                }
            },
        );
        let on_scroll = Closure::<dyn FnMut()>::new(move || menu.close());
        let capture = web_sys::AddEventListenerOptions::new();
        capture.set_capture(true);
        let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
            "pointerdown",
            on_pointer.as_ref().unchecked_ref(),
            &capture,
        );
        let _ = window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
            "scroll",
            on_scroll.as_ref().unchecked_ref(),
            &capture,
        );
        // The app lives as long as the page.
        on_pointer.forget();
        on_key.forget();
        on_scroll.forget();
    });
}

/// Stops the click that follows the current press, if one follows within 1 s.
fn swallow_next_click() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let capture = web_sys::AddEventListenerOptions::new();
    capture.set_capture(true);
    capture.set_once(true);
    let on_click = Closure::once_into_js(|event: web_sys::Event| {
        event.prevent_default();
        event.stop_propagation();
    });
    let callback: js_sys::Function = on_click.unchecked_into();
    let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
        "click", &callback, &capture,
    );
    let window_for_timeout = window.clone();
    set_timeout(1000, move || {
        let _ = window_for_timeout
            .remove_event_listener_with_callback_and_bool("click", &callback, true);
    });
}

fn next_frame(f: impl FnOnce() + 'static) {
    if let Some(window) = web_sys::window() {
        let callback = Closure::once_into_js(f);
        let _ = window.request_animation_frame(callback.unchecked_ref());
    }
}

fn set_timeout(ms: i32, f: impl FnOnce() + 'static) {
    if let Some(window) = web_sys::window() {
        let callback = Closure::once_into_js(f);
        let _ = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), ms);
    }
}
