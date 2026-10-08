//! Hosts the React island bundled from `web/gooey-island` into
//! `assets/islands` (see `docker compose run --rm islands`): the frame (D-062,
//! frame_ui.rs) and the format bar (D-027, format_bar.rs).

use dioxus::prelude::*;
use js_sys::{Function, Object, Promise, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

const ISLAND_JS: Asset = asset!("/assets/islands/gooey-island.js");
pub(crate) const ISLAND_CSS: Asset = asset!("/assets/islands/gooey-island.css");

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
