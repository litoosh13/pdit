//! pdit web app. Only approved UI pieces are shown: the metal top bar (D-031:
//! Open, New, Pages, Light/Dark, Save, and a round placeholder button), the opened
//! PDF's pages with the pages panel, and the right-click menu over them.

use dioxus::prelude::*;
use std::rc::Rc;
use wasm_bindgen::JsCast;

mod ai_ui;
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
mod zoom_ui;

use context_menu::{ContextMenu, ContextMenuState};
use editing::{EditPanel, Editing, EditingStyles, use_close_on_outside};
use fonts::Fonts;
use forms_ui::{FormFill, FormUi};
use island::TopBar;
use page_tools::{PageTools, PageToolsUi};
use pages::{OpenDocument, PageList};
use shapes_ui::{ShapeDraw, ShapeStyleBar};
use signature_ui::{Signature, SignatureUi};
use thumbnails::ThumbnailsPanel;

const PDFIUM_JS: Asset = asset!("/assets/pdfium/pdfium.js");
const PDFIUM_WASM: Asset = asset!("/assets/pdfium/pdfium.wasm");
pub(crate) const FALLBACK_FONT: Asset = asset!("/assets/fonts/NotoSans-Regular.ttf");

/// The hidden native file input that "Open" clicks.
const FILE_INPUT_ID: &str = "pdit-open-pdf";

fn main() {
    theme::apply_saved();
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut engine_ready = use_signal(|| false);
    let document = use_context_provider(|| Signal::new(None::<OpenDocument>));
    // The pages panel is shown by default; the bar's Pages hides it (D-031).
    let mut pages_panel_open = use_signal(|| true);
    // Shared, so the bookmarks list can take the Pages panel's place (D-043).
    use_context_provider(|| bookmarks_ui::PagesPanel(pages_panel_open));
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
    let ai = ai_ui::Ai::provide();
    ask_ui::Ask::provide();
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
        TopBar {
            has_doc: document.read().is_some(),
            pages_shown: pages_panel_open(),
            on_ai: move |_| ai.toggle_menu(),
            on_open: move |_| click_file_input(),
            on_new: move |_| {
                if !engine_ready() {
                    return log("pdit: the PDF engine is still starting; try again in a moment");
                }
                let bytes = pdit_core::page_ops::blank_document();
                show_document(editing, document, bytes, "Untitled.pdf".into());
            },
            on_save: move |_| save_open_document(editing, document),
            on_toggle_pages: move |_| pages_panel_open.toggle(),
        }
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
        if document.read().is_some() {
            ThumbnailsPanel { open: pages_panel_open() }
            tools_ui::ToolRail {}
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
        zoom_ui::ZoomBar {}
        search_ui::FindCard {}
        print_ui::PrintUi {}
        find_fields_ui::FindFieldsBar {}
        if document.read().is_some() {
            ai_ui::AiUi {}
            ask_ui::AskUi {}
        }
        doc_ui::DocWindow {}
        EditPanel {}
    }
}

/// Opens `bytes` as the document on screen, named `name`.
fn show_document(
    mut editing: Editing,
    mut document: Signal<Option<OpenDocument>>,
    bytes: Vec<u8>,
    name: String,
) {
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
        }
        Err(error) => log(&format!("pdit: could not open the PDF: {error}")),
    }
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
