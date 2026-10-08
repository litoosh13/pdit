//! The tools rail (D-050, redesign D-062): a strip of sections on the left
//! edge; clicking one opens its tools in a panel docked beside it, which pushes
//! the pages over (the Canva editor's layout). Rows are Beautiful UI "Sidebar
//! nav" rows with its sliding hover highlight. Tools that need a spot (Add text, Add image, Add note, …) arm
//! first; the next click on a page places them (pages.rs).
//! Look: assets/css/tools.css.

use crate::context_menu::{Action, page_actions, run};
use dioxus::prelude::*;
use pdit_core::ShapeKind;
use wasm_bindgen::JsCast;

const TOOLS_CSS: Asset = asset!("/assets/css/tools.css");
/// The rail's surface (shared with the Pages panel), also before any PDF.
const PANEL_CSS: Asset = asset!("/assets/css/thumbnails.css");
const ICON_TEXT: &str = include_str!("../assets/icons/devigner/Text.svg");
const ICON_IMAGE: &str = include_str!("../assets/icons/devigner/GalleryAdd.svg");
const ICON_SHAPES: &str = include_str!("../assets/icons/devigner/Shapes.svg");
const ICON_DOCUMENT: &str = include_str!("../assets/icons/devigner/DocumentText.svg");
const ICON_PAGE: &str = include_str!("../assets/icons/devigner/DocumentNormal.svg");
// File actions at the top of the rail (D-062): Devigner Icons (assets/icons/devigner, see SOURCES.md).
const ICON_OPEN: &str = include_str!("../assets/icons/devigner/Folder.svg");
const ICON_NEW: &str = include_str!("../assets/icons/devigner/File.svg");
pub(crate) const ICON_PRINT: &str = include_str!("../assets/icons/devigner/Printer.svg");
const ICON_CLOSE_PANEL: &str = include_str!("../assets/icons/devigner/ArrowLeft.svg");
const ICON_ABOUT: &str = include_str!("../assets/icons/devigner/Crown.svg");
/// About: the rail's last section, at its bottom; its flyout holds the About
/// panel (update_ui.rs) instead of tool rows.
const ABOUT: usize = SECTIONS.len();
/// The Page section (page actions and the thumbnails).
const PAGE: usize = 7;

/// Rail sections: (title, rail label, icon).
const SECTIONS: [(&str, &str, &str); 8] = [
    // Find opens the find card instead of a flyout (D-051).
    ("Find", "Find", crate::search_ui::ICON_FIND),
    ("Text", "Text", ICON_TEXT),
    ("Image", "Image", ICON_IMAGE),
    ("Shapes", "Shapes", ICON_SHAPES),
    ("Comment", "Comment", crate::annotations_ui::ICON_NOTE),
    ("Form", "Form", crate::form_edit_ui::ICON_FORM),
    ("Document", "Document", ICON_DOCUMENT),
    ("Pages", "Page", ICON_PAGE),
];

/// The rows of section `i` (some depend on the document's state).
fn rows(i: usize, stamps: bool) -> Vec<Action> {
    if stamps {
        return (0..crate::annotations_ui::STAMPS.len())
            .map(Action::Stamp)
            .collect();
    }
    match SECTIONS[i].0 {
        "Text" => vec![Action::AddText],
        "Image" => vec![Action::AddImage, Action::AddSignature],
        "Shapes" => vec![
            Action::AddShape(ShapeKind::Rectangle),
            Action::AddShape(ShapeKind::Ellipse),
            Action::AddShape(ShapeKind::Line),
            Action::AddShape(ShapeKind::Arrow),
            Action::AddTable,
        ],
        "Comment" => vec![
            Action::AddNote,
            Action::Draw,
            Action::StampMenu,
            Action::AllComments,
        ],
        "Form" => {
            let editing = consume_context::<crate::form_edit_ui::FormEdit>().editing();
            let mut form = vec![if editing {
                Action::DoneEditingForm
            } else {
                Action::EditForm(None)
            }];
            form.extend(crate::form_edit_ui::Tool::ALL.map(|t| Action::EditForm(Some(t))));
            form.push(Action::FindFields);
            form
        }
        "Document" => vec![
            Action::HeaderFooter,
            Action::Watermark,
            Action::LinkWebAddresses,
            Action::Bookmarks,
            Action::InsertPdf,
            Action::Print,
        ],
        _ => page_actions(),
    }
}

/// Tools that need a spot on a page: armed, then placed by the next click.
fn places(action: Action) -> bool {
    matches!(
        action,
        Action::AddText
            | Action::AddImage
            | Action::AddSignature
            | Action::AddNote
            | Action::Stamp(_)
    )
}

/// Shared rail state.
#[derive(Clone, Copy)]
pub struct Tools {
    /// The section whose tools the flyout shows (kept while it closes).
    section: Signal<Option<usize>>,
    /// The docked panel is open.
    shown: Signal<bool>,
    /// The Comment flyout lists the stamps (after "Stamp…").
    stamps: Signal<bool>,
    /// The armed placing tool.
    placing: Signal<Option<Action>>,
}

impl Tools {
    pub fn provide() -> Self {
        use_context_provider(|| Tools {
            section: Signal::new(None),
            shown: Signal::new(false),
            stamps: Signal::new(false),
            placing: Signal::new(None),
        })
    }

    pub fn placing(&self) -> bool {
        self.placing.read().is_some()
    }

    /// The armed tool, disarmed (a page click places it).
    pub fn take_placing(mut self) -> Option<Action> {
        let action = self.placing.take();
        if action.is_some() {
            self.close();
        }
        action
    }

    /// Opens section `i` in the docked panel (another open section is
    /// replaced in place).
    fn open(mut self, i: usize) {
        self.section.set(Some(i));
        self.stamps.set(false);
        self.shown.set(true);
    }

    /// The bottom bar's page count: the Page section, with the thumbnails.
    pub fn open_pages(self) {
        self.open(PAGE);
    }

    /// Closes the docked panel; its rows stay while it slides shut.
    fn close(mut self) {
        self.shown.set(false);
    }

    /// Escape: closes the flyout and disarms a placing tool.
    pub fn escape(mut self) {
        self.placing.set(None);
        self.close();
    }

    fn pick(mut self, action: Action) {
        if action == Action::StampMenu {
            self.stamps.set(true);
            return;
        }
        if places(action) {
            let again = *self.placing.peek() == Some(action);
            self.placing.set(if again { None } else { Some(action) });
            return;
        }
        self.placing.set(None);
        self.close();
        run(action, current_page(), 0.0, 0.0);
    }
}

/// The page nearest the top of the window (the "This page" actions use it).
fn current_page() -> u16 {
    crate::bookmarks_ui::current_page()
}

/// Moves the Sidebar nav highlight `hl` onto the row under the pointer.
fn highlight(mut hl: Signal<Option<(f64, f64)>>, event: &Event<MouseData>) {
    let row = event
        .data()
        .downcast::<web_sys::MouseEvent>()
        .and_then(|e| e.current_target())
        .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(row) = row {
        hl.set(Some((
            f64::from(row.offset_top()),
            f64::from(row.offset_height()),
        )));
    }
}

#[component]
pub fn ToolRail(has_document: bool) -> Element {
    let tools = use_context::<Tools>();
    let updates = use_context::<crate::update_ui::Updates>();
    let shapes = use_context::<crate::shapes_ui::ShapeDraw>();
    let mut rail_hl = use_signal(|| None::<(f64, f64)>);
    let mut fly_hl = use_signal(|| None::<(f64, f64)>);
    let section = (tools.section)();
    let shown = (tools.shown)();
    let stamps = (tools.stamps)();
    let placing = (tools.placing)();
    let armed = shapes.armed();
    let title = match (section, stamps) {
        (_, true) => "Stamp",
        (Some(ABOUT), false) => "About",
        (Some(i), false) => SECTIONS[i].0,
        (None, false) => "",
    };
    let hl_style = |hl: Option<(f64, f64)>| match hl {
        Some((top, height)) => format!("top: {top}px; height: {height}px; opacity: 1;"),
        None => "opacity: 0;".to_owned(),
    };
    rsx! {
        document::Stylesheet { href: PANEL_CSS }
        document::Stylesheet { href: TOOLS_CSS }
        document::Stylesheet { href: crate::update_ui::UPDATE_CSS }
        aside {
            class: "pdit-rail t-panel-slide",
            "data-open": "true",
            "aria-label": "Tools",
            // File actions (D-062): Open, New, and Print once a PDF is open.
            div { class: "sb-nav pdit-rail-file",
                button {
                    class: "sb-item",
                    r#type: "button",
                    title: "Open a PDF",
                    onclick: move |_| crate::open_file(),
                    span { dangerous_inner_html: ICON_OPEN, style: "display: contents" }
                    span { "Open" }
                }
                button {
                    class: "sb-item",
                    r#type: "button",
                    title: "New blank PDF",
                    onclick: move |_| crate::new_document(),
                    span { dangerous_inner_html: ICON_NEW, style: "display: contents" }
                    span { "New" }
                }
                if has_document {
                    button {
                        class: "sb-item",
                        r#type: "button",
                        title: "Print",
                        onclick: move |_| run(Action::Print, current_page(), 0.0, 0.0),
                        span { dangerous_inner_html: ICON_PRINT, style: "display: contents" }
                        span { "Print" }
                    }
                }
            }
            if has_document {
                span { class: "pdit-rail-line" }
            }
            div {
                class: "sb-nav",
                onmouseleave: move |_| rail_hl.set(None),
                span { class: "sb-hl", style: hl_style(rail_hl()) }
                for (i, (name, short, icon)) in SECTIONS.into_iter().enumerate().filter(|_| has_document) {
                    button {
                        key: "{i}",
                        class: "sb-item",
                        r#type: "button",
                        title: name,
                        "data-section": "{i}",
                        "aria-expanded": if section == Some(i) && shown { "true" } else { "false" },
                        onmouseenter: move |event| highlight(rail_hl, &event),
                        onclick: move |_| {
                            if SECTIONS[i].0 == "Find" {
                                tools.close();
                                consume_context::<crate::search_ui::Find>().open();
                            } else if *tools.section.peek() == Some(i) && *tools.shown.peek() {
                                tools.close();
                            } else {
                                tools.open(i);
                            }
                        },
                        span { dangerous_inner_html: icon, style: "display: contents" }
                        span { "{short}" }
                    }
                }
            }
            span { class: "pdit-rail-grow" }
            // An update is waiting (D-058): its row brings the prompt back.
            if let Some(label) = updates.waiting() {
                div { class: "sb-nav",
                    button {
                        class: "sb-item pdit-rail-update",
                        r#type: "button",
                        title: if label == "Restart" { "Restart to use the update" } else { "A new version of pdit is available" },
                        onclick: move |_| updates.show(),
                        span { class: "dot" }
                        span { "{label}" }
                    }
                }
            }
            div { class: "sb-nav",
                button {
                    class: "sb-item",
                    r#type: "button",
                    title: "About pdit",
                    "data-section": "{ABOUT}",
                    "aria-expanded": if section == Some(ABOUT) && shown { "true" } else { "false" },
                    onclick: move |_| {
                        if *tools.section.peek() == Some(ABOUT) && *tools.shown.peek() {
                            tools.close();
                        } else {
                            tools.open(ABOUT);
                        }
                    },
                    span { dangerous_inner_html: ICON_ABOUT, style: "display: contents" }
                    span { "About" }
                }
            }
        }
        // The docked panel (D-062): beside the rail; the pages make room for it.
        aside {
            class: "pdit-dock",
            "data-open": if shown { "true" } else { "false" },
            "aria-label": "{title} tools",
            div { class: "pdit-dock-inner",
            div { class: "pdit-dock-head",
                span { class: "title", "{title}" }
                button {
                    class: "pdit-dock-close",
                    r#type: "button",
                    title: "Close (Esc)",
                    "aria-label": "Close the panel",
                    onclick: move |_| tools.close(),
                    span { dangerous_inner_html: ICON_CLOSE_PANEL, style: "display: contents" }
                }
            }
            if section == Some(ABOUT) {
                crate::update_ui::AboutPanel {}
            } else if let Some(i) = section {
                div {
                    class: "sb-nav",
                    onmouseleave: move |_| fly_hl.set(None),
                    span { class: "sb-hl", style: hl_style(fly_hl()) }
                    for action in rows(i, stamps) {
                        button {
                            key: "{action:?}",
                            class: if action.mirrored() { "sb-item cm-mirror" } else { "sb-item" },
                            r#type: "button",
                            "aria-pressed": if placing == Some(action)
                                || matches!(action, Action::AddShape(k) if armed == Some(k))
                            {
                                "true"
                            } else {
                                "false"
                            },
                            "data-places": places(action),
                            onmouseenter: move |event| highlight(fly_hl, &event),
                            onclick: move |_| tools.pick(action),
                            span { dangerous_inner_html: action.icon(), style: "display: contents" }
                            span { style: action.label_style(), "{action.label()}" }
                        }
                    }
                }
                if i == PAGE && !stamps {
                    crate::thumbnails::PageThumbs {}
                }
            }
            }
        }
    }
}
