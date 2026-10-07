//! The tools rail (D-050): a Photoshop-style strip of sections on the left;
//! clicking one opens its tools in a panel beside it (Transitions.dev "Panel
//! reveal"). Rows are Beautiful UI "Sidebar nav" rows with its sliding hover
//! highlight. Tools that need a spot (Add text, Add image, Add note, …) arm
//! first; the next click on a page places them (pages.rs).
//! Look: assets/css/tools.css.

use crate::context_menu::{Action, page_actions, run};
use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;
use pdit_core::ShapeKind;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const TOOLS_CSS: Asset = asset!("/assets/css/tools.css");
const ICON_TEXT: &str = include_str!("../assets/icons/cartoon-type.svg");
const ICON_IMAGE: &str = include_str!("../assets/icons/cartoon-image-plus.svg");
const ICON_SHAPES: &str = include_str!("../assets/icons/cartoon-rectangle-sides.svg");
const ICON_DOCUMENT: &str = include_str!("../assets/icons/cartoon-layout-template.svg");
const ICON_PAGE: &str = include_str!("../assets/icons/cartoon-blank-page.svg");

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
    ("This page", "Page", ICON_PAGE),
];

/// The flyout's close time before another section opens (ms).
const SWITCH_MS: i32 = 180;

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
    /// The flyout is open (its Panel reveal state).
    shown: Signal<bool>,
    /// The flyout's top (px), level with its rail button.
    top: Signal<f64>,
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
            top: Signal::new(20.0),
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

    /// Opens section `i` beside its button; a different open section closes
    /// first, so both moves animate.
    fn open(mut self, i: usize) {
        let was_open = *self.shown.peek();
        self.shown.set(false);
        let mut tools = self;
        let mut show = move || {
            tools.section.set(Some(i));
            tools.stamps.set(false);
            tools.reveal(i);
        };
        if was_open {
            set_timeout(SWITCH_MS, show);
        } else {
            show();
        }
    }

    /// After the rows render: level with button `i`, kept in the window, then
    /// one frame in the closed state so the reveal runs.
    fn reveal(self, i: usize) {
        let (mut top, mut shown, section) = (self.top, self.shown, self.section);
        next_frame(move || {
            top.set(fly_top(i));
            next_frame(move || {
                if *section.peek() == Some(i) {
                    shown.set(true);
                }
            });
        });
    }

    /// Closes the flyout; its rows stay while the reveal plays backwards.
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
            if let Some(i) = *self.section.peek() {
                self.reveal(i);
            }
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

/// The flyout's top for section `i`: its button's top − 8px, inside the window.
fn fly_top(i: usize) -> f64 {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return 20.0;
    };
    let button = document
        .query_selector(&format!(".pdit-rail [data-section='{i}']"))
        .ok()
        .flatten();
    let fly = document
        .query_selector(".pdit-fly")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    let (Some(button), Some(fly)) = (button, fly) else {
        return 20.0;
    };
    let view_h = web_sys::window()
        .and_then(|w| w.inner_height().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(800.0);
    let top = button.get_bounding_client_rect().top() - 8.0;
    top.min(view_h - f64::from(fly.offset_height()) - 16.0)
        .max(16.0)
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
pub fn ToolRail() -> Element {
    let tools = use_context::<Tools>();
    let shapes = use_context::<crate::shapes_ui::ShapeDraw>();
    let mut rail_hl = use_signal(|| None::<(f64, f64)>);
    let mut fly_hl = use_signal(|| None::<(f64, f64)>);
    use_outside_close(tools);
    let section = (tools.section)();
    let shown = (tools.shown)();
    let stamps = (tools.stamps)();
    let placing = (tools.placing)();
    let armed = shapes.armed();
    let title = match (section, stamps) {
        (_, true) => "Stamp",
        (Some(i), false) => SECTIONS[i].0,
        (None, false) => "",
    };
    let hl_style = |hl: Option<(f64, f64)>| match hl {
        Some((top, height)) => format!("top: {top}px; height: {height}px; opacity: 1;"),
        None => "opacity: 0;".to_owned(),
    };
    rsx! {
        document::Stylesheet { href: TOOLS_CSS }
        aside {
            class: "pdit-rail t-panel-slide",
            "data-open": "true",
            "aria-label": "Tools",
            div {
                class: "sb-nav",
                onmouseleave: move |_| rail_hl.set(None),
                span { class: "sb-hl", style: hl_style(rail_hl()) }
                for (i, (name, short, icon)) in SECTIONS.into_iter().enumerate() {
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
        }
        aside {
            class: "pdit-fly t-panel-slide",
            "data-open": if shown { "true" } else { "false" },
            "aria-label": "{title} tools",
            style: "top: {(tools.top)()}px;",
            div { class: "pdit-panel-head",
                span { class: "title", "{title}" }
                button {
                    class: "count pdit-fly-close",
                    r#type: "button",
                    title: "Close (Esc)",
                    onclick: move |_| tools.close(),
                    "✕"
                }
            }
            if let Some(i) = section {
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
            }
        }
    }
}

/// A press outside the rail and its flyout closes the flyout (an armed
/// placing tool stays armed: that press places it).
fn use_outside_close(tools: Tools) {
    use_hook(move || {
        let Some(window) = web_sys::window() else {
            return;
        };
        let mut shown = tools.shown;
        let placing = tools.placing;
        let on_pointer = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(
            move |event: web_sys::PointerEvent| {
                if !*shown.peek() || placing.peek().is_some() {
                    return;
                }
                let inside = event
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .and_then(|el| el.closest(".pdit-rail, .pdit-fly").ok().flatten())
                    .is_some();
                if !inside {
                    shown.set(false);
                }
            },
        );
        let capture = web_sys::AddEventListenerOptions::new();
        capture.set_capture(true);
        let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
            "pointerdown",
            on_pointer.as_ref().unchecked_ref(),
            &capture,
        );
        // The app lives as long as the page.
        on_pointer.forget();
    });
}
