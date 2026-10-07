//! Click-to-edit (D-019): the selected text line, and Beautiful UI's
//! "Selection Actions" bar (assets/css/selection-actions.css) ported to Dioxus.
//! Since D-035 the bar's text row and the format bar sit together in one edit
//! panel beside the page (assets/css/edit-panel.css); the line keeps a grey
//! outline and a thin connector leads to the panel, so nothing covers the text.

use dioxus::prelude::*;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::JsCast;

const SELECTION_CSS: Asset = asset!("/assets/css/selection-actions.css");
const ERROR_CSS: Asset = asset!("/assets/css/error-shake.css");
const PANEL_CSS: Asset = asset!("/assets/css/edit-panel.css");
const IMAGE_SELECT_CSS: Asset = asset!("/assets/css/image-select.css");
/// Shown beneath the bar when no font can show the typed characters (D-019).
const UNSUPPORTED_MESSAGE: &str = "These characters can't be shown in this document.";
const ICON_SEND: &str = include_str!("../assets/icons/inkbrush-arrow-up.svg");
const ICON_KEEP: &str = include_str!("../assets/icons/inkbrush-check.svg");
const ICON_DISCARD: &str = include_str!("../assets/icons/inkbrush-close-cross.svg");

/// The text the user clicked: a single line, or a whole wrapped paragraph block
/// (reflow). `object_index` is the anchor (topmost line, used for style/traits);
/// `object_indices` is the whole block in reading order.
#[derive(Clone, PartialEq, Debug)]
pub struct Selection {
    pub page: u16,
    pub object_index: usize,
    /// The block's text objects, top to bottom (one entry for a single line).
    pub object_indices: Vec<usize>,
    /// The line's / block's text in the document right now (after any kept edit).
    pub text: String,
    /// Left, bottom, right, top in PDF points.
    pub bounds: [f32; 4],
    /// Adding new text (D-026): the point it goes at, until it is kept.
    pub adding: Option<(f32, f32)>,
}

/// The bar's state, following the source's modes (only its editing ones, D-019).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BarMode {
    /// Input with the line's text and the send button.
    Editing,
    /// A previewed edit waiting for Keep or Discard.
    Result,
}

#[derive(Clone, PartialEq, Debug)]
pub struct EditState {
    pub selection: Option<Selection>,
    pub mode: BarMode,
    pub draft: String,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            selection: None,
            mode: BarMode::Editing,
            draft: String::new(),
        }
    }
}

/// The text line under a point (PDF points), if any.
pub fn line_at(page: u16, x: f32, y: f32) -> Option<pdit_core::TextLine> {
    pdit_core::text_lines(page).ok().and_then(|lines| {
        lines.into_iter().find(|line| {
            let [left, bottom, right, top] = line.bounds;
            x >= left && x <= right && y >= bottom && y <= top
        })
    })
}

/// The paragraph block under a point as a [`Selection`] (reflow): the whole
/// wrapped block when the point is on text, else none.
fn paragraph_selection(page: u16, x: f32, y: f32) -> Option<Selection> {
    let p = pdit_core::paragraph_at(page, x, y).ok().flatten()?;
    Some(Selection {
        page,
        object_index: *p.object_indices.first().unwrap_or(&0),
        object_indices: p.object_indices,
        text: p.text,
        bounds: p.bounds,
        adding: None,
    })
}

/// Shared editing context: the state, per-page redraw counters, and the
/// fallback font bytes (fetched once at start).
#[derive(Clone, Copy)]
pub struct Editing {
    pub state: Signal<EditState>,
    pub page_versions: Signal<HashMap<u16, u32>>,
    pub fallback_font: Signal<Option<Rc<Vec<u8>>>>,
    /// The selected line's style, shown and changed by the format bar (D-027).
    pub style: Signal<Option<pdit_core::TextStyle>>,
}

impl Editing {
    pub fn provide() -> Self {
        use_context_provider(|| Editing {
            state: Signal::new(EditState::default()),
            page_versions: Signal::new(HashMap::new()),
            fallback_font: Signal::new(None),
            style: Signal::new(None),
        })
    }

    pub fn redraw(mut self, page: u16) {
        *self.page_versions.write().entry(page).or_default() += 1;
    }

    /// Selects the paragraph block at (x, y) PDF points on `page` (the whole
    /// wrapped sentence, reflow), or closes the bar if there is no text there.
    /// Any pending preview is discarded first.
    pub fn select_at(mut self, page: u16, x: f32, y: f32) {
        self.cancel();
        if let Some(selection) = paragraph_selection(page, x, y) {
            self.style
                .set(pdit_core::text_style(page, selection.object_index).ok());
            self.state.set(EditState {
                draft: selection.text.clone(),
                mode: BarMode::Editing,
                selection: Some(selection),
            });
        }
    }

    /// Add text (D-026): opens the bar, empty, at (x, y) PDF points on `page`.
    pub fn start_add(mut self, page: u16, x: f32, y: f32) {
        self.cancel();
        self.style.set(pdit_core::style_near(page, x, y).ok());
        self.state.set(EditState {
            draft: String::new(),
            mode: BarMode::Editing,
            selection: Some(Selection {
                page,
                object_index: 0,
                object_indices: Vec::new(),
                text: String::new(),
                bounds: [x, y, x, y],
                adding: Some((x, y)),
            }),
        });
    }

    /// Closes the bar without changing anything (Escape / click outside).
    pub fn cancel(mut self) {
        clear_error();
        let state = self.state.peek().clone();
        if let (Some(selection), BarMode::Result) = (&state.selection, state.mode) {
            if let Err(error) = pdit_core::discard_edit() {
                crate::log(&format!("pdit: could not discard the edit: {error}"));
            }
            self.redraw(selection.page);
        }
        self.state.set(EditState::default());
        self.style.set(None);
    }

    /// A change from the format bar (D-027): previewed right away, like an
    /// edit, so Keep and Discard apply to it. New text with nothing typed yet
    /// only remembers the style.
    pub fn restyle(self, style: pdit_core::TextStyle) {
        let mut this = self;
        this.style.set(Some(style));
        let state = self.state.peek().clone();
        let Some(selection) = state.selection else {
            return;
        };
        if selection.adding.is_some() && state.draft.trim().is_empty() {
            return;
        }
        clear_error();
        self.preview();
    }

    /// Enter / send: previews the draft on the page. The look-alike of the
    /// line's font (D-028) is loaded first when it isn't yet, so a font that
    /// can't show the text falls back to it rather than to Noto Sans.
    fn preview(self) {
        let state = self.state.peek().clone();
        let Some(selection) = state.selection else {
            return;
        };
        if selection.adding.is_some() && state.draft.trim().is_empty() {
            // Nothing typed yet: nothing to add.
            return;
        }
        let style = *self.style.peek();
        let traits = match selection.adding {
            Some((x, y)) => pdit_core::font_traits_near(selection.page, x, y),
            None => pdit_core::font_traits(selection.page, selection.object_index),
        }
        .ok()
        .flatten();
        let face = traits.as_ref().and_then(|traits| {
            let family = crate::font_catalog::family_for(traits)?;
            let bold = style.map_or(traits.bold, |s| s.bold);
            let italic = style.map_or(traits.italic, |s| s.italic);
            Some((family, bold, italic))
        });
        spawn(async move {
            let look_alike = match face {
                Some((family, bold, italic)) => {
                    match crate::font_catalog::load(family, bold, italic).await {
                        Ok(bytes) => Some((bytes, bold, italic)),
                        Err(error) => {
                            crate::log(&format!("pdit: could not load a look-alike font: {error}"));
                            None
                        }
                    }
                }
                None => None,
            };
            // Apply only if the selection is still the one this was for.
            if self.state.peek().selection.as_ref() == Some(&selection) {
                self.apply_preview(look_alike);
            }
        });
    }

    fn apply_preview(mut self, look_alike: Option<(Rc<Vec<u8>>, bool, bool)>) {
        let state = self.state.peek().clone();
        let Some(selection) = state.selection else {
            return;
        };
        let Some(noto) = self.fallback_font.peek().clone() else {
            crate::log("pdit: the fallback font is still loading; try again in a moment");
            return;
        };
        let fonts = pdit_core::Fonts {
            look_alike: look_alike
                .as_ref()
                .map(|(bytes, bold, italic)| pdit_core::LookAlike {
                    bytes,
                    bold: *bold,
                    italic: *italic,
                }),
            noto_sans: &noto,
        };
        let style = *self.style.peek();
        // A multi-line block re-wraps as one (reflow); a single object edits in
        // place. `None` new index means "keep the anchor" (reflow / restyle / edit).
        let is_reflow = selection.adding.is_none() && selection.object_indices.len() > 1;
        let outcome: Result<(Option<usize>, pdit_core::EditPreview), pdit_core::Error> =
            if is_reflow {
                pdit_core::preview_reflow(
                    selection.page,
                    &selection.object_indices,
                    &state.draft,
                    style.as_ref(),
                    &fonts,
                )
                .map(|preview| (None, preview))
            } else {
                match (selection.adding, style) {
                    (Some((x, y)), style) => pdit_core::preview_add(
                        selection.page,
                        x,
                        y,
                        &state.draft,
                        style.as_ref(),
                        &fonts,
                    )
                    .map(|added| (Some(added.object_index), added.preview)),
                    (None, Some(style)) => pdit_core::preview_styled(
                        selection.page,
                        selection.object_index,
                        &state.draft,
                        &style,
                        &fonts,
                    )
                    .map(|preview| (Some(selection.object_index), preview)),
                    (None, None) => pdit_core::preview_edit(
                        selection.page,
                        selection.object_index,
                        &state.draft,
                        &fonts,
                    )
                    .map(|preview| (Some(selection.object_index), preview)),
                }
            };
        match outcome {
            Ok((object_index, preview)) => {
                self.state.with_mut(|s| {
                    s.mode = BarMode::Result;
                    if let Some(sel) = s.selection.as_mut() {
                        sel.bounds = preview.bounds;
                        if let Some(object_index) = object_index {
                            sel.object_index = object_index;
                        }
                    }
                });
                self.redraw(selection.page);
            }
            Err(pdit_core::Error::UnsupportedCharacters { .. }) => show_error(),
            Err(error) => crate::log(&format!("pdit: {error}")),
        }
    }

    /// Keep: the edit stays; the bar returns to editing, as in the source.
    /// Kept new text becomes an ordinary line that later edits change.
    fn keep(mut self) {
        pdit_core::keep_edit();
        let state = self.state.peek().clone();
        let reflow = state
            .selection
            .as_ref()
            .is_some_and(|s| s.adding.is_none() && s.object_indices.len() > 1);
        if reflow {
            // The block was re-wrapped, so its object list changed. Re-find it at
            // its (invariant) top-left corner to refresh indices, text and bounds.
            let sel = state.selection.unwrap();
            let (ax, ay) = (sel.bounds[0] + 2.0, sel.bounds[3] - 2.0);
            if let Some(fresh) = paragraph_selection(sel.page, ax, ay) {
                self.style
                    .set(pdit_core::text_style(sel.page, fresh.object_index).ok());
                self.state.with_mut(|s| {
                    s.mode = BarMode::Editing;
                    s.draft = fresh.text.clone();
                    s.selection = Some(fresh);
                });
                return;
            }
        }
        self.state.with_mut(|s| {
            s.mode = BarMode::Editing;
            if let Some(sel) = s.selection.as_mut() {
                sel.text = s.draft.clone();
                sel.adding = None;
            }
        });
    }

    /// Before saving (D-020): an edit waiting for Keep or Discard counts as kept.
    pub fn keep_waiting(self) {
        let state = self.state.peek().clone();
        if state.selection.is_some() && state.mode == BarMode::Result {
            self.keep();
        }
    }

    /// Discard: the old text comes back; the bar returns to editing.
    fn discard(mut self) {
        let state = self.state.peek().clone();
        if let Err(error) = pdit_core::discard_edit() {
            crate::log(&format!("pdit: could not discard the edit: {error}"));
        }
        if let Some(selection) = state.selection {
            if let Some((x, y)) = selection.adding {
                // The new text is removed; what was typed stays in the bar.
                self.state.with_mut(|s| {
                    s.mode = BarMode::Editing;
                    if let Some(sel) = s.selection.as_mut() {
                        sel.bounds = [x, y, x, y];
                    }
                });
                self.redraw(selection.page);
                return;
            }
            if selection.object_indices.len() > 1 {
                // A re-wrapped block was restored; re-find it at its top-left.
                let (ax, ay) = (selection.bounds[0] + 2.0, selection.bounds[3] - 2.0);
                if let Some(fresh) = paragraph_selection(selection.page, ax, ay) {
                    self.style
                        .set(pdit_core::text_style(selection.page, fresh.object_index).ok());
                    self.state.with_mut(|s| {
                        s.mode = BarMode::Editing;
                        s.draft = fresh.text.clone();
                        s.selection = Some(fresh);
                    });
                    self.redraw(selection.page);
                    return;
                }
            }
            let original = pdit_core::text_lines(selection.page)
                .ok()
                .and_then(|lines| {
                    lines
                        .into_iter()
                        .find(|l| l.object_index == selection.object_index)
                });
            self.state.with_mut(|s| {
                s.mode = BarMode::Editing;
                if let (Some(sel), Some(line)) = (s.selection.as_mut(), original) {
                    sel.text = line.text.clone();
                    sel.bounds = line.bounds;
                    s.draft = line.text;
                }
            });
            // The original's look comes back too.
            self.style
                .set(pdit_core::text_style(selection.page, selection.object_index).ok());
            self.redraw(selection.page);
        }
    }
}

/// Loads the editing CSS. Rendered once by the app.
#[component]
pub fn EditingStyles() -> Element {
    rsx! {
        document::Stylesheet { href: SELECTION_CSS }
        document::Stylesheet { href: ERROR_CSS }
        document::Stylesheet { href: PANEL_CSS }
        document::Stylesheet { href: IMAGE_SELECT_CSS }
    }
}

/// The selected line's outline on page `page` (D-035), in the page slot's
/// coordinates (`scale` CSS px per PDF point). The editing happens in
/// [`EditPanel`], beside the page.
#[component]
pub fn SelectionOverlay(page: u16, page_width_pt: f32, page_height_pt: f32, scale: f32) -> Element {
    let _ = page_width_pt;
    let editing = use_context::<Editing>();
    let state = editing.state.read().clone();
    let Some(selection) = state.selection.filter(|s| s.page == page) else {
        return rsx! {};
    };
    let [left, bottom, right, top] = selection.bounds;
    // A few px of air around the text, like the preview the user approved.
    let pad = 4.0;
    let x = left * scale - pad;
    let y = (page_height_pt - top) * scale - pad;
    let width = (right - left) * scale + 2.0 * pad;
    let height = (top - bottom) * scale + 2.0 * pad;

    rsx! {
        div { class: "sa-root",
            div {
                class: "sa-highlight pdit-line-outline",
                style: "left: {x}px; top: {y}px; width: {width}px; height: {height}px;",
            }
        }
    }
}

/// Where the edit panel sits (D-035), measured from the page after each change.
#[derive(Clone, Copy, PartialEq, Default)]
struct Place {
    left: f64,
    top: f64,
    /// Narrow window: docked at the bottom of the page area (edit-panel.css).
    docked: bool,
    /// Connector from the line's outline to the text row: x1, y1, x2, y2.
    link: Option<[f64; 4]>,
}

/// From this window width the panel is a column beside the page; below it,
/// it docks at the bottom (edit-panel.css uses the same width).
const WIDE_MIN: f64 = 1181.0;
/// Below the top bar (20 px + 56 px + 20 px, D-031).
const TOP_MIN: f64 = 96.0;
/// Between the page and the panel.
const GAP: f64 = 20.0;

/// The edit panel (D-035): one box with the format bar (a Gooey island,
/// inline) and the Selection Actions text row, beside the page, with a thin
/// connector to the selected line. Always mounted, so the island stays loaded;
/// shown while a line is selected.
#[component]
pub fn EditPanel() -> Element {
    let editing = use_context::<Editing>();
    let place = use_signal(Place::default);
    let state = editing.state.read().clone();
    let open = state.selection.is_some();

    // Re-place after each change of the selection or mode, once it is drawn,
    // and again when the page list has made room (Panel reveal, 400 ms). A new
    // selection also scrolls its line into view above a docked panel.
    use_effect(move || {
        let _ = editing.state.read();
        schedule_place(place, true);
    });
    use_hook(move || {
        let Some(window) = web_sys::window() else {
            return;
        };
        let follow = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            if editing.state.peek().selection.is_some() {
                place_now(place, false);
            }
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_capture(true);
        options.set_passive(true);
        let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
            "scroll",
            follow.as_ref().unchecked_ref(),
            &options,
        );
        let _ = window.add_event_listener_with_callback("resize", follow.as_ref().unchecked_ref());
        // The app lives as long as the page.
        follow.forget();
    });

    let at = place();
    let class = if at.docked {
        "pdit-edit-panel pdit-plus-menu sa-root is-docked"
    } else {
        "pdit-edit-panel pdit-plus-menu sa-root"
    };
    let style = if open && !at.docked {
        format!("left: {}px; top: {}px;", at.left.round(), at.top.round())
    } else {
        String::new()
    };

    rsx! {
        div {
            class,
            "data-open": if open { "true" } else { "false" },
            style,
            div { class: "pdit-edit-box",
                crate::format_bar::FormatBar {}
                if let Some(selection) = state.selection.as_ref() {
                    div { class: "pdit-edit-text",
                        div { class: "sa-anchor t-input-wrap",
                            Bar {
                                mode: state.mode,
                                draft: state.draft.clone(),
                                adding: selection.adding.is_some(),
                                on_resize: move |_| schedule_place(place, false),
                            }
                            p { class: "t-error-msg sa-error-msg", role: "alert", {UNSUPPORTED_MESSAGE} }
                        }
                    }
                }
            }
        }
        if let (true, Some([x1, y1, x2, y2])) = (open, at.link) {
            svg { class: "pdit-edit-link", "aria-hidden": "true",
                line { x1: "{x1}", y1: "{y1}", x2: "{x2}", y2: "{y2}" }
                circle { cx: "{x1}", cy: "{y1}", r: "2.5" }
                circle { cx: "{x2}", cy: "{y2}", r: "2.5" }
            }
        }
    }
}

/// Places the panel on the next frame (after the DOM shows the change) and
/// once more after the page list's 400 ms move.
fn schedule_place(place: Signal<Place>, reveal: bool) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let now = wasm_bindgen::closure::Closure::once_into_js(move || place_now(place, reveal));
    let _ = window.request_animation_frame(now.unchecked_ref());
    set_timeout(&window, 420.0, move || place_now(place, false));
}

fn place_now(mut place: Signal<Place>, reveal: bool) {
    if let Some(next) = measure(reveal)
        && *place.peek() != next
    {
        place.set(next);
    }
}

/// Where the panel goes for the selected line on screen now. `reveal`: scroll
/// the line into view above a docked panel (only for a new selection, never
/// while the user scrolls).
fn measure(reveal: bool) -> Option<Place> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let outline = document.query_selector(".pdit-line-outline").ok()??;
    let page = outline.closest(".page").ok()??;
    let panel = document.query_selector(".pdit-edit-panel").ok()??;
    let text = document.query_selector(".pdit-edit-text").ok()??;
    let view_width = window.inner_width().ok()?.as_f64()?;
    let view_height = window.inner_height().ok()?.as_f64()?;
    let line = outline.get_bounding_client_rect();
    let page = page.get_bounding_client_rect();
    let box_rect = panel.get_bounding_client_rect();
    let row = text.get_bounding_client_rect();
    let middle = line.top() + line.height() / 2.0;

    if view_width < WIDE_MIN {
        if reveal {
            // Where the docked panel's top is (20 px above the window's bottom,
            // edit-panel.css), even if it has not moved there yet.
            let above = view_height - 20.0 - box_rect.height() - 24.0;
            if line.bottom() > above {
                window.scroll_by_with_x_and_y(0.0, line.bottom() - above);
            } else if line.top() < TOP_MIN {
                window.scroll_by_with_x_and_y(0.0, line.top() - TOP_MIN - 24.0);
            }
        }
        return Some(Place {
            docked: true,
            ..Place::default()
        });
    }

    let row_middle = row.top() - box_rect.top() + row.height() / 2.0;
    let left = (page.right() + GAP).min(view_width - box_rect.width() - 16.0);
    let lowest = (view_height - box_rect.height() - 16.0).max(TOP_MIN);
    let top = (middle - row_middle).clamp(TOP_MIN, lowest);
    Some(Place {
        left,
        top,
        docked: false,
        link: Some([line.right(), middle, left, top + row_middle]),
    })
}

#[component]
fn Bar(mode: BarMode, draft: String, adding: bool, on_resize: EventHandler<()>) -> Element {
    let mut editing = use_context::<Editing>();
    rsx! {
        div { class: "sa-bar t-input",
            div { class: "sa-content", key: "{mode:?}",
                match mode {
                    // D-035: a text field that wraps and grows, so a long line
                    // is shown whole; Enter previews, as the source's form did.
                    BarMode::Editing => rsx! {
                        form {
                            class: "sa-form",
                            onsubmit: move |event| {
                                event.prevent_default();
                                editing.preview();
                            },
                            textarea {
                                class: "sa-input",
                                rows: "1",
                                "aria-label": if adding { "New text" } else { "Edit text" },
                                placeholder: if adding { "Type new text…" } else { "" },
                                value: "{draft}",
                                autofocus: true,
                                onmounted: move |event| async move {
                                    grow_text();
                                    on_resize.call(());
                                    let _ = event.data().set_focus(true).await;
                                },
                                oninput: move |event| {
                                    clear_error();
                                    editing.state.with_mut(|s| s.draft = event.value());
                                    grow_text();
                                    on_resize.call(());
                                },
                                onkeydown: move |event| {
                                    if event.key() == Key::Enter {
                                        event.prevent_default();
                                        editing.preview();
                                    }
                                },
                            }
                        }
                        button {
                            r#type: "button",
                            class: "sa-send",
                            "aria-label": "Preview edit",
                            onclick: move |_| editing.preview(),
                            span { dangerous_inner_html: ICON_SEND, style: "display: contents" }
                        }
                    },
                    BarMode::Result => rsx! {
                        button {
                            r#type: "button",
                            class: "sa-primary",
                            onclick: move |_| editing.keep(),
                            span { dangerous_inner_html: ICON_KEEP, style: "display: contents" }
                            "Keep"
                        }
                        button {
                            r#type: "button",
                            class: "sa-control",
                            onclick: move |_| editing.discard(),
                            span { dangerous_inner_html: ICON_DISCARD, style: "display: contents" }
                            "Discard"
                        }
                    },
                }
            }
        }
    }
}

/// Fits the text field's height to its text (D-035).
fn grow_text() {
    let Some(field) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector(".pdit-edit-text textarea").ok().flatten())
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return;
    };
    let style = field.style();
    let _ = style.set_property("height", "auto");
    let _ = style.set_property("height", &format!("{}px", field.scroll_height()));
}

/// Closes the bar on Escape, or on a pointer press outside the bar and outside
/// any page (a press on a page selects a line or closes the bar itself).
pub fn use_close_on_outside(editing: Editing) {
    use_hook(|| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let on_pointer = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(
            move |event: web_sys::Event| {
                let inside = event
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .and_then(|el| {
                        el.closest(".sa-anchor, .page, .pdit-plus-menu, .pdit-edit-panel, .pdit-top-bar, .cm-menu")
                            .ok()
                            .flatten()
                    })
                    .is_some();
                if !inside && editing.state.peek().selection.is_some() {
                    editing.cancel();
                }
            },
        );
        let on_key = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if event.key() == "Escape" && editing.state.peek().selection.is_some() {
                    editing.cancel();
                }
            },
        );
        let _ = window
            .add_event_listener_with_callback("pointerdown", on_pointer.as_ref().unchecked_ref());
        let _ = window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        // The app lives as long as the page.
        on_pointer.forget();
        on_key.forget();
    });
}

thread_local! {
    /// The pending auto-revert timer of the error state.
    static REVERT_TIMER: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}

/// Transitions.dev "Error state shake" orchestration (cli/free/error-state-shake.md),
/// ported: mark the bar and its wrap as errored, replay the shake, and revert
/// after the shake plus --revert-hold.
fn show_error() {
    let Some((window, wrap, bar)) = error_elements() else {
        return;
    };
    let _ = wrap.class_list().add_1("is-error");
    let _ = bar.class_list().add_1("is-error");

    // Replay the shake from a clean baseline.
    let _ = bar.class_list().remove_1("is-shaking");
    let _ = bar
        .dyn_ref::<web_sys::HtmlElement>()
        .map(|el| el.offset_width()); // force reflow
    let _ = bar.class_list().add_1("is-shaking");

    let shake_ms = css_ms("--shake-dur-a", 80.0) * 2.0 + css_ms("--shake-dur-b", 60.0) * 2.0;
    let shaking = bar.clone();
    set_timeout(&window, shake_ms + 20.0, move || {
        let _ = shaking.class_list().remove_1("is-shaking");
    });

    // Auto-revert: hold long enough to read the message.
    cancel_revert(&window);
    let hold = css_ms("--revert-hold", 3000.0);
    let id = set_timeout(&window, shake_ms + hold, || {
        REVERT_TIMER.set(None);
        remove_error_classes();
    });
    REVERT_TIMER.set(id);
}

/// Clears the error state at once (typing, cancel).
fn clear_error() {
    if let Some(window) = web_sys::window() {
        cancel_revert(&window);
    }
    remove_error_classes();
}

fn remove_error_classes() {
    if let Some((_, wrap, bar)) = error_elements() {
        let _ = wrap.class_list().remove_1("is-error");
        let _ = bar.class_list().remove_1("is-error");
    }
}

fn cancel_revert(window: &web_sys::Window) {
    if let Some(id) = REVERT_TIMER.take() {
        window.clear_timeout_with_handle(id);
    }
}

fn error_elements() -> Option<(web_sys::Window, web_sys::Element, web_sys::Element)> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let wrap = document.query_selector(".sa-anchor").ok()??;
    let bar = document.query_selector(".sa-bar").ok()??;
    Some((window, wrap, bar))
}

/// Reads a millisecond value from the root element's CSS custom properties.
fn css_ms(name: &str, fallback: f64) -> f64 {
    web_sys::window()
        .and_then(|w| {
            let root = w.document()?.document_element()?;
            w.get_computed_style(&root).ok().flatten()
        })
        .and_then(|style| style.get_property_value(name).ok())
        .and_then(|value| value.trim().trim_end_matches("ms").parse().ok())
        .unwrap_or(fallback)
}

fn set_timeout(window: &web_sys::Window, ms: f64, f: impl FnOnce() + 'static) -> Option<i32> {
    let callback = wasm_bindgen::closure::Closure::once_into_js(f);
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), ms as i32)
        .ok()
}
