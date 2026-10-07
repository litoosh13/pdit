//! Zoom (D-049, approved mockup): a bottom-right pill — the Beautiful UI
//! Selection Actions bar — `[zoom-out] 100% [zoom-in]`; the percentage
//! animates with Transitions.dev "Number pop-in"; clicking it opens a menu
//! (Fit width, Fit page, presets). ⌘/Ctrl + − 0, and ⌘/Ctrl + scroll or a
//! trackpad pinch, zoom around the pointer. Pages read the zoom for their
//! width (pages.rs); every overlay follows the measured page width, and pages
//! redraw sharply once the zoom settles.

use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;
use wasm_bindgen::JsCast;

const ICON_IN: &str = include_str!("../assets/icons/cartoon-zoom-in.svg");
const ICON_OUT: &str = include_str!("../assets/icons/cartoon-zoom-out.svg");
const POP_IN_CSS: Asset = asset!("/assets/css/number-pop-in.css");
const CONTEXT_MENU_CSS: Asset = asset!("/assets/css/context-menu.css");

pub const MIN: f32 = 25.0;
pub const MAX: f32 = 400.0;
const STEPS: [f32; 16] = [
    25.0, 33.0, 50.0, 67.0, 75.0, 80.0, 90.0, 100.0, 110.0, 125.0, 150.0, 175.0, 200.0, 250.0,
    300.0, 400.0,
];
const PRESETS: [f32; 7] = [50.0, 75.0, 100.0, 125.0, 150.0, 200.0, 300.0];
/// Pages redraw this long after the last zoom change (a pinch sends many).
const SETTLE_MS: i32 = 150;
/// The page list's room above and below the pages (pages.css).
const LIST_TOP_PX: f64 = 96.0;
const LIST_BOTTOM_PX: f64 = 24.0;
/// CSS px per PDF point at 100 % (pages.rs).
const PX_PER_PT: f64 = 96.0 / 72.0;

/// Shared zoom state.
#[derive(Clone, Copy)]
pub struct Zoom {
    /// Percent; 100 = a page at its print size (96 px per inch).
    pub percent: Signal<f32>,
    /// Bumped once the zoom stops changing: pages redraw at the new size.
    pub settled: Signal<u32>,
    /// Bumped on every change, to replay the digit animation.
    changes: Signal<u32>,
    menu: Signal<bool>,
}

impl Zoom {
    pub fn provide() -> Self {
        use_context_provider(|| Zoom {
            percent: Signal::new(100.0),
            settled: Signal::new(0),
            changes: Signal::new(0),
            menu: Signal::new(false),
        })
    }

    /// Sets the zoom, keeping the window point (cx, cy) — default its middle —
    /// over the same spot of the document.
    pub fn set(mut self, percent: f32, anchor: Option<(f64, f64)>) {
        let percent = percent.clamp(MIN, MAX).round();
        let old = *self.percent.peek();
        if (percent - old).abs() < 0.5 {
            return;
        }
        let Some(window) = web_sys::window() else {
            return;
        };
        let (cx, cy) = anchor.unwrap_or_else(|| {
            let w = window
                .inner_width()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let h = window
                .inner_height()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            (w / 2.0, h / 2.0)
        });
        let (sx, sy) = (
            window.scroll_x().unwrap_or(0.0),
            window.scroll_y().unwrap_or(0.0),
        );
        let k = f64::from(percent / old);
        self.percent.set(percent);
        *self.changes.write() += 1;
        // After the pages took their new size, scroll so the anchor stays put.
        next_frame(move || {
            next_frame(move || {
                window.scroll_to_with_x_and_y((sx + cx) * k - cx, (sy + cy) * k - cy);
            })
        });
        let generation = *self.changes.peek();
        let (changes, mut settled) = (self.changes, self.settled);
        set_timeout(SETTLE_MS, move || {
            if *changes.peek() == generation {
                *settled.write() += 1;
            }
        });
    }

    /// The next step in (`up`) or out.
    pub fn step(self, up: bool) {
        let now = *self.percent.peek();
        let next = if up {
            STEPS
                .iter()
                .copied()
                .find(|s| *s > now + 0.5)
                .unwrap_or(MAX)
        } else {
            STEPS
                .iter()
                .rev()
                .copied()
                .find(|s| *s < now - 0.5)
                .unwrap_or(MIN)
        };
        self.set(next, None);
    }

    /// The widest page fills the page list's width (or the tallest page its height).
    fn fit(self, width: bool) {
        let Some(open) = consume_context::<Signal<Option<crate::pages::OpenDocument>>>()
            .peek()
            .clone()
        else {
            return;
        };
        let (w_pt, h_pt) = open
            .page_sizes
            .iter()
            .fold((0.0_f32, 0.0_f32), |(w, h), &(pw, ph)| {
                (w.max(pw), h.max(ph))
            });
        if w_pt <= 0.0 || h_pt <= 0.0 {
            return;
        }
        let Some(window) = web_sys::window() else {
            return;
        };
        let percent = if width {
            // The list's content box: the window minus its paddings (side panels add some).
            let Some(list) = window
                .document()
                .and_then(|d| d.query_selector(".page-list").ok().flatten())
            else {
                return;
            };
            let style = window.get_computed_style(&list).ok().flatten();
            let pad = |side: &str| {
                style
                    .as_ref()
                    .and_then(|s| s.get_property_value(side).ok())
                    .and_then(|v| v.trim_end_matches("px").parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            let inner = f64::from(list.client_width()) - pad("padding-left") - pad("padding-right");
            inner / (f64::from(w_pt) * PX_PER_PT) * 100.0
        } else {
            let h = window
                .inner_height()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            (h - LIST_TOP_PX - LIST_BOTTOM_PX) / (f64::from(h_pt) * PX_PER_PT) * 100.0
        };
        // ponytail: a fit beyond 400 % (or under 25 %) stops at the limit.
        self.set(percent as f32, None);
    }
}

/// The zoom pill (bottom right), its menu, and the keyboard / wheel zoom.
#[component]
pub fn ZoomBar() -> Element {
    let mut zoom = use_context::<Zoom>();
    let document = use_context::<Signal<Option<crate::pages::OpenDocument>>>();
    use_hook(move || listen(zoom));
    if document.read().is_none() {
        return rsx! {};
    }
    let percent = *zoom.percent.read();
    let changes = *zoom.changes.read();
    let menu = *zoom.menu.read();
    let text = format!("{percent}%");
    rsx! {
        document::Stylesheet { href: POP_IN_CSS }
        document::Stylesheet { href: CONTEXT_MENU_CSS }
        div { class: "pdit-zoom sa-root",
            div { class: "sa-bar",
                button {
                    class: "sa-control",
                    r#type: "button",
                    title: "Zoom out (⌘ −)",
                    "aria-label": "Zoom out",
                    disabled: percent <= MIN,
                    onclick: move |_| zoom.step(false),
                    span { dangerous_inner_html: ICON_OUT, style: "display: contents" }
                }
                button {
                    class: "sa-control pdit-zoom-pct",
                    r#type: "button",
                    title: "Zoom presets",
                    "aria-haspopup": "menu",
                    "aria-expanded": if menu { "true" } else { "false" },
                    onclick: move |_| zoom.menu.toggle(),
                    // Re-keyed on each change, so "Number pop-in" replays.
                    span {
                        key: "{changes}",
                        class: if changes > 0 { "t-digit-group is-animating" } else { "t-digit-group" },
                        for (i, c) in text.chars().enumerate() {
                            span {
                                key: "{i}",
                                class: "t-digit",
                                "data-stagger": if i > 0 { "{i.min(2)}" } else { "" },
                                "{c}"
                            }
                        }
                    }
                }
                button {
                    class: "sa-control",
                    r#type: "button",
                    title: "Zoom in (⌘ +)",
                    "aria-label": "Zoom in",
                    disabled: percent >= MAX,
                    onclick: move |_| zoom.step(true),
                    span { dangerous_inner_html: ICON_IN, style: "display: contents" }
                }
            }
        }
        if menu {
            ZoomMenu {}
        }
    }
}

/// The presets: the right-click menu's surface, opening upward.
#[component]
fn ZoomMenu() -> Element {
    let mut zoom = use_context::<Zoom>();
    let mut shown = use_signal(|| false);
    use_hook(move || next_frame(move || next_frame(move || shown.set(true))));
    let percent = *zoom.percent.read();
    let mut pick = move |f: &dyn Fn(Zoom)| {
        zoom.menu.set(false);
        f(zoom);
    };
    rsx! {
        div {
            class: if shown() { "cm-menu t-dropdown pdit-zoom-menu is-open" } else { "cm-menu t-dropdown pdit-zoom-menu" },
            "data-origin": "bottom-left",
            role: "menu",
            button { r#type: "button", role: "menuitem", onclick: move |_| pick(&|z| z.fit(true)), span { "Fit width" } }
            button { r#type: "button", role: "menuitem", onclick: move |_| pick(&|z| z.fit(false)), span { "Fit page" } }
            div { class: "cm-label", "Zoom" }
            for p in PRESETS {
                button {
                    key: "{p}",
                    r#type: "button",
                    role: "menuitem",
                    onclick: move |_| pick(&|z| z.set(p, None)),
                    span { "{p}%" }
                    if (p - percent).abs() < 0.5 {
                        span { class: "pdit-zoom-tick", "✓" }
                    }
                }
            }
        }
    }
}

/// ⌘/Ctrl + / − / 0, ⌘/Ctrl + wheel (and a trackpad pinch, which arrives as
/// ctrl + wheel), and a press outside the menu closing it. These listeners run
/// outside Dioxus, so they only touch signals and the window (no contexts).
fn listen(mut zoom: Zoom) {
    use wasm_bindgen::closure::Closure;
    let Some(window) = web_sys::window() else {
        return;
    };
    let on_key =
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
            if !(e.meta_key() || e.ctrl_key()) {
                if e.key() == "Escape" && *zoom.menu.peek() {
                    zoom.menu.set(false);
                }
                return;
            }
            match e.key().as_str() {
                "+" | "=" => {
                    e.prevent_default();
                    zoom.step(true);
                }
                "-" => {
                    e.prevent_default();
                    zoom.step(false);
                }
                "0" => {
                    e.prevent_default();
                    zoom.set(100.0, None);
                }
                _ => {}
            }
        });
    let on_wheel = Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |e: web_sys::WheelEvent| {
        if !(e.ctrl_key() || e.meta_key()) {
            return;
        }
        e.prevent_default();
        let now = *zoom.percent.peek();
        let next = now * (-e.delta_y() * 0.01).exp() as f32;
        zoom.set(
            next,
            Some((f64::from(e.client_x()), f64::from(e.client_y()))),
        );
    });
    let on_press =
        Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |e: web_sys::PointerEvent| {
            let inside = e
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .and_then(|el| el.closest(".pdit-zoom-menu, .pdit-zoom-pct").ok().flatten())
                .is_some();
            if !inside && *zoom.menu.peek() {
                zoom.menu.set(false);
            }
        });
    let _ = window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    // Not passive, so the browser's own page zoom can be prevented.
    let options = web_sys::AddEventListenerOptions::new();
    options.set_passive(false);
    let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
        "wheel",
        on_wheel.as_ref().unchecked_ref(),
        &options,
    );
    let _ =
        window.add_event_listener_with_callback("pointerdown", on_press.as_ref().unchecked_ref());
    // The app lives as long as the page.
    on_key.forget();
    on_wheel.forget();
    on_press.forget();
}
