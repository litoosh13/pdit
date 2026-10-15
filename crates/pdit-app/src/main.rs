//! pdit web app (redesign D-062): the frame (centre bar with Undo · Edit | AI ·
//! Redo, bottom bar), the tools rail, the opened PDF's pages with the pages
//! panel, and the right-click menu over them.

use dioxus::prelude::*;
use std::rc::Rc;
use wasm_bindgen::JsCast;

mod ai_ui;
mod analysis_cache;
mod annotations_ui;
mod ask_ui;
mod bookmarks_ui;
mod compress_ui;
mod context_menu;
mod crop_ui;
mod doc_ui;
mod editing;
mod export_ui;
mod find_fields_ui;
mod font_catalog;
mod fonts;
mod form_edit_ui;
mod format_bar;
mod forms_ui;
mod frame_ui;
mod island;
mod labels_ui;
mod links_ui;
mod page_tools;
mod pages;
mod print_ui;
mod save;
mod search_ui;
#[cfg(any(debug_assertions, feature = "self-check"))]
mod self_check;
mod shapes_ui;
mod signature_ui;
mod split_ui;
mod table_ui;
mod theme;
mod thumbnails;
mod tools_ui;
mod update_ui;
mod zoom_ui;

use context_menu::{ContextMenu, ContextMenuState};
use editing::{Editing, EditingStyles, use_close_on_outside};
use fonts::Fonts;
use forms_ui::{FormFill, FormUi};
use page_tools::{PageTools, PageToolsUi};
use pages::{OpenDocument, PageList};
use shapes_ui::{ShapeDraw, ShapeStyleBar};
use signature_ui::{Signature, SignatureUi};

const PDFIUM_JS: Asset = asset!("/assets/pdfium/pdfium.js");
const PDFIUM_WASM: Asset = asset!("/assets/pdfium/pdfium.wasm");
pub(crate) const FALLBACK_FONT: Asset = asset!("/assets/fonts/NotoSans-Regular.ttf");

/// The "Saved" toast's icon: Devigner Download, as on the Save button.
const ICON_SAVED: &str = include_str!("../assets/icons/devigner/Download.svg");
/// The hidden native file input that "Open" clicks.
const FILE_INPUT_ID: &str = "pdit-open-pdf";

fn main() {
    theme::apply();
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut engine_ready = use_signal(|| false);
    use_context_provider(|| EngineReady(engine_ready));
    let document = use_context_provider(|| Signal::new(None::<OpenDocument>));
    let mut editing = Editing::provide();
    ContextMenuState::provide();
    PageTools::provide();
    pages::PageLocks::provide();
    FormFill::provide();
    ShapeDraw::provide();
    Signature::provide();
    annotations_ui::Annotate::provide();
    links_ui::LinkUi::provide();
    bookmarks_ui::BookmarksUi::provide();
    form_edit_ui::FormEdit::provide();
    zoom_ui::Zoom::provide();
    tools_ui::Tools::provide();
    search_ui::Find::provide();
    print_ui::Print::provide();
    compress_ui::Compress::provide();
    crop_ui::Crop::provide();
    split_ui::SplitFiles::provide();
    labels_ui::PageNumbers::provide();
    export_ui::Export::provide();
    find_fields_ui::FindFields::provide();
    ai_ui::Ai::provide();
    ask_ui::Ask::provide();
    let frame = frame_ui::Frame::provide();
    update_ui::Updates::provide();
    doc_ui::DocTools::provide();
    use_close_on_outside(editing);
    use_close_popups();
    use_hook(install_offline);
    use_hook(move || listen_for_drops(editing, document, engine_ready));
    use_hook(|| {
        spawn(async move {
            match pdit_core::engine::start(&PDFIUM_JS.to_string(), &PDFIUM_WASM.to_string()).await {
                Ok(()) => {
                    engine_ready.set(true);
                    log("pdit: PDF engine ready");
                    // Desktop: a PDF pdit was opened with (or dropped while starting) opens now.
                    if let Some(open) = web_sys::window()
                        .and_then(|w| js_sys::Reflect::get(&w, &"__pditDropped".into()).ok())
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                    {
                        let _ = open.call0(&wasm_bindgen::JsValue::NULL);
                    }
                    match fetch_bytes(&FALLBACK_FONT.to_string()).await {
                        Ok(font) => editing.fallback_font.set(Some(Rc::new(font))),
                        Err(error) => log(&format!(
                            "pdit: could not load the fallback font: {error:?}"
                        )),
                    }
                    #[cfg(any(debug_assertions, feature = "self-check"))]
                    self_check::run(&FALLBACK_FONT.to_string()).await;
                }
                Err(error) => log(&format!("pdit: {error}")),
            }
        })
    });

    rsx! {
        // The online version's install manifest and tab icon (public/; relative: the page sits at the site's base path).
        document::Link { rel: "manifest", href: "manifest.webmanifest" }
        document::Link { rel: "icon", r#type: "image/svg+xml", href: "pdit-icon.svg" }
        Fonts {}
        EditingStyles {}
        frame_ui::FrameBars {}
        input {
            id: FILE_INPUT_ID,
            r#type: "file",
            accept: "application/pdf,.pdf",
            hidden: true,
            onchange: move |event| async move {
                let Some(file) = event.files().into_iter().next() else {
                    return;
                };
                if !engine_ready() {
                    log("pdit: the PDF engine is still starting; try again in a moment");
                    return;
                }
                match file.read_bytes().await {
                    Ok(bytes) => show_document(editing, document, bytes.to_vec(), file.name()),
                    Err(error) => log(&format!("pdit: could not read the file: {error}")),
                }
            },
        }
        PageList {}
        // Without a document the rail holds Open, New and About; AI mode hides it.
        if !frame.is_ai() {
            tools_ui::ToolRail { has_document: document.read().is_some() }
        }
        ContextMenu {}
        PageToolsUi {}
        FormUi {}
        ShapeStyleBar {}
        SignatureUi {}
        annotations_ui::AnnotationKeys {}
        annotations_ui::CommentsPanel {}
        bookmarks_ui::BookmarksPanel {}
        form_edit_ui::FormEditBar {}
        zoom_ui::ZoomKeys {}
        search_ui::FindCard {}
        print_ui::PrintUi {}
        compress_ui::CompressUi {}
        crop_ui::CropUi {}
        split_ui::SplitUi {}
        labels_ui::LabelsUi {}
        export_ui::ExportUi {}
        update_ui::UpdateUi {}
        ask_ui::ModelsPrompt {}
        find_fields_ui::FindFieldsBar {}
        if document.read().is_some() {
            ai_ui::AiUi {}
            ask_ui::AskUi {}
        }
        doc_ui::DocWindow {}
        editing::SelectionUi {}
    }
}

/// Opens `bytes` as the document on screen, named `name`.
fn show_document(
    mut editing: Editing,
    mut document: Signal<Option<OpenDocument>>,
    bytes: Vec<u8>,
    name: String,
) {
    ai_ui::opened_file(&bytes);
    match pdit_core::open_document(bytes) {
        Ok(page_sizes) => {
            editing.state.set(Default::default());
            // Only counts are logged: document contents stay out of the console.
            log(&format!(
                "pdit: opened a PDF with {} page(s)",
                page_sizes.len()
            ));
            let id = document.peek().as_ref().map_or(0, |d| d.id + 1);
            document.set(Some(OpenDocument {
                id,
                name,
                page_sizes,
            }));
            // A new document starts at its first page, below the centre bar, not
            // at the scroll position of the one before.
            if let Some(window) = web_sys::window() {
                window.scroll_to_with_x_and_y(0.0, 0.0);
            }
        }
        Err(error) => log(&format!("pdit: could not open the PDF: {error}")),
    }
}

/// The PDF engine has started (Open and New wait for it).
#[derive(Clone, Copy)]
struct EngineReady(Signal<bool>);

/// Open (rail): the system's file picker.
/// Open. Desktop: the system's open window, so pdit knows the file and Save writes back to it (P-050 #10).
pub(crate) fn open_file() {
    if !ai_ui::desktop::available() {
        return click_file_input();
    }
    let (editing, document, ready) = (
        consume_context::<Editing>(),
        consume_context::<Signal<Option<OpenDocument>>>(),
        consume_context::<EngineReady>().0,
    );
    spawn(async move {
        let name = ai_ui::desktop::invoke("open_pdf", &js_sys::Object::new())
            .await
            .ok()
            .and_then(|v| v.as_string());
        let Some(name) = name else { return };
        match ai_ui::desktop::invoke("read_opened", &js_sys::Object::new()).await {
            Ok(buffer) if *ready.peek() => show_document(
                editing,
                document,
                js_sys::Uint8Array::new(&buffer).to_vec(),
                name,
            ),
            Ok(_) => log("pdit: the PDF engine is still starting; try again in a moment"),
            Err(error) => log(&format!("pdit: could not read the file: {error:?}")),
        }
    });
}

/// New (rail): a blank A4 document.
pub(crate) fn new_document() {
    if !*consume_context::<EngineReady>().0.peek() {
        return log("pdit: the PDF engine is still starting; try again in a moment");
    }
    let bytes = pdit_core::page_ops::blank_document();
    // A new document has no file yet: its first Save asks where (desktop).
    if ai_ui::desktop::available() {
        spawn(async {
            let _ = ai_ui::desktop::invoke("forget_opened", &js_sys::Object::new()).await;
        });
    }
    show_document(
        consume_context(),
        consume_context(),
        bytes,
        "Untitled.pdf".into(),
    );
}

/// Save (bottom bar).
pub(crate) fn save_current() {
    save_open_document(consume_context(), consume_context());
}

/// Save (D-020): keeps an edit that is waiting for Keep/Discard, then hands
/// the document to the user as `<name>-edited.pdf`.
fn save_open_document(editing: Editing, document: Signal<Option<OpenDocument>>) {
    let Some(name) = document.peek().as_ref().map(|d| save::edited_name(&d.name)) else {
        return;
    };
    editing.keep_waiting();
    let bytes = match pdit_core::save_document() {
        Ok(bytes) => bytes,
        Err(error) => return log(&format!("pdit: could not save the PDF: {error}")),
    };
    ai_ui::saved_file(&bytes);
    if ai_ui::desktop::available() {
        // Desktop: into the open file itself (P-050 #10); a new document asks where once.
        let tools = consume_context::<page_tools::PageTools>();
        let base = document
            .peek()
            .as_ref()
            .map(|d| d.name.clone())
            .unwrap_or_default();
        spawn(async move {
            let header = [("x-name", String::from(js_sys::encode_uri_component(&base)))];
            match ai_ui::desktop::invoke_raw("save_opened", &bytes, &header).await {
                Ok(saved) => match saved.as_string() {
                    Some(file) => {
                        log(&format!("pdit: saved a PDF of {} bytes", bytes.len()));
                        tools.show(format!("Saved to {file}"), ICON_SAVED, None);
                    }
                    None => log("pdit: saving was cancelled"),
                },
                Err(error) => log(&format!("pdit: could not save the PDF: {error:?}")),
            }
        });
        return;
    }
    spawn(async move {
        // Only the size is logged: document contents stay out of the console.
        match save::save_file(&bytes, &name).await {
            Ok(true) => log(&format!("pdit: saved a PDF of {} bytes", bytes.len())),
            Ok(false) => log("pdit: saving was cancelled"),
            Err(error) => log(&format!("pdit: could not save the PDF: {error:?}")),
        }
    });
}

pub(crate) async fn fetch_bytes(url: &str) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response =
        wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url))
            .await?
            .dyn_into()?;
    let buffer = wasm_bindgen_futures::JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

fn click_file_input() {
    let input = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(FILE_INPUT_ID))
        .and_then(|element| element.dyn_into::<web_sys::HtmlInputElement>().ok());
    if let Some(input) = input {
        input.set_value(""); // so choosing the same file again still fires `change`
        input.click();
    }
}

/// A press anywhere outside a pop-up closes it (P-050 #7, #8): the Find card, the rail's docked panel, the
/// Comments and Bookmarks panels, and the toast. The rail (whose buttons toggle them) and the right-click menu
/// (whose items may open one) don't count as outside. Windows with Cancel (Print, Crop, …) keep it and Escape.
fn use_close_popups() {
    let close = use_callback(move |outside: [bool; 4]| {
        let [find, dock, panels, toast] = outside;
        if find {
            consume_context::<search_ui::Find>().close_card();
        }
        if dock {
            consume_context::<tools_ui::Tools>().close_panel();
        }
        if panels {
            consume_context::<annotations_ui::Annotate>().close_panel();
            consume_context::<bookmarks_ui::BookmarksUi>().close_panel();
        }
        if toast {
            consume_context::<PageTools>().hide_toast();
        }
    });
    use_hook(move || {
        let Some(window) = web_sys::window() else {
            return;
        };
        let on_press = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(
            move |event: web_sys::Event| {
                let Some(el) = event
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                else {
                    return;
                };
                let inside = |selector: &str| el.closest(selector).ok().flatten().is_some();
                if inside(".cm-menu") {
                    return;
                }
                close.call([
                    !inside(".pdit-find, .pdit-rail"),
                    !inside(".pdit-dock, .pdit-rail"),
                    !inside(".pdit-comments, .pdit-bookmarks, .pdit-rail"),
                    !inside(".pdit-toast"),
                ]);
            },
        );
        let _ = window.add_event_listener_with_callback_and_bool(
            "pointerdown",
            on_press.as_ref().unchecked_ref(),
            true,
        );
        on_press.forget();
    });
}

/// A PDF dragged onto pdit opens (P-050 #1). Online: the browser's drop event. Desktop: the window catches the
/// drop (and keeps the file's path) and calls `window.__pditDropped`, which fetches the file.
fn listen_for_drops(
    editing: Editing,
    document: Signal<Option<OpenDocument>>,
    engine_ready: Signal<bool>,
) {
    use js_sys::{Promise, Reflect};
    use wasm_bindgen::closure::Closure;
    let Some(window) = web_sys::window() else {
        return;
    };
    let open = move |bytes: Vec<u8>, name: String| {
        if engine_ready() {
            show_document(editing, document, bytes, name);
        } else {
            log("pdit: the PDF engine is still starting; try again in a moment");
        }
    };
    if ai_ui::desktop::available() {
        let dropped = Closure::<dyn FnMut()>::new(move || {
            wasm_bindgen_futures::spawn_local(async move {
                let name = ai_ui::desktop::invoke("take_dropped", &js_sys::Object::new())
                    .await
                    .ok()
                    .and_then(|v| v.as_string());
                let Some(name) = name else { return };
                match ai_ui::desktop::invoke("read_opened", &js_sys::Object::new()).await {
                    Ok(buffer) => open(js_sys::Uint8Array::new(&buffer).to_vec(), name),
                    Err(error) => log(&format!("pdit: could not read the dropped file: {error:?}")),
                }
            });
        });
        let _ = Reflect::set(&window, &"__pditDropped".into(), dropped.as_ref());
        dropped.forget();
        return;
    }
    // Online: allow dropping anywhere, then open the first PDF dropped.
    let over =
        Closure::<dyn FnMut(web_sys::Event)>::new(|event: web_sys::Event| event.prevent_default());
    let drop = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        event.prevent_default();
        let file = Reflect::get(&event, &"dataTransfer".into())
            .and_then(|t| Reflect::get(&t, &"files".into()))
            .and_then(|f| Reflect::get(&f, &0.into()))
            .ok()
            .filter(|f| !f.is_undefined());
        let Some(file) = file else { return };
        let name = Reflect::get(&file, &"name".into())
            .ok()
            .and_then(|n| n.as_string())
            .unwrap_or_default();
        if !name.to_lowercase().ends_with(".pdf") {
            return;
        }
        let read = Reflect::get(&file, &"arrayBuffer".into())
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
            .and_then(|f| f.call0(&file).ok())
            .and_then(|p| p.dyn_into::<Promise>().ok());
        let Some(read) = read else { return };
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(read).await {
                Ok(buffer) => open(js_sys::Uint8Array::new(&buffer).to_vec(), name),
                Err(error) => log(&format!("pdit: could not read the dropped file: {error:?}")),
            }
        });
    });
    let _ = window.add_event_listener_with_callback("dragover", over.as_ref().unchecked_ref());
    let _ = window.add_event_listener_with_callback("drop", drop.as_ref().unchecked_ref());
    over.forget();
    drop.forget();
}

/// The online version keeps its files for offline use (public/sw.js, M-007). Not in the desktop app (its files are
/// on disk) or debug builds (dx serve would keep serving old files).
fn install_offline() {
    if cfg!(debug_assertions) || ai_ui::desktop::internals().is_some() {
        return;
    }
    let register = || -> Result<(), wasm_bindgen::JsValue> {
        use js_sys::Reflect;
        let window = web_sys::window().ok_or("no window")?;
        let workers = Reflect::get(
            &Reflect::get(&window, &"navigator".into())?,
            &"serviceWorker".into(),
        )?;
        if workers.is_undefined() {
            return Ok(());
        }
        let register: js_sys::Function = Reflect::get(&workers, &"register".into())?.dyn_into()?;
        let _ = register.call1(&workers, &"sw.js".into())?;
        Ok(())
    };
    if let Err(error) = register() {
        log(&format!("pdit: offline use is not available: {error:?}"));
    }
}

pub(crate) fn log(message: &str) {
    web_sys::console::log_1(&message.into());
    #[cfg(any(debug_assertions, feature = "self-check"))]
    ai_ui::desktop::log(message);
}
