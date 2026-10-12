//! Crop (D-070; approved mockup .claude/research/page-tools-ui/): a page's
//! Crop button (its tools row) or the rail's Pages → Crop page… shows the whole
//! page with a frame (the image-selection ring and handles) to drag, and a
//! window in the Print look: Apply to This page | All pages, Remove white
//! margins, Whole page, Cancel / Done. Done sets the crop box (nothing is
//! deleted); one Undo. Look: assets/css/crop.css.

use crate::page_tools::{PageTools, next_frame, set_timeout};
use dioxus::prelude::*;
use std::rc::Rc;

const SIGNATURE_CSS: Asset = asset!("/assets/css/signature.css");
const DOC_CSS: Asset = asset!("/assets/css/doc.css");
const CROP_CSS: Asset = asset!("/assets/css/crop.css");
const IMAGE_SELECT_CSS: Asset = asset!("/assets/css/image-select.css");
/// Devigner Crop2, picked by the user.
pub const ICON_CROP: &str = include_str!("../assets/icons/devigner/Crop2.svg");
/// Transitions.dev "Modal open/close" close time (ms).
const CLOSE_MS: i32 = 200;
/// Room left around the content by "Remove white margins" (points).
const MARGIN_PT: f32 = 12.0;
/// The frame's smallest side (points).
const MIN_PT: f32 = 24.0;

#[derive(Clone, PartialEq)]
struct State {
    page: u16,
    /// The frame, [left, bottom, right, top] in PDF points.
    rect: [f32; 4],
    /// The whole page (media box).
    whole: [f32; 4],
    all: bool,
    /// The document before cropping started: Cancel and Undo put it back.
    before: Rc<Vec<u8>>,
    shown: bool,
    closing: bool,
}

/// A drag of the frame: which part (a handle, or "move"), and the last pointer position (CSS px).
#[derive(Clone, Copy, PartialEq)]
struct Drag {
    part: &'static str,
    x: f64,
    y: f64,
}

/// Shared crop state.
#[derive(Clone, Copy)]
pub struct Crop {
    state: Signal<Option<State>>,
    drag: Signal<Option<Drag>>,
}

impl Crop {
    pub fn provide() -> Self {
        use_context_provider(|| Crop {
            state: Signal::new(None),
            drag: Signal::new(None),
        })
    }

    /// Crop mode on `page`: the whole page shows, with the frame on what's visible now.
    pub fn start(mut self, page: u16) {
        if self.state.peek().as_ref().is_some_and(|s| !s.closing)
            || consume_context::<crate::pages::PageLocks>()
                .0
                .peek()
                .contains(&page)
        {
            return;
        }
        let tools = consume_context::<PageTools>();
        let Some(before) = tools.snapshot() else {
            return;
        };
        let Ok((visible, whole)) = pdit_core::page_ops::page_boxes(page) else {
            return;
        };
        if let Err(error) = pdit_core::page_ops::crop_page(page, whole) {
            return crate::log(&format!("pdit: crop: {error}"));
        }
        tools.refresh_pages();
        self.state.set(Some(State {
            page,
            rect: visible,
            whole,
            all: false,
            before,
            shown: false,
            closing: false,
        }));
        let mut state = self.state;
        next_frame(move || {
            next_frame(move || state.with_mut(|s| s.iter_mut().for_each(|s| s.shown = true)))
        });
        crate::doc_ui::place_pills_soon(".pdit-crop .t-tabs");
    }

    fn edit(mut self, f: impl FnOnce(&mut State)) {
        self.state.with_mut(|s| {
            if let Some(s) = s.as_mut() {
                f(s);
            }
        });
    }

    fn close(self) {
        self.edit(|s| s.closing = true);
        let mut state = self.state;
        set_timeout(CLOSE_MS, move || {
            if state.peek().as_ref().is_some_and(|s| s.closing) {
                state.set(None);
            }
        });
    }

    /// Cancel: the page as it was.
    fn cancel(self) {
        let Some(s) = self.state.peek().clone() else {
            return;
        };
        consume_context::<PageTools>().restore_snapshot(&s.before);
        self.close();
    }

    /// Done: the frame becomes the crop box of this page or every page.
    fn done(self) {
        let Some(s) = self.state.peek().clone() else {
            return;
        };
        let pages: Vec<u16> = if s.all {
            (0..pdit_core::page_ops::page_sizes().map_or(0, |p| p.len() as u16)).collect()
        } else {
            vec![s.page]
        };
        let message = match (pages.len(), s.rect == s.whole) {
            (1, true) => "Whole page shown".to_owned(),
            (1, false) => "Page cropped".to_owned(),
            (n, _) => format!("{n} pages cropped"),
        };
        let rect = s.rect;
        consume_context::<PageTools>().apply_since(
            s.before.clone(),
            &message,
            ICON_CROP,
            move || {
                pages
                    .iter()
                    .try_for_each(|&p| pdit_core::page_ops::crop_page(p, rect))
            },
        );
        self.close();
    }

    /// Remove white margins: the frame around what's drawn, with a little room.
    fn fit_content(self) {
        let Some(s) = self.state.peek().clone() else {
            return;
        };
        if let Ok(Some([l, b, r, t])) = pdit_core::page_ops::content_bounds(s.page) {
            let w = s.whole;
            self.edit(|s| {
                s.rect = [
                    (l - MARGIN_PT).max(w[0]),
                    (b - MARGIN_PT).max(w[1]),
                    (r + MARGIN_PT).min(w[2]),
                    (t + MARGIN_PT).min(w[3]),
                ]
            });
        }
    }

    /// Moves `part` of the frame by (dx right, dy up) points.
    fn drag_by(self, part: &str, dx: f32, dy: f32) {
        self.edit(|s| {
            let [mut l, mut b, mut r, mut t] = s.rect;
            let w = s.whole;
            if part == "move" {
                let (dx, dy) = (dx.clamp(w[0] - l, w[2] - r), dy.clamp(w[1] - b, w[3] - t));
                (l, r, b, t) = (l + dx, r + dx, b + dy, t + dy);
            } else {
                if part.contains('l') {
                    l = (l + dx).clamp(w[0], r - MIN_PT);
                }
                if part.contains('r') {
                    r = (r + dx).clamp(l + MIN_PT, w[2]);
                }
                if part.contains('b') {
                    b = (b + dy).clamp(w[1], t - MIN_PT);
                }
                if part.contains('t') {
                    t = (t + dy).clamp(b + MIN_PT, w[3]);
                }
            }
            s.rect = [l, b, r, t];
        });
    }
}

/// The frame on the page being cropped (inside the page's overlay layer, pages.rs).
#[component]
pub fn CropOverlay(page: u16, page_height_pt: f32, scale: f32) -> Element {
    let crop = use_context::<Crop>();
    let mut drag = crop.drag;
    let Some(s) = crop
        .state
        .read()
        .clone()
        .filter(|s| s.page == page && !s.closing)
    else {
        return rsx! {};
    };
    // The layer covers the whole page (shown whole while cropping); the frame is placed within it.
    let [wl, wb, wr, wt] = s.whole;
    let layer = format!(
        "left: {}px; top: {}px; width: {}px; height: {}px;",
        wl * scale,
        (page_height_pt - wt) * scale,
        (wr - wl) * scale,
        (wt - wb) * scale
    );
    let [l, b, r, t] = s.rect;
    let (x, y, w, h) = (
        (l - wl) * scale,
        (wt - t) * scale,
        (r - l) * scale,
        (t - b) * scale,
    );
    // Handles: the corners and the middle of each side.
    let handles: [(&'static str, f32, f32); 8] = [
        ("tl", 0.0, 0.0),
        ("t", 0.5, 0.0),
        ("tr", 1.0, 0.0),
        ("r", 1.0, 0.5),
        ("br", 1.0, 1.0),
        ("b", 0.5, 1.0),
        ("bl", 0.0, 1.0),
        ("l", 0.0, 0.5),
    ];
    let mut press = move |part: &'static str, event: Event<PointerData>| {
        event.stop_propagation();
        let p = event.client_coordinates();
        drag.set(Some(Drag {
            part,
            x: p.x,
            y: p.y,
        }));
        if let Some(e) = event.data().downcast::<web_sys::PointerEvent>()
            && let Some(el) = e
                .target()
                .and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(t).ok())
        {
            let _ = el.set_pointer_capture(e.pointer_id());
        }
    };
    rsx! {
        document::Stylesheet { href: IMAGE_SELECT_CSS }
        div {
            class: "pdit-crop-layer",
            style: "{layer}",
            // Clicks while cropping don't reach the page (no text selection).
            onclick: move |e| e.stop_propagation(),
            onpointermove: move |event| {
                let Some(d) = drag() else { return };
                let p = event.client_coordinates();
                crop.drag_by(d.part, ((p.x - d.x) / f64::from(scale)) as f32, -((p.y - d.y) / f64::from(scale)) as f32);
                drag.set(Some(Drag { x: p.x, y: p.y, ..d }));
            },
            onpointerup: move |_| drag.set(None),
            onpointercancel: move |_| drag.set(None),
            // What will be hidden is dimmed, clipped to the page; the frame and handles aren't clipped.
            div { class: "pdit-crop-clip",
                div {
                    class: "pdit-crop-dim",
                    style: "left: {x}px; top: {y}px; width: {w}px; height: {h}px;",
                }
            }
            div {
                class: "pdit-img-ring pdit-crop-frame",
                style: "left: {x}px; top: {y}px; width: {w}px; height: {h}px;",
                onpointerdown: move |e| press("move", e),
            }
            for (part, fx, fy) in handles {
                span {
                    key: "{part}",
                    class: "pdit-img-handle pdit-crop-handle {part}",
                    style: "left: {x + fx * w}px; top: {y + fy * h}px;",
                    onpointerdown: move |e| press(part, e),
                }
            }
        }
    }
}

/// The Crop window (docked at the bottom, as Print).
#[component]
pub fn CropUi() -> Element {
    let crop = use_context::<Crop>();
    let pill_key = crop.state.read().as_ref().map(|s| (s.shown, s.all));
    use_effect(use_reactive!(|pill_key| {
        if pill_key.is_some() {
            crate::doc_ui::place_pills_soon(".pdit-crop .t-tabs");
        }
    }));
    let Some(s) = crop.state.read().clone() else {
        return rsx! {
            document::Stylesheet { href: CROP_CSS }
        };
    };
    let class = match (s.shown, s.closing) {
        (_, true) => "doc-box t-modal is-closing",
        (true, false) => "doc-box t-modal is-open",
        (false, false) => "doc-box t-modal",
    };
    rsx! {
        document::Stylesheet { href: SIGNATURE_CSS }
        document::Stylesheet { href: DOC_CSS }
        document::Stylesheet { href: CROP_CSS }
        div { class: "doc-layer sa-root pdit-crop",
            div { class, role: "dialog", "aria-label": "Crop",
                div { class: "doc-head", span { class: "title", "Crop" } }
                div {
                    div { class: "doc-lbl", "Apply to" }
                    crate::doc_ui::Tabs {
                        options: vec![(format!("This page ({})", s.page + 1), !s.all), ("All pages".into(), s.all)],
                        on_pick: move |i| crop.edit(|s| s.all = i == 1),
                    }
                }
                div { class: "doc-row",
                    button { class: "sa-control", r#type: "button", onclick: move |_| crop.fit_content(), "Remove white margins" }
                    button { class: "sa-control", r#type: "button", onclick: move |_| crop.edit(|s| s.rect = s.whole), "Whole page" }
                }
                div { class: "crop-note",
                    "Drag the frame’s edges or corners. What’s outside is "
                    b { "hidden, not deleted" }
                    ": “Whole page” brings it back."
                }
                div { class: "doc-foot",
                    span { class: "grow" }
                    button { class: "sa-control", r#type: "button", onclick: move |_| crop.cancel(), "Cancel" }
                    button { class: "sa-primary", r#type: "button", onclick: move |_| crop.done(), "Done" }
                }
            }
        }
    }
}
