//! Annotations, part 1 (D-040): sticky notes and text markup (highlight,
//! underline, strikeout) as real PDF annotations, so every viewer shows them.
//! Also the page's words with their boxes, for selecting text to mark.
//! Every change first drops a previewed text edit, like the page ops.

use crate::edit_open::discard_edit;
use crate::render::with_open;
use crate::{Error, inspect};
use pdfium_render::prelude::*;

/// A word on a page and its box in PDF points [left, bottom, right, top].
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub bounds: [f32; 4],
}

/// A kind of text markup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkupKind {
    Highlight,
    Underline,
    Strikeout,
}

/// What an annotation pdit shows controls for is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationKind {
    Note,
    Markup(MarkupKind),
    /// A pen drawing (D-041).
    Ink,
    /// A stamp (D-041); its text is "label · date".
    Stamp,
}

/// A note or markup annotation on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    /// Index in the page's annotation list.
    pub index: usize,
    pub kind: AnnotationKind,
    /// The whole annotation's box [left, bottom, right, top].
    pub bounds: [f32; 4],
    /// Markup: one box per marked line (its quad points). Notes: empty.
    pub rects: Vec<[f32; 4]>,
    /// A note's text.
    pub text: String,
    pub color: [u8; 3],
}

/// Size of a sticky note's icon on the page, in points.
const NOTE_SIZE_PT: f32 = 20.0;

/// The words on `page`, in PDFium's reading order. A word ends at whitespace
/// (PDFium also generates spaces and line breaks between text pieces).
pub fn words(page: u16) -> Result<Vec<Word>, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let text = pdf_page.text()?;
        let mut words = Vec::new();
        let mut current: Option<Word> = None;
        for ch in text.chars().iter() {
            let c = ch.unicode_char().unwrap_or(' ');
            let bounds = if c.is_whitespace() {
                None
            } else {
                ch.loose_bounds().ok().map(|r| rect(&r))
            };
            match (bounds, current.as_mut()) {
                (Some(b), Some(word)) => {
                    word.text.push(c);
                    word.bounds = union(word.bounds, b);
                }
                (Some(b), None) => {
                    current = Some(Word {
                        text: c.to_string(),
                        bounds: b,
                    })
                }
                (None, _) => words.extend(current.take()),
            }
        }
        words.extend(current);
        Ok(words)
    })
}

/// Marks text on `page`: one annotation covering `rects` (one box per line,
/// [left, bottom, right, top] in points), in `color`. Returns its index.
pub fn add_markup(
    page: u16,
    kind: MarkupKind,
    rects: &[[f32; 4]],
    color: [u8; 3],
) -> Result<usize, Error> {
    let Some(&first) = rects.first() else {
        return Err(Error::Pdfium("nothing selected to mark".into()));
    };
    discard_edit()?;
    let bounds = rects.iter().fold(first, |a, &b| union(a, b));
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let annotations = pdf_page.annotations_mut();
        // Each markup type is its own pdfium-render type; the steps are the same.
        macro_rules! mark {
            ($annotation:expr) => {{
                let mut annotation = $annotation;
                for &r in rects {
                    annotation
                        .attachment_points_mut()
                        .create_attachment_point_at_end(quad(r))?;
                }
                annotation.set_bounds(pdf_rect(bounds))?;
                annotation.set_stroke_color(pdf_color(color))?;
                // Printed with the page (the PDF "Print" flag, D-052).
                annotation.set_is_printed(true)?;
            }};
        }
        match kind {
            MarkupKind::Highlight => mark!(annotations.create_highlight_annotation()?),
            MarkupKind::Underline => mark!(annotations.create_underline_annotation()?),
            MarkupKind::Strikeout => mark!(annotations.create_strikeout_annotation()?),
        }
        Ok(pdf_page.annotations().len() - 1)
    })
}

/// Adds a sticky note with `text` whose icon's top-left is at (`x`, `top`).
/// Returns its index. No author is set (D-040).
pub fn add_note(page: u16, x: f32, top: f32, text: &str) -> Result<usize, Error> {
    discard_edit()?;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let mut note = pdf_page.annotations_mut().create_text_annotation(text)?;
        note.set_bounds(pdf_rect([x, top - NOTE_SIZE_PT, x + NOTE_SIZE_PT, top]))?;
        note.set_stroke_color(pdf_color([255, 216, 77]))?;
        note.set_is_printed(true)?;
        Ok(pdf_page.annotations().len() - 1)
    })
}

/// Replaces the text of the note at `index`.
pub fn set_note_text(page: u16, index: usize, text: &str) -> Result<(), Error> {
    discard_edit()?;
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let mut note = pdf_page
            .annotations()
            .get(index as PdfPageAnnotationIndex)?;
        expect(&note, |k| k == AnnotationKind::Note)?;
        note.set_contents(text)?;
        Ok(())
    })
}

/// Recolours the markup at `index`.
pub fn set_markup_color(page: u16, index: usize, color: [u8; 3]) -> Result<(), Error> {
    discard_edit()?;
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let mut markup = pdf_page
            .annotations()
            .get(index as PdfPageAnnotationIndex)?;
        expect(&markup, |k| matches!(k, AnnotationKind::Markup(_)))?;
        markup.set_stroke_color(pdf_color(color))?;
        Ok(())
    })
}

/// Deletes the note or markup at `index`.
pub fn delete_annotation(page: u16, index: usize) -> Result<(), Error> {
    discard_edit()?;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let annotations = pdf_page.annotations_mut();
        let annotation = annotations.get(index as PdfPageAnnotationIndex)?;
        expect(&annotation, |_| true)?;
        annotations.delete_annotation(annotation)?;
        Ok(())
    })
}

/// Draws a pen stroke (D-041) through `points` (PDF points) as an ink
/// annotation: a smoothed path for its appearance (so every viewer shows it)
/// plus the standard `/InkList`. Returns its index.
pub fn add_ink(
    page: u16,
    points: &[(f32, f32)],
    color: [u8; 3],
    width_pt: f32,
) -> Result<usize, Error> {
    if points.len() < 2 {
        return Err(Error::Pdfium("a drawing needs at least two points".into()));
    }
    discard_edit()?;
    let pad = width_pt.max(0.5);
    let bounds = points.iter().fold(
        [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
        |[l, b, r, t], &(x, y)| {
            [
                l.min(x - pad),
                b.min(y - pad),
                r.max(x + pad),
                t.max(y + pad),
            ]
        },
    );
    let pt = PdfPoints::new;
    let index = with_open(|document| {
        let mut path = PdfPagePathObject::new(
            document,
            pt(points[0].0),
            pt(points[0].1),
            Some(pdf_color(color)),
            Some(pt(width_pt.max(0.1))),
            None,
        )?;
        // Quadratic curves through the midpoints (as the pad draws them),
        // written as cubic Béziers.
        let mut from = points[0];
        for i in 1..points.len() - 1 {
            let (cx, cy) = points[i];
            let to = ((cx + points[i + 1].0) / 2.0, (cy + points[i + 1].1) / 2.0);
            let c1 = (
                from.0 + 2.0 / 3.0 * (cx - from.0),
                from.1 + 2.0 / 3.0 * (cy - from.1),
            );
            let c2 = (
                to.0 + 2.0 / 3.0 * (cx - to.0),
                to.1 + 2.0 / 3.0 * (cy - to.1),
            );
            path.bezier_to(pt(to.0), pt(to.1), pt(c1.0), pt(c1.1), pt(c2.0), pt(c2.1))?;
            from = to;
        }
        let last = points[points.len() - 1];
        path.line_to(pt(last.0), pt(last.1))?;
        let mut pdf_page = document.pages().get(page.into())?;
        let mut ink = pdf_page.annotations_mut().create_ink_annotation()?;
        ink.set_bounds(pdf_rect(bounds))?;
        ink.set_stroke_color(pdf_color(color))?;
        ink.set_is_printed(true)?;
        ink.objects_mut().add_path_object(path)?;
        Ok(pdf_page.annotations().len() - 1)
    })?;
    add_ink_list(page, index, points)?;
    Ok(index)
}

/// Writes the stroke as the annotation's `/InkList` (pdfium-render has no
/// wrapper; the page handle and bindings are public in our copy, D-036).
fn add_ink_list(page: u16, index: usize, points: &[(f32, f32)]) -> Result<(), Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let bindings = crate::pdfium().bindings();
        let fs: Vec<FS_POINTF> = points.iter().map(|&(x, y)| FS_POINTF { x, y }).collect();
        unsafe {
            let annot = bindings.FPDFPage_GetAnnot(pdf_page.page_handle(), index as i32);
            if annot.is_null() {
                return Err(Error::Pdfium("the drawing was not found".into()));
            }
            let added = bindings.FPDFAnnot_AddInkStroke(annot, fs.as_ptr(), fs.len() as _);
            bindings.FPDFPage_CloseAnnot(annot);
            if added < 0 {
                return Err(Error::Pdfium("could not store the drawing's stroke".into()));
            }
        }
        Ok(())
    })
}

/// Places a stamp (D-041): `label` over `date` in a frame of `color`, tilted
/// 6° like a rubber stamp, its top-left at (`x`, `top`). Drawn with objects so
/// every viewer shows it. Returns its index.
pub fn add_stamp(
    page: u16,
    x: f32,
    top: f32,
    label: &str,
    date: &str,
    color: [u8; 3],
) -> Result<usize, Error> {
    const LABEL_PT: f32 = 16.0;
    const DATE_PT: f32 = 7.5;
    const PAD_X: f32 = 9.0;
    const PAD_Y: f32 = 5.0;
    const LINE_PT: f32 = 2.0;
    const TILT_DEG: f32 = 6.0;
    discard_edit()?;
    let label = label.to_uppercase();
    let pt = PdfPoints::new;
    with_open(|document| {
        let bold = document.fonts_mut().helvetica_bold();
        let regular = document.fonts_mut().helvetica();
        let mut label_obj = PdfPageTextObject::new(document, &label, bold, pt(LABEL_PT))?;
        let mut date_obj = PdfPageTextObject::new(document, date, regular, pt(DATE_PT))?;
        let label_w = label_obj.width()?.value;
        let date_w = date_obj.width()?.value;
        // The frame, laid out from its bottom-left at (0, 0) before tilting.
        let w = label_w.max(date_w) + 2.0 * PAD_X;
        let date_base = PAD_Y + 1.0;
        let label_base = date_base + DATE_PT + 3.0;
        let h = label_base + LABEL_PT * 0.72 + PAD_Y;
        let a = TILT_DEG.to_radians();
        let (rw, rh) = (w * a.cos() + h * a.sin(), w * a.sin() + h * a.cos());
        let (cx, cy) = (x + rw / 2.0, top - rh / 2.0);
        let mut frame = PdfPagePathObject::new_rect(
            document,
            PdfRect::new_from_values(0.0, 0.0, h, w),
            Some(pdf_color(color)),
            Some(pt(LINE_PT)),
            None,
        )?;
        label_obj.translate(pt((w - label_w) / 2.0), pt(label_base))?;
        date_obj.translate(pt((w - date_w) / 2.0), pt(date_base))?;
        label_obj.set_fill_color(pdf_color(color))?;
        date_obj.set_fill_color(pdf_color(color))?;
        // Tilt round the frame's centre, then move the centre into place.
        // (A macro: the transform methods are on each object type.)
        macro_rules! place {
            ($o:expr) => {{
                $o.translate(pt(-w / 2.0), pt(-h / 2.0))?;
                $o.rotate_counter_clockwise_degrees(TILT_DEG)?;
                $o.translate(pt(cx), pt(cy))?;
            }};
        }
        place!(frame);
        place!(label_obj);
        place!(date_obj);
        let mut pdf_page = document.pages().get(page.into())?;
        let mut stamp = pdf_page.annotations_mut().create_stamp_annotation()?;
        stamp.set_bounds(pdf_rect([x - 1.0, top - rh - 1.0, x + rw + 1.0, top + 1.0]))?;
        stamp.set_contents(&format!("{label} · {date}"))?;
        stamp.set_is_printed(true)?;
        let objects = stamp.objects_mut();
        objects.add_path_object(frame)?;
        objects.add_text_object(label_obj)?;
        objects.add_text_object(date_obj)?;
        Ok(pdf_page.annotations().len() - 1)
    })
}

/// The notes and markup on `page` (other annotation types are left alone).
pub fn annotations(page: u16) -> Result<Vec<Annotation>, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let mut out = Vec::new();
        for (index, annotation) in pdf_page.annotations().iter().enumerate() {
            let Some(kind) = kind_of(&annotation) else {
                continue;
            };
            let rects = annotation
                .attachment_points()
                .iter()
                .map(inspect::rect)
                .collect();
            let color = annotation
                .stroke_color()
                .map(|c| [c.red(), c.green(), c.blue()])
                .unwrap_or([0, 0, 0]);
            out.push(Annotation {
                index,
                kind,
                bounds: rect(&annotation.bounds()?),
                rects,
                text: annotation.contents().unwrap_or_default(),
                color,
            });
        }
        Ok(out)
    })
}

fn kind_of(annotation: &PdfPageAnnotation) -> Option<AnnotationKind> {
    Some(match annotation.annotation_type() {
        PdfPageAnnotationType::Text => AnnotationKind::Note,
        PdfPageAnnotationType::Highlight => AnnotationKind::Markup(MarkupKind::Highlight),
        PdfPageAnnotationType::Underline => AnnotationKind::Markup(MarkupKind::Underline),
        PdfPageAnnotationType::Strikeout => AnnotationKind::Markup(MarkupKind::Strikeout),
        PdfPageAnnotationType::Ink => AnnotationKind::Ink,
        PdfPageAnnotationType::Stamp => AnnotationKind::Stamp,
        _ => return None,
    })
}

/// Refuses to touch an annotation of another kind (a stale index).
fn expect(
    annotation: &PdfPageAnnotation,
    ok: impl Fn(AnnotationKind) -> bool,
) -> Result<(), Error> {
    match kind_of(annotation) {
        Some(kind) if ok(kind) => Ok(()),
        _ => Err(Error::Pdfium("that note or mark has changed".into())),
    }
}

/// A line's box as quad points in the order viewers read them: top-left,
/// top-right, bottom-left, bottom-right. (pdfium-render's `from_rect` goes
/// round the box, which Poppler draws as a bow-tie.)
pub(crate) fn quad([l, b, r, t]: [f32; 4]) -> PdfQuadPoints {
    PdfQuadPoints::new_from_values(l, t, r, t, l, b, r, b)
}

pub(crate) fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

pub(crate) fn rect(r: &PdfRect) -> [f32; 4] {
    [
        r.left().value,
        r.bottom().value,
        r.right().value,
        r.top().value,
    ]
}

pub(crate) fn pdf_rect([l, b, r, t]: [f32; 4]) -> PdfRect {
    PdfRect::new_from_values(b, l, t, r)
}

fn pdf_color([r, g, b]: [u8; 3]) -> PdfColor {
    PdfColor::new(r, g, b, 255)
}
