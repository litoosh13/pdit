//! Shapes draw mode (D-023, feature parity): the right-click menu arms a shape
//! tool; a small style bar sets stroke/fill/width; dragging on the page draws
//! the shape (rectangle, ellipse, line, arrow) as a PDF path object, with Undo.

use crate::page_tools::PageTools;
use dioxus::prelude::*;
use pdit_core::ShapeKind;

const SHAPES_CSS: Asset = asset!("/assets/css/shapes.css");
const ICON_RECT: &str = include_str!("../assets/icons/devigner/Stop.svg");
const ICON_ELLIPSE: &str = include_str!("../assets/icons/devigner/Stop2.svg");
const ICON_LINE: &str = include_str!("../assets/icons/devigner/Minus.svg");
const ICON_ARROW: &str = include_str!("../assets/icons/devigner/ArrowRightUp.svg");

/// The table grid's ink and line width (D-038: no style bar for tables).
pub const TABLE_INK: [u8; 3] = [31, 33, 36];
pub const TABLE_WIDTH_PT: f32 = 1.0;

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}
fn from_hex(s: &str) -> [u8; 3] {
    let p = s.trim_start_matches('#');
    let n = |i: usize| u8::from_str_radix(p.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    [n(0), n(2), n(4)]
}

/// Shared shape draw-mode state.
#[derive(Clone, Copy)]
pub struct ShapeDraw {
    /// The armed tool, while the user is in draw mode.
    armed: Signal<Option<ShapeKind>>,
    stroke: Signal<[u8; 3]>,
    width: Signal<f32>,
    fill: Signal<Option<[u8; 3]>>,
    /// The current drag while drawing: (page, [x1, y1, x2, y2] in PDF points).
    draft: Signal<Option<(u16, [f32; 4])>>,
    /// The table tool is armed (D-038): same drag, no style bar, draws 1 × 1.
    table: Signal<bool>,
    /// The pen is armed (D-041): it stays armed after each stroke until Done.
    pen: Signal<bool>,
    /// The pen's ink (index into PEN_INKS) and width in points.
    pen_ink: Signal<usize>,
    pen_width: Signal<f32>,
    /// The stroke being drawn: (page, points in PDF points).
    pen_stroke: Signal<Option<PenStroke>>,
}

/// A stroke being drawn: (page, points in PDF points).
type PenStroke = (u16, Vec<(f32, f32)>);

/// The pen's colours (D-041, approved): black, blue, red, green.
pub const PEN_INKS: [(&str, [u8; 3]); 4] = [
    ("Black", [31, 33, 36]),
    ("Blue", [29, 111, 216]),
    ("Red", [211, 58, 44]),
    ("Green", [31, 157, 85]),
];
const ICON_PEN: &str = include_str!("../assets/icons/devigner/Pen.svg");

impl ShapeDraw {
    pub fn provide() -> Self {
        use_context_provider(|| ShapeDraw {
            armed: Signal::new(None),
            stroke: Signal::new([31, 33, 36]),
            width: Signal::new(2.0),
            fill: Signal::new(None),
            draft: Signal::new(None),
            table: Signal::new(false),
            pen: Signal::new(false),
            pen_ink: Signal::new(0),
            pen_width: Signal::new(2.0),
            pen_stroke: Signal::new(None),
        })
    }

    pub fn armed(&self) -> Option<ShapeKind> {
        *self.armed.read()
    }
    /// A shape or the table tool is armed: the page is in draw mode.
    pub fn drawing(&self) -> bool {
        self.armed().is_some() || *self.table.read() || *self.pen.read()
    }
    /// The pen is armed (its strokes' release must not edit text under them).
    pub fn pen_armed(&self) -> bool {
        *self.pen.peek()
    }
    /// The in-progress drag rect on `page`, if drawing is happening there.
    pub fn draft_on(&self, page: u16) -> Option<[f32; 4]> {
        self.draft
            .read()
            .and_then(|(p, r)| (p == page).then_some(r))
    }

    /// Arms a tool from the right-click menu (draw mode).
    pub fn arm(mut self, kind: ShapeKind) {
        self.disarm();
        self.armed.set(Some(kind));
    }
    /// Arms the pen from the right-click menu (D-041).
    pub fn arm_pen(mut self) {
        self.disarm();
        self.pen.set(true);
    }
    /// Arms the table tool from the right-click menu (D-038).
    pub fn arm_table(mut self) {
        self.disarm();
        self.table.set(true);
    }
    pub fn disarm(mut self) {
        self.draft.set(None);
        self.armed.set(None);
        self.table.set(false);
        self.pen.set(false);
        self.pen_stroke.set(None);
    }
    pub fn begin(mut self, page: u16, x: f32, y: f32) {
        self.draft.set(Some((page, [x, y, x, y])));
        if *self.pen.peek() {
            self.pen_stroke.set(Some((page, vec![(x, y)])));
        }
    }
    pub fn update(mut self, x: f32, y: f32) {
        self.pen_stroke.with_mut(|s| {
            if let Some((_, points)) = s.as_mut() {
                points.push((x, y));
            }
        });
        self.draft.with_mut(|d| {
            if let Some((_, r)) = d.as_mut() {
                r[2] = x;
                r[3] = y;
            }
        });
    }

    /// Finishes the drag: draws the shape (with Undo) if big enough, then
    /// disarms. A pen stroke is saved and the pen stays armed.
    pub fn finish(mut self) {
        if *self.pen.peek() {
            self.draft.set(None);
            let Some((page, points)) = self.pen_stroke.take() else {
                return;
            };
            if points.len() < 2 {
                return;
            }
            let rgb = PEN_INKS[*self.pen_ink.peek()].1;
            let width = *self.pen_width.peek();
            consume_context::<PageTools>().apply("Drawing added", ICON_PEN, || {
                pdit_core::annotations::add_ink(page, &points, rgb, width).map(|_| ())
            });
            return;
        }
        let armed = *self.armed.peek();
        let table = *self.table.peek();
        let draft = *self.draft.peek();
        self.disarm();
        if table {
            // A table needs some room for its first cell (D-038).
            if let Some((page, [x1, y1, x2, y2])) = draft
                && (x2 - x1).abs() >= 8.0
                && (y2 - y1).abs() >= 8.0
            {
                consume_context::<PageTools>().draw_table(page, [x1, y1, x2, y2]);
            }
            return;
        }
        let (Some(kind), Some((page, [x1, y1, x2, y2]))) = (armed, draft) else {
            return;
        };
        if (x2 - x1).abs() + (y2 - y1).abs() < 3.0 {
            return;
        }
        consume_context::<PageTools>().draw_shape(
            page,
            kind,
            x1,
            y1,
            x2,
            y2,
            *self.stroke.peek(),
            *self.width.peek(),
            *self.fill.peek(),
        );
    }
}
// SHAPESUI_PLACEHOLDER
impl ShapeDraw {
    fn set_stroke(mut self, c: [u8; 3]) {
        self.stroke.set(c);
    }
    fn set_width(mut self, w: f32) {
        self.width.set(w);
    }
    fn set_fill(mut self, f: Option<[u8; 3]>) {
        self.fill.set(f);
    }
}

/// The live preview of the shape being drawn on `page` (SVG, on the white page).
/// `scale` is CSS px per PDF point. Rendered per page, like the other overlays.
#[component]
pub fn ShapeDrawOverlay(page: u16, page_height_pt: f32, scale: f32) -> Element {
    let shapes = use_context::<ShapeDraw>();
    if let Some((p, points)) = shapes.pen_stroke.read().clone()
        && p == page
    {
        let ink = PEN_INKS[*shapes.pen_ink.read()].1;
        let d = points
            .iter()
            .enumerate()
            .map(|(i, (x, y))| {
                let cmd = if i == 0 { "M" } else { "L" };
                format!("{cmd}{},{}", x * scale, (page_height_pt - y) * scale)
            })
            .collect::<Vec<_>>()
            .join(" ");
        let w = *shapes.pen_width.read() * scale;
        return rsx! {
            svg { class: "pdit-shape-preview",
                path {
                    d: "{d}",
                    fill: "none",
                    stroke: "{hex(ink)}",
                    "stroke-width": "{w}",
                    "stroke-linecap": "round",
                    "stroke-linejoin": "round",
                }
            }
        };
    }
    let Some([x1, y1, x2, y2]) = shapes.draft_on(page) else {
        return rsx! {};
    };
    let table = *shapes.table.read();
    let stroke = if table {
        hex(TABLE_INK)
    } else {
        hex(*shapes.stroke.read())
    };
    let sw = if table {
        TABLE_WIDTH_PT
    } else {
        *shapes.width.read()
    } * scale;
    let fill = (*shapes.fill.read())
        .map(hex)
        .unwrap_or_else(|| "none".to_string());
    let (px1, py1) = (x1 * scale, (page_height_pt - y1) * scale);
    let (px2, py2) = (x2 * scale, (page_height_pt - y2) * scale);
    let (l, t, w, h) = (
        px1.min(px2),
        py1.min(py2),
        (px2 - px1).abs(),
        (py2 - py1).abs(),
    );
    rsx! {
        svg { class: "pdit-shape-preview",
            match shapes.armed() {
                None if table => rsx! {
                    rect { x: "{l}", y: "{t}", width: "{w}", height: "{h}", fill: "none", stroke: "{stroke}", "stroke-width": "{sw}" }
                },
                Some(ShapeKind::Rectangle) => rsx! {
                    rect { x: "{l}", y: "{t}", width: "{w}", height: "{h}", fill: "{fill}", stroke: "{stroke}", "stroke-width": "{sw}" }
                },
                Some(ShapeKind::Ellipse) => rsx! {
                    ellipse { cx: "{l + w / 2.0}", cy: "{t + h / 2.0}", rx: "{w / 2.0}", ry: "{h / 2.0}", fill: "{fill}", stroke: "{stroke}", "stroke-width": "{sw}" }
                },
                Some(_) => rsx! {
                    line { x1: "{px1}", y1: "{py1}", x2: "{px2}", y2: "{py2}", stroke: "{stroke}", "stroke-width": "{sw}" }
                },
                None => rsx! {},
            }
        }
    }
}
// STYLEBAR_PLACEHOLDER
/// The floating shape style bar (chrome, follows the theme). Shown while a shape
/// tool is armed; sets stroke colour, fill, and width, and ends draw mode.
#[component]
pub fn ShapeStyleBar() -> Element {
    let shapes = use_context::<ShapeDraw>();
    // Escape leaves draw mode (the table tool has no bar with Done, D-038).
    use_hook(move || {
        use wasm_bindgen::JsCast;
        let on_key = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if event.key() == "Escape" && shapes.drawing() {
                    shapes.disarm();
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
    rsx! {
        document::Stylesheet { href: SHAPES_CSS }
        if *shapes.pen.read() {
            {
                let current = *shapes.pen_ink.read();
                let width = *shapes.pen_width.read();
                let mut ink = shapes.pen_ink;
                let mut pen_width = shapes.pen_width;
                rsx! {
                    div { class: "pdit-shape-bar",
                        span { class: "tool",
                            span { dangerous_inner_html: ICON_PEN }
                            span { "Draw" }
                        }
                        span { class: "grp",
                            label { "Colour" }
                            for (i, (name, [r, g, b])) in PEN_INKS.into_iter().enumerate() {
                                button {
                                    class: "pdit-swatch",
                                    r#type: "button",
                                    "aria-label": name,
                                    "aria-pressed": if i == current { "true" } else { "false" },
                                    style: "background: rgb({r}, {g}, {b});",
                                    onclick: move |_| ink.set(i),
                                }
                            }
                        }
                        span { class: "grp",
                            label { "Width" }
                            input {
                                r#type: "range", min: "1", max: "8", value: "{width}",
                                oninput: move |e| {
                                    if let Ok(w) = e.value().parse::<f32>() { pen_width.set(w); }
                                },
                            }
                            span { class: "wv", "{width}px" }
                        }
                        button { class: "act", onclick: move |_| shapes.disarm(), "Done" }
                    }
                }
            }
        } else if let Some(kind) = shapes.armed() {
            {
                let (icon, name) = match kind {
                    ShapeKind::Rectangle => (ICON_RECT, "Rectangle"),
                    ShapeKind::Ellipse => (ICON_ELLIPSE, "Ellipse"),
                    ShapeKind::Line => (ICON_LINE, "Line"),
                    ShapeKind::Arrow => (ICON_ARROW, "Arrow"),
                };
                let stroke_hex = hex(*shapes.stroke.read());
                let width = *shapes.width.read();
                let fill = *shapes.fill.read();
                let fill_hex = hex(fill.unwrap_or([207, 227, 255]));
                let has_fill = matches!(kind, ShapeKind::Rectangle | ShapeKind::Ellipse);
                rsx! {
                    div { class: "pdit-shape-bar",
                        span { class: "tool",
                            span { dangerous_inner_html: icon }
                            span { "{name}" }
                        }
                        span { class: "grp",
                            label { "Stroke" }
                            input {
                                r#type: "color", value: "{stroke_hex}",
                                oninput: move |e| shapes.set_stroke(from_hex(&e.value())),
                            }
                        }
                        if has_fill {
                            span { class: "grp",
                                label { "Fill" }
                                if fill.is_some() {
                                    input {
                                        r#type: "color", value: "{fill_hex}",
                                        oninput: move |e| shapes.set_fill(Some(from_hex(&e.value()))),
                                    }
                                    button { class: "act", onclick: move |_| shapes.set_fill(None), "\u{00d7}" }
                                } else {
                                    button {
                                        class: "sw-none", "aria-label": "Add fill",
                                        onclick: move |_| shapes.set_fill(Some([207, 227, 255])),
                                    }
                                }
                            }
                        }
                        span { class: "grp",
                            label { "Width" }
                            input {
                                r#type: "range", min: "1", max: "12", value: "{width}",
                                oninput: move |e| {
                                    if let Ok(w) = e.value().parse::<f32>() { shapes.set_width(w); }
                                },
                            }
                            span { class: "wv", "{width}px" }
                        }
                        button { class: "act", onclick: move |_| shapes.disarm(), "Done" }
                    }
                }
            }
        }
    }
}
