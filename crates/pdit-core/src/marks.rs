//! Tags pdit puts on the page objects it adds (marked content, D-081): its shapes ([`SHAPE`]) and text boxes
//! ([`TEXT_BOX`], with the box's number), so they can be picked, moved and resized later — not the page's own
//! content. pdfium-render has no wrapper for marks, so PDFium is called directly (the page handle and bindings are
//! public in our copy, D-036).

use pdfium_render::prelude::*;

pub(crate) const SHAPE: &str = "pditShape";
pub(crate) const TEXT_BOX: &str = "pditText";
/// The text box's number, in its mark.
const BOX_KEY: &str = "box";

/// The mark `name` on `object`, if it has one.
///
/// # Safety
/// `object` must be a live page object handle.
pub(crate) unsafe fn find(
    bindings: &dyn PdfiumLibraryBindings,
    object: FPDF_PAGEOBJECT,
    name: &str,
) -> Option<FPDF_PAGEOBJECTMARK> {
    let want: Vec<u16> = name.encode_utf16().collect();
    // SAFETY: as the caller promises; the name buffer outlives the calls.
    unsafe {
        (0..bindings.FPDFPageObj_CountMarks(object))
            .map(|i| bindings.FPDFPageObj_GetMark(object, i as _))
            .find(|&mark| {
                let mut buffer = [0u16; 32];
                let mut bytes = 0;
                !mark.is_null()
                    && bindings.is_true(bindings.FPDFPageObjMark_GetName(
                        mark,
                        buffer.as_mut_ptr(),
                        std::mem::size_of_val(&buffer) as _,
                        &mut bytes,
                    ))
                    && buffer.get(..(bytes as usize / 2).saturating_sub(1)) == Some(want.as_slice())
            })
    }
}

/// The number of the text box `object` belongs to.
///
/// # Safety
/// `object` must be a live page object handle.
pub(crate) unsafe fn text_box(
    bindings: &dyn PdfiumLibraryBindings,
    object: FPDF_PAGEOBJECT,
) -> Option<i32> {
    // SAFETY: as the caller promises.
    unsafe {
        let mark = find(bindings, object, TEXT_BOX)?;
        let mut value = 0;
        bindings
            .is_true(bindings.FPDFPageObjMark_GetParamIntValue(mark, BOX_KEY, &mut value))
            .then_some(value)
    }
}

/// Tags `object` with `name`, and with text box number `text_box` when given. The page's content must be written
/// again afterwards (`FPDFPage_GenerateContent`).
///
/// # Safety
/// `document` and `object` must be live handles, the object on a page of that document.
pub(crate) unsafe fn tag(
    bindings: &dyn PdfiumLibraryBindings,
    document: FPDF_DOCUMENT,
    object: FPDF_PAGEOBJECT,
    name: &str,
    text_box: Option<i32>,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe {
        let mark = bindings.FPDFPageObj_AddMark(object, name);
        !mark.is_null()
            && text_box.is_none_or(|n| {
                bindings.is_true(
                    bindings.FPDFPageObjMark_SetIntParam(document, object, mark, BOX_KEY, n),
                )
            })
    }
}
