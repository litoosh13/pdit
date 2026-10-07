//! Links (D-043). The markup bar's Link opens the link box (the note box's
//! surface) for the chosen words; a link on the page shows an outline on
//! hover and, when clicked, a bar with its address + Open/Go · Edit · Remove.
//! Right-click → Document → Link web addresses links every written-out
//! address. The box's tabs choose a web address or a page of this file (page
//! links are written with lopdf, D-045). Every change goes through the page
//! tools' snapshot + Undo toast.

use crate::page_tools::{PageTools, next_frame, set_timeout};
use dioxus::prelude::*;
use pdit_core::links::{self, Link, LinkTarget};

pub const ICON_LINK: &str = include_str!("../assets/icons/cartoon-link.svg");
const ICON_OPEN: &str = include_str!("../assets/icons/arrow-up-right.svg");
const ICON_EDIT: &str = include_str!("../assets/icons/cartoon-pencil.svg");
const ICON_TRASH: &str = include_str!("../assets/icons/cartoon-trash.svg");
/// The link box's close duration (--dropdown-close-dur).
const CLOSE_MS: i32 = 150;
/// The link box's width, CSS px (it opens to the left near the page edge).
const BOX_PX: f32 = 280.0;
/// About half the link bar's width, CSS px.
const BAR_HALF_PX: f32 = 170.0;

/// The link box: a new link on `rects`, or the link at `index`.
#[derive(Clone, PartialEq)]
struct LinkBox {
    page: u16,
    rects: Vec<[f32; 4]>,
    index: Option<usize>,
    /// Where the link went before an edit (unchanged → nothing to do).
    original: Option<LinkTarget>,
    /// The Page tab is chosen.
    to_page: bool,
    draft: String,
    page_draft: String,
    error: Option<String>,
    shown: bool,
    closing: bool,
}

/// Shared link state.
#[derive(Clone, Copy)]
pub struct LinkUi {
    /// A link that was clicked.
    bar: Signal<Option<(u16, Link)>>,
    edit: Signal<Option<LinkBox>>,
}

impl LinkUi {
    pub fn provide() -> Self {
        use_context_provider(|| LinkUi {
            bar: Signal::new(None),
            edit: Signal::new(None),
        })
    }

    /// The markup bar's Link: a new link on `rects` (one box per line).
    pub fn link_words(self, page: u16, rects: Vec<[f32; 4]>) {
        self.open(LinkBox {
            page,
            rects,
            index: None,
            original: None,
            to_page: false,
            draft: String::new(),
            page_draft: "1".into(),
            error: None,
            shown: false,
            closing: false,
        });
    }

    fn edit_link(self, page: u16, link: &Link) {
        let (to_page, draft, page_draft) = match &link.target {
            LinkTarget::Page(p) => (true, String::new(), (p + 1).to_string()),
            LinkTarget::Web(url) => (false, shown_url(url), "1".into()),
            LinkTarget::Other => return,
        };
        self.open(LinkBox {
            page,
            rects: boxes_of(link),
            index: Some(link.index),
            original: Some(link.target.clone()),
            to_page,
            draft,
            page_draft,
            error: None,
            shown: false,
            closing: false,
        });
    }

    fn open(mut self, b: LinkBox) {
        self.bar.set(None);
        self.edit.set(Some(b));
        let mut signal = self.edit;
        next_frame(move || {
            next_frame(move || {
                signal.with_mut(|b| {
                    if let Some(b) = b.as_mut() {
                        b.shown = true;
                    }
                })
            })
        });
    }

    /// A press on a page or Escape: closes the bar and the box (unsaved).
    pub fn dismiss(mut self) {
        if self.bar.peek().is_some() {
            self.bar.set(None);
        }
        self.close_box();
    }

    fn close_box(mut self) {
        if !self.edit.peek().as_ref().is_some_and(|b| !b.closing) {
            return;
        }
        self.edit.with_mut(|b| {
            if let Some(b) = b.as_mut() {
                b.closing = true;
            }
        });
        let mut signal = self.edit;
        set_timeout(CLOSE_MS, move || {
            if signal.peek().as_ref().is_some_and(|b| b.closing) {
                signal.set(None);
            }
        });
    }

    /// Add link / Save: checks the address or page, then writes it. A web
    /// link's new address is set in place; any other change removes the link
    /// and adds the new one, under one Undo.
    fn save(mut self) {
        let Some(b) = self.edit.peek().clone() else {
            return;
        };
        let pages = page_count();
        let target = if b.to_page {
            match b.page_draft.trim().parse::<u16>() {
                Ok(n) if (1..=pages).contains(&n) => Some(LinkTarget::Page(n - 1)),
                _ => None,
            }
        } else {
            links::normalize_url(&b.draft).map(LinkTarget::Web)
        };
        let Some(target) = target else {
            let message = if b.to_page {
                format!("Pick a page from 1 to {pages}.")
            } else {
                "That doesn’t look like a web or e-mail address.".to_owned()
            };
            self.edit.with_mut(|e| {
                if let Some(e) = e.as_mut() {
                    e.error = Some(message);
                }
            });
            return;
        };
        self.close_box();
        if b.original.as_ref() == Some(&target) {
            return;
        }
        let (page, rects) = (b.page, b.rects);
        let add = move |target: &LinkTarget| match target {
            LinkTarget::Web(url) => links::add_web_link(page, &rects, url).map(|_| ()),
            LinkTarget::Page(p) => links::add_page_link(page, &rects, *p).map(|_| ()),
            LinkTarget::Other => Ok(()),
        };
        let tools = consume_context::<PageTools>();
        match (b.index, &b.original, &target) {
            (None, _, _) => {
                tools.apply("Link added", ICON_LINK, || add(&target));
            }
            (Some(index), Some(LinkTarget::Web(_)), LinkTarget::Web(url)) => {
                tools.apply("Link changed", ICON_LINK, || {
                    links::set_web_link(page, index, url)
                });
            }
            (Some(index), _, _) => {
                tools.apply("Link changed", ICON_LINK, || {
                    links::delete_link(page, index)?;
                    add(&target)
                });
            }
        }
    }

    /// Right-click → Open link / Go to linked page (D-050).
    pub(crate) fn follow(self, page: u16, index: usize) {
        match link_at(page, index).map(|l| l.target) {
            Some(LinkTarget::Web(url)) => open_in_new_tab(&url),
            Some(LinkTarget::Page(p)) => crate::thumbnails::scroll_to_page(p),
            _ => {}
        }
    }

    /// Right-click → Edit link….
    pub(crate) fn edit_at(self, page: u16, index: usize) {
        if let Some(link) = link_at(page, index) {
            self.edit_link(page, &link);
        }
    }

    pub(crate) fn remove(mut self, page: u16, index: usize) {
        self.bar.set(None);
        self.edit.set(None);
        consume_context::<PageTools>().apply("Link removed", ICON_LINK, || {
            links::delete_link(page, index)
        });
    }

    /// Right-click → Document → Link web addresses.
    pub fn link_web_addresses(self) {
        self.dismiss();
        let tools = consume_context::<PageTools>();
        let Some(before) = tools.snapshot() else {
            return;
        };
        match links::link_web_addresses() {
            Ok(0) => tools.show("No written-out web addresses found".into(), ICON_LINK, None),
            Ok(n) => {
                let message = if n == 1 {
                    "1 web address linked".to_owned()
                } else {
                    format!("{n} web addresses linked")
                };
                tools.apply_since(before, &message, ICON_LINK, || Ok(()));
            }
            Err(error) => {
                crate::log(&format!("pdit: link web addresses: {error}"));
                tools.restore_snapshot(&before);
            }
        }
    }
}

fn link_at(page: u16, index: usize) -> Option<Link> {
    links::links(page)
        .ok()?
        .into_iter()
        .find(|l| l.index == index)
}

fn page_count() -> u16 {
    pdit_core::page_ops::page_sizes()
        .map(|s| u16::try_from(s.len()).unwrap_or(u16::MAX))
        .unwrap_or(1)
}

fn boxes_of(link: &Link) -> Vec<[f32; 4]> {
    if link.rects.is_empty() {
        vec![link.bounds]
    } else {
        link.rects.clone()
    }
}

/// An address as a person reads it: no "https://" or "mailto:".
fn shown_url(url: &str) -> String {
    let lower = url.to_ascii_lowercase();
    for prefix in ["https://", "http://", "mailto:"] {
        if lower.starts_with(prefix) {
            return url[prefix.len()..].to_owned();
        }
    }
    url.to_owned()
}

fn open_in_new_tab(url: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.open_with_url_and_target_and_features(url, "_blank", "noopener,noreferrer");
    }
}

/// Per page: hover areas over links, the clicked link's bar and the link box.
/// `scale` is CSS px per PDF point.
#[component]
pub fn LinkOverlay(page: u16, page_width_pt: f32, page_height_pt: f32, scale: f32) -> Element {
    let mut ui = use_context::<LinkUi>();
    // Re-read when the page is redrawn after a change (pages remount then).
    let existing = links::links(page).unwrap_or_default();
    let px = move |[l, b, r, t]: [f32; 4]| {
        (
            l * scale,
            (page_height_pt - t) * scale,
            (r - l) * scale,
            (t - b) * scale,
        )
    };
    let bar = ui
        .bar
        .read()
        .clone()
        .filter(|(p, _)| *p == page)
        .map(|(_, l)| l);
    let edit = ui.edit.read().clone().filter(|b| b.page == page);
    // Keep the tab pill under the chosen tab (hooks run on every render).
    let pill_key = edit.as_ref().map(|b| (b.shown, b.to_page));
    use_effect(use_reactive!(|pill_key| {
        if pill_key.is_some() {
            crate::doc_ui::place_pills_soon(".pdit-link-box .t-tabs");
        }
    }));
    let below = move |boxes: &[[f32; 4]]| {
        let l = boxes.iter().map(|b| b[0]).fold(f32::MAX, f32::min);
        let r = boxes.iter().map(|b| b[2]).fold(f32::MIN, f32::max);
        let bottom = boxes.iter().map(|b| b[1]).fold(f32::MAX, f32::min);
        (
            l * scale,
            (l + r) / 2.0 * scale,
            (page_height_pt - bottom) * scale + 10.0,
        )
    };
    rsx! {
        for link in existing.iter().cloned() {
            for (i, (x, y, w, h)) in boxes_of(&link).into_iter().map(px).enumerate() {
                div {
                    key: "lk-{link.index}-{i}",
                    class: if bar.as_ref().is_some_and(|b| b.index == link.index) { "pdit-link-hit is-on" } else { "pdit-link-hit" },
                    "data-link": "{page}-{link.index}",
                    style: "left: {x}px; top: {y}px; width: {w}px; height: {h}px;",
                    onpointerdown: move |event| event.stop_propagation(),
                    onclick: {
                        let link = link.clone();
                        move |event: Event<MouseData>| {
                            event.stop_propagation();
                            consume_context::<crate::annotations_ui::Annotate>().dismiss();
                            ui.bar.set(Some((page, link.clone())));
                        }
                    },
                }
            }
        }
        if let Some(link) = bar {
            {
                let (left, cx, cy) = below(&boxes_of(&link));
                // Centred under the link, but kept inside the page near its
                // edges (the bar is about 2 × BAR_HALF_PX wide).
                let width = page_width_pt * scale;
                let place = if cx < BAR_HALF_PX {
                    format!("translate({}px, {cy}px)", left.max(8.0))
                } else if cx > width - BAR_HALF_PX {
                    format!("translate({}px, {cy}px) translateX(-100%)", width - 8.0)
                } else {
                    format!("translate({cx}px, {cy}px) translateX(-50%)")
                };
                let index = link.index;
                let (label, go) = match &link.target {
                    LinkTarget::Web(url) => (shown_url(url), "Open"),
                    LinkTarget::Page(p) => (format!("Page {}", p + 1), "Go"),
                    LinkTarget::Other => ("Other link".to_owned(), ""),
                };
                rsx! {
                    div {
                        class: "sa-root sa-anchor pdit-markup-anchor",
                        style: "transform: {place};",
                        onpointerdown: move |event| event.stop_propagation(),
                        onclick: move |event| event.stop_propagation(),
                        div { class: "sa-bar",
                            span { class: "pdit-link-url", title: "{label}", "{label}" }
                            if !go.is_empty() {
                                button {
                                    class: "sa-control",
                                    r#type: "button",
                                    onclick: {
                                        let target = link.target.clone();
                                        move |_| {
                                            ui.bar.set(None);
                                            match &target {
                                                LinkTarget::Web(url) => open_in_new_tab(url),
                                                LinkTarget::Page(p) => crate::thumbnails::scroll_to_page(*p),
                                                LinkTarget::Other => {}
                                            }
                                        }
                                    },
                                    span { dangerous_inner_html: ICON_OPEN, style: "display: contents" }
                                    "{go}"
                                }
                            }
                            if link.target != LinkTarget::Other {
                                button {
                                    class: "sa-control",
                                    r#type: "button",
                                    onclick: {
                                        let link = link.clone();
                                        move |_| ui.edit_link(page, &link)
                                    },
                                    span { dangerous_inner_html: ICON_EDIT, style: "display: contents" }
                                    "Edit"
                                }
                            }
                            button {
                                class: "sa-control",
                                r#type: "button",
                                onclick: move |_| ui.remove(page, index),
                                span { dangerous_inner_html: ICON_TRASH, style: "display: contents" }
                                "Remove"
                            }
                        }
                    }
                }
            }
        }
        if let Some(b) = edit {
            {
                let (left, _, top) = below(&b.rects);
                let flip = left + BOX_PX > page_width_pt * scale;
                let left = if flip { (page_width_pt * scale - BOX_PX - 8.0).max(8.0) } else { left };
                let class = match (b.shown, b.closing) {
                    (_, true) => "cm-menu t-dropdown pdit-link-box is-closing",
                    (true, false) => "cm-menu t-dropdown pdit-link-box is-open",
                    (false, false) => "cm-menu t-dropdown pdit-link-box",
                };
                let index = b.index;
                let pages = page_count();
                rsx! {
                    div {
                        class,
                        "data-origin": "top-left",
                        style: "left: {left}px; top: {top}px;",
                        onpointerdown: move |event| event.stop_propagation(),
                        onclick: move |event| event.stop_propagation(),
                        crate::doc_ui::Tabs {
                            options: vec![("Web address".into(), !b.to_page), ("Page".into(), b.to_page)],
                            on_pick: move |i: usize| {
                                ui.edit.with_mut(|e| {
                                    if let Some(e) = e.as_mut() {
                                        e.to_page = i == 1;
                                        e.error = None;
                                    }
                                })
                            },
                        }
                        if b.to_page {
                            div { class: "pdit-link-page",
                                span { "Go to page" }
                                input {
                                    class: "pdit-link-field pdit-link-number",
                                    r#type: "number",
                                    min: "1",
                                    max: "{pages}",
                                    "aria-label": "Page number",
                                    "aria-invalid": if b.error.is_some() { "true" } else { "false" },
                                    value: "{b.page_draft}",
                                    onmounted: move |event| {
                                        spawn(async move {
                                            let _ = event.data().set_focus(true).await;
                                        });
                                    },
                                    oninput: move |event| {
                                        ui.edit.with_mut(|e| {
                                            if let Some(e) = e.as_mut() {
                                                e.page_draft = event.value();
                                                e.error = None;
                                            }
                                        })
                                    },
                                    onkeydown: move |event| {
                                        if event.key() == Key::Enter {
                                            ui.save();
                                        }
                                    },
                                }
                                span { "of {pages}" }
                            }
                        } else {
                            input {
                                class: "pdit-link-field",
                                r#type: "text",
                                placeholder: "example.org or name@example.org",
                                "aria-label": "Web address",
                                "aria-invalid": if b.error.is_some() { "true" } else { "false" },
                                value: "{b.draft}",
                                onmounted: move |event| {
                                    spawn(async move {
                                        let _ = event.data().set_focus(true).await;
                                    });
                                },
                                oninput: move |event| {
                                    ui.edit.with_mut(|e| {
                                        if let Some(e) = e.as_mut() {
                                            e.draft = event.value();
                                            e.error = None;
                                        }
                                    })
                                },
                                onkeydown: move |event| {
                                    if event.key() == Key::Enter {
                                        ui.save();
                                    }
                                },
                            }
                        }
                        if let Some(message) = b.error.clone() {
                            div { class: "pdit-link-error", role: "alert", "{message}" }
                        }
                        div { class: "row sa-root",
                            if let Some(index) = index {
                                button {
                                    class: "sa-control",
                                    r#type: "button",
                                    onclick: move |_| ui.remove(page, index),
                                    span { dangerous_inner_html: ICON_TRASH, style: "display: contents" }
                                    "Remove"
                                }
                            }
                            span { class: "grow" }
                            button {
                                class: "sa-control",
                                r#type: "button",
                                onclick: move |_| ui.close_box(),
                                "Cancel"
                            }
                            button {
                                class: "sa-primary",
                                r#type: "button",
                                onclick: move |_| ui.save(),
                                if index.is_some() { "Save" } else { "Add link" }
                            }
                        }
                    }
                }
            }
        }
    }
}
