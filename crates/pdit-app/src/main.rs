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
mod context_menu;
mod doc_ui;
mod editing;
mod find_fields_ui;
mod font_catalog;
mod fonts;
mod form_edit_ui;
mod format_bar;
mod forms_ui;
mod frame_ui;
mod island;
mod links_ui;
mod page_tools;
mod pages;
mod print_ui;
mod save;
mod search_ui;
#[cfg(debug_assertions)]
mod self_check;
mod shapes_ui;
mod signature_ui;
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
    find_fields_ui::FindFields::provide();
    ai_ui::Ai::provide();
    ask_ui::Ask::provide();
    let frame = frame_ui::Frame::provide();
    update_ui::Updates::provide();
    doc_ui::DocTools::provide();
    use_close_on_outside(editing);
    use_hook(|| {
        spawn(async move {
            match pdit_core::engine::start(&PDFIUM_JS.to_string(), &PDFIUM_WASM.to_string()).await {
                Ok(()) => {
                    engine_ready.set(true);
                    log("pdit: PDF engine ready");
                    match fetch_bytes(&FALLBACK_FONT.to_string()).await {
                        Ok(font) => editing.fallback_font.set(Some(Rc::new(font))),
                        Err(error) => log(&format!(
                            "pdit: could not load the fallback font: {error:?}"
                        )),
                    }
                    #[cfg(debug_assertions)]
                    self_check::run(&FALLBACK_FONT.to_string()).await;
                }
                Err(error) => log(&format!("pdit: {error}")),
            }
        })
    });

    rsx! {
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
        update_ui::UpdateUi {}
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
pub(crate) fn open_file() {
    click_file_input();
}

/// New (rail): a blank A4 document.
pub(crate) fn new_document() {
    if !*consume_context::<EngineReady>().0.peek() {
        return log("pdit: the PDF engine is still starting; try again in a moment");
    }
    let bytes = pdit_core::page_ops::blank_document();
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
    spawn(async move {
        // Only the size is logged: document contents stay out of the console.
        match save::save_pdf(&bytes, &name).await {
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

pub(crate) fn log(message: &str) {
    web_sys::console::log_1(&message.into());
    #[cfg(debug_assertions)]
    ai_ui::desktop::log(message);
}
