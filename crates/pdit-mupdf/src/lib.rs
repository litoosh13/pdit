//! pdit's MuPDF engine layer (desktop only; D-065). Step 1: read the paragraph under a point with MuPDF's own
//! layout analysis, and replace it: the paragraph's text is removed (a text-only redaction, line by line), and
//! the new text is laid into the same box by MuPDF's Story (HTML/CSS layout, HarfBuzz shaping, so it re-wraps and
//! complex scripts join correctly) in the PDF's own font when it has every glyph, else in a fallback font.
//!
//! Coordinates are MuPDF's page space: points, origin at the page's top-left.
#![cfg(not(target_arch = "wasm32"))]

use std::ffi::{CString, c_char};

use mupdf::pdf::{
    PdfDocument, PdfPage, PdfRedactImageMethod, PdfRedactLineArtMethod, PdfRedactOptions,
    PdfRedactTextMethod,
};
use mupdf::{Rect, TextPageFlags};
use mupdf_sys::{fz_buffer, fz_context, fz_rect, pdf_document, pdf_page};

#[derive(Debug)]
pub enum Error {
    Mupdf(mupdf::Error),
    /// The Story could not be laid out or drawn (MuPDF's message).
    Story(String),
    /// The new text needs more room than the page has below the paragraph.
    DoesNotFit,
    /// Compressing failed (MuPDF's message).
    Compress(String),
}

impl From<mupdf::Error> for Error {
    fn from(e: mupdf::Error) -> Self {
        Error::Mupdf(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Mupdf(e) => write!(f, "MuPDF: {e}"),
            Error::Story(e) => write!(f, "laying out the text: {e}"),
            Error::DoesNotFit => write!(f, "the new text doesn't fit on the page"),
            Error::Compress(e) => write!(f, "compressing: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// A paragraph as MuPDF's layout analysis sees it (one text block).
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    pub page: i32,
    /// The lines joined: a space between lines, none after a line ending in "-".
    pub text: String,
    /// [x0, y0, x1, y1], page space.
    pub bounds: [f32; 4],
    /// Each line's box, page space.
    pub lines: Vec<[f32; 4]>,
    /// The font name of the paragraph's first character, as MuPDF reports it (subset tag removed).
    pub font: String,
    /// Font size in points.
    pub size: f32,
    /// Distance between baselines, points (1.2 × size for a single line).
    pub line_pitch: f32,
    pub color: [u8; 3],
}

/// A font the new text may use when the PDF's own font lacks a glyph.
pub struct FallbackFont<'a> {
    pub data: &'a [u8],
}

unsafe extern "C" {
    fn pdit_story_onto_page(
        ctx: *mut fz_context,
        doc: *mut pdf_document,
        page: *mut pdf_page,
        place: fz_rect,
        html: *const c_char,
        css: *const c_char,
        names: *const *const c_char,
        datas: *const *const u8,
        lens: *const usize,
        nfonts: i32,
        filled: *mut fz_rect,
        more: *mut i32,
        xname: *mut c_char,
        xn: usize,
        err: *mut c_char,
        n: usize,
    ) -> i32;
}

pub struct Editor {
    doc: PdfDocument,
}

impl Editor {
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
        Ok(Self {
            doc: PdfDocument::from_bytes(bytes)?,
        })
    }

    pub fn page_count(&self) -> Result<i32, Error> {
        Ok(self.doc.page_count()?)
    }

    /// The page's text in reading order (lines joined by newlines, blocks by blank lines).
    pub fn page_text(&self, page: i32) -> Result<String, Error> {
        let tp = self
            .doc
            .load_page(page)?
            .to_text_page(TextPageFlags::empty())?;
        let mut out = String::new();
        for block in tp.blocks() {
            for line in block.lines() {
                out.extend(line.chars().filter_map(|c| c.char()));
                out.push('\n');
            }
            out.push('\n');
        }
        Ok(out)
    }

    /// The paragraph under (`x`, `y`) on `page`, if there is text there.
    pub fn paragraph_at(&self, page: i32, x: f32, y: f32) -> Result<Option<Paragraph>, Error> {
        let tp = self
            .doc
            .load_page(page)?
            .to_text_page(TextPageFlags::empty())?;
        for block in tp.blocks() {
            let b = block.bounds();
            if !(b.x0 <= x && x <= b.x1 && b.y0 <= y && y <= b.y1) {
                continue;
            }
            let mut text = String::new();
            let mut lines = Vec::new();
            let mut baselines = Vec::new();
            let (mut font, mut size, mut color) = (String::new(), 0.0, [0, 0, 0]);
            for line in block.lines() {
                let s: String = line.chars().filter_map(|c| c.char()).collect();
                let s = s.trim();
                if s.is_empty() {
                    continue;
                }
                if !text.is_empty() && !text.ends_with('-') {
                    text.push(' ');
                }
                text.push_str(s);
                let r = line.bounds();
                lines.push([r.x0, r.y0, r.x1, r.y1]);
                if let Some(c) = line.chars().next() {
                    baselines.push(c.origin().y);
                    if font.is_empty() {
                        font = c
                            .font()
                            .map(|f| strip_subset_tag(f.name()).to_owned())
                            .unwrap_or_default();
                        size = c.size();
                        let argb = c.argb();
                        color = [(argb >> 16) as u8, (argb >> 8) as u8, argb as u8];
                    }
                }
            }
            if text.is_empty() {
                continue;
            }
            let line_pitch = if baselines.len() > 1 {
                (baselines[baselines.len() - 1] - baselines[0]) / (baselines.len() - 1) as f32
            } else {
                size * 1.2
            };
            return Ok(Some(Paragraph {
                page,
                text,
                bounds: [b.x0, b.y0, b.x1, b.y1],
                lines,
                font,
                size,
                line_pitch,
                color,
            }));
        }
        Ok(None)
    }

    /// Replaces `p`'s text with `new_text`, re-wrapped to the paragraph's width with its size, colour and line
    /// spacing, in the PDF's own font when that font has every glyph, else the first fallback.
    pub fn replace_paragraph(
        &mut self,
        p: &Paragraph,
        new_text: &str,
        fallbacks: &[FallbackFont<'_>],
    ) -> Result<(), Error> {
        let mut page = PdfPage::try_from(self.doc.load_page(p.page)?)?;
        let page_bounds = page.bounds()?;
        let own = own_font_program(&page, &p.font)?;

        // Keep the old content's graphics state from leaking into the new stream.
        page.wrap_contents(&mut self.doc)?;
        // Remove the old text only (not images or drawings), line by line, inset vertically so the lines above
        // and below are never touched.
        for [x0, y0, x1, y1] in &p.lines {
            let inset = (y1 - y0) * 0.25;
            page.add_redact_annotation(Rect::new(*x0, y0 + inset, *x1, y1 - inset))?;
        }
        page.apply_redactions_with_options(PdfRedactOptions {
            black_boxes: false,
            image_method: PdfRedactImageMethod::None,
            line_art: PdfRedactLineArtMethod::None,
            text: PdfRedactTextMethod::Remove,
        })?;

        // The fonts the text may use, in order: the PDF's own, then the fallbacks. Each run of text gets the first
        // font that has its letters (MuPDF's HTML doesn't fall back along the CSS list).
        let mut fonts: Vec<(CString, &[u8])> = Vec::new();
        if let Some(data) = own.as_deref() {
            fonts.push((CString::new("own.ttf").unwrap(), data));
        }
        // Fallback fonts are whole font files; for simple scripts embed only the glyphs the text uses.
        // ponytail: right-to-left/complex text keeps the whole font (its joined forms come from the shaping
        // tables, which a cmap-only subset drops); subset with the shaper's glyphs if size matters.
        let subsets: Vec<Option<Vec<u8>>> = fallbacks
            .iter()
            .map(|f| {
                (!has_complex_script(new_text))
                    .then(|| subset_for_text(f.data, new_text))
                    .flatten()
            })
            .collect();
        for (i, f) in fallbacks.iter().enumerate() {
            let data = subsets[i].as_deref().unwrap_or(f.data);
            fonts.push((CString::new(format!("fallback{i}.ttf")).unwrap(), data));
        }
        let families: Vec<String> = fonts
            .iter()
            .map(|(name, _)| name.to_str().unwrap().trim_end_matches(".ttf").to_owned())
            .collect();
        let loaded: Vec<Option<mupdf::Font>> = fonts
            .iter()
            .map(|(_, data)| mupdf::Font::from_bytes("f", data).ok())
            .collect();
        // MuPDF's HTML gives the body a margin; the box is the paragraph's own.
        let mut css = String::from("body {margin: 0; padding: 0;}");
        for f in &families {
            css.push_str(&format!(
                "@font-face {{font-family: {f}; src: url({f}.ttf);}}"
            ));
        }
        let [r, g, b] = p.color;
        css.push_str(&format!(
            "p {{font-family: {}; font-size: {}pt; line-height: {}pt; margin: 0; color: #{r:02x}{g:02x}{b:02x};}}",
            families.join(", "),
            p.size,
            p.line_pitch
        ));
        let dir = if starts_rtl(new_text) {
            " dir=\"rtl\""
        } else {
            ""
        };
        let html = format!("<p{dir}>{}</p>", font_runs(new_text, &families, &loaded));

        // The paragraph's box, as wide as before (a little slack so the last word of a full line still fits),
        // free to grow down to the page's bottom margin.
        let [x0, y0, x1, _] = p.bounds;
        let place = fz_rect {
            x0,
            y0: y0 - 1.0,
            x1: x1 + 2.0,
            y1: page_bounds.y1 - 18.0,
        };
        let html = CString::new(html).map_err(|e| Error::Story(e.to_string()))?;
        let css = CString::new(css).map_err(|e| Error::Story(e.to_string()))?;
        let names: Vec<*const c_char> = fonts.iter().map(|(n, _)| n.as_ptr()).collect();
        let datas: Vec<*const u8> = fonts.iter().map(|(_, d)| d.as_ptr()).collect();
        let lens: Vec<usize> = fonts.iter().map(|(_, d)| d.len()).collect();
        let mut filled = fz_rect {
            x0: 0.0,
            y0: 0.0,
            x1: 0.0,
            y1: 0.0,
        };
        let mut more = 0;
        let mut xname = [0 as c_char; 512];
        let mut err = [0 as c_char; 512];
        let code = unsafe {
            pdit_story_onto_page(
                mupdf::context::context(),
                self.doc.as_raw(),
                page.as_raw(),
                place,
                html.as_ptr(),
                css.as_ptr(),
                names.as_ptr(),
                datas.as_ptr(),
                lens.as_ptr(),
                fonts.len() as i32,
                &mut filled,
                &mut more,
                xname.as_mut_ptr(),
                xname.len(),
                err.as_mut_ptr(),
                err.len(),
            )
        };
        if code != 0 {
            let msg = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) };
            return Err(Error::Story(msg.to_string_lossy().into_owned()));
        }
        if more != 0 {
            return Err(Error::DoesNotFit);
        }
        let new_fonts = unsafe { std::ffi::CStr::from_ptr(xname.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        fix_story_unicode(&page, new_fonts.split_whitespace())?;
        Ok(())
    }

    pub fn save(&self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.doc.write_to(&mut out)?;
        Ok(out)
    }
}

/// "ABCDEF+Arial" → "Arial".
fn strip_subset_tag(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

/// Lowercase, without spaces, "-" or ",", for matching a font's names ("Arial,Bold" ~ "Arial-Bold").
fn font_key(name: &str) -> String {
    strip_subset_tag(name)
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | ',' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The embedded font program of the page font MuPDF calls `name`, if any.
fn own_font_program(page: &PdfPage, name: &str) -> Result<Option<Vec<u8>>, Error> {
    let want = font_key(name);
    let Some(fonts) = page
        .object()
        .get_dict_inheritable("Resources")?
        .and_then(|r| r.get_dict("Font").ok().flatten())
    else {
        return Ok(None);
    };
    for entry in fonts.dict_iter()? {
        let (_, font) = entry?;
        let Some(font) = font.resolve()? else {
            continue;
        };
        let base = match font.get_dict("BaseFont")? {
            Some(n) => String::from_utf8_lossy(&n.as_name()?).into_owned(),
            None => continue,
        };
        let have = font_key(&base);
        // MuPDF may report "Arial" for a BaseFont "ArialMT" and the other way round.
        if !(have == want || have.starts_with(&want) || want.starts_with(&have)) {
            continue;
        }
        let descriptor = match font.get_dict("DescendantFonts")? {
            Some(d) => d
                .get_array(0)?
                .and_then(|d| d.resolve().ok().flatten())
                .and_then(|d| d.get_dict("FontDescriptor").ok().flatten()),
            None => font.get_dict("FontDescriptor")?,
        };
        let Some(descriptor) = descriptor else {
            continue;
        };
        for key in ["FontFile2", "FontFile3", "FontFile"] {
            if let Some(file) = descriptor.get_dict(key)? {
                let data = file.read_stream()?;
                if !data.is_empty() {
                    return Ok(Some(data));
                }
            }
        }
    }
    Ok(None)
}

/// `text` as HTML in spans, each run in the first font (by family name) that has its letters; a letter no
/// font has stays in the first font. Spaces stay in the current run.
fn font_runs(text: &str, families: &[String], fonts: &[Option<mupdf::Font>]) -> String {
    let has = |i: usize, c: char| {
        fonts[i]
            .as_ref()
            .is_some_and(|f| f.encode_character(c as i32).is_ok_and(|g| g > 0))
    };
    let mut out = String::new();
    let mut current: Option<usize> = None;
    let mut run = String::new();
    for c in text.chars() {
        let pick = if c.is_whitespace() {
            current.unwrap_or(0)
        } else if current.is_some_and(|i| has(i, c)) {
            current.unwrap()
        } else {
            (0..families.len()).find(|&i| has(i, c)).unwrap_or(0)
        };
        if current != Some(pick) && !run.is_empty() {
            push_run(&mut out, &families[current.unwrap()], &run);
            run.clear();
        }
        current = Some(pick);
        run.push(c);
    }
    if let (Some(i), false) = (current, run.is_empty()) {
        push_run(&mut out, &families[i], &run);
    }
    out
}

fn push_run(out: &mut String, family: &str, run: &str) {
    out.push_str(&format!(
        "<span style=\"font-family: {family}\">{}</span>",
        escape_html(run)
    ));
}

/// Whether `text` has letters of a script that needs shaping (Arabic, Persian, Hebrew, Indic, Thai…).
fn has_complex_script(text: &str) -> bool {
    text.chars().any(|c| matches!(c as u32, 0x0590..=0x0DFF | 0x0E00..=0x0FFF | 0x1000..=0x109F | 0xFB1D..=0xFEFF))
}

/// A copy of `font` with only the glyphs for `text`'s characters (plus .notdef) and a Unicode cmap; `None` when
/// the font can't be subset (the whole font is used then).
fn subset_for_text(font: &[u8], text: &str) -> Option<Vec<u8>> {
    use allsorts::binary::read::ReadScope;
    use allsorts::font::{Font, MatchingPresentation};
    use allsorts::font_data::FontData;
    use allsorts::subset::{CmapTarget, SubsetProfile, subset};
    let data = ReadScope::new(font).read::<FontData<'_>>().ok()?;
    let mut parsed = Font::new(data.table_provider(0).ok()?).ok()?;
    let mut glyphs = vec![0_u16];
    for ch in text.chars().chain([' ', '-']) {
        let (glyph, _) = parsed.lookup_glyph_index(ch, MatchingPresentation::NotRequired, None);
        if glyph != 0 && !glyphs.contains(&glyph) {
            glyphs.push(glyph);
        }
    }
    subset(
        &parsed.font_table_provider,
        &glyphs,
        &SubsetProfile::Minimal,
        CmapTarget::Unicode,
    )
    .ok()
}

/// Whether the first letter with a direction is right-to-left (Hebrew, Arabic, Persian…).
fn starts_rtl(text: &str) -> bool {
    text.chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(|c| matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF))
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The Story's fonts map the space glyph to U+00A0 and "-" to U+00AD in their ToUnicode, so the new text reads
/// back (copy, search, other apps) with no-break spaces and soft hyphens. Map them back in the fonts this edit
/// added (`names` in the page's /Font resources).
// ponytail: a real U+00A0 / U+00AD typed by the user also reads back as a plain space / hyphen.
fn fix_story_unicode<'a>(
    page: &PdfPage,
    names: impl Iterator<Item = &'a str>,
) -> Result<(), Error> {
    let Some(fonts) = page
        .object()
        .get_dict_inheritable("Resources")?
        .and_then(|r| r.get_dict("Font").ok().flatten())
    else {
        return Ok(());
    };
    for name in names {
        let Some(font) = fonts
            .get_dict(name)?
            .and_then(|f| f.resolve().ok().flatten())
        else {
            continue;
        };
        let Some(mut cmap) = font.get_dict("ToUnicode")? else {
            continue;
        };
        let text = String::from_utf8_lossy(&cmap.read_stream()?).into_owned();
        let fixed = text
            .replace("<00A0>", "<0020>")
            .replace("<00a0>", "<0020>")
            .replace("<00AD>", "<002D>")
            .replace("<00ad>", "<002D>");
        if fixed != text {
            cmap.write_stream_string(&fixed)?;
        }
    }
    Ok(())
}

/// How hard Compress PDF (D-069) works: pictures are only touched by Medium and Smallest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Lossless: unused objects and duplicates go, streams and fonts are packed.
    BestQuality,
    /// Pictures over 225 dpi to 150 dpi, JPEG quality 75.
    Medium,
    /// Pictures over 150 dpi to 100 dpi, JPEG quality 60.
    Smallest,
}

unsafe extern "C" {
    fn pdit_compress(
        ctx: *mut fz_context,
        data: *const u8,
        len: usize,
        level: i32,
        out: *mut *mut fz_buffer,
        err: *mut c_char,
        errlen: usize,
    ) -> i32;
}

/// `pdf` compressed at `level` (M-009): a new file; text, pages and fields are kept. A picture is only
/// re-encoded when that makes it smaller.
pub fn compress(pdf: &[u8], level: Level) -> Result<Vec<u8>, Error> {
    let ctx = mupdf::context::context();
    let mut out: *mut fz_buffer = std::ptr::null_mut();
    let mut err = [0 as c_char; 512];
    let level = match level {
        Level::BestQuality => 0,
        Level::Medium => 1,
        Level::Smallest => 2,
    };
    let code = unsafe {
        pdit_compress(
            ctx,
            pdf.as_ptr(),
            pdf.len(),
            level,
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    if code != 0 || out.is_null() {
        let msg = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) };
        return Err(Error::Compress(msg.to_string_lossy().into_owned()));
    }
    unsafe {
        let mut data: *mut u8 = std::ptr::null_mut();
        let len = mupdf_sys::fz_buffer_storage(ctx, out, &mut data);
        let bytes = std::slice::from_raw_parts(data, len).to_vec();
        mupdf_sys::fz_drop_buffer(ctx, out);
        Ok(bytes)
    }
}

unsafe extern "C" {
    #[allow(clippy::too_many_arguments)]
    fn pdit_set_page_labels(
        ctx: *mut fz_context,
        data: *const u8,
        len: usize,
        n: i32,
        firsts: *const i32,
        styles: *const i32,
        prefixes: *const *const c_char,
        starts: *const i32,
        out: *mut *mut fz_buffer,
        err: *mut c_char,
        errlen: usize,
    ) -> i32;
}

/// A page-number range for [`set_page_labels`]: from page `first` (from 0), `style` ('D' 1 2 3, 'r' i ii, 'R'
/// I II, 'a' a b, 'A' A B, 0 none), `prefix`, counting from `start`.
pub struct LabelRange {
    pub first: i32,
    pub style: u8,
    pub prefix: String,
    pub start: i32,
}

/// `pdf` with its page numbers (page labels, D-070) replaced by `ranges`; none: plain 1, 2, 3.
pub fn set_page_labels(pdf: &[u8], ranges: &[LabelRange]) -> Result<Vec<u8>, Error> {
    let ctx = mupdf::context::context();
    let prefixes: Vec<CString> = ranges
        .iter()
        .map(|r| CString::new(r.prefix.replace('\0', "")).unwrap_or_default())
        .collect();
    let prefix_ptrs: Vec<*const c_char> = prefixes.iter().map(|p| p.as_ptr()).collect();
    let firsts: Vec<i32> = ranges.iter().map(|r| r.first).collect();
    let styles: Vec<i32> = ranges.iter().map(|r| i32::from(r.style)).collect();
    let starts: Vec<i32> = ranges.iter().map(|r| r.start.max(1)).collect();
    let mut out: *mut fz_buffer = std::ptr::null_mut();
    let mut err = [0 as c_char; 512];
    let code = unsafe {
        pdit_set_page_labels(
            ctx,
            pdf.as_ptr(),
            pdf.len(),
            ranges.len() as i32,
            firsts.as_ptr(),
            styles.as_ptr(),
            prefix_ptrs.as_ptr(),
            starts.as_ptr(),
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    if code != 0 || out.is_null() {
        let msg = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) };
        return Err(Error::Compress(msg.to_string_lossy().into_owned()));
    }
    unsafe {
        let mut data: *mut u8 = std::ptr::null_mut();
        let len = mupdf_sys::fz_buffer_storage(ctx, out, &mut data);
        let bytes = std::slice::from_raw_parts(data, len).to_vec();
        mupdf_sys::fz_drop_buffer(ctx, out);
        Ok(bytes)
    }
}

unsafe extern "C" {
    #[allow(clippy::too_many_arguments)]
    fn pdit_convert_office(
        ctx: *mut fz_context,
        data: *const u8,
        len: usize,
        first: i32,
        last: i32,
        odt: i32,
        out: *mut *mut fz_buffer,
        err: *mut c_char,
        errlen: usize,
    ) -> i32;
}

/// Pages `first`..=`last` (from 0) of `pdf` as a Word (.docx) or, with `odt`, OpenDocument (.odt) file (D-071):
/// MuPDF's writer, with its picture and line-break bugs worked around (src/convert.c).
pub fn to_office(pdf: &[u8], first: i32, last: i32, odt: bool) -> Result<Vec<u8>, Error> {
    let ctx = mupdf::context::context();
    let mut out: *mut fz_buffer = std::ptr::null_mut();
    let mut err = [0 as c_char; 512];
    let code = unsafe {
        pdit_convert_office(
            ctx,
            pdf.as_ptr(),
            pdf.len(),
            first,
            last,
            i32::from(odt),
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        )
    };
    if code != 0 || out.is_null() {
        let msg = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) };
        return Err(Error::Compress(msg.to_string_lossy().into_owned()));
    }
    unsafe {
        let mut data: *mut u8 = std::ptr::null_mut();
        let len = mupdf_sys::fz_buffer_storage(ctx, out, &mut data);
        let bytes = std::slice::from_raw_parts(data, len).to_vec();
        mupdf_sys::fz_drop_buffer(ctx, out);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WRAPPED: &[u8] = include_bytes!("../../../fixtures/synthetic-wrapped.pdf");
    const FORM: &[u8] = include_bytes!("../../../fixtures/synthetic-form.pdf");
    const SCAN: &[u8] = include_bytes!("../../../fixtures/synthetic-scan.pdf");

    /// A zip package's entries (name, contents), read with MuPDF's archive code.
    fn entries(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
        let ctx = mupdf::context::context();
        unsafe {
            let stm = mupdf_sys::fz_open_memory(ctx, zip.as_ptr(), zip.len());
            let arch = mupdf_sys::fz_open_zip_archive_with_stream(ctx, stm);
            let out = (0..mupdf_sys::fz_count_archive_entries(ctx, arch))
                .map(|k| {
                    let name = mupdf_sys::fz_list_archive_entry(ctx, arch, k);
                    let buf = mupdf_sys::fz_read_archive_entry(ctx, arch, name);
                    let mut data: *mut u8 = std::ptr::null_mut();
                    let len = mupdf_sys::fz_buffer_storage(ctx, buf, &mut data);
                    let entry = (
                        std::ffi::CStr::from_ptr(name)
                            .to_string_lossy()
                            .into_owned(),
                        std::slice::from_raw_parts(data, len).to_vec(),
                    );
                    mupdf_sys::fz_drop_buffer(ctx, buf);
                    entry
                })
                .collect();
            mupdf_sys::fz_drop_archive(ctx, arch);
            mupdf_sys::fz_drop_stream(ctx, stm);
            out
        }
    }

    /// Word and OpenDocument: the text comes through as paragraphs, every page's picture is in the package, and
    /// no Word line break sits inside a text element (or in an .odt at all).
    #[test]
    fn office_files_are_whole() {
        const SPLIT: &[u8] = include_bytes!("../../../fixtures/synthetic-split-lines.pdf");
        for (input, last, pictures) in [(WRAPPED, 0, 0), (SPLIT, 0, 0), (SCAN, 1, 2)] {
            for odt in [false, true] {
                let file = to_office(input, 0, last, odt).unwrap();
                let entries = entries(&file);
                let (xml_name, media) = if odt {
                    ("content.xml", "Pictures/")
                } else {
                    ("word/document.xml", "word/media/")
                };
                let xml = entries
                    .iter()
                    .find(|(n, _)| n == xml_name)
                    .map(|(_, d)| String::from_utf8_lossy(d).into_owned())
                    .unwrap();
                let images = entries.iter().filter(|(n, _)| n.starts_with(media)).count();
                assert_eq!(
                    images,
                    pictures,
                    "odt {odt}: {:?}",
                    entries.iter().map(|e| &e.0).collect::<Vec<_>>()
                );
                if odt {
                    assert!(!xml.contains("<w:br/>"), "a Word break in an .odt");
                    assert_eq!(entries[0].0, "mimetype", "an .odt starts with its mimetype");
                    // Every picture fits the text width (6.5 in).
                    for frame in xml.split("<draw:frame").skip(1) {
                        let width: f32 = frame
                            .split("svg:width=\"")
                            .nth(1)
                            .and_then(|w| w.split("in\"").next())
                            .and_then(|w| w.parse().ok())
                            .unwrap();
                        assert!(width > 0.0 && width <= 6.5 + 1e-3, "frame {width} in");
                    }
                } else {
                    // Every picture has a size within the text width (6.5 in = 5943600 EMU).
                    for inline in xml.split("<wp:inline").skip(1) {
                        let cx: f64 = inline
                            .split("<wp:extent cx=\"")
                            .nth(1)
                            .and_then(|c| c.split('"').next())
                            .and_then(|c| c.parse().ok())
                            .expect("a size");
                        assert!(cx > 0.0 && cx <= 5_943_600.0, "picture {cx} EMU");
                        assert!(inline.contains("<a:ext cx="), "and its shape's size");
                    }
                    // Every <w:br/> is outside a text element.
                    for (at, _) in xml.match_indices("<w:br/>") {
                        let (open, close) = (xml[..at].rfind("<w:t"), xml[..at].rfind("</w:t>"));
                        assert!(open.is_none() || close > open, "break inside text at {at}");
                    }
                }
                if input == WRAPPED {
                    assert!(xml.contains("A separate short note stands alone."), "{xml}");
                }
            }
        }
    }

    /// Page numbers written by MuPDF read back as labels (MuPDF's own reader), and removing them leaves none.
    #[test]
    fn page_labels_are_written() {
        let label = |pdf: &[u8], page: i32| {
            let doc = PdfDocument::from_bytes(pdf).unwrap();
            let mut buf = [0 as c_char; 64];
            unsafe {
                mupdf_sys::pdf_page_label(
                    mupdf::context::context(),
                    doc.as_raw(),
                    page,
                    buf.as_mut_ptr(),
                    buf.len(),
                )
            };
            unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        };
        let two = compress(SCAN, Level::BestQuality).unwrap();
        let labelled = set_page_labels(
            &two,
            &[
                LabelRange {
                    first: 0,
                    style: b'r',
                    prefix: String::new(),
                    start: 1,
                },
                LabelRange {
                    first: 1,
                    style: b'D',
                    prefix: "P-".into(),
                    start: 7,
                },
            ],
        )
        .unwrap();
        assert_eq!(
            (label(&labelled, 0), label(&labelled, 1)),
            ("i".into(), "P-7".into())
        );
        let plain = set_page_labels(&labelled, &[]).unwrap();
        assert_eq!(
            (label(&plain, 0), label(&plain, 1)),
            // No labels left: readers number the pages 1, 2 themselves.
            (String::new(), String::new())
        );
    }

    /// Every level keeps the pages, the text and the form's fields, and never makes a file bigger.
    #[test]
    fn compress_keeps_pages_text_and_fields() {
        let fields = |pdf: &[u8]| {
            let doc = PdfDocument::from_bytes(pdf).unwrap();
            (0..doc.page_count().unwrap())
                .map(|i| doc.load_pdf_page(i).unwrap().widgets().count())
                .sum::<usize>()
        };
        for input in [WRAPPED, FORM, SCAN] {
            let before = Editor::open(input).unwrap();
            let pages = before.page_count().unwrap();
            let text: Vec<String> = (0..pages).map(|p| before.page_text(p).unwrap()).collect();
            for level in [Level::BestQuality, Level::Medium, Level::Smallest] {
                let out = compress(input, level).unwrap();
                let after = Editor::open(&out).unwrap();
                assert_eq!(after.page_count().unwrap(), pages, "{level:?}");
                for (p, t) in text.iter().enumerate() {
                    assert_eq!(&after.page_text(p as i32).unwrap(), t, "{level:?} page {p}");
                }
                assert_eq!(fields(&out), fields(input), "{level:?} fields");
                assert!(
                    out.len() <= input.len() + 64,
                    "{level:?}: {} -> {}",
                    input.len(),
                    out.len()
                );
            }
        }
        assert!(fields(FORM) >= 4, "the form fixture has its fields");
    }
    const NOTO: &[u8] = include_bytes!("../../pdit-app/assets/fonts/NotoSans-Regular.ttf");

    fn normalize(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn replaces_a_paragraph_and_reads_back_plainly() {
        let mut ed = Editor::open(WRAPPED).unwrap();
        let before = ed.page_text(0).unwrap();
        // The first paragraph: find a point inside its first line.
        let p = (0..800)
            .step_by(4)
            .find_map(|y| ed.paragraph_at(0, 80.0, y as f32).unwrap())
            .expect("a paragraph on the page");
        assert!(p.lines.len() >= 2, "the fixture's paragraph wraps: {p:?}");
        let new = p.text.replacen("reflow editor", "reflow text editor", 1);
        assert_ne!(new, p.text, "the fixture says 'reflow editor': {}", p.text);
        ed.replace_paragraph(&p, &new, &[FallbackFont { data: NOTO }])
            .unwrap();

        // Saved and reopened: the new text reads back with plain spaces and hyphens, the rest of the page as before.
        let bytes = ed.save().unwrap();
        let again = Editor::open(&bytes).unwrap();
        let after = again.page_text(0).unwrap();
        assert!(normalize(&after).contains("reflow text editor"), "{after}");
        assert!(
            !after.contains('\u{a0}') && !after.contains('\u{ad}'),
            "{after:?}"
        );
        // ponytail: the new text is drawn after the page's other content, so it reads last; reading order
        // matters for search order and AI, fix when moving the rest of pdit-core over.
        let others = |t: &str| {
            normalize(
                &normalize(t)
                    .replace(&normalize(&p.text), "")
                    .replace(&normalize(&new), ""),
            )
        };
        assert_eq!(
            others(&after),
            others(&before),
            "only the paragraph changed"
        );
    }
}
