//! Add signature (D-039): right-click → Image → Add signature opens a window
//! (Transitions.dev "Modal open/close") with Saved | Draw | Upload tabs
//! (Transitions.dev "Tabs sliding"). Place adds the signature where the menu
//! was opened, as a normal image (image options apply), with Undo. The last
//! placed signature is remembered in this browser only (localStorage), never
//! uploaded.

use crate::page_tools::{PageTools, next_frame, set_timeout};
use dioxus::prelude::*;
use wasm_bindgen::JsCast;

const SIGNATURE_CSS: Asset = asset!("/assets/css/signature.css");
/// Where the last placed signature is kept, as a `data:` URL.
const STORAGE_KEY: &str = "pdit-signature";
/// The modal's close duration (--modal-close-dur).
const CLOSE_MS: i32 = 150;
const INKS: [(&str, &str); 2] = [("Black", "#1f2124"), ("Blue", "#1d4fd8")];
/// Stroke width on the pad, CSS px.
const STROKE_PX: f64 = 2.2;
/// The exported PNG is drawn at this many pixels per pad pixel.
const EXPORT_SCALE: f64 = 3.0;
const UPLOAD_INPUT_ID: &str = "pdit-signature-upload";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Saved,
    Draw,
    Upload,
}

#[derive(Clone, PartialEq)]
struct Stroke {
    ink: &'static str,
    points: Vec<(f64, f64)>,
}

/// Shared signature-window state.
#[derive(Clone, Copy)]
pub struct Signature {
    /// Where Place puts the signature: (page, x_pt, top_pt). `Some` = open.
    at: Signal<Option<(u16, f32, f32)>>,
    /// The `.is-open` class is on (one frame after mounting, for the motion).
    shown: Signal<bool>,
    closing: Signal<bool>,
    mode: Signal<Mode>,
    ink: Signal<&'static str>,
    strokes: Signal<Vec<Stroke>>,
    /// The uploaded image, as a `data:` URL.
    uploaded: Signal<Option<String>>,
    /// The remembered signature, as a `data:` URL.
    saved: Signal<Option<String>>,
}

impl Signature {
    pub fn provide() -> Self {
        use_context_provider(|| Signature {
            at: Signal::new(None),
            shown: Signal::new(false),
            closing: Signal::new(false),
            mode: Signal::new(Mode::Draw),
            ink: Signal::new(INKS[0].1),
            strokes: Signal::new(Vec::new()),
            uploaded: Signal::new(None),
            saved: Signal::new(None),
        })
    }

    /// Opens the window; Place will put the signature at (x, top) on `page`.
    pub fn open_at(mut self, page: u16, x_pt: f32, top_pt: f32) {
        let saved = storage().and_then(|s| s.get_item(STORAGE_KEY).ok().flatten());
        self.mode.set(if saved.is_some() {
            Mode::Saved
        } else {
            Mode::Draw
        });
        self.saved.set(saved);
        self.strokes.set(Vec::new());
        self.uploaded.set(None);
        self.closing.set(false);
        self.shown.set(false);
        self.at.set(Some((page, x_pt, top_pt)));
        let mut shown = self.shown;
        next_frame(move || next_frame(move || shown.set(true)));
    }

    fn close(mut self) {
        if self.at.peek().is_none() || *self.closing.peek() {
            return;
        }
        self.closing.set(true);
        let mut at = self.at;
        set_timeout(CLOSE_MS, move || at.set(None));
    }

    /// The signature to place, as image bytes (PNG, or the uploaded JPEG).
    fn image(&self) -> Option<String> {
        match *self.mode.peek() {
            Mode::Saved => self.saved.peek().clone(),
            Mode::Upload => self.uploaded.peek().clone(),
            Mode::Draw => drawn_png(&self.strokes.peek()),
        }
    }

    fn place(self) {
        let (Some((page, x, top)), Some(url)) = (*self.at.peek(), self.image()) else {
            return;
        };
        let Some(bytes) = bytes_of(&url) else {
            return crate::log("pdit: could not read the signature image");
        };
        if let Some(storage) = storage()
            && storage.set_item(STORAGE_KEY, &url).is_err()
        {
            crate::log("pdit: the signature is too large to remember on this device");
        }
        self.close();
        consume_context::<PageTools>().add_signature(page, x, top, bytes);
    }
}

/// The signature window. Rendered once at the app root.
#[component]
pub fn SignatureUi() -> Element {
    let mut sig = use_context::<Signature>();
    // Escape closes the window.
    use_hook(move || {
        let on_key = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if event.key() == "Escape" {
                    sig.close();
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
    // The tab pill follows the selected tab (Tabs sliding wire-up).
    let mode = *sig.mode.read();
    let open = sig.at.read().is_some();
    use_effect(use_reactive!(|(mode, open)| {
        let _ = mode;
        if open {
            next_frame(place_pill);
        }
    }));

    let modal_class = match (*sig.shown.read(), *sig.closing.read()) {
        (_, true) => "sig-box t-modal is-closing",
        (true, false) => "sig-box t-modal is-open",
        (false, false) => "sig-box t-modal",
    };
    let has_saved = sig.saved.read().is_some();
    let can_place = match mode {
        Mode::Saved => has_saved,
        Mode::Draw => !sig.strokes.read().is_empty(),
        Mode::Upload => sig.uploaded.read().is_some(),
    };
    let mut tabs = Vec::new();
    if has_saved {
        tabs.push((Mode::Saved, "Saved"));
    }
    tabs.push((Mode::Draw, "Draw"));
    tabs.push((Mode::Upload, "Upload"));

    rsx! {
        document::Stylesheet { href: SIGNATURE_CSS }
        input {
            id: UPLOAD_INPUT_ID,
            r#type: "file",
            accept: "image/png,image/jpeg,.png,.jpg,.jpeg",
            hidden: true,
            onchange: move |event| async move {
                let Some(file) = event.files().into_iter().next() else { return };
                match file.read_bytes().await {
                    Ok(bytes) => sig.uploaded.set(Some(data_url(&bytes))),
                    Err(error) => crate::log(&format!("pdit: could not read the image: {error}")),
                }
            },
        }
        if open {
            div { class: "sa-root sig-root",
                div { class: "sig-layer",
                    div { class: "sig-backdrop", onclick: move |_| sig.close() }
                    div {
                        class: modal_class,
                        role: "dialog",
                        "aria-modal": "true",
                        "aria-label": "Add signature",
                        div { class: "sig-head",
                            span { class: "title", "Add signature" }
                            div { class: "t-tabs", role: "tablist",
                                span { class: "t-tabs-pill", "aria-hidden": "true" }
                                for (tab, label) in tabs {
                                    button {
                                        class: "t-tab",
                                        r#type: "button",
                                        role: "tab",
                                        "aria-selected": if tab == mode { "true" } else { "false" },
                                        onclick: move |_| sig.mode.set(tab),
                                        "{label}"
                                    }
                                }
                            }
                        }
                        match mode {
                            Mode::Saved => rsx! {
                                div { class: "sig-pad sig-image",
                                    if let Some(url) = sig.saved.read().clone() {
                                        img { src: "{url}", alt: "Saved signature" }
                                    }
                                }
                            },
                            Mode::Draw => rsx! { Pad {} },
                            Mode::Upload => rsx! {
                                div { class: "sig-pad sig-image",
                                    if let Some(url) = sig.uploaded.read().clone() {
                                        img { src: "{url}", alt: "Uploaded signature" }
                                    } else {
                                        div { class: "empty", "PNG or JPEG of your signature" }
                                    }
                                }
                            },
                        }
                        div { class: "sig-foot",
                            match mode {
                                Mode::Saved => rsx! {
                                    button {
                                        class: "sa-control",
                                        r#type: "button",
                                        onclick: move |_| {
                                            if let Some(storage) = storage() {
                                                let _ = storage.remove_item(STORAGE_KEY);
                                            }
                                            sig.saved.set(None);
                                            sig.mode.set(Mode::Draw);
                                        },
                                        "Forget"
                                    }
                                },
                                Mode::Draw => rsx! {
                                    span { class: "sig-ink",
                                        "Ink"
                                        for (name, color) in INKS {
                                            button {
                                                r#type: "button",
                                                "aria-label": name,
                                                "aria-pressed": if *sig.ink.read() == color { "true" } else { "false" },
                                                style: "background: {color};",
                                                onclick: move |_| {
                                                    sig.ink.set(color);
                                                    sig.strokes.with_mut(|s| s.iter_mut().for_each(|st| st.ink = color));
                                                },
                                            }
                                        }
                                    }
                                    button {
                                        class: "sa-control",
                                        r#type: "button",
                                        onclick: move |_| sig.strokes.set(Vec::new()),
                                        "Clear"
                                    }
                                },
                                Mode::Upload => rsx! {
                                    button {
                                        class: "sa-control",
                                        r#type: "button",
                                        onclick: move |_| click_upload(),
                                        "Choose image…"
                                    }
                                },
                            }
                            span { class: "grow" }
                            button {
                                class: "sa-control",
                                r#type: "button",
                                onclick: move |_| sig.close(),
                                "Cancel"
                            }
                            button {
                                class: "sa-primary",
                                r#type: "button",
                                disabled: !can_place,
                                onclick: move |_| sig.place(),
                                "Place"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The drawing pad: white paper in both themes, smoothed strokes.
#[component]
fn Pad() -> Element {
    let mut sig = use_context::<Signature>();
    let mut canvas = use_signal(|| None::<web_sys::HtmlCanvasElement>);
    let mut drawing = use_signal(|| false);
    let empty = sig.strokes.read().is_empty();
    // Repaint whenever the strokes (or their ink) change.
    use_effect(move || {
        let strokes = sig.strokes.read().clone();
        if let Some(c) = canvas.read().as_ref() {
            paint_pad(c, &strokes);
        }
    });
    rsx! {
        div {
            class: "sig-pad",
            onpointerdown: move |event| {
                let p = event.element_coordinates();
                if let Some(pe) = event.data().downcast::<web_sys::PointerEvent>()
                    && let Some(target) = pe.target()
                    && let Ok(element) = target.dyn_into::<web_sys::Element>()
                {
                    let _ = element.set_pointer_capture(pe.pointer_id());
                }
                drawing.set(true);
                let ink = *sig.ink.peek();
                sig.strokes.with_mut(|s| s.push(Stroke { ink, points: vec![(p.x, p.y)] }));
            },
            onpointermove: move |event| {
                if !*drawing.peek() {
                    return;
                }
                let p = event.element_coordinates();
                sig.strokes.with_mut(|s| {
                    if let Some(last) = s.last_mut() {
                        last.points.push((p.x, p.y));
                    }
                });
            },
            onpointerup: move |_| drawing.set(false),
            canvas {
                onmounted: move |event| {
                    let Some(c) = event
                        .data()
                        .downcast::<web_sys::Element>()
                        .cloned()
                        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
                    else {
                        return;
                    };
                    let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio());
                    let rect = c.get_bounding_client_rect();
                    c.set_width((rect.width() * dpr) as u32);
                    c.set_height((rect.height() * dpr) as u32);
                    canvas.set(Some(c));
                },
            }
            if empty {
                div { class: "empty", "Sign here" }
            }
        }
    }
}

fn paint_pad(canvas: &web_sys::HtmlCanvasElement, strokes: &[Stroke]) {
    let Some(ctx) = context(canvas) else { return };
    ctx.clear_rect(
        0.0,
        0.0,
        f64::from(canvas.width()),
        f64::from(canvas.height()),
    );
    let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio());
    paint(&ctx, strokes, dpr, 0.0, 0.0);
}

/// Draws `strokes` scaled by `scale`, shifted by (-dx, -dy) pad px.
fn paint(
    ctx: &web_sys::CanvasRenderingContext2d,
    strokes: &[Stroke],
    scale: f64,
    dx: f64,
    dy: f64,
) {
    ctx.set_line_cap("round");
    ctx.set_line_join("round");
    ctx.set_line_width(STROKE_PX * scale);
    for stroke in strokes {
        let p: Vec<(f64, f64)> = stroke
            .points
            .iter()
            .map(|&(x, y)| ((x - dx) * scale, (y - dy) * scale))
            .collect();
        ctx.set_stroke_style_str(stroke.ink);
        ctx.begin_path();
        ctx.move_to(p[0].0, p[0].1);
        if p.len() == 1 {
            ctx.line_to(p[0].0 + 0.1, p[0].1);
        }
        // Quadratic curves through the midpoints smooth the pointer samples.
        for i in 1..p.len().saturating_sub(1) {
            let (mx, my) = ((p[i].0 + p[i + 1].0) / 2.0, (p[i].1 + p[i + 1].1) / 2.0);
            ctx.quadratic_curve_to(p[i].0, p[i].1, mx, my);
        }
        if p.len() > 1 {
            let last = p[p.len() - 1];
            ctx.line_to(last.0, last.1);
        }
        ctx.stroke();
    }
}

/// The drawn signature as a transparent PNG `data:` URL, trimmed to the ink.
fn drawn_png(strokes: &[Stroke]) -> Option<String> {
    let points = strokes.iter().flat_map(|s| s.points.iter());
    let (mut l, mut t, mut r, mut b) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in points {
        (l, t, r, b) = (l.min(x), t.min(y), r.max(x), b.max(y));
    }
    if l > r {
        return None;
    }
    let pad = STROKE_PX * 2.0;
    let (l, t) = (l - pad, t - pad);
    let (w, h) = (r + pad - l, b + pad - t);
    let canvas = web_sys::window()?
        .document()?
        .create_element("canvas")
        .ok()?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()?;
    canvas.set_width((w * EXPORT_SCALE).ceil() as u32);
    canvas.set_height((h * EXPORT_SCALE).ceil() as u32);
    paint(&context(&canvas)?, strokes, EXPORT_SCALE, l, t);
    canvas.to_data_url_with_type("image/png").ok()
}

fn context(canvas: &web_sys::HtmlCanvasElement) -> Option<web_sys::CanvasRenderingContext2d> {
    canvas.get_context("2d").ok()??.dyn_into().ok()
}

/// The bytes inside a base64 `data:` URL.
fn bytes_of(url: &str) -> Option<Vec<u8>> {
    let (_, b64) = url.split_once("base64,")?;
    let binary = web_sys::window()?.atob(b64).ok()?;
    Some(binary.chars().map(|c| c as u8).collect())
}

/// `bytes` (PNG or JPEG) as a base64 `data:` URL.
fn data_url(bytes: &[u8]) -> String {
    let mime = if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else {
        "image/jpeg"
    };
    let binary: String = bytes.iter().map(|&b| b as char).collect();
    let b64 = web_sys::window()
        .and_then(|w| w.btoa(&binary).ok())
        .unwrap_or_default();
    format!("data:{mime};base64,{b64}")
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn click_upload() {
    let input = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(UPLOAD_INPUT_ID))
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(input) = input {
        input.click();
    }
}

/// Moves the tab pill under the selected tab (Tabs sliding: write the tab's
/// offsetLeft / offsetWidth onto the pill).
fn place_pill() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let tab = document
        .query_selector(".sig-box .t-tab[aria-selected='true']")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    let pill = document
        .query_selector(".sig-box .t-tabs-pill")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    if let (Some(tab), Some(pill)) = (tab, pill) {
        let style = pill.style();
        let _ = style.set_property("transform", &format!("translateX({}px)", tab.offset_left()));
        let _ = style.set_property("width", &format!("{}px", tab.offset_width()));
    }
}

/// For the self-check: a small transparent PNG drawn by the pad's exporter.
#[cfg(debug_assertions)]
pub fn sample_png() -> Option<Vec<u8>> {
    let strokes = [Stroke {
        ink: INKS[0].1,
        points: vec![(0.0, 10.0), (20.0, 0.0), (40.0, 12.0)],
    }];
    bytes_of(&drawn_png(&strokes)?)
}
