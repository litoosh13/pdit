//! What each page needs (D-055): its own text (read straight from the PDF) or
//! OCR (a scan: a picture of text), or nothing (blank).

use crate::Error;
use crate::render::with_open;
use pdfium_render::prelude::*;

/// What a page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    /// Has its own text.
    Text,
    /// Mostly a picture, with (almost) no text of its own: needs OCR.
    Scan,
    /// Neither.
    Blank,
}

/// Fewer characters than this is "no text of its own" (a stray page number).
const MIN_CHARS: usize = 20;
/// A page this much covered by images is a scan when it has no text.
const MIN_IMAGE_SHARE: f32 = 0.3;

/// What page `page` is.
pub fn page_kind(page: u16) -> Result<PageKind, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let chars = pdf_page
            .text()?
            .all()
            .chars()
            .filter(|c| !c.is_whitespace())
            .count();
        if chars >= MIN_CHARS {
            return Ok(PageKind::Text);
        }
        let area = (pdf_page.width().value * pdf_page.height().value).max(1.0);
        let images: f32 = pdf_page
            .objects()
            .iter()
            .filter(|o| o.as_image_object().is_some())
            .filter_map(|o| o.bounds().ok())
            .map(|b| b.width().value * b.height().value)
            .sum();
        Ok(if images / area >= MIN_IMAGE_SHARE {
            PageKind::Scan
        } else {
            PageKind::Blank
        })
    })
}
