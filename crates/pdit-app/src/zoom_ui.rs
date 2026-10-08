//! Zoom (D-049): the bottom bar's slider sets it (frame_ui.rs, D-062);
//! ⌘/Ctrl + − 0, and ⌘/Ctrl + scroll or a trackpad pinch, zoom around the
//! pointer. Pages read the zoom for their
//! width (pages.rs); every overlay follows the measured page width, and pages
//! redraw sharply once the zoom settles.

use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;
use wasm_bindgen::JsCast;

pub const MIN: f32 = 25.0;
pub const MAX: f32 = 400.0;
const STEPS: [f32; 16] = [
    25.0, 33.0, 50.0, 67.0, 75.0, 80.0, 90.0, 100.0, 110.0, 125.0, 150.0, 175.0, 200.0, 250.0,
    300.0, 400.0,
];
/// Pages redraw this long after the last zoom change (a pinch sends many).
const SETTLE_MS: i32 = 150;

/// Shared zoom state.
#[derive(Clone, Copy)]
pub struct Zoom {
    /// Percent; 100 = a page at its print size (96 px per inch).
    pub percent: Signal<f32>,
    /// Bumped once the zoom stops changing: pages redraw at the new size.
    pub settled: Signal<u32>,
    /// Bumped on every change (pages redraw once it stops changing).
    changes: Signal<u32>,
}

impl Zoom {
    pub fn provide() -> Self {
        use_context_provider(|| Zoom {
            percent: Signal::new(100.0),
            settled: Signal::new(0),
            changes: Signal::new(0),
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
}

/// The keyboard and wheel zoom (the slider is in the bottom bar).
#[component]
pub fn ZoomKeys() -> Element {
    let zoom = use_context::<Zoom>();
    use_hook(move || listen(zoom));
    rsx! {}
}

/// ⌘/Ctrl + / − / 0, ⌘/Ctrl + wheel (and a trackpad pinch, which arrives as
/// ctrl + wheel). These listeners run outside Dioxus, so they only touch
/// signals and the window (no contexts).
fn listen(zoom: Zoom) {
    use wasm_bindgen::closure::Closure;
    let Some(window) = web_sys::window() else {
        return;
    };
    let on_key =
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
            if !(e.meta_key() || e.ctrl_key()) {
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
    let _ = window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    // Not passive, so the browser's own page zoom can be prevented.
    let options = web_sys::AddEventListenerOptions::new();
    options.set_passive(false);
    let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
        "wheel",
        on_wheel.as_ref().unchecked_ref(),
        &options,
    );
    // The app lives as long as the page.
    on_key.forget();
    on_wheel.forget();
}
