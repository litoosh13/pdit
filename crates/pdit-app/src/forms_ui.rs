//! Fill-form UI (D-036): the open document's real form fields, overlaid on the
//! page and filled in place — text fields typed into, checkboxes/radios toggled,
//! combo/list boxes picked from a dropdown. Values go through the engine
//! (`fill_text` / `toggle_choice` / `select_option`), which writes a fresh
//! appearance, and the page is redrawn so the value shows at once.

use crate::editing::Editing;
use dioxus::prelude::*;
use pdit_core::FieldKind;
use wasm_bindgen::JsCast;

const FORM_CSS: Asset = asset!("/assets/css/form-fill.css");
const CONTEXT_MENU_CSS: Asset = asset!("/assets/css/context-menu.css");

/// An open combo/list dropdown: the field and where to place the menu (CSS px).
#[derive(Clone, PartialEq)]
struct ComboOpen {
    page: u16,
    annotation: usize,
    options: Vec<String>,
    x: f64,
    y: f64,
    width: f64,
}

/// Shared fill-form state.
#[derive(Clone, Copy)]
pub struct FormFill {
    /// The text field being edited inline: (page, annotation).
    active: Signal<Option<(u16, usize)>>,
    combo: Signal<Option<ComboOpen>>,
    /// Bumped after any change, so the overlays re-read the fields.
    version: Signal<u32>,
}

impl FormFill {
    pub fn provide() -> Self {
        use_context_provider(|| FormFill {
            active: Signal::new(None),
            combo: Signal::new(None),
            version: Signal::new(0),
        })
    }

    /// Redraws the page (so the new appearance shows) and refreshes the overlays.
    fn refresh(mut self, page: u16) {
        consume_context::<Editing>().redraw(page);
        *self.version.write() += 1;
    }

    fn fill(mut self, page: u16, annotation: usize, text: &str) {
        if let Err(error) = pdit_core::fill_text(page, annotation, text) {
            crate::log(&format!("pdit: could not fill the field: {error}"));
        }
        self.active.set(None);
        self.refresh(page);
    }

    fn toggle(self, page: u16, annotation: usize) {
        if let Err(error) = pdit_core::toggle_choice(page, annotation) {
            crate::log(&format!("pdit: could not toggle the field: {error}"));
        }
        self.refresh(page);
    }

    fn choose(mut self, page: u16, annotation: usize, option: usize) {
        if let Err(error) = pdit_core::select_option(page, annotation, option) {
            crate::log(&format!("pdit: could not choose the option: {error}"));
        }
        self.combo.set(None);
        self.refresh(page);
    }

    /// Starts editing a text field inline (closes any open dropdown).
    fn start_edit(mut self, page: u16, annotation: usize) {
        self.combo.set(None);
        self.active.set(Some((page, annotation)));
    }

    /// Commits the inline text input's current value to the field. Guarded so
    /// the blur that follows Enter (Enter already committed and cleared the
    /// active field) doesn't fire a second, empty commit that wipes the value.
    fn commit_active(self, page: u16, annotation: usize) {
        if *self.active.peek() != Some((page, annotation)) {
            return;
        }
        let text = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| {
                d.query_selector(".pdit-form-field.is-active input.ffinput")
                    .ok()
                    .flatten()
            })
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
            .map(|i| i.value())
            .unwrap_or_default();
        self.fill(page, annotation, &text);
    }

    /// Opens the combo/list dropdown under the field `id`.
    fn open_combo(mut self, page: u16, annotation: usize, options: Vec<String>, id: &str) {
        self.active.set(None);
        let rect = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(id))
            .map(|e| e.get_bounding_client_rect());
        if let Some(r) = rect {
            self.combo.set(Some(ComboOpen {
                page,
                annotation,
                options,
                x: r.left(),
                y: r.bottom() + 4.0,
                width: r.width(),
            }));
        }
    }
}
// OVERLAY_PLACEHOLDER
/// The form fields on `page`, drawn over it and filled in place (D-036). `scale`
/// is CSS px per PDF point. Rendered per page, like the text/image overlays.
#[component]
pub fn FormOverlay(page: u16, page_height_pt: f32, scale: f32) -> Element {
    let form = use_context::<FormFill>();
    let _ = form.version.read(); // re-read the fields after any change
    // While the form is being edited (D-047), clicks place/select fields.
    if *use_context::<crate::form_edit_ui::FormEdit>()
        .on_signal()
        .read()
    {
        return rsx! {};
    }
    let fields = pdit_core::form_fields(page).unwrap_or_default();
    if fields.is_empty() {
        return rsx! {};
    }
    let active = *form.active.read();
    rsx! {
        for field in fields {
            {
                let (l, b, r, t) = field.rect;
                let annotation = field.annotation;
                let style = format!(
                    "left: {}px; top: {}px; width: {}px; height: {}px;",
                    l * scale,
                    (page_height_pt - t) * scale,
                    (r - l) * scale,
                    (t - b) * scale,
                );
                let id = format!("pdit-ff-{page}-{annotation}");
                if field.read_only {
                    rsx! { div { key: "{annotation}", class: "pdit-form-field is-readonly", style } }
                } else {
                    // PDFium renders the widgets (borders, values, checks, the
                    // combo arrow) into the page canvas, so each overlay is just a
                    // tinted, clickable hotspot — except the active text field,
                    // whose opaque input covers the canvas value while editing.
                    match field.kind {
                        FieldKind::Checkbox | FieldKind::Radio => {
                            let cls = if field.kind == FieldKind::Radio {
                                "pdit-form-field ffradio"
                            } else {
                                "pdit-form-field ffcheck"
                            };
                            rsx! {
                                div {
                                    key: "{annotation}", class: "{cls}", style,
                                    onclick: move |e| { e.stop_propagation(); form.toggle(page, annotation); },
                                }
                            }
                        }
                        FieldKind::ComboBox | FieldKind::ListBox => {
                            let options = field.options.clone();
                            rsx! {
                                div {
                                    key: "{annotation}", id: "{id}", class: "pdit-form-field ffcombo", style,
                                    onclick: move |e| { e.stop_propagation(); form.open_combo(page, annotation, options.clone(), &id); },
                                }
                            }
                        }
                        _ => {
                            if active == Some((page, annotation)) {
                                let value = field.value.clone().unwrap_or_default();
                                rsx! {
                                    div { key: "{annotation}", class: "pdit-form-field is-active", style,
                                        input {
                                            class: "ffinput", value: "{value}", autofocus: true,
                                            onmounted: move |e| async move { let _ = e.data().set_focus(true).await; },
                                            onkeydown: move |e| {
                                                if e.key() == Key::Enter { e.prevent_default(); form.commit_active(page, annotation); }
                                            },
                                            onblur: move |_| form.commit_active(page, annotation),
                                        }
                                    }
                                }
                            } else {
                                rsx! {
                                    div {
                                        key: "{annotation}", class: "pdit-form-field", style,
                                        onclick: move |e| { e.stop_propagation(); form.start_edit(page, annotation); },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
// FORMUI_PLACEHOLDER
/// The combo/list dropdown and the fill-form stylesheets. Rendered once by the
/// app. The dropdown is chrome (context-menu.css), so it follows the theme.
#[component]
pub fn FormUi() -> Element {
    let form = use_context::<FormFill>();
    use_close_combo(form);
    let combo = form.combo.read().clone();
    rsx! {
        document::Stylesheet { href: FORM_CSS }
        document::Stylesheet { href: CONTEXT_MENU_CSS }
        if let Some(combo) = combo {
            div {
                class: "cm-menu t-dropdown is-open",
                role: "menu",
                style: "left: {combo.x}px; top: {combo.y}px; min-width: {combo.width}px;",
                for (index, option) in combo.options.iter().enumerate() {
                    button {
                        r#type: "button",
                        role: "menuitem",
                        onclick: move |_| form.choose(combo.page, combo.annotation, index),
                        "{option}"
                    }
                }
            }
        }
    }
}

/// Closes the open dropdown on a press outside it.
fn use_close_combo(form: FormFill) {
    use_hook(|| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let on_pointer = wasm_bindgen::prelude::Closure::<dyn FnMut(web_sys::Event)>::new(
            move |event: web_sys::Event| {
                if form.combo.peek().is_none() {
                    return;
                }
                let inside = event
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .and_then(|el| el.closest(".cm-menu").ok().flatten())
                    .is_some();
                if !inside {
                    let mut combo = form.combo;
                    combo.set(None);
                }
            },
        );
        let _ = window
            .add_event_listener_with_callback("pointerdown", on_pointer.as_ref().unchecked_ref());
        on_pointer.forget();
    });
}
