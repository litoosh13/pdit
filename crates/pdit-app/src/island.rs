//! Hosts the React island bundled from `web/gooey-island` into
//! `assets/islands` (see `docker compose run --rm islands`): the metal top bar
//! (D-031) and the format bar (D-027, format_bar.rs).

use dioxus::prelude::*;
use js_sys::{Function, Object, Promise, Reflect};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

const ISLAND_JS: Asset = asset!("/assets/islands/gooey-island.js");
const ISLAND_CSS: Asset = asset!("/assets/islands/gooey-island.css");

/// The top bar (D-031): the round AI button (D-055), then Open, New, Pages,
/// Light/Dark and Save. The island handles Light/Dark itself.
#[component]
pub fn TopBar(
    has_doc: bool,
    pages_shown: bool,
    ai: crate::ai_ui::AiView,
    on_ai: EventHandler<()>,
    on_ai_pick: EventHandler<String>,
    on_ai_download: EventHandler<()>,
    on_ai_cancel: EventHandler<()>,
    on_open: EventHandler<()>,
    on_new: EventHandler<()>,
    on_save: EventHandler<()>,
    on_toggle_pages: EventHandler<()>,
) -> Element {
    // The island's `update` function, once mounted.
    let update = use_hook(|| Rc::new(RefCell::new(None::<Function>)));
    let mut ready = use_signal(|| false);

    let push = update.clone();
    use_effect(use_reactive!(|(has_doc, pages_shown, ai)| {
        let _ = ready();
        if let Some(update) = push.borrow().as_ref() {
            let state = Object::new();
            let _ = Reflect::set(&state, &"hasDoc".into(), &has_doc.into());
            let _ = Reflect::set(&state, &"pagesShown".into(), &pages_shown.into());
            let _ = Reflect::set(&state, &"ai".into(), &ai_object(&ai));
            let _ = update.call1(&JsValue::NULL, &state);
        }
    }));

    rsx! {
        document::Stylesheet { href: ISLAND_CSS }
        div {
            // Positioned by top-bar.css (it moves over the pages when the
            // pages panel is shown).
            class: "pdit-top-bar-host",
            onmounted: move |event| {
                let update = update.clone();
                async move {
                    let Some(element) = event.data().downcast::<web_sys::Element>().cloned() else {
                        return;
                    };
                    let options = Object::new();
                    for (name, handler) in [
                        ("onAi", on_ai),
                        ("onAiDownload", on_ai_download),
                        ("onAiCancel", on_ai_cancel),
                        ("onOpen", on_open),
                        ("onNew", on_new),
                        ("onSave", on_save),
                        ("onTogglePages", on_toggle_pages),
                    ] {
                        let closure = Closure::<dyn Fn()>::new(move || handler.call(()));
                        let _ = Reflect::set(&options, &name.into(), closure.as_ref());
                        // The bar lives as long as the page.
                        closure.forget();
                    }
                    let pick = Closure::<dyn Fn(JsValue)>::new(move |id: JsValue| {
                        on_ai_pick.call(id.as_string().unwrap_or_default())
                    });
                    let _ = Reflect::set(&options, &"onAiPick".into(), pick.as_ref());
                    pick.forget();
                    match mount_with(&element, "mountTopBar", &options).await {
                        Ok(controller) => {
                            *update.borrow_mut() = Reflect::get(&controller, &"update".into())
                                .ok()
                                .and_then(|f| f.dyn_into::<Function>().ok());
                            ready.set(true);
                        }
                        Err(error) => crate::log(&format!("pdit: could not load the top bar: {error:?}")),
                    }
                }
            },
        }
    }
}

/// The AI menu's state for the island: `{ needsModels, progress: [done,
/// total] | null, error, drops: [{ id, label, disabled, title }] }`.
fn ai_object(ai: &crate::ai_ui::AiView) -> JsValue {
    let o = Object::new();
    let _ = Reflect::set(&o, &"needsModels".into(), &ai.needs_models.into());
    let progress = match ai.progress {
        Some((done, total)) => js_sys::Array::of2(&done.into(), &total.into()).into(),
        None => JsValue::NULL,
    };
    let _ = Reflect::set(&o, &"progress".into(), &progress);
    let error = ai.error.as_deref().map_or(JsValue::NULL, JsValue::from);
    let _ = Reflect::set(&o, &"error".into(), &error);
    let drops = js_sys::Array::new();
    for (id, label, disabled, title) in &ai.drops {
        let d = Object::new();
        let _ = Reflect::set(&d, &"id".into(), &(*id).into());
        let _ = Reflect::set(&d, &"label".into(), &(*label).into());
        let _ = Reflect::set(&d, &"disabled".into(), &(*disabled).into());
        let _ = Reflect::set(&d, &"title".into(), &title.as_str().into());
        drops.push(&d);
    }
    let _ = Reflect::set(&o, &"drops".into(), &drops);
    o.into()
}

/// Loads the island and calls its `export(element, options)`.
pub(crate) async fn mount_with(
    element: &web_sys::Element,
    export: &str,
    options: &Object,
) -> Result<JsValue, JsValue> {
    let import = Function::new_with_args("url", "return import(url)");
    let module = JsFuture::from(Promise::from(
        import.call1(&JsValue::NULL, &ISLAND_JS.to_string().into())?,
    ))
    .await?;
    let mount: Function = Reflect::get(&module, &export.into())?.dyn_into()?;
    mount.call2(&JsValue::NULL, element, options)
}
