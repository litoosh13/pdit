use crate::{Error, pdfium, reserve_for_pdfium_copy};
use pdfium_render::prelude::*;
use serde::Serialize;

/// One text object on a page.
#[derive(Debug, Clone, Serialize)]
pub struct TextObjectInfo {
    pub object_index: usize,
    pub text: String,
    pub font_name: String,
    pub font_embedded: Option<bool>,
    pub font_size: f32,
    /// Left, bottom, right, top in PDF points.
    pub bounds: [f32; 4],
}

/// One page: size, full text, and its text objects.
#[derive(Debug, Clone, Serialize)]
pub struct PageInfo {
    pub width: f32,
    pub height: f32,
    pub page_text: String,
    pub text_objects: Vec<TextObjectInfo>,
}

pub(crate) fn rect(bounds: PdfQuadPoints) -> [f32; 4] {
    [
        bounds.left().value,
        bounds.bottom().value,
        bounds.right().value,
        bounds.top().value,
    ]
}

/// Lists every page with its text and text objects.
pub fn inspect(bytes: Vec<u8>) -> Result<Vec<PageInfo>, Error> {
    reserve_for_pdfium_copy(bytes.len());
    let document = pdfium().load_pdf_from_byte_vec(bytes, None)?;
    let mut pages = Vec::new();
    for page in document.pages().iter() {
        let mut text_objects = Vec::new();
        for (object_index, object) in page.objects().iter().enumerate() {
            if let Some(text) = object.as_text_object() {
                let font = text.font();
                text_objects.push(TextObjectInfo {
                    object_index,
                    text: text.text(),
                    font_name: font.name(),
                    font_embedded: font.is_embedded().ok(),
                    font_size: text.unscaled_font_size().value,
                    bounds: rect(object.bounds()?),
                });
            }
        }
        pages.push(PageInfo {
            width: page.width().value,
            height: page.height().value,
            page_text: page.text()?.all(),
            text_objects,
        });
    }
    Ok(pages)
}
