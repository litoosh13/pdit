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

/// A file's media type and description, from its name's extension (PDF unless known otherwise).
fn kind(name: &str) -> (&'static str, &'static str, &'static str) {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => ("image/png", "PNG picture", ".png"),
        "jpg" | "jpeg" => ("image/jpeg", "JPEG picture", ".jpg"),
        "txt" => ("text/plain", "Text", ".txt"),
        "docx" => (
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "Word document",
            ".docx",
        ),
        "odt" => (
            "application/vnd.oasis.opendocument.text",
            "OpenDocument text",
            ".odt",
        ),
        _ => ("application/pdf", "PDF document", ".pdf"),
    }
}

/// Hands `bytes` to the user as `name` (its type from the extension). Returns
/// false when the user cancelled the dialog, which is not an error.
pub async fn save_file(bytes: &[u8], name: &str) -> Result<bool, JsValue> {
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
    let (mime, description, ext) = kind(name);
    let accept = Object::new();
    Reflect::set(&accept, &mime.into(), &Array::of1(&ext.into()))?;
    let pdf_type = Object::new();
    Reflect::set(&pdf_type, &"description".into(), &description.into())?;
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
    options.set_type(kind(name).0);
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

/// Where Split into files (D-070) puts its files: picked once, then filled.
pub enum Folder {
    /// The desktop app's picked folder (desktop/src/files.rs), by name.
    Desktop(String),
    /// A folder from the browser's showDirectoryPicker (Chrome, Edge), and its name.
    Browser(JsValue, String),
    /// No folder picker: one download per file.
    Downloads,
}

impl Folder {
    /// Asks the user for a folder; None when they cancelled.
    pub async fn pick() -> Result<Option<Folder>, JsValue> {
        if crate::ai_ui::desktop::available() {
            let name = crate::ai_ui::desktop::invoke("pick_folder", &Object::new()).await?;
            return Ok(name.as_string().map(Folder::Desktop));
        }
        let window = web_sys::window().ok_or("no window")?;
        let Ok(picker) =
            Reflect::get(&window, &"showDirectoryPicker".into())?.dyn_into::<js_sys::Function>()
        else {
            return Ok(Some(Folder::Downloads));
        };
        let options = Object::new();
        Reflect::set(&options, &"mode".into(), &"readwrite".into())?;
        match JsFuture::from(Promise::from(picker.call1(&window, &options)?)).await {
            Ok(dir) => {
                let name = Reflect::get(&dir, &"name".into())?
                    .as_string()
                    .unwrap_or_default();
                Ok(Some(Folder::Browser(dir, name)))
            }
            Err(error) if error_name(&error) == "AbortError" => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The folder's name for messages ("Downloads" for downloads).
    pub fn name(&self) -> &str {
        match self {
            Folder::Desktop(name) | Folder::Browser(_, name) => name,
            Folder::Downloads => "Downloads",
        }
    }

    /// Writes `bytes` as `name` (or "name (2).pdf" when taken), returning the name used.
    pub async fn save(&self, bytes: &[u8], name: &str) -> Result<String, JsValue> {
        match self {
            Folder::Desktop(_) => {
                let encoded = String::from(js_sys::encode_uri_component(name));
                let used = crate::ai_ui::desktop::invoke_raw(
                    "save_into_folder",
                    bytes,
                    &[("x-name", encoded)],
                )
                .await?;
                Ok(used.as_string().unwrap_or_else(|| name.to_owned()))
            }
            Folder::Browser(dir, _) => {
                let (stem, ext) = name.rsplit_once('.').unwrap_or((name, "pdf"));
                let mut used = name.to_owned();
                for k in 2..1000 {
                    // Taken when the folder already has a file by that name.
                    if JsFuture::from(Promise::from(call1(
                        dir,
                        "getFileHandle",
                        &used.as_str().into(),
                    )?))
                    .await
                    .is_err()
                    {
                        break;
                    }
                    used = format!("{stem} ({k}).{ext}");
                }
                let options = Object::new();
                Reflect::set(&options, &"create".into(), &true.into())?;
                let get =
                    Reflect::get(dir, &"getFileHandle".into())?.dyn_into::<js_sys::Function>()?;
                let handle = JsFuture::from(Promise::from(get.call2(
                    dir,
                    &used.as_str().into(),
                    &options,
                )?))
                .await?;
                let writable =
                    JsFuture::from(Promise::from(call0(&handle, "createWritable")?)).await?;
                JsFuture::from(Promise::from(call1(
                    &writable,
                    "write",
                    &Uint8Array::from(bytes),
                )?))
                .await?;
                JsFuture::from(Promise::from(call0(&writable, "close")?)).await?;
                Ok(used)
            }
            Folder::Downloads => {
                let window = web_sys::window().ok_or("no window")?;
                download(&window, &Uint8Array::from(bytes), name)?;
                Ok(name.to_owned())
            }
        }
    }
}
