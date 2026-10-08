//! Table "+" buttons (D-038): the table drawn in this session shows a ring and
//! two "+" buttons, layout B: add a column (above the table, near its right
//! end) and add a row (on its left, near its bottom). Dragging a line resizes
//! cells: an inner line trades size between its two neighbours, the right /
//! bottom edge resizes the last column / row. Drawing uses the shapes draw
//! mode (right-click → Shapes → Table).

use crate::page_tools::PageTools;
use dioxus::prelude::*;

const TABLE_CSS: Asset = asset!("/assets/css/table.css");
const IMAGE_SELECT_CSS: Asset = asset!("/assets/css/image-select.css");
const ICON_PLUS: &str = include_str!("../assets/icons/devigner/Plus.svg");

/// The ring and "+" buttons for the table on `page`. `scale` is CSS px per PDF
/// point. Rendered per page, like the other overlays.
#[component]
pub fn TableOverlay(page: u16, page_width_pt: f32, page_height_pt: f32, scale: f32) -> Element {
    let tools = use_context::<PageTools>();
    let mut drag = use_signal(|| None::<LineDrag>);
    let Some(sel) = tools.table().filter(|t| t.page == page) else {
        return rsx! {};
    };
    let [left, bottom, right, top] = sel.layout.bounds();
    // Grab strips along every line but the left and top edges:
    // (is a column line, line number, strip left, top, width, height in px).
    let mut lines = Vec::new();
    let mut x = left;
    for (i, w) in sel.layout.col_widths.iter().enumerate() {
        x += w;
        let px = x * scale - GRAB_PX / 2.0;
        lines.push((
            true,
            i + 1,
            px,
            (page_height_pt - top) * scale,
            GRAB_PX,
            (top - bottom) * scale,
        ));
    }
    let mut y = top;
    for (i, h) in sel.layout.row_heights.iter().enumerate() {
        y -= h;
        let py = (page_height_pt - y) * scale - GRAB_PX / 2.0;
        lines.push((
            false,
            i + 1,
            left * scale,
            py,
            (right - left) * scale,
            GRAB_PX,
        ));
    }
    let handles = rsx! {
        for (is_col, line, lx, ly, lw, lh) in lines {
            div {
                key: "{is_col}-{line}",
                class: if is_col { "pdit-table-line is-col" } else { "pdit-table-line is-row" },
                style: "left: {lx}px; top: {ly}px; width: {lw}px; height: {lh}px;",
                onpointerdown: move |event| {
                    event.stop_propagation();
                    let Some(layout) = tools.table().map(|t| t.layout) else { return };
                    if let Some(pe) = event.data().downcast::<web_sys::PointerEvent>()
                        && let Some(target) = pe.target()
                        && let Ok(element) = wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(target)
                    {
                        let _ = element.set_pointer_capture(pe.pointer_id());
                    }
                    let point = event.client_coordinates();
                    tools.begin_table_resize();
                    drag.set(Some(LineDrag {
                        is_col,
                        line,
                        start: if is_col { point.x } else { point.y },
                        layout,
                    }));
                },
                onpointermove: move |event| {
                    let Some(d) = drag.peek().clone() else { return };
                    let point = event.client_coordinates();
                    let moved = ((if d.is_col { point.x } else { point.y }) - d.start) as f32 / scale;
                    tools.resize_table(resized(&d, moved, page_width_pt));
                },
                onpointerup: move |event| {
                    event.stop_propagation();
                    if drag.peek().is_some() {
                        drag.set(None);
                        tools.end_table_resize();
                    }
                },
                onclick: move |event| event.stop_propagation(),
            }
        }
    };
    if !sel.shown {
        return rsx! {
            document::Stylesheet { href: TABLE_CSS }
            {handles}
        };
    }
    let pad = 3.0_f32;
    let x = left * scale - pad;
    let y = (page_height_pt - top) * scale - pad;
    let width = (right - left) * scale + 2.0 * pad;
    let height = (top - bottom) * scale + 2.0 * pad;
    // (label, add a row?, centre x px, centre y px)
    let buttons = [
        ("Add column", false, x + width - 10.0, y - 14.0),
        ("Add row", true, x - 14.0, y + height - 10.0),
    ];
    rsx! {
        document::Stylesheet { href: IMAGE_SELECT_CSS }
        document::Stylesheet { href: TABLE_CSS }
        div {
            class: "pdit-img-ring",
            style: "left: {x}px; top: {y}px; width: {width}px; height: {height}px;",
        }
        {handles}
        for (label, add_row, cx, cy) in buttons {
            button {
                r#type: "button",
                class: "pdit-table-plus",
                title: label,
                "aria-label": label,
                style: "left: {cx}px; top: {cy}px;",
                // Keep the press from reaching the page (select text / deselect).
                onpointerdown: move |event| event.stop_propagation(),
                onclick: move |event| {
                    event.stop_propagation();
                    tools.grow_table(add_row);
                },
                span { dangerous_inner_html: ICON_PLUS, style: "display: contents" }
            }
        }
    }
}

/// Width of the invisible strip along a table line that takes the drag.
const GRAB_PX: f32 = 10.0;
/// A cell can't be dragged smaller than this.
const MIN_CELL_PT: f32 = 10.0;
/// A table resized by its right / bottom edge stays this far inside the page.
const EDGE_PT: f32 = 8.0;

/// A line being dragged: which line (1 = after the first column/row), where
/// the drag started (client px), and the table's layout at that moment.
#[derive(Clone)]
struct LineDrag {
    is_col: bool,
    line: usize,
    start: f64,
    layout: pdit_core::TableLayout,
}

/// The layout after moving the dragged line by `moved` PDF points (right /
/// down). An inner line trades size between its two neighbours; the right /
/// bottom edge resizes the last column / row, kept on the page.
fn resized(d: &LineDrag, moved: f32, page_width_pt: f32) -> pdit_core::TableLayout {
    let mut out = d.layout.clone();
    let [_, bottom, right, _] = d.layout.bounds();
    let sizes = if d.is_col {
        &mut out.col_widths
    } else {
        &mut out.row_heights
    };
    let i = d.line - 1;
    let first = sizes[i];
    if d.line < sizes.len() {
        let second = sizes[i + 1];
        let moved = moved.clamp(MIN_CELL_PT - first, second - MIN_CELL_PT);
        sizes[i] = first + moved;
        sizes[i + 1] = second - moved;
    } else {
        let room = if d.is_col {
            page_width_pt - EDGE_PT - right
        } else {
            bottom - EDGE_PT
        };
        sizes[i] = first + moved.clamp(MIN_CELL_PT - first, room.max(0.0));
    }
    out
}
