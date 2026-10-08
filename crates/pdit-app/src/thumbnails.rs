//! The page thumbnails (D-029): the open PDF's pages as Beautiful UI mini
//! cards, each rendered lazily as it scrolls into view. Since D-062 they sit in
//! the tools rail's Page section (docked panel), opened from the rail or the
//! bottom bar's page count. Cards can be dragged to reorder the document
//! (page_ops::move_page, with the Undo toast).

use crate::page_tools::PageTools;
use crate::pages::OpenDocument;
use dioxus::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const THUMBS_CSS: Asset = asset!("/assets/css/thumbnails.css");

/// CSS pixels the thumbnail is drawn at (before the device pixel ratio): the
/// card's width in the docked panel (thumbnails.css `.pdit-thumbs-wrap`).
const THUMB_WIDTH_CSS: f64 = 200.0;

#[component]
pub fn PageThumbs() -> Element {
    let document = use_context::<Signal<Option<OpenDocument>>>();
    let tools = use_context::<PageTools>();
    let doc = document();
    let count = doc.as_ref().map_or(0, |d| d.page_sizes.len());
    let doc_id = doc.as_ref().map_or(0, |d| d.id);

    // The display order (page indices). Reset whenever the document or its page
    // count changes (e.g. after a reorder is applied and the pages reload).
    let mut order = use_signal(Vec::<u16>::new);
    use_effect(use_reactive!(|(doc_id, count)| {
        let _ = doc_id;
        order.set((0..count as u16).collect());
    }));
    // The page index currently being dragged, if any.
    let mut drag = use_signal(|| None::<u16>);

    let display: Vec<u16> = if order.read().len() == count && count > 0 {
        order()
    } else {
        (0..count as u16).collect()
    };

    // Applies the current visual order to the document when a drag ends.
    let mut apply = move || {
        if let Some(dragged) = drag() {
            let final_pos = order.read().iter().position(|&p| p == dragged);
            drag.set(None);
            match final_pos {
                // Released where it was grabbed: a click, so show that page.
                Some(pos) if pos == dragged as usize => scroll_to_page(dragged),
                Some(pos) => tools.move_page(dragged, pos as u16),
                None => {}
            }
        }
    };

    rsx! {
        document::Stylesheet { href: THUMBS_CSS }
        if let Some(doc) = doc {
            div {
                class: "pdit-thumbs-wrap",
                // A drag that ends outside the cards (or leaves the list) still drops.
                onpointerup: move |_| apply(),
                onpointerleave: move |_| apply(),
                div {
                    class: "pdit-thumbs",
                    // While dragging, reorder the view as the pointer passes cards.
                    onpointermove: move |event| {
                        if drag().is_none() {
                            return;
                        }
                        reorder_to_pointer(order, drag, event);
                    },
                    onpointerup: move |_| apply(),
                    for page in display {
                        Thumb {
                            key: "{doc.id}-{page}",
                            index: page,
                            width_pt: doc.page_sizes[page as usize].0,
                            height_pt: doc.page_sizes[page as usize].1,
                            dragging: drag() == Some(page),
                            on_grab: move |_| drag.set(Some(page)),
                        }
                    }
                }
            }
        }
    }
}

/// Scrolls the page list so page `index` is in view (thumbnail click).
pub(crate) fn scroll_to_page(index: u16) {
    let page = web_sys::window().and_then(|w| w.document()).and_then(|d| {
        d.query_selector(&format!(".page-list .page[data-page='{index}']"))
            .ok()
            .flatten()
    });
    if let Some(page) = page {
        page.scroll_into_view();
    }
}

/// Moves the dragged page to the pointer's position in the display order.
fn reorder_to_pointer(
    mut order: Signal<Vec<u16>>,
    drag: Signal<Option<u16>>,
    event: Event<PointerData>,
) {
    let Some(dragged) = drag() else { return };
    let point = event.client_coordinates();
    let (x, y) = (point.x, point.y);
    let Some(card) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.element_from_point(x as f32, y as f32))
        .and_then(|el| el.closest(".pdit-thumb").ok().flatten())
    else {
        return;
    };
    let Some(over) = card
        .get_attribute("data-page")
        .and_then(|s| s.parse::<u16>().ok())
    else {
        return;
    };
    if over == dragged {
        return;
    }
    let rect = card.get_bounding_client_rect();
    let below = y > rect.top() + rect.height() / 2.0;
    order.with_mut(|ord| {
        let Some(cur) = ord.iter().position(|&p| p == dragged) else {
            return;
        };
        let Some(target) = ord.iter().position(|&p| p == over) else {
            return;
        };
        let mut insert = if below { target + 1 } else { target };
        ord.remove(cur);
        if cur < insert {
            insert -= 1;
        }
        ord.insert(insert.min(ord.len()), dragged);
    });
}

/// Keeps a card's IntersectionObserver alive; disconnects it on drop.
#[derive(Default)]
struct Watcher(Option<(web_sys::IntersectionObserver, ObserverCallback)>);

type ObserverCallback = Closure<dyn FnMut(js_sys::Array)>;

impl Drop for Watcher {
    fn drop(&mut self) {
        if let Some((observer, _)) = self.0.take() {
            observer.disconnect();
        }
    }
}

#[component]
fn Thumb(
    index: u16,
    width_pt: f32,
    height_pt: f32,
    dragging: bool,
    on_grab: EventHandler<()>,
) -> Element {
    let revealed = use_signal(|| false);
    let watcher = use_hook(|| Rc::new(RefCell::new(Watcher::default())));

    rsx! {
        div {
            class: if dragging { "pdit-thumb is-dragging" } else { "pdit-thumb" },
            "data-page": "{index}",
            onpointerdown: move |event| {
                if event.trigger_button() == Some(dioxus::html::input_data::MouseButton::Primary) {
                    event.prevent_default();
                    on_grab.call(());
                }
            },
            div {
                class: "page",
                style: "aspect-ratio: {width_pt} / {height_pt};",
                onmounted: move |event| {
                    let Some(slot) = event.data().downcast::<web_sys::Element>().cloned() else {
                        return;
                    };
                    if let Err(error) = watch(&slot, index, revealed, &watcher) {
                        crate::log(&format!("pdit: could not watch thumbnail {}: {error:?}", index + 1));
                    }
                },
                canvas { "aria-label": "Page {index + 1} thumbnail" }
            }
            div { class: "bar",
                span { class: "num", "{index + 1}" }
                if let Some(label) = size_label(width_pt, height_pt) {
                    span { class: "meta", "{label}" }
                }
            }
        }
    }
}

/// Observes the card; the first time it comes near the panel's view, draws the
/// thumbnail and stops watching.
fn watch(
    slot: &web_sys::Element,
    index: u16,
    mut revealed: Signal<bool>,
    watcher: &Rc<RefCell<Watcher>>,
) -> Result<(), JsValue> {
    let target = slot.clone();
    let done = Rc::downgrade(watcher);
    let callback = ObserverCallback::new(move |entries: js_sys::Array| {
        let near = entries.iter().any(|entry| {
            entry
                .unchecked_into::<web_sys::IntersectionObserverEntry>()
                .is_intersecting()
        });
        if !near || revealed() {
            return;
        }
        let canvas = target
            .query_selector("canvas")
            .ok()
            .flatten()
            .and_then(|c| c.dyn_into::<web_sys::HtmlCanvasElement>().ok());
        let Some(canvas) = canvas else { return };
        match draw(&canvas, index) {
            Ok(()) => revealed.set(true),
            Err(error) => {
                crate::log(&format!(
                    "pdit: could not render thumbnail {}: {error}",
                    index + 1
                ));
            }
        }
        if let Some(watcher) = done.upgrade()
            && let Some((observer, _)) = watcher.borrow().0.as_ref()
        {
            observer.disconnect();
        }
    });
    // The panel scroll container is the root, so cards render as they scroll in.
    let root = slot.closest(".pdit-thumbs").ok().flatten();
    let options = web_sys::IntersectionObserverInit::new();
    if let Some(root) = root.as_ref() {
        options.set_root(Some(root));
    }
    options.set_root_margin("100% 0px");
    let observer = web_sys::IntersectionObserver::new_with_options(
        callback.as_ref().unchecked_ref(),
        &options,
    )?;
    observer.observe(slot);
    watcher.borrow_mut().0 = Some((observer, callback));
    Ok(())
}

/// Draws the page into the thumbnail canvas at the thumbnail width times the
/// device pixel ratio.
fn draw(canvas: &web_sys::HtmlCanvasElement, index: u16) -> Result<(), String> {
    let ratio = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0);
    let width_px = (THUMB_WIDTH_CSS * ratio).round().max(1.0) as u32;
    let image = pdit_core::render_page(index, width_px).map_err(|e| e.to_string())?;
    canvas.set_width(image.width());
    canvas.set_height(image.height());
    let context = canvas
        .get_context("2d")
        .map_err(|e| format!("{e:?}"))?
        .ok_or("no 2d context")?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .map_err(|_| "not a 2d context")?;
    context
        .put_image_data(&image, 0.0, 0.0)
        .map_err(|e| format!("{e:?}"))
}

/// A short label for common page sizes (points, either orientation).
fn size_label(width_pt: f32, height_pt: f32) -> Option<&'static str> {
    let (short, long) = if width_pt <= height_pt {
        (width_pt, height_pt)
    } else {
        (height_pt, width_pt)
    };
    let near = |a: f32, b: f32| (short - a).abs() <= 6.0 && (long - b).abs() <= 6.0;
    if near(595.0, 842.0) {
        Some("A4")
    } else if near(612.0, 792.0) {
        Some("Letter")
    } else if near(612.0, 1008.0) {
        Some("Legal")
    } else if near(842.0, 1191.0) {
        Some("A3")
    } else if near(420.0, 595.0) {
        Some("A5")
    } else {
        None
    }
}
