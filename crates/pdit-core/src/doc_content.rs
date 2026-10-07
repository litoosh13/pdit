//! Whole-document content (D-042): headers & footers (with page numbers and
//! the date) and watermarks, written into the chosen pages as text objects.
//! Each object pdit adds carries a content mark (`pdit-header-footer` /
//! `pdit-watermark`) holding the settings, so pdit can find them again to
//! show the current settings, replace them, or remove them.

use crate::edit_open::discard_edit;
use crate::render::with_open;
use crate::{Error, pdfium};
use pdfium_render::prelude::*;

const HF_TAG: &str = "pdit-header-footer";
const WM_TAG: &str = "pdit-watermark";
/// Header/footer distance from the page's top/bottom edge, as a share of its height.
const HF_EDGE: f32 = 0.045;
/// Header/footer distance from the left/right edge, as a share of its width.
const HF_SIDE: f32 = 0.07;

/// Header and footer: left, centre and right text each, where `{page}`,
/// `{pages}` and `{date}` fill in per page.
#[derive(Clone, Debug, PartialEq)]
pub struct HeaderFooter {
    pub header: [String; 3],
    pub footer: [String; 3],
    pub size_pt: f32,
    /// First and last page (1-based, inclusive); `None` = all pages.
    pub pages: Option<(u16, u16)>,
}

/// A watermark: `text` across the middle of the chosen pages.
#[derive(Clone, Debug, PartialEq)]
pub struct Watermark {
    pub text: String,
    pub color: [u8; 3],
    /// 0.0–1.0.
    pub opacity: f32,
    pub size_pt: f32,
    pub diagonal: bool,
    pub pages: Option<(u16, u16)>,
}

/// Replaces pdit's header & footer with `hf` (`date` fills `{date}`).
pub fn set_header_footer(hf: &HeaderFooter, date: &str) -> Result<(), Error> {
    discard_edit()?;
    remove_tagged(HF_TAG)?;
    let params = hf_params(hf);
    with_open(|document| {
        let total = document.pages().len();
        let font = document.fonts_mut().helvetica();
        for index in 0..total {
            let n = index + 1;
            if !in_range(hf.pages, n) {
                continue;
            }
            let mut page = document.pages().get(index)?;
            let (w, h) = (page.width().value, page.height().value);
            let map = to_content(&page);
            let rows = [
                (&hf.header, h - h * HF_EDGE - hf.size_pt * 0.75),
                (&hf.footer, h * HF_EDGE),
            ];
            for (texts, baseline) in rows {
                for (slot, template) in texts.iter().enumerate() {
                    let text = fill(template, n, total, date);
                    if text.trim().is_empty() {
                        continue;
                    }
                    let mut object =
                        PdfPageTextObject::new(document, &text, font, PdfPoints::new(hf.size_pt))?;
                    object.set_fill_color(PdfColor::new(51, 51, 51, 255))?;
                    let tw = object.width()?.value;
                    let x = match slot {
                        0 => w * HF_SIDE,
                        1 => (w - tw) / 2.0,
                        _ => w - w * HF_SIDE - tw,
                    };
                    object.translate(PdfPoints::new(x), PdfPoints::new(baseline))?;
                    place(&mut object, &map)?;
                    page.objects_mut().add_text_object(object)?;
                    tag_last(document, &page, HF_TAG, &params)?;
                }
            }
            regenerate(&page);
        }
        Ok(())
    })
}

/// Replaces pdit's watermark with `wm`.
pub fn set_watermark(wm: &Watermark) -> Result<(), Error> {
    discard_edit()?;
    remove_tagged(WM_TAG)?;
    if wm.text.trim().is_empty() {
        return Ok(());
    }
    let params = wm_params(wm);
    let [r, g, b] = wm.color;
    let alpha = (wm.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    with_open(|document| {
        let total = document.pages().len();
        let font = document.fonts_mut().helvetica_bold();
        for index in 0..total {
            if !in_range(wm.pages, index + 1) {
                continue;
            }
            let mut page = document.pages().get(index)?;
            let (w, h) = (page.width().value, page.height().value);
            let mut object =
                PdfPageTextObject::new(document, &wm.text, font, PdfPoints::new(wm.size_pt))?;
            object.set_fill_color(PdfColor::new(r, g, b, alpha))?;
            let tw = object.width()?.value;
            // Centre the text on the page, then tilt it round the centre.
            object.translate(
                PdfPoints::new(-tw / 2.0),
                PdfPoints::new(-wm.size_pt * 0.35),
            )?;
            if wm.diagonal {
                object.rotate_counter_clockwise_degrees(45.0)?;
            }
            object.translate(PdfPoints::new(w / 2.0), PdfPoints::new(h / 2.0))?;
            place(&mut object, &to_content(&page))?;
            page.objects_mut().add_text_object(object)?;
            tag_last(document, &page, WM_TAG, &params)?;
            regenerate(&page);
        }
        Ok(())
    })
}

/// Removes pdit's header & footer; returns how many objects went.
pub fn remove_header_footer() -> Result<usize, Error> {
    discard_edit()?;
    remove_tagged(HF_TAG)
}

/// Removes pdit's watermark; returns how many objects went.
pub fn remove_watermark() -> Result<usize, Error> {
    discard_edit()?;
    remove_tagged(WM_TAG)
}

/// The header & footer pdit added, if any (read from its marks).
pub fn header_footer() -> Result<Option<HeaderFooter>, Error> {
    Ok(first_params(HF_TAG)?.map(|p| HeaderFooter {
        header: [get(&p, "h0"), get(&p, "h1"), get(&p, "h2")],
        footer: [get(&p, "f0"), get(&p, "f1"), get(&p, "f2")],
        size_pt: get(&p, "size").parse().unwrap_or(9.0),
        pages: range_of(&p),
    }))
}

/// The watermark pdit added, if any (read from its marks).
pub fn watermark() -> Result<Option<Watermark>, Error> {
    Ok(first_params(WM_TAG)?.map(|p| {
        let rgb: Vec<u8> = get(&p, "color")
            .split(',')
            .filter_map(|v| v.parse().ok())
            .collect();
        Watermark {
            text: get(&p, "text"),
            color: [
                rgb.first().copied().unwrap_or(138),
                rgb.get(1).copied().unwrap_or(141),
                rgb.get(2).copied().unwrap_or(147),
            ],
            opacity: get(&p, "opacity").parse().unwrap_or(0.25),
            size_pt: get(&p, "size").parse().unwrap_or(54.0),
            diagonal: get(&p, "diagonal") != "0",
            pages: range_of(&p),
        }
    }))
}

/// How to turn a position laid out on the page as displayed into the page's
/// own (unrotated) coordinates: rotate counter-clockwise by the page's
/// rotation, then move by (tx, ty). Pages with `/Rotate` show their content
/// turned; this keeps headers, footers and watermarks upright where the
/// reader sees them.
struct ToContent {
    degrees: f32,
    tx: f32,
    ty: f32,
}

fn to_content(page: &PdfPage) -> ToContent {
    // width/height are as displayed; the media box is swapped for 90 / 270.
    let (w, h) = (page.width().value, page.height().value);
    let (degrees, tx, ty) = match page.rotation() {
        Ok(PdfPageRenderRotation::Degrees90) => (90.0, h, 0.0),
        Ok(PdfPageRenderRotation::Degrees180) => (180.0, w, h),
        Ok(PdfPageRenderRotation::Degrees270) => (270.0, 0.0, w),
        _ => (0.0, 0.0, 0.0),
    };
    ToContent { degrees, tx, ty }
}

/// Moves a text object laid out in display coordinates into place.
fn place(object: &mut PdfPageTextObject, map: &ToContent) -> Result<(), PdfiumError> {
    if map.degrees != 0.0 {
        object.rotate_counter_clockwise_degrees(map.degrees)?;
        object.translate(PdfPoints::new(map.tx), PdfPoints::new(map.ty))?;
    }
    Ok(())
}

// ---- settings as mark parameters ----

type Params = Vec<(&'static str, String)>;

fn hf_params(hf: &HeaderFooter) -> Params {
    let mut p: Params = vec![("size", hf.size_pt.to_string())];
    for (i, t) in hf.header.iter().enumerate() {
        p.push((["h0", "h1", "h2"][i], t.clone()));
    }
    for (i, t) in hf.footer.iter().enumerate() {
        p.push((["f0", "f1", "f2"][i], t.clone()));
    }
    push_range(&mut p, hf.pages);
    p
}

fn wm_params(wm: &Watermark) -> Params {
    let [r, g, b] = wm.color;
    let mut p: Params = vec![
        ("text", wm.text.clone()),
        ("color", format!("{r},{g},{b}")),
        ("opacity", wm.opacity.to_string()),
        ("size", wm.size_pt.to_string()),
        ("diagonal", if wm.diagonal { "1" } else { "0" }.to_owned()),
    ];
    push_range(&mut p, wm.pages);
    p
}

fn push_range(p: &mut Params, pages: Option<(u16, u16)>) {
    if let Some((from, to)) = pages {
        p.push(("from", from.to_string()));
        p.push(("to", to.to_string()));
    }
}

fn range_of(p: &[(String, String)]) -> Option<(u16, u16)> {
    Some((get(p, "from").parse().ok()?, get(p, "to").parse().ok()?))
}

fn get(p: &[(String, String)], key: &str) -> String {
    p.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn in_range(pages: Option<(u16, u16)>, n: i32) -> bool {
    pages.is_none_or(|(from, to)| n >= i32::from(from) && n <= i32::from(to))
}

fn fill(template: &str, page: i32, pages: i32, date: &str) -> String {
    template
        .replace("{page}", &page.to_string())
        .replace("{pages}", &pages.to_string())
        .replace("{date}", date)
}

// ---- content marks (raw PDFium calls; pdfium-render has no public wrapper) ----

/// Tags the page's last object with `tag` and the settings.
fn tag_last(
    document: &PdfDocument,
    page: &PdfPage,
    tag: &str,
    params: &Params,
) -> Result<(), Error> {
    let bindings = pdfium().bindings();
    unsafe {
        let count = bindings.FPDFPage_CountObjects(page.page_handle());
        let object = bindings.FPDFPage_GetObject(page.page_handle(), count - 1);
        if object.is_null() {
            return Err(Error::Pdfium("the new object was not found".into()));
        }
        let mark = bindings.FPDFPageObj_AddMark(object, tag);
        if mark.is_null() {
            return Err(Error::Pdfium("could not tag the new object".into()));
        }
        for (key, value) in params {
            bindings.FPDFPageObjMark_SetStringParam(document.handle(), object, mark, key, value);
        }
    }
    Ok(())
}

/// Writes the page's objects (and their new marks) into its content stream.
fn regenerate(page: &PdfPage) {
    unsafe {
        pdfium()
            .bindings()
            .FPDFPage_GenerateContent(page.page_handle());
    }
}

/// Handles of the objects on `page` that carry `tag`.
fn tagged_on(page: &PdfPage, tag: &str) -> Vec<FPDF_PAGEOBJECT> {
    let bindings = pdfium().bindings();
    let mut found = Vec::new();
    unsafe {
        for i in 0..bindings.FPDFPage_CountObjects(page.page_handle()) {
            let object = bindings.FPDFPage_GetObject(page.page_handle(), i);
            if object.is_null() {
                continue;
            }
            for m in 0..bindings.FPDFPageObj_CountMarks(object).max(0) {
                let mark = bindings.FPDFPageObj_GetMark(object, m as _);
                if !mark.is_null() && mark_name(mark).as_deref() == Some(tag) {
                    found.push(object);
                    break;
                }
            }
        }
    }
    found
}

fn remove_tagged(tag: &str) -> Result<usize, Error> {
    with_open(|document| {
        let bindings = pdfium().bindings();
        let mut removed = 0;
        for index in 0..document.pages().len() {
            let page = document.pages().get(index)?;
            let objects = tagged_on(&page, tag);
            if objects.is_empty() {
                continue;
            }
            for object in objects {
                unsafe {
                    if bindings.FPDFPage_RemoveObject(page.page_handle(), object) != 0 {
                        bindings.FPDFPageObj_Destroy(object);
                        removed += 1;
                    }
                }
            }
            regenerate(&page);
        }
        Ok(removed)
    })
}

/// The settings stored on the first object tagged `tag`, if any.
fn first_params(tag: &str) -> Result<Option<Vec<(String, String)>>, Error> {
    with_open(|document| {
        let bindings = pdfium().bindings();
        for index in 0..document.pages().len() {
            let page = document.pages().get(index)?;
            let Some(&object) = tagged_on(&page, tag).first() else {
                continue;
            };
            let mut out = Vec::new();
            unsafe {
                for m in 0..bindings.FPDFPageObj_CountMarks(object).max(0) {
                    let mark = bindings.FPDFPageObj_GetMark(object, m as _);
                    if mark_name(mark).as_deref() != Some(tag) {
                        continue;
                    }
                    for key in [
                        "h0", "h1", "h2", "f0", "f1", "f2", "size", "from", "to", "text", "color",
                        "opacity", "diagonal",
                    ] {
                        if let Some(value) = mark_param(mark, key) {
                            out.push((key.to_owned(), value));
                        }
                    }
                }
            }
            return Ok(Some(out));
        }
        Ok(None)
    })
}

/// Calls a PDFium "write UTF-16 into this buffer" function. The buffer is a
/// real, aligned `u16` buffer (the wasm bridge copies into it even when only
/// asking for the length, so a null pointer is not allowed), grown if needed.
/// Lengths are in bytes, as PDFium counts them.
fn read_utf16(
    mut call: impl FnMut(*mut u16, std::os::raw::c_ulong, &mut std::os::raw::c_ulong) -> i32,
) -> Option<String> {
    let mut buf = vec![0u16; 256];
    let mut len: std::os::raw::c_ulong = 0;
    if call(buf.as_mut_ptr(), (buf.len() * 2) as _, &mut len) == 0 {
        return None;
    }
    if len as usize > buf.len() * 2 {
        buf = vec![0u16; (len as usize).div_ceil(2)];
        if call(buf.as_mut_ptr(), (buf.len() * 2) as _, &mut len) == 0 {
            return None;
        }
    }
    buf.truncate(len as usize / 2);
    Some(
        String::from_utf16_lossy(&buf)
            .trim_end_matches('\0')
            .to_owned(),
    )
}

unsafe fn mark_name(mark: FPDF_PAGEOBJECTMARK) -> Option<String> {
    let bindings = pdfium().bindings();
    read_utf16(|buf, len, out| unsafe { bindings.FPDFPageObjMark_GetName(mark, buf, len, out) })
}

unsafe fn mark_param(mark: FPDF_PAGEOBJECTMARK, key: &str) -> Option<String> {
    let bindings = pdfium().bindings();
    read_utf16(|buf, len, out| unsafe {
        bindings.FPDFPageObjMark_GetParamStringValue(mark, key, buf, len, out)
    })
}
