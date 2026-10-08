//! Click-to-edit (D-019): the selected text line keeps a grey outline; since
//! the redesign (D-062) its format bar floats under the centre bar and
//! Devigner's MenuDock sits at the line (island `mountSelectionDock`): Edit,
//! Comment, Copy, Delete, More ▸ Add link. Editing is inline (user 2026-10-08):
//! Edit or a double-click puts a text box over the paragraph on the page;
//! Enter or a click outside applies it (one Undo), Esc leaves it unchanged.
//! Look: assets/css/edit-panel.css.

use dioxus::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const SELECTION_CSS: Asset = asset!("/assets/css/selection-actions.css");
const ERROR_CSS: Asset = asset!("/assets/css/error-shake.css");
const PANEL_CSS: Asset = asset!("/assets/css/edit-panel.css");
const IMAGE_SELECT_CSS: Asset = asset!("/assets/css/image-select.css");
const ICON_TEXT_CHANGED: &str = include_str!("../assets/icons/devigner/Pen2.svg");
const UNSUPPORTED: &str = "These characters can't be shown in this document.";

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
    /// Typing in the inline box over the selected text.
    pub typing: Signal<bool>,
}

impl Editing {
    pub fn provide() -> Self {
        use_context_provider(|| Editing {
            state: Signal::new(EditState::default()),
            page_versions: Signal::new(HashMap::new()),
            fallback_font: Signal::new(None),
            style: Signal::new(None),
            typing: Signal::new(false),
        })
    }

    /// Starts typing in the selected text (double-click, Edit, "Edit text").
    pub fn open_edit(mut self) {
        if self.state.peek().selection.is_some() {
            self.typing.set(true);
        }
    }

    /// Enter / a click outside while typing: applies the typed text as one
    /// change with Undo; nothing changed closes the box.
    pub fn commit(mut self) {
        let state = self.state.peek().clone();
        let Some(selection) = state.selection else {
            self.typing.set(false);
            return;
        };
        if state.draft == selection.text {
            self.typing.set(false);
            if selection.adding.is_some() {
                self.cancel();
            }
            return;
        }
        // Desktop: a text-only change to existing text goes to MuPDF (D-065); anything else, or when MuPDF
        // declines, the PDFium edit below.
        let restyled = *self.style.peek()
            != pdit_core::text_style(selection.page, selection.object_index).ok();
        if crate::ai_ui::desktop::available() && selection.adding.is_none() && !restyled {
            spawn(async move {
                if let Err(error) = self.edit_with_mupdf(&selection, &state.draft).await {
                    crate::log(&format!("pdit: MuPDF edit not used: {error}"));
                    self.preview_and_keep();
                }
            });
            return;
        }
        self.preview_and_keep();
    }

    /// Replaces the selected paragraph with `text` in the desktop app's MuPDF (D-065): the document goes to the
    /// desktop side and comes back edited; the page is shown again, with one Undo.
    async fn edit_with_mupdf(self, selection: &Selection, text: &str) -> Result<(), String> {
        let tools = consume_context::<crate::page_tools::PageTools>();
        let before = tools.snapshot().ok_or("no snapshot")?;
        let height = pdit_core::page_ops::page_sizes()
            .map_err(|e| e.to_string())?
            .get(usize::from(selection.page))
            .map_or(842.0, |s| s.1);
        // A point just inside the paragraph's first line, from the page's top-left (MuPDF's page space).
        // ponytail: ignores the page's rotation and a crop box not at the origin.
        let [left, _, _, top] = selection.bounds;
        let encode = |s: &str| String::from(js_sys::encode_uri_component(s));
        let headers = [
            ("x-page", selection.page.to_string()),
            ("x-x", (left + 2.0).to_string()),
            ("x-y", (height - top + 2.0).to_string()),
            ("x-expected", encode(&selection.text)),
            ("x-text", encode(text)),
        ];
        let edited = crate::ai_ui::desktop::invoke_raw("edit_paragraph", &before, &headers)
            .await
            .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
        let bytes = js_sys::Uint8Array::new(&edited).to_vec();
        if bytes.is_empty() {
            return Err("no document came back".into());
        }
        tools.apply_since(before, "Text changed", ICON_TEXT_CHANGED, move || {
            pdit_core::page_ops::restore(bytes).map(|_| ())
        });
        Ok(())
    }

    /// Esc while typing: the box closes, the text stays as it was.
    pub fn stop_typing(mut self) {
        clear_error();
        self.typing.set(false);
        let state = self.state.peek().clone();
        match state.selection {
            Some(sel) if sel.adding.is_some() => self.cancel(),
            Some(sel) => self.state.with_mut(|s| s.draft = sel.text),
            None => {}
        }
    }

    pub fn redraw(mut self, page: u16) {
        *self.page_versions.write().entry(page).or_default() += 1;
    }

    /// Selects the paragraph block at (x, y) PDF points on `page` (the whole
    /// wrapped sentence, reflow), or closes the bar if there is no text there.
    /// Any pending preview is discarded first.
    pub fn select_at(mut self, page: u16, x: f32, y: f32) {
        // The first click outside the box while typing only ends the typing.
        if *self.typing.peek() {
            self.commit();
            return;
        }
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
        self.typing.set(true);
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
        self.typing.set(false);
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

    /// A change from the format bar (D-027): applied right away (with Undo),
    /// like an edit. New text with nothing typed yet only remembers the style.
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
        self.preview_and_keep();
    }

    /// Applies the draft as one change: a snapshot for Undo, the preview,
    /// then Keep and the Undo toast (the centre bar's Undo).
    fn preview_and_keep(self) {
        let before = consume_context::<crate::page_tools::PageTools>().snapshot();
        self.preview(before);
    }

    /// Previews the draft on the page, then keeps it with `undo`. The look-alike of the
    /// line's font (D-028) is loaded first when it isn't yet, so a font that
    /// can't show the text falls back to it rather than to Noto Sans.
    fn preview(self, undo: Option<Rc<Vec<u8>>>) {
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
                self.apply_preview(look_alike, undo);
            }
        });
    }

    fn apply_preview(
        mut self,
        look_alike: Option<(Rc<Vec<u8>>, bool, bool)>,
        undo: Option<Rc<Vec<u8>>>,
    ) {
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
                self.typing.set(false);
                self.keep();
                consume_context::<crate::page_tools::PageTools>().show(
                    "Text changed".to_owned(),
                    ICON_TEXT_CHANGED,
                    undo,
                );
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
/// coordinates (`scale` CSS px per PDF point); while typing, the inline box
/// over the text (white, in the text's size, wrapping at the block's width).
#[component]
pub fn SelectionOverlay(page: u16, page_width_pt: f32, page_height_pt: f32, scale: f32) -> Element {
    let mut editing = use_context::<Editing>();
    let state = editing.state.read().clone();
    let typing = *editing.typing.read();
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

    let size = editing.style.read().map_or(12.0, |s| s.size) * scale;
    // New text has no width yet: up to the page's right margin.
    let width = if selection.adding.is_some() {
        ((page_width_pt - left) * scale - 24.0).max(120.0)
    } else {
        width
    };
    let line = size * 1.2;
    let draft = state.draft.clone();
    // One line stays one line in the PDF: the box grows sideways, no wrapping.
    let one_line = selection.object_indices.len() <= 1;

    rsx! {
        div { class: "sa-root",
            div {
                class: "sa-highlight pdit-line-outline",
                style: "left: {x}px; top: {y}px; width: {width}px; height: {height}px;",
            }
            if typing {
                div {
                    class: "t-input-wrap pdit-inline-wrap",
                    style: "left: {x}px; top: {y}px; width: {width}px;",
                    // Clicks inside the box stay inside (no reselect, no image pick).
                    onclick: move |event| event.stop_propagation(),
                    onpointerdown: move |event| event.stop_propagation(),
                    ondoubleclick: move |event| event.stop_propagation(),
                    textarea {
                        class: "t-input pdit-inline-edit",
                        wrap: if one_line { "off" } else { "soft" },
                        style: "font-size: {size}px; line-height: {line}px; min-height: {height}px;",
                        value: "{draft}",
                        aria_label: "Edit text",
                        onmounted: move |event| {
                            if let Some(el) = event.data().downcast::<web_sys::Element>() {
                                fit_height(el);
                                if let Some(area) = el.dyn_ref::<web_sys::HtmlTextAreaElement>() {
                                    let _ = area.focus();
                                    let end = area.value().len() as u32;
                                    let _ = area.set_selection_range(end, end);
                                }
                            }
                        },
                        oninput: move |event| {
                            clear_error();
                            editing.state.with_mut(|s| s.draft = event.value());
                            if let Some(el) = web_sys::window()
                                .and_then(|w| w.document())
                                .and_then(|d| d.query_selector(".pdit-inline-edit").ok().flatten())
                            {
                                fit_height(&el);
                            }
                        },
                        onkeydown: move |event| {
                            if event.key() == Key::Enter && !event.modifiers().shift() {
                                event.prevent_default();
                                editing.commit();
                            }
                        },
                    }
                    p { class: "t-error-msg sa-error-msg", role: "alert", "{UNSUPPORTED}" }
                }
            }
        }
    }
}

/// Grows the inline box to its text, so nothing scrolls inside it (sideways
/// too when it doesn't wrap).
fn fit_height(el: &web_sys::Element) {
    if let Some(el) = el.dyn_ref::<web_sys::HtmlElement>() {
        let style = el.style();
        let _ = style.set_property("height", "auto");
        let _ = style.set_property("height", &format!("{}px", el.scroll_height()));
        if el.get_attribute("wrap").as_deref() == Some("off") {
            let _ = style.set_property("width", "100%");
            if el.scroll_width() > el.client_width() {
                let _ = style.set_property("width", &format!("{}px", el.scroll_width() + 8));
            }
        }
    }
}

/// The selection's bars (D-062, approved mockup .claude/research/canva-ui/):
/// the format bar (and Link) in a toolbar under the centre bar, and Devigner's
/// MenuDock just above the selected line — the dock grows upward when it opens,
/// so above the line it never covers the text being edited; below the line only
/// when there is no room above. Always mounted, so the island stays loaded.
#[component]
pub fn SelectionUi() -> Element {
    let mut editing = use_context::<Editing>();
    let state = editing.state.read().clone();
    let open = state.selection.is_some();
    let mut at = use_signal(|| None::<(f64, f64, bool)>);
    let update = use_hook(|| Rc::new(RefCell::new(None::<js_sys::Function>)));
    let mut ready = use_signal(|| false);

    // Island callbacks go through Dioxus callbacks (contexts, spawn).
    let on_edit = use_callback(move |_: ()| editing.open_edit());
    let on_delete = use_callback(move |_: ()| {
        editing.state.with_mut(|s| s.draft = String::new());
        editing.preview_and_keep();
    });
    let line_action = move |action: crate::context_menu::Action| {
        let Some(sel) = editing.state.peek().selection.clone() else {
            return;
        };
        let [left, bottom, right, top] = sel.bounds;
        let (x, y) = match action {
            // A note beside the line's end, level with its top.
            crate::context_menu::Action::AddNote => (right + 4.0, top),
            // A point inside the line.
            _ => (left + 1.0, (bottom + top) / 2.0),
        };
        editing.cancel();
        crate::context_menu::run(action, sel.page, x, y);
    };
    let on_comment = use_callback(move |_: ()| line_action(crate::context_menu::Action::AddNote));
    let on_link =
        use_callback(move |_: ()| line_action(crate::context_menu::Action::AddLinkToLine));

    // Follow the line: after each change, and on scroll / resize.
    use_effect(move || {
        let _ = editing.state.read();
        crate::page_tools::next_frame(move || at.set(measure()));
    });
    use_hook(move || {
        let Some(window) = web_sys::window() else {
            return;
        };
        let follow = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            if editing.state.peek().selection.is_some() {
                at.set(measure());
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

    // The dock's state.
    let push = update.clone();
    use_effect(use_reactive!(|state| {
        let _ = ready();
        let Some(update) = push.borrow().clone() else {
            return;
        };
        let o = js_sys::Object::new();
        let set = |k: &str, v: JsValue| {
            let _ = js_sys::Reflect::set(&o, &k.into(), &v);
        };
        if let Some(sel) = state.selection.as_ref() {
            set(
                "selKey",
                format!("{}-{}-{}", sel.page, sel.object_index, sel.adding.is_some()).into(),
            );
            set("text", sel.text.as_str().into());
            set("adding", sel.adding.is_some().into());
        }
        let _ = update.call1(&JsValue::NULL, &o);
    }));

    let dock_style = match at() {
        Some((x, y, above)) if open => {
            if above {
                format!("left: {x:.0}px; bottom: {y:.0}px;")
            } else {
                format!("left: {x:.0}px; top: {y:.0}px;")
            }
        }
        _ => String::new(),
    };
    let link = move |_| line_action(crate::context_menu::Action::AddLinkToLine);
    let adding = state.selection.as_ref().is_some_and(|s| s.adding.is_some());
    rsx! {
        div {
            class: "pdit-sel-toolbar",
            "data-open": if open { "true" } else { "false" },
            crate::format_bar::FormatBar {}
            if !adding {
                span { class: "pdit-sel-sep" }
                button { class: "pdit-sel-chip", r#type: "button", onclick: link, "Link" }
            }
        }
        div {
            class: "pdit-sel-dock",
            "data-open": if open && at().is_some() { "true" } else { "false" },
            style: "{dock_style}",
            onmounted: move |event| {
                let update = update.clone();
                async move {
                    let Some(element) = event.data().downcast::<web_sys::Element>().cloned() else {
                        return;
                    };
                    let options = js_sys::Object::new();
                    let on = |name: &str, f: JsValue| {
                        let _ = js_sys::Reflect::set(&options, &name.into(), &f);
                    };
                    let unit = |cb: Callback<()>| {
                        Closure::<dyn FnMut()>::new(move || cb.call(())).into_js_value()
                    };
                    on("onEdit", unit(on_edit));
                    on("onDelete", unit(on_delete));
                    on("onComment", unit(on_comment));
                    on("onLink", unit(on_link));
                    match crate::island::mount_with(&element, "mountSelectionDock", &options).await {
                        Ok(controller) => {
                            *update.borrow_mut() = js_sys::Reflect::get(&controller, &"update".into())
                                .ok()
                                .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
                            ready.set(true);
                        }
                        Err(error) => crate::log(&format!("pdit: could not load the selection dock: {error:?}")),
                    }
                }
            },
        }
    }
}

/// Where the dock goes for the selected line on screen now: its centre x and,
/// above the line, the distance from the window's bottom to its foot (the dock
/// grows upward); below it (no room above), its top. `None` without an outline.
fn measure() -> Option<(f64, f64, bool)> {
    let window = web_sys::window()?;
    let outline = window
        .document()?
        .query_selector(".pdit-line-outline")
        .ok()??;
    let line = outline.get_bounding_client_rect();
    let view_w = window.inner_width().ok()?.as_f64()?;
    let view_h = window.inner_height().ok()?.as_f64()?;
    let x = (line.left() + line.width() / 2.0).clamp(160.0, view_w - 160.0);
    // Room above: below the centre bar and the toolbar (≈ 110 px), plus the dock.
    if line.top() > 170.0 {
        Some((x, view_h - line.top() + 10.0, true))
    } else {
        Some((x, line.bottom() + 10.0, false))
    }
}

/// Closes the bar on Escape, or on a pointer press outside the bar and outside
/// any page (a press on a page selects a line or closes the bar itself).
pub fn use_close_on_outside(editing: Editing) {
    // Window listeners run outside Dioxus's runtime; applying an edit needs it
    // (contexts, spawn), so they go through callbacks.
    let outside = use_callback(move |_: ()| {
        if *editing.typing.peek() {
            editing.commit();
        } else {
            editing.cancel();
        }
    });
    let escape = use_callback(move |_: ()| {
        if *editing.typing.peek() {
            editing.stop_typing();
        } else {
            editing.cancel();
        }
    });
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
                        el.closest(".sa-anchor, .page, .pdit-plus-menu, .pdit-sel-toolbar, .pdit-sel-dock, .pdit-frame, .cm-menu")
                            .ok()
                            .flatten()
                    })
                    .is_some();
                if !inside && editing.state.peek().selection.is_some() {
                    outside.call(());
                }
            },
        );
        let on_key = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if event.key() == "Escape" && editing.state.peek().selection.is_some() {
                    escape.call(());
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
    let wrap = document.query_selector(".pdit-inline-wrap").ok()??;
    let bar = document.query_selector(".pdit-inline-edit").ok()??;
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
