//! Editing one text object of the open document, or adding a new one, with a
//! preview that can be kept or discarded (the "Keep" / "Discard" of the
//! editing bar).

use crate::edit::EditMethod;
use crate::inspect::rect;
use crate::render::with_open;
use crate::{Error, reserve_for_pdfium_copy};
use pdfium_render::prelude::*;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;

/// One text object on a page, for finding what the user clicked.
#[derive(Debug, Clone, Serialize)]
pub struct TextLine {
    pub object_index: usize,
    pub text: String,
    /// Left, bottom, right, top in PDF points.
    pub bounds: [f32; 4],
}

/// The state of a previewed (not yet kept) edit.
#[derive(Debug, Clone, Serialize)]
pub struct EditPreview {
    pub method: EditMethod,
    /// The text PDFium reads back from the edited object.
    pub text: String,
    pub bounds: [f32; 4],
}

/// A new text object added as a preview (D-026).
#[derive(Debug, Clone, Serialize)]
pub struct AddPreview {
    /// Where the new object sits in the page's object list.
    pub object_index: usize,
    pub preview: EditPreview,
}

/// What is needed to undo a previewed edit: the original objects, taken off
/// the page untouched (none for added text). They are put back on discard
/// (exactly as they were) and freed on keep.
struct Pending {
    page: u16,
    index: usize,
    /// Objects taken off the page from `index` on (the text, then its pdit
    /// underline if it had one), to put back in order.
    removed: Vec<PdfPageObject<'static>>,
    /// How many objects the preview put at `index` (the text, plus its
    /// underline when underlined).
    added: usize,
}

thread_local! {
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
    /// Fallback fonts embedded in the open document, by the characters their
    /// subset holds (D-028), so the same characters reuse the same font.
    static FALLBACK: RefCell<HashMap<String, PdfFontToken>> = RefCell::new(HashMap::new());
}

/// Called when the open document changes: pending edits and the cached font
/// belong to the old document.
pub(crate) fn forget_document() {
    PENDING.with_borrow_mut(|pending| *pending = None);
    FALLBACK.with_borrow_mut(|fonts| fonts.clear());
}

/// Lists the text objects on page `page` of the open document.
pub fn text_lines(page: u16) -> Result<Vec<TextLine>, Error> {
    with_open(|document| {
        let page = document.pages().get(page.into())?;
        let mut lines = Vec::new();
        for (object_index, object) in page.objects().iter().enumerate() {
            if let Some(text) = object.as_text_object() {
                lines.push(TextLine {
                    object_index,
                    text: text.text(),
                    bounds: rect(object.bounds()?),
                });
            }
        }
        Ok(lines)
    })
}

/// A paragraph block: consecutive text lines that share a left margin, font size
/// and line spacing, joined into one editable unit for reflow (D-035 later item).
#[derive(Debug, Clone, Serialize)]
pub struct Paragraph {
    /// The block's text-object indices, in reading order (top to bottom).
    pub object_indices: Vec<usize>,
    /// The lines joined into one string (a space at each wrap; a trailing hyphen
    /// joins with none).
    pub text: String,
    /// Combined bounds [left, bottom, right, top] in PDF points.
    pub bounds: [f32; 4],
}

/// The paragraph block containing the text line at (`x`, `y`) PDF points on
/// `page`, if any. The clicked line is grown up and down while the neighbour
/// shares its left margin and font size, is spaced like a text line, and the
/// line it wraps from is "full" (reaches near the block's right margin) — so a
/// wrapped sentence groups into one block, but a short standalone line, a
/// blank-line gap, or a differently-placed line ends it.
pub fn paragraph_at(page: u16, x: f32, y: f32) -> Result<Option<Paragraph>, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        struct Line {
            index: usize,
            text: String,
            l: f32,
            b: f32,
            r: f32,
            t: f32,
            size: f32,
        }
        let mut lines = Vec::new();
        for (index, object) in pdf_page.objects().iter().enumerate() {
            if let Some(text) = object.as_text_object() {
                let [l, b, r, t] = rect(object.bounds()?);
                lines.push(Line {
                    index,
                    text: text.text(),
                    l,
                    b,
                    r,
                    t,
                    size: text.scaled_font_size().value,
                });
            }
        }
        // The clicked line: topmost (last in object order) whose bounds hold the point.
        let Some(click) = lines
            .iter()
            .rposition(|ln| x >= ln.l && x <= ln.r && y >= ln.b && y <= ln.t)
        else {
            return Ok(None);
        };
        // Reading order: top to bottom (descending top).
        let mut order: Vec<usize> = (0..lines.len()).collect();
        order.sort_by(|&a, &b| lines[b].t.total_cmp(&lines[a].t));
        let pos = order.iter().position(|&i| i == click).unwrap();
        let base = &lines[click];
        let size = base.size.max(1.0);
        let aligned = |a: &Line| (a.l - base.l).abs() <= 3.0 && (a.size - size).abs() <= 0.2 * size;
        let spaced = |upper: &Line, lower: &Line| {
            let d = upper.t - lower.t;
            d > 0.6 * size && d < 2.2 * size
        };
        // The raw run of consecutive aligned, text-spaced lines around the click.
        let mut run_start = pos;
        while run_start > 0
            && aligned(&lines[order[run_start - 1]])
            && spaced(&lines[order[run_start - 1]], &lines[order[run_start]])
        {
            run_start -= 1;
        }
        let mut run_end = pos;
        while run_end + 1 < order.len()
            && aligned(&lines[order[run_end + 1]])
            && spaced(&lines[order[run_end]], &lines[order[run_end + 1]])
        {
            run_end += 1;
        }
        // Within the run, group by sentence: a line continues onto the next only
        // when it is "full" (reaches near the run's right margin, so it wrapped
        // for lack of room) and does not already finish a sentence (. ! ?). So a
        // wrapped sentence groups down to its period, while separate sentences or
        // a short line each stand alone.
        let run_max = order[run_start..=run_end]
            .iter()
            .map(|&i| lines[i].r)
            .fold(f32::MIN, f32::max);
        let slack = (2.5 * size).max(0.12 * (run_max - base.l));
        let wraps =
            |a: &Line| a.r >= run_max - slack && !a.text.trim_end().ends_with(['.', '!', '?']);
        let mut start = pos;
        while start > run_start && wraps(&lines[order[start - 1]]) {
            start -= 1;
        }
        let mut end = pos;
        while end < run_end && wraps(&lines[order[end]]) {
            end += 1;
        }
        let mut text = String::new();
        let (mut l, mut b, mut r, mut t) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (k, &oi) in order[start..=end].iter().enumerate() {
            let ln = &lines[oi];
            if k > 0 {
                if text.ends_with('-') {
                    text.pop();
                } else {
                    text.push(' ');
                }
            }
            text.push_str(ln.text.trim());
            l = l.min(ln.l);
            b = b.min(ln.b);
            r = r.max(ln.r);
            t = t.max(ln.t);
        }
        let object_indices = order[start..=end]
            .iter()
            .map(|&oi| lines[oi].index)
            .collect();
        Ok(Some(Paragraph {
            object_indices,
            text,
            bounds: [l, b, r, t],
        }))
    })
}

/// Which font a line uses (D-027).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FontChoice {
    /// The font the document already uses for that text (Noto Sans if it
    /// cannot show the characters).
    Document,
    /// The bundled Noto Sans.
    NotoSans,
}

/// How a text line looks (D-027). Bold and italic are simulated: bold is the
/// fill plus a thin stroke in the same colour, italic is a slant.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TextStyle {
    pub font: FontChoice,
    /// Font size in points, as it appears on the page.
    pub size: f32,
    pub color: [u8; 3],
    pub bold: bool,
    pub italic: bool,
    /// A pdit underline: a thin bar under upright text, in the text colour.
    pub underline: bool,
}

/// What the PDF says about a text object's font (D-028), for choosing a
/// look-alike when that font cannot show new text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FontTraits {
    /// The font's name without a subset prefix (`ABCDEF+Arial-BoldMT` →
    /// `Arial-BoldMT`).
    pub name: String,
    pub bold: bool,
    pub italic: bool,
    pub serif: bool,
    pub fixed_pitch: bool,
    /// Symbol and dingbat fonts: no look-alike applies.
    pub symbolic: bool,
}

impl FontTraits {
    fn of(font: &PdfFont<'_>) -> Self {
        let full = font.name();
        let name = match full.split_once('+') {
            Some((prefix, rest))
                if prefix.len() == 6 && prefix.chars().all(|c| c.is_ascii_uppercase()) =>
            {
                rest.to_owned()
            }
            _ => full,
        };
        let lower = name.to_ascii_lowercase();
        let heavy_name = ["bold", "black", "heavy", "semibold", "demi"]
            .iter()
            .any(|w| lower.contains(w));
        let heavy_weight = match font.weight() {
            Ok(PdfFontWeight::Weight600)
            | Ok(PdfFontWeight::Weight700Bold)
            | Ok(PdfFontWeight::Weight800)
            | Ok(PdfFontWeight::Weight900) => true,
            Ok(PdfFontWeight::Custom(weight)) => weight >= 600,
            _ => false,
        };
        FontTraits {
            bold: heavy_name || heavy_weight || font.is_bold_reenforced(),
            italic: font.is_italic()
                || font.italic_angle().is_ok_and(|angle| angle != 0)
                || lower.contains("italic")
                || lower.contains("oblique"),
            serif: font.is_serif(),
            fixed_pitch: font.is_fixed_pitch(),
            symbolic: font.is_symbolic(),
            name,
        }
    }
}

/// The fonts new or replaced text can fall back to when a PDF font cannot
/// show it: a bundled look-alike of that font (D-028), then Noto Sans.
#[derive(Debug, Clone, Copy)]
pub struct Fonts<'a> {
    pub look_alike: Option<LookAlike<'a>>,
    pub noto_sans: &'a [u8],
}

impl<'a> Fonts<'a> {
    /// Noto Sans only.
    pub fn noto(noto_sans: &'a [u8]) -> Self {
        Fonts {
            look_alike: None,
            noto_sans,
        }
    }
}

/// A bundled look-alike face and whether it is a real bold / italic face (so
/// those are not simulated on top).
#[derive(Debug, Clone, Copy)]
pub struct LookAlike<'a> {
    pub bytes: &'a [u8],
    pub bold: bool,
    pub italic: bool,
}

/// Slant of simulated italic: tan(12°).
const ITALIC_SLANT: f32 = 0.2126;
/// Stroke width of simulated bold, as a share of the font size.
const BOLD_STROKE: f32 = 0.03;
/// pdit's underline: its top sits this far below the baseline, and it is this
/// thick, as shares of the font size.
const UNDERLINE_GAP: f32 = 0.08;
const UNDERLINE_THICKNESS: f32 = 0.05;

/// The look a new or replaced text object starts from: copied from an existing
/// object, or the defaults for a page without text.
struct Base {
    /// The object's matrix without pdit's italic slant.
    matrix: PdfMatrix,
    fill: PdfColor,
    /// Font size before the matrix scales it.
    size: PdfPoints,
    font: Option<PdfFontToken>,
    /// The font's traits (none for the defaults of a page without text).
    traits: Option<FontTraits>,
    /// Drawn with pdit's simulated bold (fill + stroke).
    stroked: bool,
    /// Drawn with pdit's simulated italic slant (removed from `matrix`).
    slanted: bool,
    /// Set by [`with_underline`] when the next object is pdit's underline.
    underline: bool,
}

impl Base {
    fn of(object: &PdfPageObject<'_>) -> Result<Option<Self>, Error> {
        let Some(text) = object.as_text_object() else {
            return Ok(None);
        };
        let mut matrix = object.matrix()?;
        // pdit's own simulated italic (see make_text): unslant it, so it can be
        // turned off and is never applied twice.
        let slant_c = matrix.c() - matrix.a() * ITALIC_SLANT;
        let slant_d = matrix.d() - matrix.b() * ITALIC_SLANT;
        // Cosine of the angle between the text's x and y axes: 0 for upright
        // text, about 0.2 with pdit's slant.
        let (a, b) = (matrix.a(), matrix.b());
        let cosine = |c: f32, d: f32| (a * c + b * d) / (a.hypot(b) * c.hypot(d)).max(f32::EPSILON);
        let slanted =
            cosine(matrix.c(), matrix.d()).abs() > 0.1 && cosine(slant_c, slant_d).abs() < 0.01;
        if slanted {
            matrix.set_c(slant_c);
            matrix.set_d(slant_d);
        }
        Ok(Some(Base {
            matrix,
            fill: object.fill_color()?,
            size: text.unscaled_font_size(),
            font: Some(text.font().token()),
            traits: Some(FontTraits::of(&text.font())),
            stroked: text.render_mode() == PdfPageTextRenderMode::FilledThenStroked,
            slanted,
            underline: false,
        }))
    }

    /// How much the matrix scales text vertically.
    fn scale(&self) -> f32 {
        let scale = self.matrix.c().hypot(self.matrix.d());
        if scale > 0.0 { scale } else { 1.0 }
    }

    fn style(&self) -> TextStyle {
        TextStyle {
            font: FontChoice::Document,
            size: self.size.value * self.scale(),
            color: [self.fill.red(), self.fill.green(), self.fill.blue()],
            bold: self.stroked || self.traits.as_ref().is_some_and(|t| t.bold),
            italic: self.slanted || self.traits.as_ref().is_some_and(|t| t.italic),
            underline: self.underline,
        }
    }
}

/// The traits of text object `index`'s font on `page`.
pub fn font_traits(page: u16, index: usize) -> Result<Option<FontTraits>, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        Ok(with_underline(&pdf_page, index)?.traits)
    })
}

/// The traits of the font new text at (`x`, `y`) on `page` would copy (the
/// nearest text's), if the page has text.
pub fn font_traits_near(page: u16, x: f32, y: f32) -> Result<Option<FontTraits>, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        Ok(nearest_base(&pdf_page, x, y)?.traits)
    })
}

/// The current look of text object `index` on `page`.
pub fn text_style(page: u16, index: usize) -> Result<TextStyle, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        Ok(with_underline(&pdf_page, index)?.style())
    })
}

/// The text object `index` on `page` as a [`Base`], noting whether the object
/// after it is pdit's underline for it.
fn with_underline(pdf_page: &PdfPage<'_>, index: usize) -> Result<Base, Error> {
    let objects = pdf_page.objects();
    let object = objects.get(index)?;
    let mut base =
        Base::of(&object)?.ok_or_else(|| Error::Pdfium(format!("object {index} is not text")))?;
    if index + 1 < objects.len() {
        let text_bounds = rect(object.bounds()?);
        base.underline = is_underline_for(&base, text_bounds, &objects.get(index + 1)?)?;
    }
    Ok(base)
}

/// pdit marks nothing inside the file, so its underline is recognised by
/// shape: a filled bar right after upright text, in the text's colour, as wide
/// as the text, just under the baseline.
fn is_underline_for(
    base: &Base,
    text: [f32; 4],
    candidate: &PdfPageObject<'_>,
) -> Result<bool, Error> {
    if candidate.as_path_object().is_none() || !is_upright(&base.matrix) {
        return Ok(false);
    }
    let [left, bottom, right, top] = rect(candidate.bounds()?);
    let size = base.size.value * base.scale();
    let baseline = base.matrix.f();
    let fill = candidate.fill_color()?;
    Ok((left - text[0]).abs() < 1.5
        && (right - text[2]).abs() < 1.5
        && top - bottom <= size * 0.12
        && top <= baseline
        && top >= baseline - size * 0.3
        && (fill.red(), fill.green(), fill.blue())
            == (base.fill.red(), base.fill.green(), base.fill.blue()))
}

/// Text that runs left to right without rotation (a slant is fine).
fn is_upright(matrix: &PdfMatrix) -> bool {
    matrix.a() > 0.0 && matrix.b().abs() < 1e-3 * matrix.a()
}

/// Text that [`insert_text`] placed on the page.
struct Placed {
    method: EditMethod,
    bounds: [f32; 4],
    /// Objects added at the index: the text, plus its underline if any.
    added: usize,
}

/// Makes a text object in `style`, placed by `base`'s matrix. Bold and italic
/// are simulated only where the font face isn't already bold / italic.
fn make_text(
    document: &PdfDocument<'static>,
    text: &str,
    font: PdfFontToken,
    (face_bold, face_italic): (bool, bool),
    base: &Base,
    style: &TextStyle,
) -> Result<PdfPageTextObject<'static>, Error> {
    let scale = base.scale();
    let mut matrix = base.matrix;
    if style.italic && !face_italic {
        // Shear in text space: x' = x + slant * y.
        matrix.set_c(matrix.c() + matrix.a() * ITALIC_SLANT);
        matrix.set_d(matrix.d() + matrix.b() * ITALIC_SLANT);
    }
    let size = PdfPoints::new(style.size / scale);
    let [red, green, blue] = style.color;
    let color = PdfColor::new(red, green, blue, base.fill.alpha());
    let mut object = PdfPageTextObject::new(document, text, font, size)?;
    object.reset_matrix(matrix)?;
    object.set_fill_color(color)?;
    if style.bold && !face_bold {
        object.set_render_mode(PdfPageTextRenderMode::FilledThenStroked)?;
        object.set_stroke_color(color)?;
        object.set_stroke_width(PdfPoints::new(size.value * BOLD_STROKE))?;
    }
    Ok(object)
}

/// Inserts `text` at `index`, trying the base's own font and then Noto Sans
/// (only Noto Sans when the style asks for it), keeping the first whose
/// readback matches. Returns the method and bounds, or what the last attempt
/// read back.
fn insert_text(
    document: &mut PdfDocument<'static>,
    pdf_page: &mut PdfPage<'static>,
    index: usize,
    text: &str,
    base: &Base,
    style: Option<&TextStyle>,
    fonts: &Fonts<'_>,
) -> Result<Result<Placed, String>, Error> {
    let style = style.copied().unwrap_or(base.style());
    // The fonts to try, in order: the object's own font (unless it is bold or
    // italic and the style turns that off), the look-alike, then Noto Sans.
    let wants_noto = style.font == FontChoice::NotoSans;
    let traits = base.traits.as_ref();
    let (own_bold, own_italic) = traits.map_or((false, false), |t| (t.bold, t.italic));
    let own_fits = !(own_bold && !style.bold) && !(own_italic && !style.italic);
    let mut candidates = Vec::new();
    if let (Some(font), false, true) = (base.font, wants_noto, own_fits) {
        candidates.push((EditMethod::Native, Some(font), None, (own_bold, own_italic)));
    }
    if let (Some(look), false) = (fonts.look_alike, wants_noto) {
        candidates.push((
            EditMethod::LookAlike,
            None,
            Some(look.bytes),
            (look.bold, look.italic),
        ));
    }
    candidates.push((
        EditMethod::FallbackFont,
        None,
        Some(fonts.noto_sans),
        (false, false),
    ));
    let mut read_back = String::new();
    for (method, own, bytes, face) in candidates {
        let font = match (own, bytes) {
            (Some(font), _) => font,
            (None, Some(bytes)) => fallback_font_token(document, bytes, text)?,
            (None, None) => continue,
        };
        let object = make_text(document, text, font, face, base, &style)?;
        let objects = pdf_page.objects_mut();
        let inserted = objects.insert_object_at_index(index, PdfPageObject::Text(object))?;
        read_back = inserted
            .as_text_object()
            .map(|t| t.text())
            .unwrap_or_default();
        if read_back == text {
            let bounds = rect(inserted.bounds()?);
            drop(inserted);
            let mut added = 1;
            if style.underline && is_upright(&base.matrix) {
                let [red, green, blue] = style.color;
                let color = PdfColor::new(red, green, blue, base.fill.alpha());
                let top = base.matrix.f() - style.size * UNDERLINE_GAP;
                let bar = PdfPagePathObject::new_rect(
                    document,
                    PdfRect::new_from_values(
                        top - style.size * UNDERLINE_THICKNESS,
                        bounds[0],
                        top,
                        bounds[2],
                    ),
                    None,
                    None,
                    Some(color),
                )?;
                objects.insert_object_at_index(index + 1, PdfPageObject::Path(bar))?;
                added = 2;
            }
            return Ok(Ok(Placed {
                method,
                bounds,
                added,
            }));
        }
        drop(inserted);
        objects.remove_object_at_index(index)?;
    }
    Ok(Err(read_back))
}

/// Replaces the whole text of text object `index` on `page` with `new_text`, as
/// a preview. Any earlier preview is discarded first.
///
/// The original object is never modified: it is taken off the page and kept,
/// and a new object with the same size, transform, colour and z-order takes its
/// place, in the original's font if that font can show the text, otherwise in
/// a look-alike or Noto Sans (`fonts`). Discarding puts the untouched original back, so the page
/// looks exactly as before. If no font can show the text, the original is put
/// back and [`Error::UnsupportedCharacters`] is returned.
pub fn preview_edit(
    page: u16,
    index: usize,
    new_text: &str,
    fonts: &Fonts<'_>,
) -> Result<EditPreview, Error> {
    preview_replace(page, index, new_text, None, fonts)
}

/// Like [`preview_edit`], with the line restyled to `style` (D-027). The style
/// always starts from the untouched original, so repeated changes don't stack.
pub fn preview_styled(
    page: u16,
    index: usize,
    new_text: &str,
    style: &TextStyle,
    fonts: &Fonts<'_>,
) -> Result<EditPreview, Error> {
    preview_replace(page, index, new_text, Some(style), fonts)
}

fn preview_replace(
    page: u16,
    index: usize,
    new_text: &str,
    style: Option<&TextStyle>,
    fonts: &Fonts<'_>,
) -> Result<EditPreview, Error> {
    discard_edit()?;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let base = with_underline(&pdf_page, index)?;
        let mut removed = vec![pdf_page.objects_mut().remove_object_at_index(index)?];
        if base.underline {
            // The old underline goes too; a new one follows the new text.
            removed.push(pdf_page.objects_mut().remove_object_at_index(index)?);
        }
        match insert_text(
            document,
            &mut pdf_page,
            index,
            new_text,
            &base,
            style,
            fonts,
        )? {
            Ok(Placed {
                method,
                bounds,
                added,
            }) => {
                PENDING.with_borrow_mut(|pending| {
                    *pending = Some(Pending {
                        page,
                        index,
                        removed,
                        added,
                    })
                });
                Ok(EditPreview {
                    method,
                    text: new_text.to_owned(),
                    bounds,
                })
            }
            Err(read_back) => {
                for (offset, object) in removed.into_iter().enumerate() {
                    pdf_page
                        .objects_mut()
                        .insert_object_at_index(index + offset, object)?;
                }
                Err(Error::UnsupportedCharacters {
                    intended: new_text.to_owned(),
                    reads_back_as: read_back,
                })
            }
        }
    })
}

/// Width in points of `text` placed with `base`/`style`, measured by inserting a
/// throwaway object at the end of the page and reading its bounds. Returns
/// `None` when no font can show it.
fn measure_width(
    document: &mut PdfDocument<'static>,
    pdf_page: &mut PdfPage<'static>,
    base: &Base,
    style: &TextStyle,
    fonts: &Fonts<'_>,
    text: &str,
) -> Result<Option<f32>, Error> {
    let index = pdf_page.objects().len();
    match insert_text(document, pdf_page, index, text, base, Some(style), fonts)? {
        Ok(Placed { bounds, added, .. }) => {
            for _ in 0..added {
                pdf_page.objects_mut().remove_object_at_index(index)?;
            }
            Ok(Some(bounds[2] - bounds[0]))
        }
        Err(_) => Ok(None),
    }
}

/// Greedily wraps `text` into lines no wider than `wrap_width`, using per-word
/// widths measured in the block's font. A word wider than the column gets its
/// own (overflowing) line. Returns `None` if a word can't be shown.
fn wrap_lines(
    document: &mut PdfDocument<'static>,
    pdf_page: &mut PdfPage<'static>,
    base: &Base,
    style: &TextStyle,
    fonts: &Fonts<'_>,
    text: &str,
    wrap_width: f32,
) -> Result<Option<Vec<String>>, Error> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Ok(Some(vec![String::new()]));
    }
    // Space width ≈ width("a a") − 2·width("a").
    let one = measure_width(document, pdf_page, base, style, fonts, "a")?;
    let two = measure_width(document, pdf_page, base, style, fonts, "a a")?;
    let space_w = match (one, two) {
        (Some(o), Some(t)) => (t - 2.0 * o).max(0.0),
        _ => 0.0,
    };
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0.0_f32;
    for word in words {
        let Some(word_w) = measure_width(document, pdf_page, base, style, fonts, word)? else {
            return Ok(None);
        };
        if cur.is_empty() {
            cur.push_str(word);
            cur_w = word_w;
        } else if cur_w + space_w + word_w > wrap_width {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
            cur_w = word_w;
        } else {
            cur.push(' ');
            cur.push_str(word);
            cur_w += space_w + word_w;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    Ok(Some(lines))
}

/// Replaces a wrapped paragraph (the text objects `indices`, from
/// [`paragraph_at`]) with `new_text` re-wrapped across lines at the block's
/// column width, as one preview (reflow). Each line keeps the block's left
/// margin, baseline spacing, font, size and colour. Only works on a block whose
/// objects are contiguous in the page's object list; otherwise it edits the
/// topmost line alone. Discarding restores the originals; keeping frees them.
pub fn preview_reflow(
    page: u16,
    indices: &[usize],
    new_text: &str,
    style: Option<&TextStyle>,
    fonts: &Fonts<'_>,
) -> Result<EditPreview, Error> {
    discard_edit()?;
    let mut idx: Vec<usize> = indices.to_vec();
    idx.sort_unstable();
    idx.dedup();
    let Some(&min) = idx.first() else {
        return Err(Error::Pdfium("no lines to reflow".into()));
    };
    // A single object, or a non-contiguous block, is edited as one line.
    if idx.len() < 2 || *idx.last().unwrap() != min + idx.len() - 1 {
        return preview_replace(page, min, new_text, style, fonts);
    }
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        // Geometry: each line's baseline (matrix f), left (matrix e), right edge.
        let (mut left_x, mut max_right) = (f32::MAX, f32::MIN);
        let mut baselines = Vec::new();
        let mut top_index = min;
        let mut top_baseline = f32::MIN;
        for &i in &idx {
            let object = pdf_page.objects().get(i)?;
            let matrix = object.matrix()?;
            baselines.push(matrix.f());
            left_x = left_x.min(matrix.e());
            max_right = max_right.max(rect(object.bounds()?)[2]);
            if matrix.f() > top_baseline {
                top_baseline = matrix.f();
                top_index = i;
            }
        }
        let mut base = with_underline(&pdf_page, top_index)?;
        base.underline = false; // v1: reflow doesn't carry per-line underlines.
        let style = style.copied().unwrap_or(base.style());
        baselines.sort_by(|a, b| b.total_cmp(a));
        let line_height = if baselines.len() >= 2 {
            baselines[0] - baselines[1]
        } else {
            base.size.value * base.scale() * 1.2
        };
        let wrap_width = (max_right - left_x).max(1.0);
        // Wrap the new text to the column (measured before removing anything).
        let Some(lines) = wrap_lines(
            document,
            &mut pdf_page,
            &base,
            &style,
            fonts,
            new_text,
            wrap_width,
        )?
        else {
            return Err(Error::UnsupportedCharacters {
                intended: new_text.to_owned(),
                reads_back_as: String::new(),
            });
        };
        // Take the block's objects off the page (contiguous at `min`), to restore
        // on discard or free on keep.
        let mut removed = Vec::new();
        for _ in 0..idx.len() {
            removed.push(pdf_page.objects_mut().remove_object_at_index(min)?);
        }
        // Place each wrapped line at the block's left, one line-height apart.
        let mut method = EditMethod::Native;
        let mut added = 0usize;
        let (mut l, mut b, mut r, mut t) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (i, line) in lines.iter().enumerate() {
            base.matrix.set_e(left_x);
            base.matrix.set_f(top_baseline - i as f32 * line_height);
            match insert_text(
                document,
                &mut pdf_page,
                min + added,
                line,
                &base,
                Some(&style),
                fonts,
            )? {
                Ok(Placed {
                    method: m,
                    bounds,
                    added: a,
                }) => {
                    if method == EditMethod::Native {
                        method = m;
                    }
                    added += a;
                    l = l.min(bounds[0]);
                    b = b.min(bounds[1]);
                    r = r.max(bounds[2]);
                    t = t.max(bounds[3]);
                }
                Err(read_back) => {
                    // Put the block back untouched and report the failure.
                    for _ in 0..added {
                        pdf_page.objects_mut().remove_object_at_index(min)?;
                    }
                    for (offset, object) in removed.into_iter().enumerate() {
                        pdf_page
                            .objects_mut()
                            .insert_object_at_index(min + offset, object)?;
                    }
                    return Err(Error::UnsupportedCharacters {
                        intended: new_text.to_owned(),
                        reads_back_as: read_back,
                    });
                }
            }
        }
        PENDING.with_borrow_mut(|pending| {
            *pending = Some(Pending {
                page,
                index: min,
                removed,
                added,
            })
        });
        Ok(EditPreview {
            method,
            text: new_text.to_owned(),
            bounds: [l, b, r, t],
        })
    })
}

/// Adds `text` on `page` at (`x`, `y`) in PDF points, as a preview. Any earlier
/// preview is discarded first.
///
/// The new text copies the nearest text object's font, size, colour and
/// transform (D-026), restyled by `style` when given (D-027), starting at `x`
/// and vertically centred on `y`. If that font cannot show the text, or the
/// page has no text, `fonts` are used (black, 12 pt, when there is
/// nothing to copy). If no font can show the text, nothing is added and
/// [`Error::UnsupportedCharacters`] is returned.
pub fn preview_add(
    page: u16,
    x: f32,
    y: f32,
    text: &str,
    style: Option<&TextStyle>,
    fonts: &Fonts<'_>,
) -> Result<AddPreview, Error> {
    discard_edit()?;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let mut base = nearest_base(&pdf_page, x, y)?;
        // The baseline sits about a third of the text height below the point.
        let height = style.map_or(base.size.value * base.scale(), |s| s.size);
        base.matrix.set_e(x);
        base.matrix.set_f(y - height * 0.35);

        let index = pdf_page.objects().len();
        match insert_text(document, &mut pdf_page, index, text, &base, style, fonts)? {
            Ok(Placed {
                method,
                bounds,
                added,
            }) => {
                PENDING.with_borrow_mut(|pending| {
                    *pending = Some(Pending {
                        page,
                        index,
                        removed: Vec::new(),
                        added,
                    })
                });
                Ok(AddPreview {
                    object_index: index,
                    preview: EditPreview {
                        method,
                        text: text.to_owned(),
                        bounds,
                    },
                })
            }
            Err(read_back) => Err(Error::UnsupportedCharacters {
                intended: text.to_owned(),
                reads_back_as: read_back,
            }),
        }
    })
}

/// The style new text at (`x`, `y`) on `page` starts with: the nearest text's
/// (D-026), or black 12 pt Noto Sans on a page without text.
pub fn style_near(page: u16, x: f32, y: f32) -> Result<TextStyle, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let base = nearest_base(&pdf_page, x, y)?;
        let mut style = base.style();
        if base.font.is_none() {
            style.font = FontChoice::NotoSans;
        }
        Ok(style)
    })
}

/// The look of the text object nearest to (`x`, `y`), or the defaults for a
/// page without text.
fn nearest_base(pdf_page: &PdfPage<'_>, x: f32, y: f32) -> Result<Base, Error> {
    let mut nearest = None;
    let mut nearest_distance = f32::INFINITY;
    for object in pdf_page.objects().iter() {
        let Some(base) = Base::of(&object)? else {
            continue;
        };
        let [left, bottom, right, top] = rect(object.bounds()?);
        let dx = (left - x).max(x - right).max(0.0);
        let dy = (bottom - y).max(y - top).max(0.0);
        let distance = dx.hypot(dy);
        if distance < nearest_distance {
            nearest_distance = distance;
            nearest = Some(base);
        }
    }
    Ok(nearest.unwrap_or(Base {
        matrix: PdfMatrix::IDENTITY,
        fill: PdfColor::BLACK,
        size: PdfPoints::new(12.0),
        font: None,
        traits: None,
        stroked: false,
        slanted: false,
        underline: false,
    }))
}

/// Keeps the previewed edit in the document.
pub fn keep_edit() {
    // Dropping the pending state frees the removed original object.
    PENDING.with_borrow_mut(|pending| *pending = None);
}

/// Undoes the previewed edit, if there is one, putting the original object back
/// exactly as it was.
pub fn discard_edit() -> Result<(), Error> {
    let Some(pending) = PENDING.with_borrow_mut(|pending| pending.take()) else {
        return Ok(());
    };
    with_open(|document| {
        let mut page = document.pages().get(pending.page.into())?;
        let objects = page.objects_mut();
        for _ in 0..pending.added {
            objects.remove_object_at_index(pending.index)?;
        }
        for (offset, object) in pending.removed.into_iter().enumerate() {
            objects.insert_object_at_index(pending.index + offset, object)?;
        }
        Ok(())
    })
}

/// Saves the open document (including kept edits) to bytes.
pub fn save_document() -> Result<Vec<u8>, Error> {
    with_open(|document| Ok(document.save_to_bytes()?))
}

/// The fallback font for `text`, embedded as a subset holding only the glyphs
/// `text` needs (D-028). If subsetting fails, the whole font is embedded once
/// and reused.
fn fallback_font_token(
    document: &mut PdfDocument<'static>,
    font: &[u8],
    text: &str,
) -> Result<PdfFontToken, Error> {
    let mut chars: Vec<char> = text.chars().collect();
    chars.sort_unstable();
    chars.dedup();
    // Which font (the app keeps each font's bytes alive, so its address and
    // length identify it), then which characters.
    let face = format!("{:x}:{}:", font.as_ptr() as usize, font.len());
    let key: String = face.clone() + &chars.into_iter().collect::<String>();
    if let Some(token) = FALLBACK.with_borrow(|fonts| fonts.get(&key).copied()) {
        return Ok(token);
    }
    let (key, bytes) = match crate::subset::subset_for_text(font, text) {
        Ok(subset) => (key, subset),
        Err(_) => {
            // The whole font, under a key no text produces.
            let whole = face + "\u{0}whole";
            if let Some(token) = FALLBACK.with_borrow(|fonts| fonts.get(&whole).copied()) {
                return Ok(token);
            }
            (whole, font.to_vec())
        }
    };
    reserve_for_pdfium_copy(bytes.len());
    let token = document
        .fonts_mut()
        .load_true_type_from_bytes(&bytes, true)?;
    FALLBACK.with_borrow_mut(|fonts| fonts.insert(key, token));
    Ok(token)
}

/// A recognised word to lay over a scan: its text and box
/// [left, bottom, right, top] in PDF points.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerWord {
    pub text: String,
    pub rect: [f32; 4],
}

/// Makes a scanned page searchable (D-055): each word goes on the page as
/// invisible text (render mode 3) in Noto Sans, sized and stretched to its box,
/// so Find, select and copy work and other apps can search it too. Words Noto
/// Sans can't show (Arabic script) are left out. Returns how many were added.
pub fn add_text_layer(page: u16, words: &[LayerWord], noto_sans: &[u8]) -> Result<usize, Error> {
    discard_edit()?;
    let words: Vec<&LayerWord> = words
        .iter()
        .filter(|w| !w.text.trim().is_empty() && crate::subset::covers(noto_sans, &w.text))
        .collect();
    if words.is_empty() {
        return Ok(0);
    }
    let all: String = words.iter().map(|w| w.text.as_str()).collect();
    with_open(|document| {
        let font = fallback_font_token(document, noto_sans, &all)?;
        let mut pdf_page = document.pages().get(page.into())?;
        for word in &words {
            let [left, bottom, right, top] = word.rect;
            let height = (top - bottom).max(1.0);
            let mut object =
                PdfPageTextObject::new(document, &word.text, font, PdfPoints::new(height))?;
            object.set_render_mode(PdfPageTextRenderMode::Invisible)?;
            // Stretch the word to its box; its baseline about a fifth of the height
            // above the box's bottom (below it hang the descenders).
            let natural = object.bounds().map_or(0.0, |b| b.width().value);
            let stretch = if natural > 0.0 {
                (right - left) / natural
            } else {
                1.0
            };
            object.reset_matrix(PdfMatrix::new(
                stretch,
                0.0,
                0.0,
                1.0,
                left,
                bottom + height * 0.2,
            ))?;
            pdf_page.objects_mut().add_text_object(object)?;
        }
        Ok(words.len())
    })
}
