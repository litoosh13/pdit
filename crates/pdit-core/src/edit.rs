use crate::inspect::rect;
use crate::{Error, pdfium, reserve_for_pdfium_copy};
use pdfium_render::prelude::*;
use serde::Serialize;

/// How an edited text object ended up being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EditMethod {
    /// The object's own font could show the new text.
    Native,
    /// The object's font could not, so it was rewritten in a bundled
    /// look-alike of that font (D-028).
    LookAlike,
    /// The object's font lacked characters, so it was rewritten in the fallback font.
    FallbackFont,
}

/// What happened to one edited text object.
#[derive(Debug, Clone, Serialize)]
pub struct EditReport {
    pub page: usize,
    pub object_index: usize,
    pub intended: String,
    pub method: EditMethod,
    /// The text PDFium reads back from the saved object.
    pub final_text: String,
    pub original_font: String,
    pub bounds_before: [f32; 4],
    pub bounds_after: [f32; 4],
}

/// The edited document plus a report per changed text object.
#[derive(Debug, Clone)]
pub struct EditResult {
    pub bytes: Vec<u8>,
    pub reports: Vec<EditReport>,
}

/// An object whose own font could not show its new text.
struct NeedsFallback {
    report: usize,
    page: PdfPageIndex,
    index: usize,
    size: PdfPoints,
    matrix: PdfMatrix,
    fill: PdfColor,
}

/// Replaces `find` with `replacement` in every text object containing it, and
/// verifies each edit by reading the text back.
///
/// Subset fonts often lack characters that the new text needs; PDFium then drops
/// them silently. Such objects are rewritten in place in `fallback_font` (a full
/// TrueType font), keeping size, transform, colour and z-order. The fallback font
/// is only embedded when needed. If even the fallback cannot show the text, this
/// returns [`Error::UnsupportedCharacters`] and nothing is saved.
pub fn replace_text_verified(
    bytes: Vec<u8>,
    find: &str,
    replacement: &str,
    fallback_font: &[u8],
) -> Result<EditResult, Error> {
    reserve_for_pdfium_copy(bytes.len());
    let mut document = pdfium().load_pdf_from_byte_vec(bytes, None)?;
    let mut reports = Vec::new();
    let mut needs_fallback = Vec::new();

    // Pass 1: edit natively and read back.
    for (page_number, mut page) in document.pages().iter().enumerate() {
        let targets: Vec<usize> = page
            .objects()
            .iter()
            .enumerate()
            .filter(|(_, o)| o.as_text_object().is_some_and(|t| t.text().contains(find)))
            .map(|(i, _)| i)
            .collect();
        for &index in &targets {
            let mut object = page.objects().get(index)?;
            let bounds_before = rect(object.bounds()?);
            let matrix = object.matrix()?;
            let fill = object.fill_color()?;
            let Some(text) = object.as_text_object_mut() else {
                continue;
            };
            let intended = text.text().replace(find, replacement);
            let original_font = text.font().name();
            let size = text.unscaled_font_size();
            text.set_text(&intended)?;
            let read_back = text.text();
            let method = if read_back == intended {
                EditMethod::Native
            } else {
                needs_fallback.push(NeedsFallback {
                    report: reports.len(),
                    page: page_number as PdfPageIndex,
                    index,
                    size,
                    matrix,
                    fill,
                });
                EditMethod::FallbackFont
            };
            reports.push(EditReport {
                page: page_number,
                object_index: index,
                intended,
                method,
                final_text: read_back,
                original_font,
                bounds_before,
                bounds_after: rect(object.bounds()?),
            });
        }
        if !targets.is_empty() {
            page.regenerate_content()?;
        }
    }
    if reports.is_empty() {
        return Err(Error::TextNotFound(find.to_owned()));
    }

    // Pass 2: only if needed, embed the fallback font and swap the failed objects.
    if !needs_fallback.is_empty() {
        let font = load_font(&mut document, fallback_font)?;
        for item in &needs_fallback {
            let report = &mut reports[item.report];
            let mut page = document.pages().get(item.page)?;
            let mut object = PdfPageTextObject::new(&document, &report.intended, font, item.size)?;
            object.reset_matrix(item.matrix)?;
            object.set_fill_color(item.fill)?;
            let objects = page.objects_mut();
            objects.remove_object_at_index(item.index)?;
            let inserted =
                objects.insert_object_at_index(item.index, PdfPageObject::Text(object))?;
            let read_back = inserted
                .as_text_object()
                .map(|t| t.text())
                .unwrap_or_default();
            if read_back != report.intended {
                return Err(Error::UnsupportedCharacters {
                    intended: report.intended.clone(),
                    reads_back_as: read_back,
                });
            }
            report.final_text = read_back;
            report.bounds_after = rect(inserted.bounds()?);
            page.regenerate_content()?;
        }
    }

    Ok(EditResult {
        bytes: document.save_to_bytes()?,
        reports,
    })
}

fn load_font(document: &mut PdfDocument, font: &[u8]) -> Result<PdfFontToken, Error> {
    reserve_for_pdfium_copy(font.len());
    Ok(document.fonts_mut().load_true_type_from_bytes(font, true)?)
}
