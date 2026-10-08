//! The format bar (D-027): a Gooey island (web/gooey-island, `mountFormatBar`)
//! shown while a text line is selected. It shows the line's style and reports
//! changes, which are previewed like edits (Keep / Discard). Since D-062 it sits
//! inline in the selection toolbar under the centre bar (editing.rs).

use crate::editing::Editing;
use dioxus::prelude::*;
use js_sys::{Array, Function, Object, Reflect};
use pdit_core::{FontChoice, TextStyle};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[component]
pub fn FormatBar() -> Element {
    let editing = use_context::<Editing>();
    // The island's `update` function, once mounted.
    let update = use_hook(|| Rc::new(RefCell::new(None::<Function>)));
    let mut ready = use_signal(|| false);
    // Called from the island (outside Dioxus's own events), so it goes through
    // a Dioxus callback.
    let restyle = use_callback(move |style: TextStyle| editing.restyle(style));

    // Push the selection's style to the island whenever it changes.
    let push = update.clone();
    use_effect(move || {
        let visible = editing.state.read().selection.is_some();
        let style = *editing.style.read();
        let _ = ready();
        if let Some(update) = push.borrow().as_ref() {
            let _ = update.call1(&JsValue::NULL, &state_to_js(visible, style));
        }
    });

    rsx! {
        div {
            // Inline in the selection toolbar (D-062); the island draws no surface.
            class: "pdit-edit-format",
            onmounted: move |event| {
                let update = update.clone();
                async move {
                    let Some(element) = event.data().downcast::<web_sys::Element>().cloned() else {
                        return;
                    };
                    let on_change = Closure::<dyn Fn(JsValue)>::new(move |value: JsValue| {
                        if let Some(style) = style_from_js(&value) {
                            restyle.call(style);
                        }
                    });
                    let options = Object::new();
                    let _ = Reflect::set(&options, &"onChange".into(), on_change.as_ref());
                    let _ = Reflect::set(&options, &"inline".into(), &true.into());
                    // The bar lives as long as the page.
                    on_change.forget();
                    match crate::island::mount_with(&element, "mountFormatBar", &options).await {
                        Ok(controller) => {
                            let function = Reflect::get(&controller, &"update".into())
                                .ok()
                                .and_then(|f| f.dyn_into::<Function>().ok());
                            *update.borrow_mut() = function;
                            ready.set(true);
                        }
                        Err(error) => {
                            crate::log(&format!("pdit: could not load the format bar: {error:?}"))
                        }
                    }
                }
            },
        }
    }
}

fn state_to_js(visible: bool, style: Option<TextStyle>) -> JsValue {
    let state = Object::new();
    let _ = Reflect::set(&state, &"visible".into(), &visible.into());
    let style = style.map_or(JsValue::NULL, |style| {
        let object = Object::new();
        let font = match style.font {
            FontChoice::Document => "Document",
            FontChoice::NotoSans => "NotoSans",
        };
        let color = Array::of3(
            &style.color[0].into(),
            &style.color[1].into(),
            &style.color[2].into(),
        );
        let _ = Reflect::set(&object, &"font".into(), &font.into());
        let _ = Reflect::set(&object, &"size".into(), &style.size.into());
        let _ = Reflect::set(&object, &"color".into(), &color);
        let _ = Reflect::set(&object, &"bold".into(), &style.bold.into());
        let _ = Reflect::set(&object, &"italic".into(), &style.italic.into());
        let _ = Reflect::set(&object, &"underline".into(), &style.underline.into());
        object.into()
    });
    let _ = Reflect::set(&state, &"style".into(), &style);
    state.into()
}

fn style_from_js(value: &JsValue) -> Option<TextStyle> {
    let get = |key: &str| Reflect::get(value, &key.into()).ok();
    let font = match get("font")?.as_string()?.as_str() {
        "NotoSans" => FontChoice::NotoSans,
        _ => FontChoice::Document,
    };
    let color = Array::from(&get("color")?);
    let channel = |i: u32| color.get(i).as_f64().map(|v| v.clamp(0.0, 255.0) as u8);
    Some(TextStyle {
        font,
        size: get("size")?.as_f64()? as f32,
        color: [channel(0)?, channel(1)?, channel(2)?],
        bold: get("bold")?.as_bool()?,
        italic: get("italic")?.as_bool()?,
        underline: get("underline")?.as_bool()?,
    })
}
