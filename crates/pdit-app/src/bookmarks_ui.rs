//! Bookmarks panel (D-043, approved mockup): right-click → Document →
//! Bookmarks opens the list where the Pages panel sits (the Pages panel steps
//! aside meanwhile). Click a bookmark to go to its page; rename (pencil or
//! double-click) or delete it; Add this page; From headings. Each change
//! writes the whole list (lopdf, D-045) with the page tools' Undo toast.

use crate::page_tools::PageTools;
use dioxus::prelude::*;
use pdit_core::bookmarks::{self, Bookmark};

pub const ICON_BOOKMARK: &str = include_str!("../assets/icons/cartoon-bookmark.svg");
const ICON_EDIT: &str = include_str!("../assets/icons/cartoon-pencil.svg");
const ICON_TRASH: &str = include_str!("../assets/icons/cartoon-trash.svg");

/// Whether the Pages panel is open; shared so the bookmarks list can take
/// its place.
#[derive(Clone, Copy)]
pub struct PagesPanel(pub Signal<bool>);

/// Shared bookmarks-panel state.
#[derive(Clone, Copy)]
pub struct BookmarksUi {
    open: Signal<bool>,
    /// The Pages panel's state before the list opened, restored on close.
    pages_before: Signal<bool>,
    /// The row being renamed and its draft title.
    renaming: Signal<Option<(usize, String)>>,
}

impl BookmarksUi {
    pub fn provide() -> Self {
        use_context_provider(|| BookmarksUi {
            open: Signal::new(false),
            pages_before: Signal::new(false),
            renaming: Signal::new(None),
        })
    }

    /// Right-click → Document → Bookmarks.
    pub fn open(mut self) {
        if *self.open.peek() {
            return;
        }
        let mut pages = consume_context::<PagesPanel>().0;
        self.pages_before.set(*pages.peek());
        pages.set(false);
        self.open.set(true);
    }

    fn close(mut self) {
        self.open.set(false);
        self.renaming.set(None);
        consume_context::<PagesPanel>()
            .0
            .set(*self.pages_before.peek());
    }

    /// Writes `list` as the document's bookmarks, with Undo.
    fn write(self, message: &str, list: Vec<Bookmark>) {
        consume_context::<PageTools>()
            .apply(message, ICON_BOOKMARK, || bookmarks::set_bookmarks(&list));
    }

    fn rename_done(mut self, list: &[Bookmark]) {
        let Some((i, draft)) = self.renaming.take() else {
            return;
        };
        let title = draft.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() || list.get(i).is_none_or(|b| b.title == title) {
            return;
        }
        let mut next = list.to_vec();
        next[i].title = title;
        self.write("Bookmark renamed", next);
    }

    fn delete(self, list: &[Bookmark], i: usize) {
        let mut next = list.to_vec();
        if i < next.len() {
            next.remove(i);
            self.write("Bookmark deleted", next);
        }
    }

    /// Bookmarks the page nearest the top of the window, named after its
    /// first heading (or first line), in page order; then renames it.
    fn add_this_page(mut self, list: &[Bookmark]) {
        let page = current_page();
        let headings = bookmarks::heading_bookmarks().unwrap_or_default();
        let (title, top) = headings
            .iter()
            .find(|b| b.page == Some(page))
            .map(|b| (b.title.clone(), b.top))
            .or_else(|| {
                pdit_core::text_lines(page)
                    .ok()
                    .and_then(|l| l.into_iter().next())
                    .map(|l| (l.text.trim().to_owned(), Some(l.bounds[3] + 8.0)))
            })
            .filter(|(t, _)| !t.is_empty())
            .unwrap_or_else(|| (format!("Page {}", page + 1), None));
        let at = list
            .iter()
            .position(|b| b.page.is_some_and(|p| p > page))
            .unwrap_or(list.len());
        let mut next = list.to_vec();
        next.insert(
            at,
            Bookmark {
                title: title.clone(),
                page: Some(page),
                top,
                level: 0,
            },
        );
        self.write("Bookmark added", next);
        self.renaming.set(Some((at, title)));
    }

    fn make_from_headings(self) {
        let found = bookmarks::heading_bookmarks().unwrap_or_default();
        if found.is_empty() {
            consume_context::<PageTools>().show(
                "No headings found (no text larger than the body text)".into(),
                ICON_BOOKMARK,
                None,
            );
            return;
        }
        let message = format!("{} bookmarks made from headings", found.len());
        self.write(&message, found);
    }
}

/// The page whose top is nearest the top of the window.
pub(crate) fn current_page() -> u16 {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return 0;
    };
    let Ok(pages) = document.query_selector_all(".page-list .page[data-page]") else {
        return 0;
    };
    let mut best = (0, f64::MAX);
    for i in 0..pages.length() {
        use wasm_bindgen::JsCast;
        let Some(el) = pages
            .item(i)
            .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        else {
            continue;
        };
        let rect = el.get_bounding_client_rect();
        // A page counts once it reaches the upper part of the window.
        let distance = if rect.bottom() < 80.0 {
            f64::MAX
        } else {
            rect.top().abs()
        };
        if distance < best.1 {
            let page = el
                .get_attribute("data-page")
                .and_then(|p| p.parse().ok())
                .unwrap_or(0);
            best = (page, distance);
        }
    }
    best.0
}

#[component]
pub fn BookmarksPanel() -> Element {
    let mut ui = use_context::<BookmarksUi>();
    // Re-read after every change: the page refresh updates the document signal.
    let document = use_context::<Signal<Option<crate::pages::OpenDocument>>>();
    let has_doc = document.read().is_some();
    let open = *ui.open.read() && has_doc;
    let list = if open {
        bookmarks::bookmarks().unwrap_or_default()
    } else {
        Vec::new()
    };
    let renaming = ui.renaming.read().clone();
    if !has_doc {
        return rsx! {};
    }
    rsx! {
        aside {
            class: "pdit-panel t-panel-slide pdit-bookmarks",
            "data-open": if open { "true" } else { "false" },
            "aria-label": "Bookmarks",
            div { class: "pdit-panel-head",
                span { class: "title", "Bookmarks" }
                span { class: "count", "{list.len()}" }
                span { class: "grow" }
                button {
                    class: "pdit-bm-btn",
                    r#type: "button",
                    onclick: move |_| ui.close(),
                    "Close"
                }
            }
            div { class: "pdit-bm-list",
                if list.is_empty() {
                    div { class: "pdit-bm-empty",
                        "No bookmarks yet. "
                        b { "Add this page" }
                        " bookmarks the page you’re on; "
                        b { "From headings" }
                        " makes them from the document’s headings."
                    }
                }
                for (i, bm) in list.iter().cloned().enumerate() {
                    {
                        let editing = renaming.as_ref().filter(|(r, _)| *r == i).map(|(_, d)| d.clone());
                        let list_rename = list.clone();
                        let list_blur = list.clone();
                        let list_delete = list.clone();
                        let depth = usize::from(bm.level.min(4));
                        rsx! {
                            div {
                                key: "{i}-{bm.title}",
                                class: "pdit-bm-row",
                                style: "padding-left: {8 + 18 * depth}px;",
                                tabindex: "0",
                                onclick: move |_| {
                                    if ui.renaming.peek().is_none()
                                        && let Some(p) = bm.page
                                    {
                                        crate::thumbnails::scroll_to_page(p);
                                    }
                                },
                                ondoubleclick: {
                                    let title = bm.title.clone();
                                    move |_| ui.renaming.set(Some((i, title.clone())))
                                },
                                span { dangerous_inner_html: ICON_BOOKMARK, style: "display: contents" }
                                if let Some(draft) = editing {
                                    input {
                                        class: "pdit-bm-input",
                                        r#type: "text",
                                        "aria-label": "Bookmark name",
                                        value: "{draft}",
                                        onmounted: move |event| {
                                            spawn(async move {
                                                let _ = event.data().set_focus(true).await;
                                            });
                                        },
                                        onclick: move |event| event.stop_propagation(),
                                        oninput: move |event| ui.renaming.set(Some((i, event.value()))),
                                        onkeydown: move |event| match event.key() {
                                            Key::Enter => ui.rename_done(&list_rename),
                                            Key::Escape => ui.renaming.set(None),
                                            _ => {}
                                        },
                                        onblur: move |_| ui.rename_done(&list_blur),
                                    }
                                } else {
                                    span { class: "t", title: "{bm.title}", "{bm.title}" }
                                    if let Some(p) = bm.page {
                                        span { class: "pg", "p. {p + 1}" }
                                    }
                                    button {
                                        class: "pdit-bm-act",
                                        r#type: "button",
                                        "aria-label": "Rename",
                                        onclick: {
                                            let title = bm.title.clone();
                                            move |event: Event<MouseData>| {
                                                event.stop_propagation();
                                                ui.renaming.set(Some((i, title.clone())));
                                            }
                                        },
                                        span { dangerous_inner_html: ICON_EDIT, style: "display: contents" }
                                    }
                                    button {
                                        class: "pdit-bm-act",
                                        r#type: "button",
                                        "aria-label": "Delete",
                                        onclick: move |event: Event<MouseData>| {
                                            event.stop_propagation();
                                            ui.delete(&list_delete, i);
                                        },
                                        span { dangerous_inner_html: ICON_TRASH, style: "display: contents" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "pdit-bm-foot",
                button {
                    class: "pdit-bm-btn",
                    r#type: "button",
                    onclick: {
                        let list = list.clone();
                        move |_| ui.add_this_page(&list)
                    },
                    "Add this page"
                }
                button {
                    class: "pdit-bm-btn",
                    r#type: "button",
                    onclick: move |_| ui.make_from_headings(),
                    "From headings"
                }
            }
        }
    }
}
