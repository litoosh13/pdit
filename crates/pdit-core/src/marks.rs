//! Tags pdit puts on the page objects it adds (marked content, D-081): its shapes ([`SHAPE`]) and text boxes
//! ([`TEXT_BOX`], with the box's number), so they can be picked, moved and resized later — not the page's own
//! content. pdfium-render has no wrapper for marks, so PDFium is called directly (the page handle and bindings are
//! public in our copy, D-036).

use pdfium_render::prelude::*;

pub(crate) const SHAPE: &str = "pditShape";
pub(crate) const TEXT_BOX: &str = "pditText";
/// The text box's number, in its mark.
const BOX_KEY: &str = "box";
/// The text box's wrap width in points, in its mark (pdit's own boxes; MuPDF's have none).
const WIDTH_KEY: &str = "w";

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

/// The wrap width kept in `object`'s text-box mark.
///
/// # Safety
/// `object` must be a live page object handle.
pub(crate) unsafe fn box_width(
    bindings: &dyn PdfiumLibraryBindings,
    object: FPDF_PAGEOBJECT,
) -> Option<i32> {
    // SAFETY: as the caller promises.
    unsafe {
        let mark = find(bindings, object, TEXT_BOX)?;
        let mut value = 0;
        bindings
            .is_true(bindings.FPDFPageObjMark_GetParamIntValue(mark, WIDTH_KEY, &mut value))
            .then_some(value)
    }
}

/// The text box's text as typed (line breaks kept), in its first line's mark: hex of its UTF-8, as PDFium keeps
/// mark strings as bytes.
const SOURCE_KEY: &str = "src";

/// Keeps `text` (a text box's typed text) in `object`'s text-box mark. The page's content must be written again.
///
/// # Safety
/// `document` and `object` must be live handles, the object on a page of that document.
pub(crate) unsafe fn set_box_source(
    bindings: &dyn PdfiumLibraryBindings,
    document: FPDF_DOCUMENT,
    object: FPDF_PAGEOBJECT,
    text: &str,
) -> bool {
    let hex: String = text.bytes().map(|b| format!("{b:02x}")).collect();
    // SAFETY: as the caller promises.
    unsafe {
        find(bindings, object, TEXT_BOX).is_some_and(|mark| {
            bindings.is_true(
                bindings.FPDFPageObjMark_SetStringParam(document, object, mark, SOURCE_KEY, &hex),
            )
        })
    }
}

/// The typed text kept in `object`'s text-box mark ([`set_box_source`]).
///
/// # Safety
/// `object` must be a live page object handle.
pub(crate) unsafe fn box_source(
    bindings: &dyn PdfiumLibraryBindings,
    object: FPDF_PAGEOBJECT,
) -> Option<String> {
    // SAFETY: as the caller promises; the buffer outlives the call that fills it.
    unsafe {
        let mark = find(bindings, object, TEXT_BOX)?;
        let mut bytes = 0;
        if !bindings.is_true(bindings.FPDFPageObjMark_GetParamStringValue(
            mark,
            SOURCE_KEY,
            std::ptr::null_mut(),
            0,
            &mut bytes,
        )) {
            return None;
        }
        let mut buffer = vec![0u16; bytes as usize / 2 + 1];
        bindings.FPDFPageObjMark_GetParamStringValue(
            mark,
            SOURCE_KEY,
            buffer.as_mut_ptr() as _,
            (buffer.len() * 2) as _,
            &mut bytes,
        );
        let hex = String::from_utf16_lossy(&buffer[..(bytes as usize / 2).saturating_sub(1)]);
        let utf8 = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        String::from_utf8(utf8).ok()
    }
}

/// Tags `object` with `name`, and with text box number `text_box` (and its wrap `width`) when given. The page's content must be written
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
    width: Option<i32>,
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
            && width.is_none_or(|w| {
                bindings.is_true(
                    bindings.FPDFPageObjMark_SetIntParam(document, object, mark, WIDTH_KEY, w),
                )
            })
    }
}
