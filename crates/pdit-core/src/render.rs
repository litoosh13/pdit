//! The document the user has open, kept loaded in PDFium, and rendering its
//! pages for display. Keeping it loaded means each page render reuses the one
//! parsed document instead of copying and parsing the whole file again.

use crate::{Error, pdfium, reserve_for_pdfium_copy};
use pdfium_render::prelude::*;
use std::cell::RefCell;

thread_local! {
    static OPEN: RefCell<Option<PdfDocument<'static>>> = const { RefCell::new(None) };
}

/// Runs `f` with the open document, or fails if none is open.
pub(crate) fn with_open<R>(
    f: impl FnOnce(&mut PdfDocument<'static>) -> Result<R, Error>,
) -> Result<R, Error> {
    OPEN.with_borrow_mut(|open| match open.as_mut() {
        Some(document) => f(document),
        None => Err(Error::Pdfium("no document is open".into())),
    })
}

/// Opens `bytes` as the current document (closing any previous one) and returns
/// each page's size in PDF points (width, height). Pages are not loaded here.
pub fn open_document(bytes: Vec<u8>) -> Result<Vec<(f32, f32)>, Error> {
    reserve_for_pdfium_copy(bytes.len());
    let document = pdfium().load_pdf_from_byte_vec(bytes, None)?;
    let sizes = document
        .pages()
        .page_sizes()?
        .iter()
        .map(|rect| (rect.width().value, rect.height().value))
        .collect();
    crate::edit_open::forget_document();
    OPEN.with_borrow_mut(|open| *open = Some(document));
    Ok(sizes)
}

/// Closes the current document, freeing its memory in PDFium.
pub fn close_document() {
    crate::edit_open::forget_document();
    OPEN.with_borrow_mut(|open| *open = None);
}

/// Page `index` drawn for paper (D-052): `width_px` wide, print quality, form
/// fields filled in, and comments and marks only when `annotations` is true.
pub fn render_page_for_print(
    index: u16,
    width_px: u32,
    annotations: bool,
) -> Result<web_sys::ImageData, Error> {
    OPEN.with_borrow(|open| {
        let document = open
            .as_ref()
            .ok_or_else(|| Error::Pdfium("no document is open".into()))?;
        let page = document.pages().get(index.into())?;
        let bitmap = page.render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(width_px as Pixels)
                .render_form_data(true)
                .render_annotations(annotations)
                .use_print_quality(true),
        )?;
        bitmap
            .as_image_data()
            .map_err(|error| Error::Pdfium(format!("{error:?}")))
    })
}

/// Renders page `index` of the open document to browser image data,
/// `width_px` pixels wide (height follows the page's aspect ratio).
pub fn render_page(index: u16, width_px: u32) -> Result<web_sys::ImageData, Error> {
    OPEN.with_borrow(|open| {
        let document = open
            .as_ref()
            .ok_or_else(|| Error::Pdfium("no document is open".into()))?;
        let page = document.pages().get(index.into())?;
        let bitmap = page.render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(width_px as Pixels)
                .render_form_data(true),
        )?;
        bitmap
            .as_image_data()
            .map_err(|error| Error::Pdfium(format!("{error:?}")))
    })
}
