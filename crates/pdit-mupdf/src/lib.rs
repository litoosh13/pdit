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
    PdfRedactTextMethod, PdfWriteOptions,
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
    /// Text is still in a redacted area of the saved file: the copy must not be used.
    RedactCheck(String),
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
            Error::RedactCheck(e) => write!(f, "redaction check failed: {e}"),
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
    /// Each line's text as MuPDF reads it (an older Persian PDF: in display order, see pdit-core `rtl`).
    pub line_texts: Vec<String>,
    /// The font name of the paragraph's first character, as MuPDF reports it (subset tag removed).
    pub font: String,
    /// Font size in points.
    pub size: f32,
    /// Distance between baselines, points (1.2 × size for a single line).
    pub line_pitch: f32,
    pub color: [u8; 3],
}

/// The size, line spacing (points) and colour of text laid out by the Story.
struct TextLook {
    size: f32,
    line_pitch: f32,
    color: [u8; 3],
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
        mark: *const c_char,
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
    /// Marked content put around text this editor adds (`mark_text_box`).
    mark: Option<CString>,
}

impl Editor {
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
        Ok(Self {
            doc: PdfDocument::from_bytes(bytes)?,
            mark: None,
        })
    }

    /// Text added from now on (`replace_paragraph`, `add_text`) is tagged as pdit's text box `number`
    /// (`/pditText <</box n>> BDC`, as pdit-core tags it), so it stays one movable box (D-081).
    pub fn mark_text_box(&mut self, number: i32) {
        self.mark = CString::new(format!("/pditText <</box {number}>> BDC")).ok();
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
            let mut line_texts = Vec::new();
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
                line_texts.push(s.to_owned());
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
                line_texts,
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

        // The paragraph's box, as wide as before (a little slack so the last word of a full line still fits),
        // free to grow down to the page's bottom margin.
        let [x0, y0, x1, _] = p.bounds;
        let place = fz_rect {
            x0,
            y0: y0 - 1.0,
            x1: x1 + 2.0,
            y1: page_bounds.y1 - 18.0,
        };
        let text = TextLook {
            size: fitted_size(p, new_text, own.as_deref(), fallbacks),
            line_pitch: p.line_pitch,
            color: p.color,
        };
        self.place_story(
            &mut page,
            own.as_deref(),
            place,
            &[new_text],
            &text,
            fallbacks,
        )
    }

    /// Adds `text` (paragraphs by line) on `page` in a box `width` points wide whose top-left is (`x`, `top`) in
    /// MuPDF's page space (from the visible area's top-left, y down), at `size` points in `color` (P-050 #16): the
    /// text is laid out by MuPDF's HTML (shaping, right-to-left paragraphs for Persian/Arabic) in the fallback
    /// fonts, each run in the first that has its letters.
    #[allow(clippy::too_many_arguments)]
    pub fn add_text(
        &mut self,
        page: i32,
        x: f32,
        top: f32,
        width: f32,
        text: &str,
        size: f32,
        color: [u8; 3],
        fallbacks: &[FallbackFont<'_>],
    ) -> Result<(), Error> {
        let mut page = PdfPage::try_from(self.doc.load_page(page)?)?;
        let page_bounds = page.bounds()?;
        page.wrap_contents(&mut self.doc)?;
        let place = fz_rect {
            x0: x,
            y0: top,
            x1: x + width,
            y1: page_bounds.y1 - 18.0,
        };
        let look = TextLook {
            size,
            line_pitch: size * 1.2,
            color,
        };
        let paragraphs: Vec<&str> = text.lines().collect();
        self.place_story(&mut page, None, place, &paragraphs, &look, fallbacks)
    }

    /// Lays `paragraphs` out in `place` on `page` with MuPDF's Story: the PDF's `own` font when it has every
    /// glyph, else the fallbacks, run by run; a right-to-left paragraph runs right-to-left.
    fn place_story(
        &mut self,
        page: &mut PdfPage,
        own: Option<&[u8]>,
        place: fz_rect,
        paragraphs: &[&str],
        look: &TextLook,
        fallbacks: &[FallbackFont<'_>],
    ) -> Result<(), Error> {
        let new_text = paragraphs.join("\n");
        let new_text = new_text.as_str();
        // The fonts the text may use, in order: the PDF's own, then the fallbacks. Each run of text gets the first
        // font that has its letters (MuPDF's HTML doesn't fall back along the CSS list).
        let mut fonts: Vec<(CString, &[u8])> = Vec::new();
        if let Some(data) = own {
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
        let [r, g, b] = look.color;
        css.push_str(&format!(
            "p {{font-family: {}; font-size: {}pt; line-height: {}pt; margin: 0; color: #{r:02x}{g:02x}{b:02x};}}",
            families.join(", "),
            look.size,
            look.line_pitch
        ));
        // One HTML paragraph per paragraph, each in its own direction; an empty one keeps its line.
        let html: String = paragraphs
            .iter()
            .map(|para| {
                let dir = if starts_rtl(para) { " dir=\"rtl\"" } else { "" };
                if para.trim().is_empty() {
                    "<p>&#160;</p>".to_owned()
                } else {
                    format!("<p{dir}>{}</p>", font_runs(para, &families, &loaded))
                }
            })
            .collect();

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
                self.mark.as_ref().map_or(std::ptr::null(), |m| m.as_ptr()),
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
        fix_story_unicode(page, new_fonts.split_whitespace())?;
        Ok(())
    }

    pub fn save(&self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.doc.write_to(&mut out)?;
        Ok(out)
    }
}

/// The size for `new_text` replacing `p` (P-050 #18). When the PDF's own font can't show a script that needs shaping
/// (an older Persian PDF's font has only letter shapes), a fallback draws it, and the paragraph's size would make it
/// look far bigger: the size is scaled so a letter is as wide, on average, as the old text's letters were.
fn fitted_size(
    p: &Paragraph,
    new_text: &str,
    own: Option<&[u8]>,
    fallbacks: &[FallbackFont<'_>],
) -> f32 {
    let has = |font: &mupdf::Font, c: char| font.encode_character(c as i32).is_ok_and(|g| g > 0);
    let shows = |font: &mupdf::Font| {
        new_text
            .chars()
            .filter(|c| !c.is_whitespace())
            .all(|c| has(font, c))
    };
    let own_shows = own
        .and_then(|data| mupdf::Font::from_bytes("own", data).ok())
        .is_some_and(|f| shows(&f));
    if !has_complex_script(new_text) || own_shows {
        return p.size;
    }
    let Some(font) = fallbacks
        .iter()
        .filter_map(|f| mupdf::Font::from_bytes("fallback", f.data).ok())
        .find(|f| shows(f))
    else {
        return p.size;
    };
    // Per letter: the old lines' width over their letters, against the new text's width per letter in the
    // fallback (the old text's letter shapes may not be in the fallback font at all).
    let letters = |t: &str| t.chars().filter(|c| !c.is_whitespace()).count() as f32;
    let em: f32 = new_text
        .chars()
        .filter(|c| !c.is_whitespace())
        .filter_map(|c| font.encode_character(c as i32).ok().filter(|g| *g > 0))
        .filter_map(|g| font.advance_glyph(g).ok())
        .sum();
    let old_width: f32 = p.lines.iter().map(|l| l[2] - l[0]).sum();
    let (old_letters, new_letters) = (letters(&p.text), letters(new_text));
    if em <= 0.0 || old_width <= 0.0 || old_letters == 0.0 || new_letters == 0.0 {
        return p.size;
    }
    let size = (old_width / old_letters) / (em / new_letters);
    size.clamp(0.4 * p.size, p.size)
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

/// An area to remove (D-086): a page from 0 and [x0, y0, x1, y1] in page space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RedactArea {
    pub page: i32,
    pub rect: [f32; 4],
}

/// `pdf` with `areas` really removed (RESEARCH.md 2026-11-03), behind black boxes: every glyph touching an area, the
/// pixels and lines under it, annotations and form fields in it. Always gone too, since they can repeat redacted
/// content: tags, XFA form data, the thumbnails of redacted pages and the descriptions (/Alt, /ActualText, /E) on those
/// pages. `hidden`: also the title and author, XMP,
/// bookmarks, attachments and private application data. The file is rewritten without unused objects or old revisions, and
/// read back: text still in an area is an error, so the copy is never handed out.
pub fn redact(pdf: &[u8], areas: &[RedactArea], hidden: bool) -> Result<Vec<u8>, Error> {
    let doc = PdfDocument::from_bytes(pdf)?;
    let mut pages: Vec<i32> = areas.iter().map(|a| a.page).collect();
    pages.sort_unstable();
    pages.dedup();
    for &number in &pages {
        let mut page = doc.load_pdf_page(number)?;
        let rects: Vec<Rect> = areas
            .iter()
            .filter(|a| a.page == number)
            .map(|a| Rect::new(a.rect[0], a.rect[1], a.rect[2], a.rect[3]))
            .collect();
        for rect in &rects {
            page.add_redact_annotation(*rect)?;
        }
        page.apply_redactions_with_options(PdfRedactOptions {
            black_boxes: true,
            image_method: PdfRedactImageMethod::Pixels,
            line_art: PdfRedactLineArtMethod::RemoveIfTouched,
            text: PdfRedactTextMethod::Remove,
        })?;
        // MuPDF's redaction leaves form fields alone.
        let mut doomed = Vec::new();
        for widget in page.widgets() {
            let bounds = widget.annotation().bounds()?;
            if rects.iter().any(|r| overlaps(r, &bounds)) {
                doomed.push(widget);
            }
        }
        for widget in doomed {
            forget_in_parents(widget.annotation().object())?;
            page.delete_widget(widget)?;
        }
        // Nor does it take every kind of annotation; a popup holds its parent's note.
        let mut popups = Vec::new();
        let mut comments = Vec::new();
        for annotation in page.annotations() {
            if rects.iter().any(|r| {
                overlaps(
                    r,
                    &annotation.bounds().unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
                )
            }) {
                if let Some(popup) = annotation.object().get_dict("Popup")? {
                    popups.push(popup.as_indirect()?);
                }
                comments.push(annotation);
            }
        }
        for annotation in comments {
            page.delete_annotation(annotation)?;
        }
        if let Some(mut list) = page.object().get_dict("Annots")? {
            for i in (0..list.len()? as i32).rev() {
                if list
                    .get_array(i)?
                    .is_some_and(|a| a.as_indirect().is_ok_and(|n| popups.contains(&n)))
                {
                    list.array_delete(i)?;
                }
            }
        }
        page.object().dict_delete("Thumb")?;
        forget_descriptions(&page.object(), 0)?;
    }
    let mut root = doc.catalog()?;
    for key in ["StructTreeRoot", "MarkInfo"] {
        root.dict_delete(key)?;
    }
    if let Some(mut form) = root.get_dict("AcroForm")? {
        form.dict_delete("XFA")?;
    }
    if hidden {
        doc.trailer()?.dict_delete("Info")?;
        for key in ["Metadata", "Outlines", "PieceInfo", "AF", "Collection"] {
            root.dict_delete(key)?;
        }
        if let Some(mut names) = root.get_dict("Names")? {
            names.dict_delete("EmbeddedFiles")?;
        }
        for number in 0..doc.page_count()? {
            let mut page = doc.load_pdf_page(number)?;
            for key in ["Metadata", "PieceInfo", "AF"] {
                page.object().dict_delete(key)?;
            }
            let attached: Vec<_> = page
                .annotations()
                .filter(|a| {
                    matches!(
                        a.r#type(),
                        Ok(mupdf::pdf::PdfAnnotationType::FileAttachment)
                    )
                })
                .collect();
            for annotation in attached {
                page.delete_annotation(annotation)?;
            }
        }
    }
    let mut options = PdfWriteOptions::default();
    options
        .set_garbage_level(4)
        .set_clean(true)
        .set_sanitize(true)
        .set_compress(true)
        .set_compress_images(true);
    let mut clean = Vec::new();
    doc.write_to_with_options(&mut clean, options)?;
    // Best quality keeps every picture as it is and drops the removed letters from embedded fonts.
    let out = compress(&clean, Level::BestQuality)?;
    check_redacted(&out, areas)?;
    Ok(out)
}

/// Marked-content descriptions on a page or form, and in what it uses: they describe what was there, also for what's
/// left of a partly redacted picture.
fn forget_descriptions(owner: &mupdf::pdf::PdfObject, depth: u32) -> Result<(), Error> {
    if let Some(mut contents) = owner.get_dict("Contents")? {
        if contents.is_array()? {
            for i in 0..contents.len()? as i32 {
                if let Some(mut part) = contents.get_array(i)? {
                    forget_in_stream(&mut part)?;
                }
            }
        } else {
            forget_in_stream(&mut contents)?;
        }
    }
    let Some(resources) = owner.get_dict_inheritable("Resources")? else {
        return Ok(());
    };
    if let Some(properties) = resources.get_dict("Properties")? {
        for i in 0..properties.dict_len()? as i32 {
            if let Some(mut property) = properties.get_dict_val(i)? {
                for key in DESCRIPTIONS {
                    property.dict_delete(key)?;
                }
            }
        }
    }
    // Forms can hold forms; a loop of them ends here.
    if let Some(objects) = resources.get_dict("XObject")?
        && depth < 8
    {
        for i in 0..objects.dict_len()? as i32 {
            let Some(mut object) = objects.get_dict_val(i)? else {
                continue;
            };
            object.dict_delete("Alt")?;
            let form = object
                .get_dict("Subtype")?
                .is_some_and(|s| s.as_name().is_ok_and(|n| n == b"Form"));
            if form {
                forget_in_stream(&mut object)?;
                forget_descriptions(&object, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn forget_in_stream(stream: &mut mupdf::pdf::PdfObject) -> Result<(), Error> {
    if let Some(clean) = strip_descriptions(&stream.read_stream()?) {
        stream.write_stream_buffer(&mupdf::Buffer::from_bytes(&clean)?)?;
    }
    Ok(())
}

const DESCRIPTIONS: [&str; 3] = ["Alt", "ActualText", "E"];

/// A content stream without descriptions in the dictionaries of its BDC operators; None if it has none.
fn strip_descriptions(c: &[u8]) -> Option<Vec<u8>> {
    let delimiter = |b: u8| b.is_ascii_whitespace() || b"()<>[]{}/%".contains(&b);
    let mut out = Vec::new();
    let mut copied = 0;
    let mut i = 0;
    while i < c.len() {
        if c[i] == b'<' && c.get(i + 1) == Some(&b'<') {
            let end = skip_object(c, i)?;
            let mut op = end;
            while c.get(op).is_some_and(|b| b.is_ascii_whitespace()) {
                op += 1;
            }
            if c[op..].starts_with(b"BDC") && c.get(op + 3).is_none_or(|&b| delimiter(b)) {
                out.extend_from_slice(&c[copied..i]);
                strip_dict(&c[i..end], &mut out)?;
                copied = end;
            }
            i = end;
        } else if matches!(c[i], b'(' | b'<' | b'[' | b'/') {
            i = skip_object(c, i)?;
        } else if c[i] == b'%' {
            while i < c.len() && c[i] != b'\n' && c[i] != b'\r' {
                i += 1;
            }
        } else if c[i..].starts_with(b"ID")
            && (i == 0 || delimiter(c[i - 1]))
            && c.get(i + 2).is_some_and(|&b| delimiter(b))
        {
            // An inline image's data is binary: on to its EI.
            i += 2;
            loop {
                let ei = c[i..].windows(2).position(|w| w == b"EI")? + i;
                i = ei + 2;
                if c[ei - 1].is_ascii_whitespace() && c.get(ei + 2).is_none_or(|&b| delimiter(b)) {
                    break;
                }
            }
        } else {
            i += 1;
        }
    }
    if out.is_empty() {
        return None;
    }
    out.extend_from_slice(&c[copied..]);
    (out != c).then_some(out)
}

/// `dict` (from << to >>) without the description keys.
fn strip_dict(dict: &[u8], out: &mut Vec<u8>) -> Option<()> {
    out.extend_from_slice(b"<<");
    let end = dict.len() - 2;
    let mut i = 2;
    while i < end {
        if dict[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let key_end = skip_object(dict, i)?;
        let mut value = key_end;
        while dict[value].is_ascii_whitespace() {
            value += 1;
        }
        let value_end = skip_object(dict, value)?;
        let name = String::from_utf8_lossy(&dict[i + 1..key_end])
            .replace("#6C", "l")
            .replace("#6c", "l");
        if !DESCRIPTIONS.contains(&name.as_str()) {
            out.push(b' ');
            out.extend_from_slice(&dict[i..value_end]);
        }
        i = value_end;
    }
    out.extend_from_slice(b" >>");
    Some(())
}

/// The end of the object starting at `i`; None if the stream ends first.
fn skip_object(c: &[u8], mut i: usize) -> Option<usize> {
    let regular = |b: u8| !b.is_ascii_whitespace() && !b"()<>[]{}/%".contains(&b);
    match *c.get(i)? {
        b'(' => {
            let mut depth = 0;
            loop {
                match *c.get(i)? {
                    b'\\' => i += 1,
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        b'<' if c.get(i + 1) == Some(&b'<') => {
            i += 2;
            loop {
                while c.get(i)?.is_ascii_whitespace() {
                    i += 1;
                }
                if c[i..].starts_with(b">>") {
                    return Some(i + 2);
                }
                i = skip_object(c, i)?;
            }
        }
        b'<' => Some(i + c[i..].iter().position(|&b| b == b'>')? + 1),
        b'[' => {
            i += 1;
            loop {
                while c.get(i)?.is_ascii_whitespace() {
                    i += 1;
                }
                if c[i] == b']' {
                    return Some(i + 1);
                }
                i = skip_object(c, i)?;
            }
        }
        b'/' => {
            i += 1;
            while c.get(i).is_some_and(|&b| regular(b)) {
                i += 1;
            }
            Some(i)
        }
        b if regular(b) => {
            while c.get(i).is_some_and(|&b| regular(b)) {
                i += 1;
            }
            Some(i)
        }
        _ => None,
    }
}

/// A grouped field's name and value live on its parents, which outlive the widget. Other widgets of the group keep
/// showing what they drew, but the value itself goes.
fn forget_in_parents(mut field: mupdf::pdf::PdfObject) -> Result<(), Error> {
    while let Some(mut parent) = field.get_dict("Parent")? {
        for key in ["V", "DV"] {
            parent.dict_delete(key)?;
        }
        if let Some(mut kids) = parent.get_dict("Kids")? {
            let number = field.as_indirect()?;
            for i in (0..kids.len()? as i32).rev() {
                if kids
                    .get_array(i)?
                    .is_some_and(|k| k.as_indirect().ok() == Some(number))
                {
                    kids.array_delete(i)?;
                }
            }
            if kids.len()? == 0 {
                for key in ["T", "TU", "TM", "Opt"] {
                    parent.dict_delete(key)?;
                }
            }
        }
        field = parent;
    }
    Ok(())
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}

/// No character's centre is in an area of the saved file.
fn check_redacted(pdf: &[u8], areas: &[RedactArea]) -> Result<(), Error> {
    let doc = PdfDocument::from_bytes(pdf)?;
    for area in areas {
        let text = doc
            .load_pdf_page(area.page)?
            .to_text_page(TextPageFlags::empty())?;
        let [x0, y0, x1, y1] = area.rect;
        for block in text.blocks() {
            for line in block.lines() {
                for c in line.chars() {
                    let q = c.quad();
                    let (x, y) = ((q.ul.x + q.lr.x) / 2.0, (q.ul.y + q.lr.y) / 2.0);
                    if x > x0 && x < x1 && y > y0 && y < y1 {
                        return Err(Error::RedactCheck(format!("page {}", area.page + 1)));
                    }
                }
            }
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
    /// The area of a word on a page, as the page shows it (also on a rotated page).
    fn word_area(pdf: &[u8], page: i32, word: &str) -> RedactArea {
        let doc = PdfDocument::from_bytes(pdf).unwrap();
        let text = doc
            .load_pdf_page(page)
            .unwrap()
            .to_text_page(TextPageFlags::empty())
            .unwrap();
        for block in text.blocks() {
            for line in block.lines() {
                let chars: Vec<_> = line.chars().collect();
                let s: String = chars.iter().filter_map(|c| c.char()).collect();
                if let Some(at) = s.find(word) {
                    let (first, last) = (chars[at].quad(), chars[at + word.len() - 1].quad());
                    let xs = [
                        first.ul.x, first.ll.x, last.ur.x, last.lr.x, first.ur.x, last.ul.x,
                    ];
                    let ys = [
                        first.ul.y, first.ur.y, last.ll.y, last.lr.y, first.ll.y, last.ul.y,
                    ];
                    let min = |v: &[f32]| v.iter().cloned().fold(f32::MAX, f32::min);
                    let max = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max);
                    return RedactArea {
                        page,
                        rect: [min(&xs), min(&ys) + 2.0, max(&xs), max(&ys) - 2.0],
                    };
                }
            }
        }
        panic!("{word} is not on page {page}");
    }

    /// The file with its streams unpacked, as bytes to search.
    fn unpacked(pdf: &[u8]) -> String {
        let doc = PdfDocument::from_bytes(pdf).unwrap();
        let mut options = PdfWriteOptions::default();
        options.set_decompress(true);
        let mut out = Vec::new();
        doc.write_to_with_options(&mut out, options).unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    const REDACT: &[u8] = include_bytes!("../../../fixtures/synthetic-redact.pdf");
    /// Everything marked on the redaction fixture. y counts from the top of the 792 pt page.
    fn redact_areas() -> Vec<RedactArea> {
        let r = |x0: f32, y0: f32, x1: f32, y1: f32| RedactArea {
            page: 0,
            rect: [x0, 792.0 - y1, x1, 792.0 - y0],
        };
        vec![
            word_area(REDACT, 0, "SECRET-MIDLINE"),
            r(70.0, 656.0, 300.0, 674.0),  // kerned
            r(70.0, 636.0, 300.0, 652.0),  // invisible
            r(70.0, 616.0, 300.0, 633.0),  // under a white cover
            r(70.0, 587.0, 150.0, 604.0),  // ActualText
            r(70.0, 567.0, 150.0, 584.0),  // Alt
            r(70.0, 537.0, 300.0, 555.0),  // form XObject
            r(70.0, 517.0, 300.0, 534.0),  // hidden layer
            r(320.0, 300.0, 520.0, 330.0), // pattern
            r(330.0, 466.0, 420.0, 484.0), // ActualText in a named property
            r(72.0, 300.0, 172.0, 500.0),  // left half of the picture
            r(330.0, 700.0, 560.0, 730.0), // FreeText
            r(330.0, 660.0, 560.0, 690.0), // link
            r(330.0, 620.0, 560.0, 650.0), // field
            r(330.0, 580.0, 560.0, 610.0), // field with a parent
            r(330.0, 540.0, 560.0, 570.0), // highlight
            word_area(REDACT, 2, "SECRET-ROTATED"),
        ]
    }

    #[test]
    fn redact_removes_what_is_marked_and_nothing_else() {
        let out = redact(REDACT, &redact_areas(), false).unwrap();
        let file = unpacked(&out);
        for gone in [
            "SECRET-MIDLINE",
            "-KERNED",
            "SECRET-INVISIBLE",
            "SECRET-COVERED",
            "SECRET-ACTUALTEXT",
            "SECRET-ALT-TEXT",
            "SECRET-FORMXOBJECT",
            "SECRET-LAYER",
            "SECRET-PATTERN",
            "SECRET-ANNOT-INSIDE",
            "SECRET-LINK",
            "SECRET-FIELDNAME",
            "SECRET-FIELDVALUE",
            "SECRET-PARENTFIELD",
            "SECRET-KIDVALUE",
            "SECRET-HIGHLIGHT",
            "SECRET-POPUP",
            "SECRET-ROTATED",
            "SECRET-STRUCT",
            "SECRET-XFA",
            "SECRET-THUMB",
            "SECRET-FIGURE-ALT",
            "SECRET-NAMED",
            "SECRET-FORM-ALT",
            "SECRET-OLDREVISION",
            "SECRET-ORPHAN",
        ] {
            assert!(!file.contains(gone), "{gone} is still in the file");
        }
        for kept in [
            "KEEP-LINE-ONE",
            "keep-after",
            "KEEP-LINE-LAST",
            "KEEP-PAGE-TWO",
            "KEEP-ROTATED",
            "SECRET-OTHERPAGE",
            "SECRET-ANNOT-OUTSIDE",
            "SECRET-INFO-TITLE",
            "SECRET-BOOKMARK",
            "SECRET-ATTACHED-CONTENT",
        ] {
            assert!(file.contains(kept), "{kept} was removed");
        }
    }

    #[test]
    fn descriptions_go_and_the_rest_of_a_stream_stays() {
        // A BDC word and an unbalanced parenthesis in an inline image's data mustn't derail the reading.
        let stream = b"/Span <</ActualText (a \\) b (c)) /Lang (fa) /Alt <41> >> BDC q BI /W 1 /H 1 ID (BDC <</Alt (x)>> BDC EI Q EMC /P <</E(y)>>BDC EMC";
        let out = String::from_utf8_lossy(&strip_descriptions(stream).unwrap()).into_owned();
        assert_eq!(
            out,
            "/Span << /Lang (fa) >> BDC q BI /W 1 /H 1 ID (BDC <</Alt (x)>> BDC EI Q EMC /P << >>BDC EMC"
        );
        assert_eq!(strip_descriptions(b"q (<</Alt (x)>> BDC) Tj Q"), None);
        assert_eq!(
            strip_descriptions(b"/P <</Lang (fa)>> BDC EMC").map(|_| ()),
            Some(())
        );
    }

    #[test]
    fn redact_can_remove_hidden_data_too() {
        let file = unpacked(&redact(REDACT, &redact_areas(), true).unwrap());
        for gone in [
            "SECRET-INFO-TITLE",
            "SECRET-XMP",
            "SECRET-BOOKMARK",
            "SECRET-ATTACH",
        ] {
            assert!(!file.contains(gone), "{gone} is still in the file");
        }
        assert!(file.contains("KEEP-LINE-ONE"));
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

    /// A text box (P-050 #16): English and Persian paragraphs added on a blank part of the page. The English
    /// reads back as written; the Persian is there (MuPDF's shaped fonts read back as Arabic presentation forms).
    #[test]
    fn adds_text_in_two_scripts() {
        const VAZIRMATN: &[u8] =
            include_bytes!("../../../desktop/fonts/vazirmatn/Vazirmatn-Regular.ttf");
        let mut ed = Editor::open(WRAPPED).unwrap();
        let fonts = [
            FallbackFont { data: NOTO },
            FallbackFont { data: VAZIRMATN },
        ];
        let text = "A new note in English that wraps inside its box.\n\nروزانه و ۴ساعته نزولی است.";
        ed.add_text(0, 72.0, 500.0, 200.0, text, 12.0, [0, 0, 0], &fonts)
            .unwrap();
        let saved = ed.save().unwrap();
        if let Some(out) = std::env::var_os("PDIT_DEBUG_OUT") {
            std::fs::write(out, &saved).unwrap();
        }
        let after = Editor::open(&saved).unwrap().page_text(0).unwrap();
        let flat = normalize(&after);
        assert!(
            flat.contains("A new note in English that wraps inside its box."),
            "{after}"
        );
        let arabic = after
            .chars()
            .filter(|c| matches!(*c as u32, 0x0600..=0x06FF | 0xFB50..=0xFEFF))
            .count();
        assert!(arabic >= 15, "the Persian letters are on the page: {after}");
        // The original text is still there.
        assert!(
            flat.contains("A separate short note stands alone."),
            "{after}"
        );
    }
}
