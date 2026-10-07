//! Print (D-052): the Header & footer window's look (doc.css), opened with
//! ⌘P / Ctrl+P or the rail's Document → Print…. The chosen pages are drawn for
//! paper by PDFium (about 200 dpi, comments and marks optional) into a
//! print-only container outside the app, then the system print window opens
//! (printer, copies, paper, orientation). Look: assets/css/print.css.

use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const SIGNATURE_CSS: Asset = asset!("/assets/css/signature.css");
const DOC_CSS: Asset = asset!("/assets/css/doc.css");
const PRINT_CSS: Asset = asset!("/assets/css/print.css");
/// The print-only container: a direct child of <body>, outside the app.
const ROOT_ID: &str = "pdit-print-root";
/// Paper resolution the pages are drawn at…
const PRINT_DPI: f32 = 200.0;
/// …unless all chosen pages together would need more memory than this (bytes
/// of RGBA canvas); long documents then print at a lower resolution.
// ponytail: one memory budget for the whole job; print in batches if long
// documents need full resolution.
const PRINT_BUDGET: f32 = 600_000_000.0;
/// Transitions.dev "Modal open/close" close time (ms).
const CLOSE_MS: i32 = 200;

#[derive(Clone, PartialEq)]
struct Dialog {
    /// 0 all pages, 1 this page, 2 a range.
    which: usize,
    current: u16,
    from: u16,
    to: u16,
    comments: bool,
    busy: Option<String>,
    shown: bool,
    closing: bool,
}

/// Shared print state.
#[derive(Clone, Copy)]
pub struct Print {
    dialog: Signal<Option<Dialog>>,
}

impl Print {
    pub fn provide() -> Self {
        use_context_provider(|| Print {
            dialog: Signal::new(None),
        })
    }

    /// Opens the window (with the page nearest the top as "this page").
    pub fn open(mut self) {
        if self.dialog.peek().as_ref().is_some_and(|d| !d.closing) {
            return;
        }
        let pages = page_count();
        self.dialog.set(Some(Dialog {
            which: 0,
            current: crate::bookmarks_ui::current_page() + 1,
            from: 1,
            to: pages,
            comments: true,
            busy: None,
            shown: false,
            closing: false,
        }));
        let mut dialog = self.dialog;
        next_frame(move || {
            next_frame(move || {
                dialog.with_mut(|d| {
                    if let Some(d) = d.as_mut() {
                        d.shown = true;
                    }
                })
            })
        });
        crate::doc_ui::place_pills_soon(".pdit-print .t-tabs");
    }

    pub fn close(mut self) {
        if self
            .dialog
            .peek()
            .as_ref()
            .is_none_or(|d| d.closing || d.busy.is_some())
        {
            return;
        }
        self.dialog.with_mut(|d| {
            if let Some(d) = d.as_mut() {
                d.closing = true;
            }
        });
        let mut dialog = self.dialog;
        set_timeout(CLOSE_MS, move || {
            if dialog.peek().as_ref().is_some_and(|d| d.closing) {
                dialog.set(None);
            }
        });
    }

    fn edit(mut self, f: impl FnOnce(&mut Dialog)) {
        self.dialog.with_mut(|d| {
            if let Some(d) = d.as_mut() {
                f(d);
            }
        });
    }

    /// Draws the chosen pages into the print container, then opens the
    /// system print window.
    fn print(self) {
        let Some(d) = self.dialog.peek().clone() else {
            return;
        };
        let pages: Vec<u16> = match d.which {
            0 => (0..page_count()).collect(),
            1 => vec![d.current - 1],
            _ => {
                let (a, b) = (d.from.min(d.to), d.from.max(d.to));
                (a - 1..b).collect()
            }
        };
        let sizes = pdit_core::page_ops::page_sizes().unwrap_or_default();
        let area: f32 = pages
            .iter()
            .filter_map(|&p| sizes.get(usize::from(p)))
            .map(|(w, h)| w / 72.0 * h / 72.0 * 4.0)
            .sum();
        let dpi = PRINT_DPI.min((PRINT_BUDGET / area.max(1.0)).sqrt());
        spawn(async move {
            let Some(root) = print_root() else { return };
            root.set_inner_html("");
            let total = pages.len();
            for (k, &page) in pages.iter().enumerate() {
                self.edit(|d| d.busy = Some(format!("Preparing page {} of {total}…", k + 1)));
                // Let the status show before the (blocking) drawing.
                pause(30).await;
                let width_pt = sizes.get(usize::from(page)).map_or(595.0, |s| s.0);
                let width_px = (width_pt / 72.0 * dpi).round() as u32;
                match page_canvas(page, width_px, d.comments) {
                    Ok(canvas) => {
                        let _ = root.append_child(&canvas);
                    }
                    Err(error) => {
                        crate::log(&format!(
                            "pdit: could not prepare page {} for printing: {error:?}",
                            page + 1
                        ));
                    }
                }
            }
            self.edit(|d| d.busy = None);
            self.close();
            // After the window's close animation, so it isn't in the way.
            pause(CLOSE_MS + 20).await;
            if let Some(window) = web_sys::window() {
                let _ = window.print();
            }
        });
    }
}

/// The print-only container, made on first use.
fn print_root() -> Option<web_sys::Element> {
    let document = web_sys::window()?.document()?;
    if let Some(root) = document.get_element_by_id(ROOT_ID) {
        return Some(root);
    }
    let root = document.create_element("div").ok()?;
    root.set_id(ROOT_ID);
    document.body()?.append_child(&root).ok()?;
    // Free the drawn pages once the print window is done.
    let to_clear = root.clone();
    let on_after = Closure::<dyn FnMut()>::new(move || to_clear.set_inner_html(""));
    let _ = web_sys::window()?
        .add_event_listener_with_callback("afterprint", on_after.as_ref().unchecked_ref());
    on_after.forget();
    Some(root)
}

/// Page `page` drawn for paper, as a canvas (printed as it is; nothing to
/// encode or decode).
fn page_canvas(
    page: u16,
    width_px: u32,
    comments: bool,
) -> Result<web_sys::HtmlCanvasElement, JsValue> {
    let image = pdit_core::render_page_for_print(page, width_px, comments)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let canvas: web_sys::HtmlCanvasElement = document.create_element("canvas")?.dyn_into()?;
    canvas.set_width(image.width());
    canvas.set_height(image.height());
    canvas.set_attribute("aria-label", &format!("Page {}", page + 1))?;
    let context: web_sys::CanvasRenderingContext2d = canvas
        .get_context("2d")?
        .ok_or("no 2d context")?
        .dyn_into()?;
    context.put_image_data(&image, 0.0, 0.0)?;
    Ok(canvas)
}

pub(crate) async fn pause(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

fn page_count() -> u16 {
    pdit_core::page_ops::page_sizes().map_or(1, |s| s.len().max(1) as u16)
}

/// The window, and ⌘P / Ctrl+P.
#[component]
pub fn PrintUi() -> Element {
    let print = use_context::<Print>();
    let document = use_context::<Signal<Option<crate::pages::OpenDocument>>>();
    // The window listener runs outside Dioxus; it only counts the key and the
    // effect opens the window inside Dioxus (as AnnotationKeys).
    let mut opens = use_signal(|| 0u32);
    use_effect(move || {
        if opens() > 0 && document.peek().is_some() {
            print.open();
        }
    });
    use_hook(move || {
        let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if (event.meta_key() || event.ctrl_key()) && event.key().eq_ignore_ascii_case("p") {
                    event.prevent_default();
                    opens += 1;
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
    // Keep the tab pills under the chosen tabs.
    let pill_key = print
        .dialog
        .read()
        .as_ref()
        .map(|d| (d.shown, d.which, d.comments));
    use_effect(use_reactive!(|pill_key| {
        if pill_key.is_some() {
            crate::doc_ui::place_pills_soon(".pdit-print .t-tabs");
        }
    }));

    let Some(d) = print.dialog.read().clone() else {
        return rsx! {
            document::Stylesheet { href: PRINT_CSS }
        };
    };
    let class = match (d.shown, d.closing) {
        (_, true) => "doc-box t-modal is-closing",
        (true, false) => "doc-box t-modal is-open",
        (false, false) => "doc-box t-modal",
    };
    let pages = page_count();
    let this_page = format!("This page ({})", d.current);
    let busy = d.busy.is_some();
    rsx! {
        document::Stylesheet { href: SIGNATURE_CSS }
        document::Stylesheet { href: DOC_CSS }
        document::Stylesheet { href: PRINT_CSS }
        div { class: "doc-layer sa-root pdit-print",
            div { class, role: "dialog", "aria-label": "Print",
                div { class: "doc-head", span { class: "title", "Print" } }
                div {
                    div { class: "doc-lbl", "Pages" }
                    crate::doc_ui::Tabs {
                        options: vec![
                            ("All pages".into(), d.which == 0),
                            (this_page, d.which == 1),
                            ("Pages…".into(), d.which == 2),
                        ],
                        on_pick: move |i| print.edit(|d| d.which = i),
                    }
                }
                if d.which == 2 {
                    div { class: "doc-row",
                        label { "From" }
                        input {
                            class: "doc-field doc-range", r#type: "number", min: "1", max: "{pages}", value: "{d.from}",
                            "aria-label": "From page",
                            oninput: move |e| {
                                let v = e.value().parse::<u16>().unwrap_or(1).clamp(1, pages);
                                print.edit(|d| d.from = v);
                            },
                        }
                        label { "to" }
                        input {
                            class: "doc-field doc-range", r#type: "number", min: "1", max: "{pages}", value: "{d.to}",
                            "aria-label": "To page",
                            oninput: move |e| {
                                let v = e.value().parse::<u16>().unwrap_or(pages).clamp(1, pages);
                                print.edit(|d| d.to = v);
                            },
                        }
                        span { class: "doc-wv", "of {pages}" }
                    }
                }
                div {
                    div { class: "doc-lbl", "Comments and marks" }
                    crate::doc_ui::Tabs {
                        options: vec![("Print them".into(), d.comments), ("Leave out".into(), !d.comments)],
                        on_pick: move |i| print.edit(|d| d.comments = i == 0),
                    }
                }
                div { class: "doc-foot",
                    span { class: "pdit-print-status",
                        {d.busy.clone().unwrap_or_else(|| "Printer, copies and paper size come next, in the system print window.".into())}
                    }
                    span { class: "grow" }
                    button {
                        class: "sa-control", r#type: "button", disabled: busy,
                        onclick: move |_| print.close(),
                        "Cancel"
                    }
                    button {
                        class: "sa-primary", r#type: "button", disabled: busy,
                        onclick: move |_| print.print(),
                        "Print…"
                    }
                }
            }
        }
    }
}
