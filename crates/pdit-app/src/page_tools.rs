//! Page tools (D-029) and their Undo toast (D-030). Every page change keeps a
//! snapshot of the document from just before it; the toast's Undo puts that
//! snapshot back. Extracting a page doesn't change the document, so it saves a
//! new file and shows no toast.

use crate::editing::Editing;
use crate::pages::OpenDocument;
use dioxus::prelude::*;
use pdit_core::page_ops;
use wasm_bindgen::JsCast;

const TOAST_CSS: Asset = asset!("/assets/css/toast.css");
const ICON_BLANK: &str = include_str!("../assets/icons/devigner/DocumentNormal.svg");
const ICON_COPY: &str = include_str!("../assets/icons/devigner/Copy.svg");
const ICON_IMPORT: &str = include_str!("../assets/icons/devigner/Import.svg");
const ICON_ROTATE: &str = include_str!("../assets/icons/devigner/RotateLeft.svg");
const ICON_TRASH: &str = include_str!("../assets/icons/devigner/TrashBinMinimalistic.svg");
const ICON_UNDO: &str = include_str!("../assets/icons/devigner/UndoLeft.svg");
const ICON_MOVE: &str = include_str!("../assets/icons/devigner/SortVertical.svg");
const ICON_IMAGE: &str = include_str!("../assets/icons/devigner/GalleryAdd.svg");
const ICON_SHAPE: &str = include_str!("../assets/icons/devigner/Stop.svg");
const ICON_TABLE: &str = include_str!("../assets/icons/devigner/Grid3x3.svg");
const ICON_SIGNATURE: &str = include_str!("../assets/icons/devigner/Magicpen.svg");
/// The image-options bar's Delete icon (red by CSS, image-select.css, D-023a).
const ICON_PERMANENT_DELETE: &str =
    include_str!("../assets/icons/devigner/TrashBinMinimalistic.svg");

/// The hidden file input that "Insert pages from PDF" clicks.
const INSERT_INPUT_ID: &str = "pdit-insert-pdf";
/// The hidden file input that "Add image" clicks.
const ADD_IMAGE_INPUT_ID: &str = "pdit-add-image";
/// A newly added image spans this fraction of the page width (D-023).
const ADD_IMAGE_WIDTH_FRACTION: f32 = 0.4;
/// A placed signature spans at most this fraction of the page width (D-039)…
const SIGNATURE_WIDTH_FRACTION: f32 = 0.3;
/// …and at most this many points.
const SIGNATURE_MAX_WIDTH_PT: f32 = 160.0;
/// A growing table stops this far from the page edge (D-038).
const EDGE_PT: f32 = 8.0;
/// Text added in a table cell starts this far inside the cell (D-038).
const CELL_INSET_PT: f32 = 4.0;
/// How long the toast stays (proposed value, accepted with D-030).
const TOAST_MS: i32 = 6000;

/// A page change.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PageAction {
    RotateLeft,
    RotateRight,
    InsertBlank,
    Duplicate,
    Delete,
}

/// The toast on screen, with the snapshot its Undo restores.
#[derive(Clone, PartialEq)]
struct Toast {
    id: u64,
    message: String,
    icon: &'static str,
    open: bool,
    undo: Option<std::rc::Rc<Vec<u8>>>,
}

/// A selected image object (D-023a): which object, and its current bounds in
/// PDF points [left, bottom, right, top], for the ring and actions bar.
#[derive(Clone, PartialEq, Debug)]
pub struct ImageSelection {
    pub page: u16,
    pub object_index: usize,
    pub bounds: [f32; 4],
}

/// The table drawn in this session (D-038), which its "+" buttons grow and its
/// lines resize: its grid's object index and layout (PDF points). `shown` is
/// whether its ring and "+" buttons are on screen. Any other document change
/// forgets it (the index may no longer be its grid).
#[derive(Clone, PartialEq, Debug)]
pub struct TableSelection {
    pub page: u16,
    pub object_index: usize,
    pub layout: pdit_core::TableLayout,
    pub shown: bool,
}

/// Shared page-tool state.
#[derive(Clone, Copy)]
pub struct PageTools {
    toast: Signal<Option<Toast>>,
    /// The page "Insert pages from PDF" inserts after, while the picker is open.
    insert_after: Signal<Option<u16>>,
    /// Where "Add image" will place the picked image: (page, x_pt, top_pt).
    image_at: Signal<Option<(u16, f32, f32)>>,
    /// The selected placed image (D-023a), if any.
    image_selection: Signal<Option<ImageSelection>>,
    /// The document snapshot taken when an image drag (move or resize) begins,
    /// so the whole drag is a single Undo (D-023a). The drag itself edits the
    /// document live and redraws the page; the toast + Undo land on release.
    drag_undo: Signal<Option<std::rc::Rc<Vec<u8>>>>,
    /// The table the "+" buttons grow (D-038), if any.
    table: Signal<Option<TableSelection>>,
    /// The table as it was before the last table step, which the toast's Undo
    /// brings back (`None` after a draw: Undo removes the table).
    table_before: Signal<Option<TableSelection>>,
}

impl PageTools {
    pub fn provide() -> Self {
        use_context_provider(|| PageTools {
            toast: Signal::new(None),
            insert_after: Signal::new(None),
            image_at: Signal::new(None),
            image_selection: Signal::new(None),
            drag_undo: Signal::new(None),
            table: Signal::new(None),
            table_before: Signal::new(None),
        })
    }

    /// Runs `action` on page `page`, then shows the toast with an Undo.
    pub fn run(self, action: PageAction, page: u16) {
        let result = self.change(|| match action {
            PageAction::RotateLeft => page_ops::rotate_page(page, false),
            PageAction::RotateRight => page_ops::rotate_page(page, true),
            PageAction::InsertBlank => page_ops::insert_blank_page(page),
            PageAction::Duplicate => page_ops::duplicate_page(page),
            PageAction::Delete => page_ops::delete_page(page),
        });
        let (message, icon) = match action {
            PageAction::RotateLeft | PageAction::RotateRight => ("Page rotated", ICON_ROTATE),
            PageAction::InsertBlank => ("Blank page added", ICON_BLANK),
            PageAction::Duplicate => ("Page duplicated", ICON_COPY),
            PageAction::Delete => ("Page deleted", ICON_TRASH),
        };
        match result {
            Ok(undo) => self.show(message.to_owned(), icon, Some(undo)),
            Err(error) => crate::log(&format!("pdit: {message}: {error}")),
        }
    }

    /// Reorder (D-029): moves page `from` to index `to`, then shows the toast.
    pub fn move_page(self, from: u16, to: u16) {
        if from == to {
            return;
        }
        match self.change(|| page_ops::move_page(from, to)) {
            Ok(undo) => self.show("Page moved".to_owned(), ICON_MOVE, Some(undo)),
            Err(error) => crate::log(&format!("pdit: could not move the page: {error}")),
        }
    }

    /// "Extract page as PDF": saves page `page` as `<name>-page-N.pdf`.
    pub fn extract(self, page: u16) {
        let name = document_signal()
            .peek()
            .as_ref()
            .map(|d| crate::save::derived_name(&d.name, &format!("page-{}", page + 1)))
            .unwrap_or_default();
        match page_ops::extract_page(page) {
            Ok(bytes) => {
                spawn(async move {
                    if let Err(error) = crate::save::save_pdf(&bytes, &name).await {
                        crate::log(&format!("pdit: could not save the page: {error:?}"));
                    }
                });
            }
            Err(error) => crate::log(&format!("pdit: could not extract the page: {error}")),
        }
    }

    /// "Insert pages from PDF…": opens the file picker; the pages go after
    /// page `page`.
    pub fn pick_pdf_to_insert(mut self, page: u16) {
        self.insert_after.set(Some(page));
        click_input(INSERT_INPUT_ID);
    }

    /// "Add image" (D-023): opens the image picker; the picked image lands with
    /// its top-left at (`x_pt`, `top_pt`) on `page`.
    pub fn pick_image_at(mut self, page: u16, x_pt: f32, top_pt: f32) {
        self.image_at.set(Some((page, x_pt, top_pt)));
        click_input(ADD_IMAGE_INPUT_ID);
    }

    /// Draws a shape on `page` (D-023, feature parity), then shows the Undo toast.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_shape(
        self,
        page: u16,
        kind: pdit_core::ShapeKind,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        stroke: [u8; 3],
        width: f32,
        fill: Option<[u8; 3]>,
    ) {
        match self.change(|| page_ops::add_shape(page, kind, x1, y1, x2, y2, stroke, width, fill)) {
            Ok(undo) => self.show("Shape added".to_owned(), ICON_SHAPE, Some(undo)),
            Err(error) => crate::log(&format!("pdit: could not draw the shape: {error}")),
        }
    }

    /// Draws a 1 × 1 table over the dragged rect on `page` (D-038), selects it
    /// so its "+" buttons show, and shows the Undo toast.
    pub fn draw_table(mut self, page: u16, [x1, y1, x2, y2]: [f32; 4]) {
        use crate::shapes_ui::{TABLE_INK, TABLE_WIDTH_PT};
        let (l, b, r, t) = (x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2));
        let index = std::rc::Rc::new(std::cell::Cell::new(0));
        let out = index.clone();
        let result = self.change(move || {
            out.set(page_ops::add_table(
                page,
                l,
                b,
                r,
                t,
                1,
                1,
                TABLE_INK,
                TABLE_WIDTH_PT,
            )?);
            Ok(())
        });
        match result {
            Ok(undo) => {
                self.table.set(Some(TableSelection {
                    page,
                    object_index: index.get(),
                    layout: pdit_core::TableLayout {
                        left: l,
                        top: t,
                        col_widths: vec![r - l],
                        row_heights: vec![t - b],
                    },
                    shown: true,
                }));
                self.show("Table added".to_owned(), ICON_TABLE, Some(undo));
            }
            Err(error) => crate::log(&format!("pdit: could not draw the table: {error}")),
        }
    }

    /// The table for the overlay to draw its ring, "+" buttons and line handles.
    pub fn table(&self) -> Option<TableSelection> {
        self.table.read().clone()
    }

    /// A "+" button (D-038): adds a row (below) or a column (to the right) of
    /// the average size. Where that would leave the page, the table keeps its
    /// size and every row/column shrinks to make room.
    pub fn grow_table(mut self, add_row: bool) {
        let Some(sel) = self.table.peek().clone() else {
            return;
        };
        let page_width = page_ops::page_sizes()
            .ok()
            .and_then(|sizes| sizes.get(sel.page as usize).map(|s| s.0))
            .unwrap_or(0.0);
        let mut new = sel.layout.clone();
        let [_, b, r, _] = new.bounds();
        let (sizes, room) = if add_row {
            (&mut new.row_heights, b - EDGE_PT)
        } else {
            (&mut new.col_widths, page_width - EDGE_PT - r)
        };
        let (total, n) = (sizes.iter().sum::<f32>(), sizes.len() as f32);
        if total / n <= room {
            sizes.push(total / n);
        } else {
            sizes.iter_mut().for_each(|s| *s *= n / (n + 1.0));
            sizes.push(total / (n + 1.0));
        }
        let index = std::rc::Rc::new(std::cell::Cell::new(sel.object_index));
        let out = index.clone();
        let (old, next) = (sel.layout.clone(), new.clone());
        let result = self.change(move || {
            out.set(replace_table(sel.page, sel.object_index, &old, &next)?);
            Ok(())
        });
        match result {
            Ok(undo) => {
                let (rows, cols) = (new.row_heights.len(), new.col_widths.len());
                self.table_before.set(Some(sel.clone()));
                self.table.set(Some(TableSelection {
                    object_index: index.get(),
                    layout: new,
                    ..sel
                }));
                let what = if add_row { "Row" } else { "Column" };
                self.show(
                    format!("{what} added ({rows} × {cols})"),
                    ICON_TABLE,
                    Some(undo),
                );
            }
            Err(error) => crate::log(&format!("pdit: could not grow the table: {error}")),
        }
    }

    /// Starts dragging a table line (D-038): snapshots once so the whole drag
    /// is a single Undo, and remembers the table as it was.
    pub fn begin_table_resize(mut self) {
        self.begin_image_drag();
        self.table_before.set(self.table.peek().clone());
    }

    /// Live step of a line drag: lays the table out as `new` (cell text moves
    /// with its cells) and redraws the page.
    pub fn resize_table(mut self, new: pdit_core::TableLayout) {
        let Some(sel) = self.table.peek().clone() else {
            return;
        };
        if new == sel.layout {
            return;
        }
        match replace_table(sel.page, sel.object_index, &sel.layout, &new) {
            Ok(index) => {
                self.table.set(Some(TableSelection {
                    object_index: index,
                    layout: new,
                    ..sel
                }));
                self.redraw(sel.page);
            }
            Err(error) => crate::log(&format!("pdit: could not resize the table: {error}")),
        }
    }

    /// Ends a line drag: shows the Undo toast if the table changed.
    pub fn end_table_resize(mut self) {
        let Some(undo) = self.drag_undo.peek().clone() else {
            return;
        };
        self.drag_undo.set(None);
        let changed = self.table_before.peek().as_ref().map(|t| &t.layout)
            != self.table.peek().as_ref().map(|t| &t.layout);
        if changed {
            self.show("Table resized".to_owned(), ICON_TABLE, Some(undo));
        }
    }

    /// A press on `page` at (x, y) PDF points outside draw mode: shows the
    /// table's "+" buttons when it lands on the table, hides them otherwise.
    pub fn table_press(mut self, page: u16, x: f32, y: f32) {
        let Some(sel) = self.table.peek().clone() else {
            return;
        };
        let [l, b, r, t] = sel.layout.bounds();
        let on = sel.page == page && x >= l && x <= r && y >= b && y <= t;
        if on != sel.shown {
            self.table.set(Some(TableSelection { shown: on, ..sel }));
        }
    }

    /// Where Add text starts for a right-click at (x, y) on `page` (D-038):
    /// inside a cell of the table, snapped to the cell's top-left inset
    /// (Add text centres the line on y); elsewhere, the point itself.
    pub fn snap_to_cell(&self, page: u16, x: f32, y: f32) -> (f32, f32) {
        let Some(sel) = self.table.peek().clone() else {
            return (x, y);
        };
        let [l, b, r, t] = sel.layout.bounds();
        if sel.page != page || x < l || x > r || y < b || y > t {
            return (x, y);
        }
        let mut cell_left = l;
        for w in &sel.layout.col_widths {
            if x <= cell_left + w {
                break;
            }
            cell_left += w;
        }
        let (mut cell_top, mut cell_h) = (t, 0.0);
        for h in &sel.layout.row_heights {
            cell_h = *h;
            if y >= cell_top - h {
                break;
            }
            cell_top -= h;
        }
        (
            cell_left + CELL_INSET_PT,
            cell_top - (cell_h / 2.0).min(CELL_INSET_PT + 6.0),
        )
    }

    /// Places a signature image (D-039) with its top-left at (`x_pt`, `top_pt`)
    /// on `page`, at most 30% of the page width, selects it (image options
    /// apply), and shows the Undo toast.
    pub fn add_signature(self, page: u16, x_pt: f32, top_pt: f32, png: Vec<u8>) {
        let page_width = page_ops::page_sizes()
            .ok()
            .and_then(|sizes| sizes.get(page as usize).map(|s| s.0))
            .unwrap_or(0.0);
        let width_pt = (page_width * SIGNATURE_WIDTH_FRACTION).min(SIGNATURE_MAX_WIDTH_PT);
        if width_pt <= 0.0 {
            return;
        }
        match self.change(|| page_ops::add_image(page, png, x_pt, top_pt, width_pt)) {
            Ok(undo) => {
                self.select_image(page, x_pt + 1.0, top_pt - 1.0);
                self.show("Signature added".to_owned(), ICON_SIGNATURE, Some(undo));
            }
            Err(error) => crate::log(&format!("pdit: could not add the signature: {error}")),
        }
    }

    /// Runs a document change from another tool (annotations, D-040): the
    /// same snapshot, page refresh and Undo toast as the page tools.
    pub fn apply(
        self,
        message: &str,
        icon: &'static str,
        change: impl FnOnce() -> Result<(), pdit_core::Error>,
    ) -> bool {
        match self.change(change) {
            Ok(undo) => {
                self.show(message.to_owned(), icon, Some(undo));
                true
            }
            Err(error) => {
                crate::log(&format!("pdit: {message}: {error}"));
                false
            }
        }
    }

    /// A snapshot of the document now, for a tool that edits in several steps
    /// and offers one Undo for all of them (D-042).
    pub fn snapshot(self) -> Option<std::rc::Rc<Vec<u8>>> {
        match page_ops::snapshot() {
            Ok(s) => Some(std::rc::Rc::new(s)),
            Err(error) => {
                crate::log(&format!("pdit: could not snapshot: {error}"));
                None
            }
        }
    }

    /// Shows the pages again after a change made outside the page tools.
    pub fn refresh_pages(self) {
        if let Ok(sizes) = page_ops::page_sizes() {
            refresh(sizes);
        }
    }

    /// Puts `snapshot` back and shows the pages again (no toast).
    pub fn restore_snapshot(self, snapshot: &std::rc::Rc<Vec<u8>>) {
        consume_context::<Editing>().cancel();
        match page_ops::restore(snapshot.as_ref().clone()) {
            Ok(sizes) => refresh(sizes),
            Err(error) => crate::log(&format!("pdit: could not restore: {error}")),
        }
    }

    /// Runs `change` without taking a snapshot (the caller made one), shows
    /// the pages again, then the toast whose Undo puts `undo` back.
    pub fn apply_since(
        self,
        undo: std::rc::Rc<Vec<u8>>,
        message: &str,
        icon: &'static str,
        change: impl FnOnce() -> Result<(), pdit_core::Error>,
    ) -> bool {
        consume_context::<Editing>().cancel();
        let result = change().and_then(|()| page_ops::page_sizes());
        match result {
            Ok(sizes) => {
                refresh(sizes);
                self.show(message.to_owned(), icon, Some(undo));
                true
            }
            Err(error) => {
                crate::log(&format!("pdit: {message}: {error}"));
                false
            }
        }
    }

    fn add_image(self, bytes: Vec<u8>) {
        let Some((page, x_pt, top_pt)) = *self.image_at.peek() else {
            return;
        };
        let width_pt = page_ops::page_sizes()
            .ok()
            .and_then(|sizes| sizes.get(page as usize).map(|s| s.0))
            .unwrap_or(0.0)
            * ADD_IMAGE_WIDTH_FRACTION;
        if width_pt <= 0.0 {
            return;
        }
        match self.change(|| page_ops::add_image(page, bytes, x_pt, top_pt, width_pt)) {
            Ok(undo) => self.show("Image added".to_owned(), ICON_IMAGE, Some(undo)),
            Err(error) => crate::log(&format!("pdit: could not add the image: {error}")),
        }
    }

    /// "Select image" (D-023a): selects the image object under (x, y) PDF points
    /// on `page`, so the ring and actions bar appear. Closes any text edit.
    pub fn select_image(mut self, page: u16, x: f32, y: f32) {
        consume_context::<Editing>().cancel();
        match page_ops::image_at(page, x, y) {
            Ok(Some(hit)) => self.image_selection.set(Some(ImageSelection {
                page,
                object_index: hit.object_index,
                bounds: hit.bounds,
            })),
            _ => self.image_selection.set(None),
        }
    }

    /// The selected image, for the overlay to draw its ring and bar.
    pub fn image_selection(&self) -> Option<ImageSelection> {
        self.image_selection.read().clone()
    }

    /// Deselects the image (click elsewhere, Escape).
    pub fn clear_image_selection(mut self) {
        if self.image_selection.peek().is_some() {
            self.image_selection.set(None);
        }
    }

    /// Rotates the selected image 90° (D-023a), keeping it centred where it is,
    /// then shows the Undo toast. Both the rotate and the re-centring move are
    /// one change, so one Undo puts the image back.
    pub fn rotate_image_selection(self, clockwise: bool) {
        let Some(sel) = self.image_selection.peek().clone() else {
            return;
        };
        let degrees = if clockwise { 90.0 } else { -90.0 };
        let [l, b, r, t] = sel.bounds;
        let (cx, cy) = ((l + r) / 2.0, (b + t) / 2.0);
        let final_bounds = std::rc::Rc::new(std::cell::Cell::new(sel.bounds));
        let out = final_bounds.clone();
        let result = self.change(move || {
            let [rl, rb, rr, rt] = page_ops::rotate_image(sel.page, sel.object_index, degrees)?;
            let (ncx, ncy) = ((rl + rr) / 2.0, (rb + rt) / 2.0);
            out.set(page_ops::move_image(
                sel.page,
                sel.object_index,
                cx - ncx,
                cy - ncy,
            )?);
            Ok(())
        });
        match result {
            Ok(undo) => {
                self.set_selection_bounds(final_bounds.get());
                self.show("Image rotated".to_owned(), ICON_ROTATE, Some(undo));
            }
            Err(error) => crate::log(&format!("pdit: could not rotate the image: {error}")),
        }
    }

    /// Deletes the selected image (D-023a), clears the selection, and shows the
    /// Undo toast.
    pub fn delete_image_selection(mut self) {
        let Some(sel) = self.image_selection.peek().clone() else {
            return;
        };
        match self.change(|| page_ops::delete_image(sel.page, sel.object_index)) {
            Ok(undo) => {
                self.image_selection.set(None);
                self.show("Image deleted".to_owned(), ICON_TRASH, Some(undo));
            }
            Err(error) => crate::log(&format!("pdit: could not delete the image: {error}")),
        }
    }

    fn set_selection_bounds(mut self, bounds: [f32; 4]) {
        self.image_selection.with_mut(|s| {
            if let Some(s) = s.as_mut() {
                s.bounds = bounds;
            }
        });
    }

    /// Redraws one page in place (no remount), like a text-edit preview.
    fn redraw(self, page: u16) {
        consume_context::<Editing>().redraw(page);
    }

    /// Starts an image drag (move or resize): snapshots the document once so the
    /// whole gesture is a single Undo. The drag then edits live via
    /// [`Self::drag_move`] / [`Self::drag_resize`], and [`Self::end_image_drag`]
    /// shows the toast.
    pub fn begin_image_drag(mut self) {
        match page_ops::snapshot() {
            Ok(snap) => self.drag_undo.set(Some(std::rc::Rc::new(snap))),
            Err(error) => crate::log(&format!("pdit: could not snapshot for the drag: {error}")),
        }
    }

    /// Live step of a move drag: shifts the selected image by (`dx_pt` right,
    /// `dy_pt` up) and redraws the page so the image itself follows the pointer.
    pub fn drag_move(self, page: u16, dx_pt: f32, dy_pt: f32) {
        let Some(sel) = self.image_selection.peek().clone() else {
            return;
        };
        if let Ok(bounds) = page_ops::move_image(page, sel.object_index, dx_pt, dy_pt) {
            self.set_selection_bounds(bounds);
            self.redraw(page);
        }
    }

    /// Live step of a resize drag: scales the selected image to `width_pt`
    /// (aspect kept) and keeps the fixed (opposite) corner put — `fixed_left`/
    /// `fixed_top` pick which corner stays, `anchor` is its held position in PDF
    /// points. Redraws the page so the image resizes under the pointer.
    pub fn drag_resize(
        self,
        page: u16,
        width_pt: f32,
        fixed_left: bool,
        fixed_top: bool,
        anchor: (f32, f32),
    ) {
        let Some(sel) = self.image_selection.peek().clone() else {
            return;
        };
        let Ok([l, b, r, t]) = page_ops::resize_image(page, sel.object_index, width_pt) else {
            return;
        };
        let fx = if fixed_left { l } else { r };
        let fy = if fixed_top { t } else { b };
        if let Ok(bounds) =
            page_ops::move_image(page, sel.object_index, anchor.0 - fx, anchor.1 - fy)
        {
            self.set_selection_bounds(bounds);
            self.redraw(page);
        }
    }

    /// Ends an image drag: shows the Undo toast for the whole gesture. `resized`
    /// picks the message/icon (moved vs resized). The page canvas is already
    /// current from the live redraws, so we don't `refresh` here — that would
    /// remount the page and flash the skeleton. (The pages-panel thumbnail then
    /// lags until the next refresh-triggering edit; acceptable for a drag.)
    pub fn end_image_drag(mut self, resized: bool) {
        let Some(undo) = self.drag_undo.peek().clone() else {
            return;
        };
        self.drag_undo.set(None);
        let (message, icon) = if resized {
            ("Image resized", ICON_IMAGE)
        } else {
            ("Image moved", ICON_MOVE)
        };
        self.show(message.to_owned(), icon, Some(undo));
    }

    fn insert_pdf(self, bytes: Vec<u8>) {
        let Some(page) = *self.insert_after.peek() else {
            return;
        };
        let mut count = 0;
        let result = self.change(|| {
            count = page_ops::insert_pdf(bytes, page)?;
            Ok(())
        });
        match result {
            Ok(undo) => {
                let message = match count {
                    1 => "1 page inserted".to_owned(),
                    n => format!("{n} pages inserted"),
                };
                self.show(message, ICON_IMPORT, Some(undo));
            }
            Err(error) => crate::log(&format!("pdit: could not insert the PDF: {error}")),
        }
    }

    /// Snapshots the document, runs `change`, and shows the new pages.
    /// Returns the snapshot for Undo.
    fn change(
        self,
        change: impl FnOnce() -> Result<(), pdit_core::Error>,
    ) -> Result<std::rc::Rc<Vec<u8>>, pdit_core::Error> {
        consume_context::<Editing>().cancel();
        // The change may move objects; the table's index is then unknown.
        // (Table draw/grow set it again afterwards.)
        let (mut table, mut before) = (self.table, self.table_before);
        table.set(None);
        before.set(None);
        let undo = page_ops::snapshot()?;
        change()?;
        refresh(page_ops::page_sizes()?);
        Ok(std::rc::Rc::new(undo))
    }

    pub(crate) fn show(
        mut self,
        message: String,
        icon: &'static str,
        undo: Option<std::rc::Rc<Vec<u8>>>,
    ) {
        let id = self.toast.peek().as_ref().map_or(0, |t| t.id + 1);
        self.toast.set(Some(Toast {
            id,
            message,
            icon,
            open: false,
            undo,
        }));
        let mut toast = self.toast;
        // One frame closed, so the opening transition runs.
        next_frame(move || {
            toast.with_mut(|t| {
                if let Some(t) = t.as_mut().filter(|t| t.id == id) {
                    t.open = true;
                }
            })
        });
        set_timeout(TOAST_MS, move || self.hide(id));
    }

    fn hide(mut self, id: u64) {
        self.toast.with_mut(|t| {
            if let Some(t) = t.as_mut().filter(|t| t.id == id) {
                // The snapshot stays until the next toast replaces this one, so
                // the Undo button doesn't vanish while the toast fades out; a
                // closed toast takes no clicks (pointer-events, toast.css).
                t.open = false;
            }
        });
    }

    /// The last page-tool step can be undone (the centre bar's Undo, D-062).
    pub fn can_undo(&self) -> bool {
        self.toast.read().as_ref().is_some_and(|t| t.undo.is_some())
    }

    /// The centre bar's Undo: the same as the last toast's Undo.
    pub fn undo_last(self) {
        self.undo();
    }

    fn undo(mut self) {
        let Some(toast) = self.toast.peek().clone() else {
            return;
        };
        self.table.set(None);
        self.hide(toast.id);
        let Some(snapshot) = toast.undo else {
            return;
        };
        // Used once: a second Undo must not restore the same snapshot again.
        self.toast.with_mut(|t| {
            if let Some(t) = t.as_mut() {
                t.undo = None;
            }
        });
        let before = self
            .table_before
            .peek()
            .clone()
            .map(|t| TableSelection { shown: true, ..t });
        consume_context::<Editing>().cancel();
        match page_ops::restore(snapshot.as_ref().clone()) {
            Ok(sizes) => {
                refresh(sizes);
                // Undoing a table step brings back the table it grew from.
                self.table.set(before);
                self.table_before.set(None);
            }
            Err(error) => crate::log(&format!("pdit: could not undo: {error}")),
        }
    }
}

fn document_signal() -> Signal<Option<OpenDocument>> {
    consume_context::<Signal<Option<OpenDocument>>>()
}

/// Shows the document's pages again after a change (new page sizes, and a new
/// id so every page redraws).
fn refresh(page_sizes: Vec<(f32, f32)>) {
    let mut document = document_signal();
    let current = document.peek().clone();
    if let Some(current) = current {
        document.set(Some(OpenDocument {
            id: current.id + 1,
            name: current.name,
            page_sizes,
        }));
    }
}

/// The toast and the hidden file input for inserting a PDF. Rendered once.
#[component]
pub fn PageToolsUi() -> Element {
    let tools = use_context::<PageTools>();
    let toast = tools.toast.read().clone();
    rsx! {
        document::Stylesheet { href: TOAST_CSS }
        input {
            id: INSERT_INPUT_ID,
            r#type: "file",
            accept: "application/pdf,.pdf",
            hidden: true,
            onchange: move |event| async move {
                let Some(file) = event.files().into_iter().next() else {
                    return;
                };
                match file.read_bytes().await {
                    Ok(bytes) => tools.insert_pdf(bytes.to_vec()),
                    Err(error) => crate::log(&format!("pdit: could not read the file: {error}")),
                }
            },
        }
        input {
            id: ADD_IMAGE_INPUT_ID,
            r#type: "file",
            accept: "image/png,image/jpeg,.png,.jpg,.jpeg",
            hidden: true,
            onchange: move |event| async move {
                let Some(file) = event.files().into_iter().next() else {
                    return;
                };
                match file.read_bytes().await {
                    Ok(bytes) => tools.add_image(bytes.to_vec()),
                    Err(error) => crate::log(&format!("pdit: could not read the image: {error}")),
                }
            },
        }
        div { class: "pdit-toast-host",
            if let Some(toast) = toast {
                div {
                    key: "{toast.id}",
                    class: if toast.open { "pdit-toast t-toast sa-root is-open" } else { "pdit-toast t-toast sa-root" },
                    role: "status",
                    "aria-live": "polite",
                    span {
                        class: "toast-icon",
                        "aria-hidden": "true",
                        dangerous_inner_html: toast.icon,
                    }
                    span { "{toast.message}" }
                    if toast.undo.is_some() {
                        button {
                            r#type: "button",
                            class: "sa-primary",
                            onclick: move |_| tools.undo(),
                            span { dangerous_inner_html: ICON_UNDO, style: "display: contents" }
                            "Undo"
                        }
                    }
                }
            }
        }
    }
}

/// An in-progress corner-resize drag (D-023a): which corner stays fixed and
/// where it is held (PDF points), plus the page's left edge in CSS px for
/// converting pointer x to points.
#[derive(Clone, Copy)]
struct ResizeDrag {
    fixed_left: bool,
    fixed_top: bool,
    anchor: (f32, f32),
    page_left: f64,
}

/// The selected image's metal ring, four corner resize handles, and the shared
/// Selection Actions bar (D-023a). Rendered per page like
/// [`crate::editing::SelectionOverlay`]; shows only while the selected image is
/// on this page. `scale` is CSS px per PDF point. Move-to-drag is handled by the
/// page's pointer events; the handles here resize (aspect kept), and the bar
/// rotates 90° L/R and deletes. All edits redraw live so the image follows.
#[component]
pub fn ImageOverlay(page: u16, page_height_pt: f32, scale: f32) -> Element {
    let tools = use_context::<PageTools>();
    let mut resize = use_signal(|| None::<ResizeDrag>);
    let Some(sel) = tools.image_selection().filter(|s| s.page == page) else {
        return rsx! {};
    };
    let [left, bottom, right, top] = sel.bounds;
    // A few px of air around the image, like the text line's outline.
    let pad = 3.0_f32;
    let x = left * scale - pad;
    let y = (page_height_pt - top) * scale - pad;
    let width = (right - left) * scale + 2.0 * pad;
    let height = (top - bottom) * scale + 2.0 * pad;
    let bar_x = (left + (right - left) / 2.0) * scale;
    let bar_y = (page_height_pt - bottom) * scale + 10.0;
    // (css class, corner x px, corner y px, fixed_left, fixed_top, anchor pt).
    // The fixed corner is the one diagonally opposite the dragged handle.
    let corners = [
        ("tl", x, y, false, false, (right, bottom)),
        ("tr", x + width, y, true, false, (left, bottom)),
        ("bl", x, y + height, false, true, (right, top)),
        ("br", x + width, y + height, true, true, (left, top)),
    ];
    rsx! {
        div { class: "sa-root",
            div {
                class: "pdit-img-ring",
                style: "left: {x}px; top: {y}px; width: {width}px; height: {height}px;",
            }
            for (cls, cx, cy, fixed_left, fixed_top, anchor) in corners {
                div {
                    class: "pdit-img-handle {cls}",
                    style: "left: {cx}px; top: {cy}px;",
                    onpointerdown: move |event| {
                        event.stop_propagation();
                        let Some(pe) = event
                            .data()
                            .downcast::<web_sys::PointerEvent>()
                            .cloned()
                        else {
                            return;
                        };
                        let page_left = pe
                            .target()
                            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                            .and_then(|el| el.closest(".page").ok().flatten())
                            .map(|p| p.get_bounding_client_rect().left())
                            .unwrap_or(0.0);
                        if let Some(el) = pe
                            .target()
                            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                        {
                            let _ = el.set_pointer_capture(pe.pointer_id());
                        }
                        tools.begin_image_drag();
                        resize.set(Some(ResizeDrag { fixed_left, fixed_top, anchor, page_left }));
                    },
                    onpointermove: move |event| {
                        let Some(rd) = resize() else { return };
                        let pointer_x_pt =
                            ((event.client_coordinates().x - rd.page_left) / f64::from(scale)) as f32;
                        let width_pt = (pointer_x_pt - rd.anchor.0).abs().max(8.0);
                        tools.drag_resize(page, width_pt, rd.fixed_left, rd.fixed_top, rd.anchor);
                    },
                    onpointerup: move |_| {
                        if resize.peek().is_some() {
                            resize.set(None);
                            tools.end_image_drag(true);
                        }
                    },
                }
            }
            div {
                class: "sa-anchor",
                style: "left: {bar_x}px; top: {bar_y}px; transform: translateX(-50%);",
                // The bar sits over empty page space below the image; without
                // this, the page's pointer-down would clear the selection before
                // a bar button's click runs, so Rotate/Delete would do nothing.
                onpointerdown: move |event| event.stop_propagation(),
                div { class: "sa-bar",
                    button {
                        r#type: "button",
                        class: "sa-control",
                        "aria-label": "Rotate left",
                        onclick: move |event| {
                            event.stop_propagation();
                            tools.rotate_image_selection(false);
                        },
                        span { dangerous_inner_html: ICON_ROTATE, style: "display: contents" }
                    }
                    button {
                        r#type: "button",
                        class: "sa-control sa-mirror",
                        "aria-label": "Rotate right",
                        onclick: move |event| {
                            event.stop_propagation();
                            tools.rotate_image_selection(true);
                        },
                        span { dangerous_inner_html: ICON_ROTATE, style: "display: contents" }
                    }
                    button {
                        r#type: "button",
                        class: "sa-primary",
                        onclick: move |event| {
                            event.stop_propagation();
                            tools.delete_image_selection();
                        },
                        span { class: "sa-danger", dangerous_inner_html: ICON_PERMANENT_DELETE, style: "display: contents" }
                        "Delete"
                    }
                }
            }
        }
    }
}

/// Clears and clicks a hidden file input by id (so choosing the same file again
/// still fires `change`).
fn click_input(id: &str) {
    if let Some(input) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
    {
        input.set_value("");
        input.click();
    }
}

pub(crate) fn next_frame(f: impl FnOnce() + 'static) {
    if let Some(window) = web_sys::window() {
        let callback = wasm_bindgen::closure::Closure::once_into_js(f);
        let _ = window.request_animation_frame(callback.unchecked_ref());
    }
}

pub(crate) fn set_timeout(ms: i32, f: impl FnOnce() + 'static) {
    if let Some(window) = web_sys::window() {
        let callback = wasm_bindgen::closure::Closure::once_into_js(f);
        let _ = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), ms);
    }
}

/// [`page_ops::replace_table`] with the table's ink (D-038: no style bar).
fn replace_table(
    page: u16,
    object_index: usize,
    old: &pdit_core::TableLayout,
    new: &pdit_core::TableLayout,
) -> Result<usize, pdit_core::Error> {
    use crate::shapes_ui::{TABLE_INK, TABLE_WIDTH_PT};
    page_ops::replace_table(page, object_index, old, new, TABLE_INK, TABLE_WIDTH_PT)
}
