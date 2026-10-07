//! Shows an opened PDF as a vertical list of pages. Each page slot starts as a
//! pulsing skeleton and cross-fades into the rendered page (Transitions.dev
//! "Skeleton loader and reveal", assets/css/skeleton.css), framed with
//! Beautiful UI's card surface (assets/css/pages.css).

use crate::context_menu::ContextMenuState;
use crate::editing::{Editing, SelectionOverlay};
use crate::page_tools::{ImageOverlay, PageTools};
use dioxus::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const SKELETON_CSS: Asset = asset!("/assets/css/skeleton.css");
const PAGES_CSS: Asset = asset!("/assets/css/pages.css");

/// CSS pixels per PDF point at 100% zoom (96 dpi / 72 pt per inch).
const PX_PER_PT: f32 = 96.0 / 72.0;

/// A PDF the user opened, shared with the page views through context. The
/// document itself stays loaded in pdit-core (`open_document`).
#[derive(Clone)]
pub struct OpenDocument {
    /// Changes for every opened file, so page views re-render.
    pub id: u64,
    /// The opened file's name, for naming the saved copy (D-020).
    pub name: String,
    /// Page sizes in PDF points (width, height).
    pub page_sizes: Vec<(f32, f32)>,
}

#[component]
pub fn PageList() -> Element {
    let document = use_context::<Signal<Option<OpenDocument>>>();
    rsx! {
        // Loaded even with no PDF open: pages.css also sets the app background.
        document::Stylesheet { href: SKELETON_CSS }
        document::Stylesheet { href: PAGES_CSS }
        if let Some(open) = document() {
            div { class: "page-list",
                for (index, (width, height)) in open.page_sizes.iter().copied().enumerate() {
                    PageView {
                        key: "{open.id}-{index}",
                        index: index as u16,
                        width_pt: width,
                        height_pt: height,
                    }
                }
            }
        }
    }
}

/// Keeps a page's IntersectionObserver (and its callback) alive while the page
/// view exists, and disconnects it when the view goes away.
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

/// How far outside the visible area a page starts rendering: one screen height
/// above and below, so pages are usually ready before they scroll into view.
const RENDER_MARGIN: &str = "100% 0px";

/// An in-progress image drag-to-move (D-023a): where the press began and the
/// last point applied (CSS px), the slot's px-per-point, and whether the pointer
/// has crossed the click threshold (so a plain click doesn't move or snapshot).
#[derive(Clone, Copy)]
struct ImageDrag {
    start_x: f64,
    start_y: f64,
    last_x: f64,
    last_y: f64,
    per_pt: f64,
    moved: bool,
}

#[component]
fn PageView(index: u16, width_pt: f32, height_pt: f32) -> Element {
    let revealed = use_signal(|| false);
    let watcher = use_hook(|| Rc::new(RefCell::new(Watcher::default())));
    let editing = use_context::<Editing>();
    let context_menu = use_context::<ContextMenuState>();
    let tools = use_context::<PageTools>();
    let shapes = use_context::<crate::shapes_ui::ShapeDraw>();
    let annotate = use_context::<crate::annotations_ui::Annotate>();
    let forms = use_context::<crate::form_edit_ui::FormEdit>();
    let rail = use_context::<crate::tools_ui::Tools>();
    // The slot's on-screen width, for converting PDF points to CSS pixels. Kept
    // current by a ResizeObserver: the slot narrows when a side panel opens
    // (Pages, D-029; text edit panel, D-035), and a stale width misplaces the
    // selected line's outline (P-036).
    let mut slot_width = use_signal(|| 0.0_f64);
    let sizer = use_hook(|| Rc::new(RefCell::new(None::<SlotSizer>)));
    let mut slot_element = use_signal(|| None::<web_sys::Element>);
    // An in-progress image drag-to-move (D-023a).
    let mut img_drag = use_signal(|| None::<ImageDrag>);
    // Zoom (D-049): the page's width; pages redraw once the zoom settles.
    let zoom = use_context::<crate::zoom_ui::Zoom>();
    let percent = *zoom.percent.read();
    let settled = *zoom.settled.read();
    let width_px = width_pt * PX_PER_PT * percent / 100.0;
    // Re-measure the page after a zoom change, so overlays scale at once
    // (the ResizeObserver also does this, but only when the window paints).
    use_effect(use_reactive!(|percent| {
        let _ = percent;
        crate::page_tools::next_frame(move || {
            if let Some(slot) = slot_element.peek().clone() {
                slot_width.set(slot.get_bounding_client_rect().width());
            }
        });
    }));

    // Redraw after an edit changed this page (preview, discard).
    let version = editing
        .page_versions
        .read()
        .get(&index)
        .copied()
        .unwrap_or(0);
    use_effect(use_reactive!(|version, settled| {
        let _ = (version, settled);
        if !*revealed.peek() {
            return;
        }
        let canvas = slot_element
            .peek()
            .as_ref()
            .and_then(|slot| slot.query_selector("canvas").ok().flatten())
            .and_then(|c| c.dyn_into::<web_sys::HtmlCanvasElement>().ok());
        if let Some(canvas) = canvas
            && let Err(error) = draw(&canvas, index)
        {
            crate::log(&format!(
                "pdit: could not redraw page {}: {error}",
                index + 1
            ));
        }
    }));

    let scale = (slot_width() / f64::from(width_pt)) as f32;
    let arming = shapes.drawing() || rail.placing();
    let page_class = match (revealed(), arming) {
        (true, true) => "page t-skel is-revealed pdit-arming",
        (true, false) => "page t-skel is-revealed",
        (false, true) => "page t-skel pdit-arming",
        (false, false) => "page t-skel",
    };
    rsx! {
        div {
            class: page_class,
            "data-page": "{index}",
            // Up to 100 % a page also shrinks to fit a narrow window; zoomed
            // in, it may be wider than the window (the list scrolls sideways).
            style: if percent <= 100.0 {
                format!("width: min(100%, {width_px}px); aspect-ratio: {width_pt} / {height_pt};")
            } else {
                format!("width: {width_px}px; aspect-ratio: {width_pt} / {height_pt};")
            },
            // Render only when the page comes near the screen (P-028).
            onmounted: move |event| {
                let Some(slot) = event.data().downcast::<web_sys::Element>().cloned() else {
                    return;
                };
                slot_width.set(slot.get_bounding_client_rect().width());
                *sizer.borrow_mut() = SlotSizer::observe(&slot, slot_width);
                if let Err(error) = watch(&slot, index, revealed, &watcher) {
                    crate::log(&format!("pdit: could not watch page {}: {error:?}", index + 1));
                }
                slot_element.set(Some(slot));
            },
            // Click-to-edit (D-019): select the text line under the pointer.
            // Image select / drag-to-move is handled by the pointer events below.
            onclick: move |event| {
                let Some(slot) = slot_element.peek().clone() else { return };
                let rect = slot.get_bounding_client_rect();
                slot_width.set(rect.width());
                let point = event.client_coordinates();
                let per_pt = rect.width() / f64::from(width_pt);
                let x = ((point.x - rect.left()) / per_pt) as f32;
                let y = height_pt - ((point.y - rect.top()) / per_pt) as f32;
                // A drag across words (D-040) or a pen stroke (D-041) ends in a
                // click: not an edit.
                // A tool armed in the tools rail (D-050) is placed here.
                if let Some(action) = rail.take_placing() {
                    return crate::context_menu::run(action, index, x, y);
                }
                if annotate.take_click() || shapes.pen_armed() || forms.editing() {
                    return;
                }
                editing.select_at(index, x, y);
            },
            // Click-to-select an image, then drag it to move (D-023a). Text
            // lines keep priority (handled by onclick); a press on empty space
            // deselects any image.
            onpointerdown: move |event| {
                if event.trigger_button() != Some(dioxus::html::input_data::MouseButton::Primary) {
                    return;
                }
                let Some(slot) = slot_element.peek().clone() else { return };
                let rect = slot.get_bounding_client_rect();
                let point = event.client_coordinates();
                let per_pt = rect.width() / f64::from(width_pt);
                let x = ((point.x - rect.left()) / per_pt) as f32;
                let y = height_pt - ((point.y - rect.top()) / per_pt) as f32;
                // An armed rail tool waits for the click (onclick).
                if rail.placing() {
                    return;
                }
                // Form edit mode (D-047): place, select, move or resize fields.
                if forms.editing() {
                    forms.press(index, x, y, per_pt as f32);
                    if forms.busy()
                        && let Some(pe) = event.data().downcast::<web_sys::PointerEvent>()
                    {
                        let _ = slot.set_pointer_capture(pe.pointer_id());
                    }
                    return;
                }
                // Shape draw mode (D-023): start drawing the armed shape.
                if shapes.drawing() {
                    shapes.begin(index, x, y);
                    if let Some(pe) = event.data().downcast::<web_sys::PointerEvent>() {
                        let _ = slot.set_pointer_capture(pe.pointer_id());
                    }
                    return;
                }
                tools.table_press(index, x, y);
                annotate.dismiss();
                if crate::editing::line_at(index, x, y).is_some() {
                    tools.clear_image_selection();
                    // A drag from here selects words to mark (D-040); a plain
                    // click still edits the line (onclick).
                    annotate.press(index, x, y);
                    if let Some(pe) = event.data().downcast::<web_sys::PointerEvent>() {
                        let _ = slot.set_pointer_capture(pe.pointer_id());
                    }
                    return;
                }
                match pdit_core::page_ops::image_at(index, x, y) {
                    Ok(Some(_)) => {
                        tools.select_image(index, x, y);
                        // Capture the pointer so move/up still fire if the drag
                        // leaves the page, avoiding a stuck drag.
                        if let Some(pe) = event.data().downcast::<web_sys::PointerEvent>() {
                            let _ = slot.set_pointer_capture(pe.pointer_id());
                        }
                        img_drag.set(Some(ImageDrag {
                            start_x: point.x,
                            start_y: point.y,
                            last_x: point.x,
                            last_y: point.y,
                            per_pt,
                            moved: false,
                        }));
                    }
                    _ => tools.clear_image_selection(),
                }
            },
            onpointermove: move |event| {
                if forms.busy() {
                    if let Some(slot) = slot_element.peek().clone() {
                        let rect = slot.get_bounding_client_rect();
                        let point = event.client_coordinates();
                        let per_pt = rect.width() / f64::from(width_pt);
                        let x = ((point.x - rect.left()) / per_pt) as f32;
                        let y = height_pt - ((point.y - rect.top()) / per_pt) as f32;
                        forms.drag_to(x, y);
                    }
                    return;
                }
                if shapes.drawing() {
                    if shapes.draft_on(index).is_some()
                        && let Some(slot) = slot_element.peek().clone()
                    {
                        let rect = slot.get_bounding_client_rect();
                        let point = event.client_coordinates();
                        let per_pt = rect.width() / f64::from(width_pt);
                        let x = ((point.x - rect.left()) / per_pt) as f32;
                        let y = height_pt - ((point.y - rect.top()) / per_pt) as f32;
                        shapes.update(x, y);
                    }
                    return;
                }
                if annotate.dragging() {
                    if let Some(slot) = slot_element.peek().clone() {
                        let rect = slot.get_bounding_client_rect();
                        let point = event.client_coordinates();
                        let per_pt = rect.width() / f64::from(width_pt);
                        let x = ((point.x - rect.left()) / per_pt) as f32;
                        let y = height_pt - ((point.y - rect.top()) / per_pt) as f32;
                        annotate.drag_to(x, y);
                    }
                    return;
                }
                let Some(drag) = img_drag() else { return };
                let point = event.client_coordinates();
                if !drag.moved {
                    // Ignore tiny jitter so a click stays a click (no snapshot).
                    if (point.x - drag.start_x).abs() + (point.y - drag.start_y).abs() <= 3.0 {
                        return;
                    }
                    tools.begin_image_drag();
                    img_drag.with_mut(|d| {
                        if let Some(d) = d.as_mut() {
                            d.moved = true;
                        }
                    });
                }
                // Apply the step since the last point, and redraw live so the
                // image (not just the ring) follows the pointer.
                let dx_pt = ((point.x - drag.last_x) / drag.per_pt) as f32;
                // Screen y grows downward; PDF y grows upward.
                let dy_pt = -(((point.y - drag.last_y) / drag.per_pt) as f32);
                tools.drag_move(index, dx_pt, dy_pt);
                img_drag.with_mut(|d| {
                    if let Some(d) = d.as_mut() {
                        d.last_x = point.x;
                        d.last_y = point.y;
                    }
                });
            },
            onpointerup: move |_| {
                if forms.busy() {
                    forms.release();
                    return;
                }
                if shapes.drawing() {
                    shapes.finish();
                    return;
                }
                if annotate.dragging() {
                    annotate.release();
                    return;
                }
                let Some(drag) = img_drag() else { return };
                img_drag.set(None);
                if drag.moved {
                    tools.end_image_drag(false);
                }
            },
            // Right-click menu (D-023): options for what is under the pointer;
            // where none apply, the browser's own menu shows.
            oncontextmenu: move |event| {
                let Some(slot) = slot_element.peek().clone() else { return };
                let rect = slot.get_bounding_client_rect();
                let point = event.client_coordinates();
                let per_pt = rect.width() / f64::from(width_pt);
                let x = ((point.x - rect.left()) / per_pt) as f32;
                let y = height_pt - ((point.y - rect.top()) / per_pt) as f32;
                if context_menu.open_at(point.x, point.y, index, (x, y)) {
                    event.prevent_default();
                }
            },
            div { class: "t-skel-skeleton is-pulsing",
                div { class: "page-skeleton-block" }
            }
            div { class: "t-skel-content",
                canvas { "aria-label": "Page {index + 1}" }
            }
            SelectionOverlay {
                page: index,
                page_width_pt: width_pt,
                page_height_pt: height_pt,
                scale,
            }
            ImageOverlay {
                page: index,
                page_height_pt: height_pt,
                scale,
            }
            crate::search_ui::FindOverlay {
                page: index,
                page_height_pt: height_pt,
                scale,
            }
            crate::forms_ui::FormOverlay {
                page: index,
                page_height_pt: height_pt,
                scale,
            }
            crate::shapes_ui::ShapeDrawOverlay {
                page: index,
                page_height_pt: height_pt,
                scale,
            }
            crate::annotations_ui::AnnotationOverlay {
                page: index,
                page_width_pt: width_pt,
                page_height_pt: height_pt,
                scale,
            }
            crate::find_fields_ui::FindFieldsOverlay {
                page: index,
                page_height_pt: height_pt,
                scale,
            }
            crate::form_edit_ui::FormEditOverlay {
                page: index,
                page_width_pt: width_pt,
                page_height_pt: height_pt,
                scale,
            }
            crate::links_ui::LinkOverlay {
                page: index,
                page_width_pt: width_pt,
                page_height_pt: height_pt,
                scale,
            }
            crate::doc_ui::DocPreview {
                page: index,
                page_width_pt: width_pt,
                page_height_pt: height_pt,
                scale,
            }
            crate::table_ui::TableOverlay {
                page: index,
                page_width_pt: width_pt,
                page_height_pt: height_pt,
                scale,
            }
        }
    }
}

/// A ResizeObserver keeping a page slot's width signal current (P-036);
/// disconnected when the page view goes away.
struct SlotSizer {
    observer: web_sys::ResizeObserver,
    _callback: Closure<dyn FnMut(js_sys::Array)>,
}

impl SlotSizer {
    fn observe(slot: &web_sys::Element, mut width: Signal<f64>) -> Option<Self> {
        let target = slot.clone();
        let callback = Closure::<dyn FnMut(js_sys::Array)>::new(move |_entries: js_sys::Array| {
            let now = target.get_bounding_client_rect().width();
            if (*width.peek() - now).abs() > 0.1 {
                width.set(now);
            }
        });
        let observer = web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()).ok()?;
        observer.observe(slot);
        Some(Self {
            observer,
            _callback: callback,
        })
    }
}

impl Drop for SlotSizer {
    fn drop(&mut self) {
        self.observer.disconnect();
    }
}

/// Starts observing the page slot; the first time it comes within
/// RENDER_MARGIN of the viewport, the page is drawn and revealed.
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
            Err(error) => crate::log(&format!(
                "pdit: could not render page {}: {error}",
                index + 1
            )),
        }
        // Drawn once; stop watching.
        if let Some(watcher) = done.upgrade()
            && let Some((observer, _)) = watcher.borrow().0.as_ref()
        {
            observer.disconnect();
        }
    });
    let options = web_sys::IntersectionObserverInit::new();
    options.set_root_margin(RENDER_MARGIN);
    let observer = web_sys::IntersectionObserver::new_with_options(
        callback.as_ref().unchecked_ref(),
        &options,
    )?;
    observer.observe(slot);
    watcher.borrow_mut().0 = Some((observer, callback));
    Ok(())
}

/// Renders the page at its slot's on-screen width times the device pixel ratio.
/// The slot (`.t-skel`) is measured, not the canvas: the slot's size is set
/// inline, while the canvas may not have its stylesheet applied yet on mount.
fn draw(canvas: &web_sys::HtmlCanvasElement, index: u16) -> Result<(), String> {
    let ratio = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0);
    let slot_width = canvas
        .closest(".t-skel")
        .ok()
        .flatten()
        .map_or(0.0, |slot| slot.get_bounding_client_rect().width());
    // WebKit refuses canvases above ~16.7 million pixels; a zoomed-in page on a
    // Retina screen can exceed that, so it is drawn smaller and scaled up.
    // ponytail: high zoom then looks softer; tile the page if that matters.
    let aspect = canvas
        .closest(".t-skel")
        .ok()
        .flatten()
        .map(|slot| slot.get_bounding_client_rect())
        .filter(|r| r.width() > 0.0)
        .map_or(1.414, |r| r.height() / r.width());
    let max_width = (16_000_000.0 / aspect).sqrt();
    let width_px = (slot_width * ratio).min(max_width).round().max(1.0) as u32;
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
