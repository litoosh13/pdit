//! Page tools on the open document (D-029): rotate, insert a blank page,
//! duplicate, delete, extract a page as a new PDF, and insert the pages of
//! another PDF. Every change first drops a previewed text edit. Undo is a
//! snapshot of the whole document taken before the change ([`snapshot`],
//! [`restore`]).

use crate::edit_open::discard_edit;
use crate::render::{open_document, with_open};
use crate::{Error, pdfium, reserve_for_pdfium_copy};
use pdfium_render::prelude::*;

/// Each page's size in PDF points (width, height), as displayed (rotation
/// applied).
pub fn page_sizes() -> Result<Vec<(f32, f32)>, Error> {
    with_open(|document| {
        let mut sizes = Vec::new();
        for page in document.pages().iter() {
            sizes.push((page.width().value, page.height().value));
        }
        Ok(sizes)
    })
}

/// The whole open document as bytes, for undoing the next change.
pub fn snapshot() -> Result<Vec<u8>, Error> {
    discard_edit()?;
    with_open(|document| Ok(document.save_to_bytes()?))
}

/// Puts a [`snapshot`] back as the open document; returns its page sizes.
pub fn restore(snapshot: Vec<u8>) -> Result<Vec<(f32, f32)>, Error> {
    open_document(snapshot)?;
    page_sizes()
}

/// Turns page `index` a quarter turn clockwise, or anticlockwise.
pub fn rotate_page(index: u16, clockwise: bool) -> Result<(), Error> {
    discard_edit()?;
    with_open(|document| {
        let mut page = document.pages().get(index.into())?;
        let quarter_turns = match page.rotation()? {
            PdfPageRenderRotation::None => 0,
            PdfPageRenderRotation::Degrees90 => 1,
            PdfPageRenderRotation::Degrees180 => 2,
            PdfPageRenderRotation::Degrees270 => 3,
        };
        let next = (quarter_turns + if clockwise { 1 } else { 3 }) % 4;
        page.set_rotation(match next {
            1 => PdfPageRenderRotation::Degrees90,
            2 => PdfPageRenderRotation::Degrees180,
            3 => PdfPageRenderRotation::Degrees270,
            _ => PdfPageRenderRotation::None,
        });
        Ok(())
    })
}

/// Inserts an empty page after page `index`, the same size as that page.
pub fn insert_blank_page(index: u16) -> Result<(), Error> {
    discard_edit()?;
    with_open(|document| {
        let size = document.pages().page_size(index.into())?;
        let paper = PdfPagePaperSize::from_points(size.width(), size.height());
        document
            .pages_mut()
            .create_page_at_index(paper, i32::from(index) + 1)?;
        Ok(())
    })
}

/// Inserts a copy of page `index` right after it.
pub fn duplicate_page(index: u16) -> Result<(), Error> {
    // PDFium copies pages between documents, so the page comes from a
    // temporary copy of the open document.
    let copy = snapshot()?;
    with_open(|document| {
        reserve_for_pdfium_copy(copy.len());
        let source = pdfium().load_pdf_from_byte_vec(copy, None)?;
        document.pages_mut().copy_page_from_document(
            &source,
            index.into(),
            i32::from(index) + 1,
        )?;
        Ok(())
    })
}

/// Moves the page at `from` so it ends up at index `to` (same page count).
///
/// pdfium-render 0.9.4 exposes no page reorder — `FPDF_MovePages` needs the raw
/// document handle, which is crate-private — so this copies the page into the
/// target slot from a reloaded copy (like [`duplicate_page`]) and deletes the
/// original. The copy re-embeds the page's own objects, so a moved page can
/// leave its original objects unreferenced until a save drops them (see P-031,
/// the same shape as P-029). Undo restores the pre-move snapshot.
pub fn move_page(from: u16, to: u16) -> Result<(), Error> {
    if from == to {
        return Ok(());
    }
    let copy = snapshot()?;
    with_open(|document| {
        let count = document.pages().len();
        let (from, to) = (i32::from(from), i32::from(to));
        if from < 0 || to < 0 || from >= count || to >= count {
            return Err(Error::Pdfium("page index out of range".into()));
        }
        reserve_for_pdfium_copy(copy.len());
        let source = pdfium().load_pdf_from_byte_vec(copy, None)?;
        // Insert the copy so that, once the original is deleted, it lands at
        // `to`. Moving later (to > from): the original keeps index `from`, so
        // put the copy at to+1. Moving earlier: the original shifts to from+1,
        // so put the copy at `to`.
        let (dest, original) = if to > from {
            (to + 1, from)
        } else {
            (to, from + 1)
        };
        document
            .pages_mut()
            .copy_page_from_document(&source, from, dest)?;
        document.pages().get(original)?.delete()?;
        Ok(())
    })
}

/// Adds a PNG/JPEG image to page `index`, scaled to `width_pt` (aspect kept)
/// with its top-left corner at (`x_pt`, `top_pt`) in PDF points measured from
/// the page's bottom-left. Decoded with the `image` crate (D-023).
pub fn add_image(
    index: u16,
    bytes: Vec<u8>,
    x_pt: f32,
    top_pt: f32,
    width_pt: f32,
) -> Result<(), Error> {
    discard_edit()?;
    let img = image::load_from_memory(&bytes)
        .map_err(|e| Error::Pdfium(format!("could not read the image: {e}")))?;
    let (px_w, px_h) = (img.width().max(1), img.height().max(1));
    let height_pt = width_pt * px_h as f32 / px_w as f32;
    // Uploading the decoded pixels to PDFium is a large Rust->PDFium copy (P-023).
    reserve_for_pdfium_copy((px_w * px_h * 4) as usize);
    with_open(|document| {
        let mut page = document.pages().get(index.into())?;
        page.objects_mut().create_image_object(
            PdfPoints::new(x_pt),
            PdfPoints::new(top_pt - height_pt),
            &img,
            Some(PdfPoints::new(width_pt)),
            Some(PdfPoints::new(height_pt)),
        )?;
        Ok(())
    })
}

/// Replaces page `index`'s content with a picture of itself, `width_px` wide
/// (like a scan: no text of its own afterwards). Annotations stay.
pub fn rasterize_page(index: u16, width_px: u32) -> Result<(), Error> {
    discard_edit()?;
    let image = crate::render_page(index, width_px)?;
    let (w, h) = (image.width(), image.height());
    let pixels = image::RgbaImage::from_raw(w, h, image.data().to_vec())
        .ok_or_else(|| Error::Pdfium("could not copy the page image".into()))?;
    let img = image::DynamicImage::ImageRgba8(pixels);
    reserve_for_pdfium_copy((w * h * 4) as usize);
    with_open(|document| {
        let mut page = document.pages().get(index.into())?;
        while !page.objects().is_empty() {
            page.objects_mut().remove_object_at_index(0)?;
        }
        let (width, height) = (page.width(), page.height());
        page.objects_mut().create_image_object(
            PdfPoints::new(0.0),
            PdfPoints::new(0.0),
            &img,
            Some(width),
            Some(height),
        )?;
        Ok(())
    })
}

/// A shape the user can draw (D-023, feature parity).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Line,
    Arrow,
}

/// Draws `kind` on `page` as a PDF path object, from corner/end (`x1`,`y1`) to
/// (`x2`,`y2`) in PDF points, with `stroke` colour, `width_pt` stroke width, and
/// an optional `fill` (rectangle/ellipse only). Drops any previewed text edit
/// first, like the other page ops.
#[allow(clippy::too_many_arguments)]
pub fn add_shape(
    page: u16,
    kind: ShapeKind,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    stroke: [u8; 3],
    width_pt: f32,
    fill: Option<[u8; 3]>,
) -> Result<(), Error> {
    discard_edit()?;
    let color = |[r, g, b]: [u8; 3]| PdfColor::new(r, g, b, 255);
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let stroke_color = color(stroke);
        let width = PdfPoints::new(width_pt.max(0.1));
        match kind {
            ShapeKind::Rectangle | ShapeKind::Ellipse => {
                let rect = PdfRect::new_from_values(y1.min(y2), x1.min(x2), y1.max(y2), x1.max(x2));
                let objects = pdf_page.objects_mut();
                if kind == ShapeKind::Rectangle {
                    objects.create_path_object_rect(
                        rect,
                        Some(stroke_color),
                        Some(width),
                        fill.map(color),
                    )?;
                } else {
                    objects.create_path_object_ellipse(
                        rect,
                        Some(stroke_color),
                        Some(width),
                        fill.map(color),
                    )?;
                }
            }
            ShapeKind::Line => {
                pdf_page.objects_mut().create_path_object_line(
                    PdfPoints::new(x1),
                    PdfPoints::new(y1),
                    PdfPoints::new(x2),
                    PdfPoints::new(y2),
                    stroke_color,
                    width,
                )?;
            }
            ShapeKind::Arrow => {
                // Shaft plus a two-stroke arrowhead at the (x2, y2) end.
                let mut path = PdfPagePathObject::new(
                    document,
                    PdfPoints::new(x1),
                    PdfPoints::new(y1),
                    Some(stroke_color),
                    Some(width),
                    None,
                )?;
                path.line_to(PdfPoints::new(x2), PdfPoints::new(y2))?;
                let angle = (y2 - y1).atan2(x2 - x1);
                let head = (width_pt * 4.0).clamp(8.0, 22.0);
                let spread = 0.42_f32;
                let head_at =
                    |a: f32| (x2 - head * (angle + a).cos(), y2 - head * (angle + a).sin());
                let (hx1, hy1) = head_at(-spread);
                let (hx2, hy2) = head_at(spread);
                path.move_to(PdfPoints::new(hx1), PdfPoints::new(hy1))?;
                path.line_to(PdfPoints::new(x2), PdfPoints::new(y2))?;
                path.line_to(PdfPoints::new(hx2), PdfPoints::new(hy2))?;
                pdf_page
                    .objects_mut()
                    .add_object(PdfPageObject::Path(path))?;
            }
        }
        Ok(())
    })
}

/// Draws a table grid on `page` ("draw column", D-023): the outer border from
/// corner (`x1`,`y1`) to (`x2`,`y2`) in PDF points, split into `rows` × `cols`
/// equal cells, as one path object so it moves as a unit. Cell text is added
/// with Add text. Returns the grid's object index, for [`replace_table`].
/// Drops any previewed text edit first.
#[allow(clippy::too_many_arguments)]
pub fn add_table(
    page: u16,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    rows: u16,
    cols: u16,
    stroke: [u8; 3],
    width_pt: f32,
) -> Result<usize, Error> {
    let (rows, cols) = (rows.clamp(1, 100), cols.clamp(1, 100));
    let (left, right) = (x1.min(x2), x1.max(x2));
    let (bottom, top) = (y1.min(y2), y1.max(y2));
    let col_widths = vec![(right - left) / f32::from(cols); cols.into()];
    let row_heights = vec![(top - bottom) / f32::from(rows); rows.into()];
    discard_edit()?;
    draw_grid(page, left, top, &col_widths, &row_heights, stroke, width_pt)
}

/// A table's layout in PDF points: its top-left corner, then the column widths
/// (left to right) and row heights (top to bottom).
#[derive(Clone, Debug, PartialEq)]
pub struct TableLayout {
    pub left: f32,
    pub top: f32,
    pub col_widths: Vec<f32>,
    pub row_heights: Vec<f32>,
}

impl TableLayout {
    /// [left, bottom, right, top].
    pub fn bounds(&self) -> [f32; 4] {
        let width: f32 = self.col_widths.iter().sum();
        let height: f32 = self.row_heights.iter().sum();
        [self.left, self.top - height, self.left + width, self.top]
    }
    /// Left edge of each column, then the right edge.
    fn xs(&self) -> Vec<f32> {
        edges(self.left, &self.col_widths, 1.0)
    }
    /// Top edge of each row, then the bottom edge.
    fn ys(&self) -> Vec<f32> {
        edges(self.top, &self.row_heights, -1.0)
    }
    fn valid(&self) -> bool {
        let ok = |sizes: &[f32]| {
            (1..=100).contains(&sizes.len()) && sizes.iter().all(|s| s.is_finite() && *s > 0.0)
        };
        ok(&self.col_widths) && ok(&self.row_heights)
    }
}

fn edges(start: f32, sizes: &[f32], direction: f32) -> Vec<f32> {
    let mut out = vec![start];
    for size in sizes {
        out.push(out[out.len() - 1] + direction * size);
    }
    out
}

/// Replaces a table drawn by [`add_table`] (D-038): the grid at `object_index`,
/// laid out as `old`, becomes `new` (rows/columns added or resized). Text whose
/// centre lies in a cell moves with that cell. Refuses, changing nothing,
/// unless that object is still a path at `old`'s bounds, so a stale index never
/// deletes other content. Returns the new grid's object index.
pub fn replace_table(
    page: u16,
    object_index: usize,
    old: &TableLayout,
    new: &TableLayout,
    stroke: [u8; 3],
    width_pt: f32,
) -> Result<usize, Error> {
    if !old.valid() || !new.valid() {
        return Err(Error::Pdfium(
            "a table needs 1 to 100 rows and columns".into(),
        ));
    }
    discard_edit()?;
    let (old_xs, old_ys, new_xs, new_ys) = (old.xs(), old.ys(), new.xs(), new.ys());
    let [left, bottom, right, top] = old.bounds();
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let still_there = {
            let grid = pdf_page.objects().get(object_index)?;
            // Path bounds may include the stroke, so allow its width plus a point.
            let near = crate::inspect::rect(grid.bounds()?)
                .iter()
                .zip(old.bounds())
                .all(|(a, b)| (a - b).abs() <= width_pt + 1.0);
            grid.as_path_object().is_some() && near
        };
        if !still_there {
            return Err(Error::Pdfium(
                "the table was changed; select it again".into(),
            ));
        }
        // Text in a cell (by its centre) moves by that cell's shift.
        let mut moves = Vec::new();
        for (index, object) in pdf_page.objects().iter().enumerate() {
            if object.as_text_object().is_none() {
                continue;
            }
            let [l, b, r, t] = crate::inspect::rect(object.bounds()?);
            let (cx, cy) = ((l + r) / 2.0, (b + t) / 2.0);
            if cx < left || cx > right || cy < bottom || cy > top {
                continue;
            }
            let col = old_xs[1..].iter().position(|&x| cx <= x).unwrap_or(0);
            let row = old_ys[1..].iter().position(|&y| cy >= y).unwrap_or(0);
            if col + 1 < new_xs.len() && row + 1 < new_ys.len() {
                moves.push((index, new_xs[col] - old_xs[col], new_ys[row] - old_ys[row]));
            }
        }
        let objects = pdf_page.objects_mut();
        for (index, dx, dy) in moves {
            let mut object = objects.get(index)?;
            object.translate(PdfPoints::new(dx), PdfPoints::new(dy))?;
        }
        objects.remove_object_at_index(object_index)?;
        Ok(())
    })?;
    draw_grid(
        page,
        new.left,
        new.top,
        &new.col_widths,
        &new.row_heights,
        stroke,
        width_pt,
    )
}

/// Draws the grid (outer border plus inner lines) as one path object and
/// returns its object index.
fn draw_grid(
    page: u16,
    left: f32,
    top: f32,
    col_widths: &[f32],
    row_heights: &[f32],
    stroke: [u8; 3],
    width_pt: f32,
) -> Result<usize, Error> {
    let layout = TableLayout {
        left,
        top,
        col_widths: col_widths.to_vec(),
        row_heights: row_heights.to_vec(),
    };
    let [_, bottom, right, _] = layout.bounds();
    let (xs, ys) = (layout.xs(), layout.ys());
    let pt = PdfPoints::new;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let [r, g, b] = stroke;
        let mut path = PdfPagePathObject::new(
            document,
            pt(left),
            pt(bottom),
            Some(PdfColor::new(r, g, b, 255)),
            Some(pt(width_pt.max(0.1))),
            None,
        )?;
        path.line_to(pt(right), pt(bottom))?;
        path.line_to(pt(right), pt(top))?;
        path.line_to(pt(left), pt(top))?;
        path.close_path()?;
        for &y in &ys[1..ys.len() - 1] {
            path.move_to(pt(left), pt(y))?;
            path.line_to(pt(right), pt(y))?;
        }
        for &x in &xs[1..xs.len() - 1] {
            path.move_to(pt(x), pt(bottom))?;
            path.line_to(pt(x), pt(top))?;
        }
        pdf_page
            .objects_mut()
            .add_object(PdfPageObject::Path(path))?;
        Ok(pdf_page.objects().len() - 1)
    })
}

/// An image object on a page, for the image-options UI (D-023a).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageHit {
    /// The object's index in the page's object list.
    pub object_index: usize,
    /// Axis-aligned bounds in PDF points: [left, bottom, right, top].
    pub bounds: [f32; 4],
}

/// The topmost image object on `page` whose bounds contain (`x_pt`, `y_pt`)
/// (PDF points from the page's bottom-left), if any. Later objects paint over
/// earlier ones, so the last match wins. Used to select a placed image
/// (D-023a). Read-only: it drops no previewed edit.
pub fn image_at(page: u16, x_pt: f32, y_pt: f32) -> Result<Option<ImageHit>, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let mut found = None;
        for (object_index, object) in pdf_page.objects().iter().enumerate() {
            if object.as_image_object().is_none() {
                continue;
            }
            let bounds = crate::inspect::rect(object.bounds()?);
            let [left, bottom, right, top] = bounds;
            if x_pt >= left && x_pt <= right && y_pt >= bottom && y_pt <= top {
                found = Some(ImageHit {
                    object_index,
                    bounds,
                });
            }
        }
        Ok(found)
    })
}

/// Moves the object at `object_index` on `page` by (`dx_pt` right, `dy_pt` up)
/// in PDF points, and returns its new bounds [left, bottom, right, top].
/// The caller selects the object with [`image_at`] first (D-023a).
pub fn move_image(
    page: u16,
    object_index: usize,
    dx_pt: f32,
    dy_pt: f32,
) -> Result<[f32; 4], Error> {
    discard_edit()?;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let mut object = pdf_page.objects_mut().get(object_index)?;
        object.translate(PdfPoints::new(dx_pt), PdfPoints::new(dy_pt))?;
        Ok(crate::inspect::rect(object.bounds()?))
    })
}

/// Resizes the object at `object_index` on `page` to `width_pt` wide, keeping
/// its aspect ratio, and returns its new bounds [left, bottom, right, top].
/// Select the object with [`image_at`] first (D-023a).
///
/// The scale is applied about the PDF page origin, so the object's position
/// shifts; the caller reads the returned bounds and repositions with
/// [`move_image`] if it needs a fixed anchor. Size is correct regardless.
// ponytail: scale about origin, not object centre — UI re-anchors via move_image + returned bounds. Anchor in-engine only if a caller needs it.
pub fn resize_image(page: u16, object_index: usize, width_pt: f32) -> Result<[f32; 4], Error> {
    discard_edit()?;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let mut object = pdf_page.objects_mut().get(object_index)?;
        let [left, _, right, _] = crate::inspect::rect(object.bounds()?);
        let current = right - left;
        if current <= 0.0 || width_pt <= 0.0 {
            return Err(Error::Pdfium("invalid image width".into()));
        }
        let factor = width_pt / current;
        object.scale(factor, factor)?;
        Ok(crate::inspect::rect(object.bounds()?))
    })
}

/// Rotates the object at `object_index` on `page` clockwise by `degrees`, and
/// returns its new (axis-aligned) bounds [left, bottom, right, top]. Select the
/// object with [`image_at`] first (D-023a).
///
/// Rotation is about the PDF page origin, so the object's position shifts; the
/// caller repositions with [`move_image`] using the returned bounds if needed.
pub fn rotate_image(page: u16, object_index: usize, degrees: f32) -> Result<[f32; 4], Error> {
    discard_edit()?;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let mut object = pdf_page.objects_mut().get(object_index)?;
        object.rotate_clockwise_degrees(degrees)?;
        Ok(crate::inspect::rect(object.bounds()?))
    })
}

/// Deletes the object at `object_index` on `page` (any kind: a selected image
/// from [`image_at`], D-023a). The removed object is dropped, freeing it.
pub fn delete_image(page: u16, object_index: usize) -> Result<(), Error> {
    discard_edit()?;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        pdf_page
            .objects_mut()
            .remove_object_at_index(object_index)?;
        Ok(())
    })
}

/// Deletes page `index`. The last page can't be deleted.
pub fn delete_page(index: u16) -> Result<(), Error> {
    discard_edit()?;
    with_open(|document| {
        if document.pages().len() <= 1 {
            return Err(Error::Pdfium("a PDF needs at least one page".into()));
        }
        document.pages().get(index.into())?.delete()?;
        Ok(())
    })
}

/// Page `index` as a new one-page PDF.
///
/// Made from a reloaded copy of the open document with every other page
/// deleted. Creating an empty document and copying the page into it
/// (`create_new_pdf` + import) made PDFium abort after two earlier imports
/// from temporary documents (duplicate / insert), reproducibly; loading a
/// copy doesn't.
pub fn extract_page(index: u16) -> Result<Vec<u8>, Error> {
    let copy = snapshot()?;
    reserve_for_pdfium_copy(copy.len());
    let extracted = pdfium().load_pdf_from_byte_vec(copy, None)?;
    let count = extracted.pages().len();
    let keep = i32::from(index);
    for page in (0..count).rev() {
        if page != keep {
            extracted.pages().get(page)?.delete()?;
        }
    }
    let bytes = extracted.save_to_bytes()?;
    drop(extracted);
    Ok(bytes)
}

/// A new PDF with one blank A4 page, for "New" (D-031). Open it with
/// [`crate::open_document`].
///
/// Written out directly rather than with `create_new_pdf`: after pages have
/// been copied in from other documents, PDFium aborts on a new empty document
/// (P-030), and a user may press New after duplicating or inserting pages.
pub fn blank_document() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << >> >>",
    ];
    let mut pdf = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

/// Inserts all pages of the PDF `bytes` after page `index`; returns how many.
pub fn insert_pdf(bytes: Vec<u8>, index: u16) -> Result<usize, Error> {
    discard_edit()?;
    with_open(|document| {
        reserve_for_pdfium_copy(bytes.len());
        let source = pdfium().load_pdf_from_byte_vec(bytes, None)?;
        let count = source.pages().len();
        if count == 0 {
            return Ok(0);
        }
        document.pages_mut().copy_page_range_from_document(
            &source,
            0..=count - 1,
            i32::from(index) + 1,
        )?;
        Ok(usize::try_from(count).unwrap_or(0))
    })
}
