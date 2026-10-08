//! Starting PDFium in the browser.
//!
//! PDFium runs as its own Emscripten WebAssembly module. [`start`] loads its
//! JavaScript loader, instantiates it (pointing it at the `.wasm` file's URL), and
//! binds it to `pdfium-render` inside our own module.

use crate::Error;
use js_sys::{Function, Object, Promise, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

fn js_error(value: JsValue) -> Error {
    Error::EngineStart(value.as_string().unwrap_or_else(|| format!("{value:?}")))
}

/// Loads PDFium from the given URLs and connects it to pdfium-render.
/// Call once, before any other pdit-core function.
pub async fn start(pdfium_js_url: &str, pdfium_wasm_url: &str) -> Result<(), Error> {
    // pdfium-render reports a failed PDFium call (and its name) through `log`
    // just before it panics; without a logger that line is lost (P-043).
    let _ = log::set_logger(&ConsoleLogger);
    log::set_max_level(log::LevelFilter::Warn);
    load_script(pdfium_js_url).await?;

    let factory: Function = Reflect::get(&js_sys::global(), &"PDFiumModule".into())
        .map_err(js_error)?
        .dyn_into()
        .map_err(|_| Error::EngineStart("PDFiumModule() not found in pdfium.js".into()))?;

    // Emscripten looks for pdfium.wasm next to pdfium.js by default; our bundler
    // renames assets, so tell it the real URL.
    let wasm_url = pdfium_wasm_url.to_owned();
    let locate_file =
        Closure::<dyn Fn(String) -> String>::new(move |_name: String| wasm_url.clone());
    let options = Object::new();
    Reflect::set(&options, &"locateFile".into(), locate_file.as_ref()).map_err(js_error)?;
    locate_file.forget(); // PDFium may call it again later; keep it alive for the page's lifetime.

    let promise: Promise = factory
        .call1(&JsValue::NULL, &options)
        .map_err(js_error)?
        .into();
    let pdfium_module = JsFuture::from(promise).await.map_err(js_error)?;

    // pdfium-render only exposes its initializer through our module's JavaScript
    // glue, so call it there, exactly as its own browser example does.
    let initialize = glue_function("initialize_pdfium_render").await?;
    let bound = initialize
        .call3(
            &JsValue::NULL,
            &pdfium_module,
            &wasm_bindgen::exports(),
            &JsValue::FALSE,
        )
        .map_err(js_error)?;
    if bound != JsValue::TRUE {
        return Err(Error::EngineStart(
            "pdfium-render could not bind to PDFium".into(),
        ));
    }
    let _ = crate::pdfium();
    Ok(())
}

/// Finds a function exported by our own module's wasm-bindgen JavaScript glue.
/// The glue is one of the page's module scripts; importing it again returns the
/// already-running instance rather than loading it twice.
async fn glue_function(name: &str) -> Result<Function, Error> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| Error::EngineStart("no document".into()))?;
    let scripts = document
        .query_selector_all("script[type=module][src]")
        .map_err(js_error)?;
    let import = Function::new_with_args("url", "return import(url)");
    for i in 0..scripts.length() {
        let Some(script) = scripts
            .get(i)
            .and_then(|n| n.dyn_into::<web_sys::HtmlScriptElement>().ok())
        else {
            continue;
        };
        let Ok(promise) = import.call1(&JsValue::NULL, &script.src().into()) else {
            continue;
        };
        let Ok(namespace) = JsFuture::from(Promise::from(promise)).await else {
            continue;
        };
        if let Ok(function) = Reflect::get(&namespace, &name.into())
            && let Ok(function) = function.dyn_into::<Function>()
        {
            return Ok(function);
        }
    }
    Err(Error::EngineStart(format!(
        "{name} not found in the app's JavaScript glue"
    )))
}

async fn load_script(url: &str) -> Result<(), Error> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| Error::EngineStart("no document".into()))?;
    let script: web_sys::HtmlScriptElement = document
        .create_element("script")
        .map_err(js_error)?
        .unchecked_into();
    script.set_src(url);
    let loaded = Promise::new(&mut |resolve, reject| {
        script.set_onload(Some(&resolve));
        script.set_onerror(Some(&reject));
    });
    document
        .head()
        .ok_or_else(|| Error::EngineStart("no <head>".into()))?
        .append_child(&script)
        .map_err(js_error)?;
    JsFuture::from(loaded)
        .await
        .map(|_| ())
        .map_err(|_| Error::EngineStart(format!("could not load {url}")))
}

/// Prints `log` warnings and errors (pdfium-render's) to the browser console.
struct ConsoleLogger;

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            web_sys::console::error_1(&format!("{}: {}", record.level(), record.args()).into());
        }
    }

    fn flush(&self) {}
}
