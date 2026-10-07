//! Saving the edited PDF (D-020): the browser's "Save as" dialog where it
//! exists (Chrome, Edge), otherwise a normal download.

use js_sys::{Array, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// `lease.pdf` → `lease-edited.pdf`.
pub fn edited_name(original: &str) -> String {
    derived_name(original, "edited")
}

/// `lease.pdf` with `suffix` → `lease-<suffix>.pdf`.
pub fn derived_name(original: &str, suffix: &str) -> String {
    let stem = match original.len().checked_sub(4) {
        Some(cut)
            if original.is_char_boundary(cut) && original[cut..].eq_ignore_ascii_case(".pdf") =>
        {
            &original[..cut]
        }
        _ => original,
    };
    let stem = if stem.trim().is_empty() {
        "document"
    } else {
        stem
    };
    format!("{stem}-{suffix}.pdf")
}

/// Hands `bytes` to the user as `name`. Returns false when the user cancelled
/// the dialog, which is not an error.
pub async fn save_pdf(bytes: &[u8], name: &str) -> Result<bool, JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let data = Uint8Array::from(bytes);
    let picker = Reflect::get(&window, &"showSaveFilePicker".into())?;
    match picker.dyn_into::<js_sys::Function>() {
        Ok(picker) => save_with_picker(&window, &picker, &data, name).await,
        Err(_) => download(&window, &data, name).map(|()| true),
    }
}

async fn save_with_picker(
    window: &web_sys::Window,
    picker: &js_sys::Function,
    data: &Uint8Array,
    name: &str,
) -> Result<bool, JsValue> {
    let accept = Object::new();
    Reflect::set(
        &accept,
        &"application/pdf".into(),
        &Array::of1(&".pdf".into()),
    )?;
    let pdf_type = Object::new();
    Reflect::set(&pdf_type, &"description".into(), &"PDF document".into())?;
    Reflect::set(&pdf_type, &"accept".into(), &accept)?;
    let options = Object::new();
    Reflect::set(&options, &"suggestedName".into(), &name.into())?;
    Reflect::set(&options, &"types".into(), &Array::of1(&pdf_type))?;

    let handle = match JsFuture::from(Promise::from(picker.call1(window, &options)?)).await {
        Ok(handle) => handle,
        Err(error) if error_name(&error) == "AbortError" => return Ok(false),
        Err(error) => return Err(error),
    };
    let writable = JsFuture::from(Promise::from(call0(&handle, "createWritable")?)).await?;
    JsFuture::from(Promise::from(call1(&writable, "write", data)?)).await?;
    JsFuture::from(Promise::from(call0(&writable, "close")?)).await?;
    Ok(true)
}

/// Firefox and Safari: a download to the browser's download folder.
fn download(window: &web_sys::Window, data: &Uint8Array, name: &str) -> Result<(), JsValue> {
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/pdf");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&Array::of1(data), &options)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let document = window.document().ok_or("no document")?;
    let link: web_sys::HtmlAnchorElement = document.create_element("a")?.dyn_into()?;
    link.set_href(&url);
    link.set_download(name);
    link.click();
    // Freed after the browser has started the download.
    let callback = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    window.set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 1000)?;
    Ok(())
}

fn call0(target: &JsValue, method: &str) -> Result<JsValue, JsValue> {
    Reflect::get(target, &method.into())?
        .dyn_into::<js_sys::Function>()?
        .call0(target)
}

fn call1(target: &JsValue, method: &str, arg: &JsValue) -> Result<JsValue, JsValue> {
    Reflect::get(target, &method.into())?
        .dyn_into::<js_sys::Function>()?
        .call1(target, arg)
}

fn error_name(error: &JsValue) -> String {
    Reflect::get(error, &"name".into())
        .ok()
        .and_then(|n| n.as_string())
        .unwrap_or_default()
}
