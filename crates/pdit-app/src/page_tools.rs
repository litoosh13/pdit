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
/// Merge PDF (D-075): Devigner Documents, picked by the user.
pub const ICON_MERGE: &str = include_str!("../assets/icons/devigner/Documents.svg");
/// Crop image (D-073): Devigner Crop, picked by the user.
const ICON_CROP: &str = include_str!("../assets/icons/devigner/Crop.svg");
/// Remove background (D-074): Devigner Grid eraser.
const ICON_REMOVE_BACKGROUND: &str = include_str!("../assets/icons/devigner/GridEraser.svg");
pub(crate) const ICON_FIT: &str = include_str!("../assets/icons/devigner/MaximizeSquare2.svg");
/// The smallest crop frame side (PDF points).
const MIN_CROP_PT: f32 = 4.0;
const ICON_UNDO: &str = include_str!("../assets/icons/devigner/UndoLeft.svg");
const ICON_MOVE: &str = include_str!("../assets/icons/devigner/SortVertical.svg");
const ICON_IMAGE: &str = include_str!("../assets/icons/devigner/GalleryAdd.svg");
const ICON_SHAPE: &str = include_str!("../assets/icons/devigner/Stop.svg");
const ICON_TABLE: &str = include_str!("../assets/icons/devigner/Grid3x3.svg");
const ICON_SIGNATURE: &str = include_str!("../assets/icons/devigner/Magicpen.svg");
/// Clean scan (D-068, Devigner Broom, picked by the user).
pub const ICON_BROOM: &str = include_str!("../assets/icons/devigner/Broom.svg");
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
    /// The picture's or shape's object index, or the stamp's or drawing's annotation index.
    pub object_index: usize,
    pub bounds: [f32; 4],
    pub kind: Placed,
}

/// What is selected (D-081): all are moved, resized and deleted the same way; rotate, crop and background are for
/// pictures only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placed {
    Picture,
    /// A shape pdit drew (a page object with its tag).
    Shape,
    Stamp,
    /// A pen drawing.
    Drawing,
    /// Text added with pdit (its lines move and scale together; `object_index` is its box number).
    TextBox,
}

impl Placed {
    fn name(self) -> &'static str {
        match self {
            Placed::Picture => "Image",
            Placed::Shape => "Shape",
            Placed::Stamp => "Stamp",
            Placed::Drawing => "Drawing",
            Placed::TextBox => "Text",
        }
    }
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
    /// The picker is Merge PDF's (D-075): the pages go at the end.
    merging: Signal<bool>,
    /// Where "Add image" will place the picked image: (page, x_pt, top_pt).
    image_at: Signal<Option<(u16, f32, f32)>>,
    /// The selected placed image (D-023a), if any.
    image_selection: Signal<Option<ImageSelection>>,
    /// Cropping the selected image (D-073): the frame [left, bottom, right, top] in PDF points.
    image_crop: Signal<Option<[f32; 4]>>,
    /// The document snapshot taken when an image drag (move or resize) begins,
    /// so the whole drag is a single Undo (D-023a). The drag itself edits the
    /// document live and redraws the page; the toast + Undo land on release.
    drag_undo: Signal<Option<std::rc::Rc<Vec<u8>>>>,
    /// The table the "+" buttons grow (D-038), if any.
    table: Signal<Option<TableSelection>>,
    /// The table as it was before the last table step, which the toast's Undo
    /// brings back (`None` after a draw: Undo removes the table).
    table_before: Signal<Option<TableSelection>>,
    /// Whether Undo / Redo can go a step (P-050 #2: the engine's history, read by [`Self::sync_history`]).
    undoable: Signal<(bool, bool)>,
}

impl PageTools {
    pub fn provide() -> Self {
        use_context_provider(|| PageTools {
            toast: Signal::new(None),
            insert_after: Signal::new(None),
            merging: Signal::new(false),
            image_at: Signal::new(None),
            image_selection: Signal::new(None),
            image_crop: Signal::new(None),
            drag_undo: Signal::new(None),
            table: Signal::new(None),
            table_before: Signal::new(None),
            undoable: Signal::new((false, false)),
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
                    if let Err(error) = crate::save::save_file(&bytes, &name).await {
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
        self.merging.set(false);
        click_input(INSERT_INPUT_ID);
    }

    /// Merge PDF… (D-075): opens the file picker; the picked PDF's pages go after the last page.
    pub fn pick_pdf_to_merge(mut self) {
        let pages = page_ops::page_sizes().map_or(0, |s| s.len());
        if pages == 0 {
            return;
        }
        self.insert_after.set(Some((pages - 1) as u16));
        self.merging.set(true);
        click_input(INSERT_INPUT_ID);
    }

    /// "Add image" (D-023): opens the image picker; the picked image lands with
    /// its top-left at (`x_pt`, `top_pt`) on `page`.
    /// The rail's Add image: the picture window right away; the picture goes near the top of `page`, centred
    /// (it can be moved after).
    pub fn pick_image(self, page: u16) {
        let (w, h) = page_ops::page_sizes()
            .ok()
            .and_then(|sizes| sizes.get(page as usize).copied())
            .unwrap_or((0.0, 0.0));
        let (ox, oy) = page_ops::page_origin(page).unwrap_or((0.0, 0.0));
        self.pick_image_at(
            page,
            ox + w * (1.0 - ADD_IMAGE_WIDTH_FRACTION) / 2.0,
            oy + h * 0.85,
        );
    }

    /// A picture dragged onto pdit: it lands on the page being looked at, as the rail's Add image places it.
    pub fn drop_image(mut self, bytes: Vec<u8>) {
        let page = crate::bookmarks_ui::current_page();
        let (w, h) = page_ops::page_sizes()
            .ok()
            .and_then(|sizes| sizes.get(page as usize).copied())
            .unwrap_or((0.0, 0.0));
        let (ox, oy) = page_ops::page_origin(page).unwrap_or((0.0, 0.0));
        self.image_at.set(Some((
            page,
            ox + w * (1.0 - ADD_IMAGE_WIDTH_FRACTION) / 2.0,
            oy + h * 0.85,
        )));
        self.add_image(bytes);
    }

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

    /// Clean scan (D-068): `pages` become white paper with dark text, with one
    /// Undo; nothing changes if one of them fails.
    pub fn clean_scans(self, pages: &[u16]) {
        if pages.is_empty() {
            return self.show("No scanned pages to clean".into(), ICON_BROOM, None);
        }
        consume_context::<Editing>().cancel();
        let Some(before) = self.snapshot() else {
            return;
        };
        let cleaned = pages
            .iter()
            .try_for_each(|&p| pdit_core::scan_clean::clean_scan(p).map(drop));
        match cleaned {
            Ok(()) => {
                self.refresh_pages();
                let message = match pages.len() {
                    1 => "Scan cleaned".to_owned(),
                    n => format!("{n} scanned pages cleaned"),
                };
                self.show(message, ICON_BROOM, Some(before));
            }
            Err(error) => {
                self.restore_snapshot(&before);
                crate::log(&format!("pdit: clean scan: {error}"));
                self.show("This scan couldn't be cleaned".into(), ICON_BROOM, None);
            }
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
        self.image_crop.set(None);
        self.image_selection.set(placed_at(page, x, y));
    }

    /// Something that can be selected, moved and resized is under (x, y) on `page` (D-081).
    pub fn movable_at(&self, page: u16, x: f32, y: f32) -> bool {
        placed_at(page, x, y).is_some()
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

    /// Crop (D-073): the crop frame over the whole selected image.
    pub fn start_image_crop(mut self) {
        if let Some(sel) = self.image_selection.peek().clone() {
            self.image_crop.set(Some(sel.bounds));
        }
    }

    pub fn image_crop(&self) -> Option<[f32; 4]> {
        *self.image_crop.read()
    }

    pub fn cancel_image_crop(mut self) {
        self.image_crop.set(None);
    }

    /// Moves the crop frame's `part` (a handle: "tl", "t", …, or "move") by (`dx`, `dy`) PDF points, inside the
    /// image.
    pub fn drag_image_crop(mut self, part: &str, dx: f32, dy: f32) {
        let (Some(sel), Some([mut l, mut b, mut r, mut t])) =
            (self.image_selection.peek().clone(), *self.image_crop.peek())
        else {
            return;
        };
        let [il, ib, ir, it] = sel.bounds;
        if part == "move" {
            let mx = dx.clamp(il - l, ir - r);
            let my = dy.clamp(ib - b, it - t);
            (l, r, b, t) = (l + mx, r + mx, b + my, t + my);
        }
        if part.contains('l') {
            l = (l + dx).clamp(il, r - MIN_CROP_PT);
        }
        if part.contains('r') {
            r = (r + dx).clamp(l + MIN_CROP_PT, ir);
        }
        if part.contains('b') {
            b = (b + dy).clamp(ib, t - MIN_CROP_PT);
        }
        if part.starts_with('t') {
            t = (t + dy).clamp(b + MIN_CROP_PT, it);
        }
        self.image_crop.set(Some([l, b, r, t]));
    }

    /// Crop: cuts the selected image to the frame; the rest stays in place. Undo toast.
    pub fn apply_image_crop(mut self) {
        let (Some(sel), Some(frame)) =
            (self.image_selection.peek().clone(), *self.image_crop.peek())
        else {
            return;
        };
        self.image_crop.set(None);
        let bounds = std::rc::Rc::new(std::cell::Cell::new(sel.bounds));
        let out = bounds.clone();
        match self.change(move || {
            out.set(page_ops::crop_image(sel.page, sel.object_index, frame)?);
            Ok(())
        }) {
            Ok(undo) => {
                self.set_selection_bounds(bounds.get());
                self.show("Image cropped".to_owned(), ICON_CROP, Some(undo));
            }
            Err(error) => crate::log(&format!("pdit: could not crop the image: {error}")),
        }
    }

    /// Remove background (D-074): only the ink of the selected picture stays (signature, stamp, handwriting on
    /// paper); Undo toast.
    pub fn remove_background_selection(self) {
        let Some(sel) = self.image_selection.peek().clone() else {
            return;
        };
        match self.change(|| page_ops::remove_image_background(sel.page, sel.object_index)) {
            Ok(undo) => self.show(
                "Background removed".to_owned(),
                ICON_REMOVE_BACKGROUND,
                Some(undo),
            ),
            Err(error) => crate::log(&format!("pdit: could not remove the background: {error}")),
        }
    }

    /// Fit to page: the selected picture covers the whole page.
    pub fn fit_image_selection(self) {
        let Some(sel) = self.image_selection.peek().clone() else {
            return;
        };
        let bounds = std::rc::Rc::new(std::cell::Cell::new(sel.bounds));
        let out = bounds.clone();
        let result = self.change(move || {
            out.set(page_ops::fit_image_to_page(sel.page, sel.object_index)?);
            Ok(())
        });
        match result {
            Ok(undo) => {
                self.set_selection_bounds(bounds.get());
                self.show("Picture fits the page".to_owned(), ICON_FIT, Some(undo));
            }
            Err(error) => crate::log(&format!("pdit: could not fit the picture: {error}")),
        }
    }

    /// Deletes the selected image (D-023a), clears the selection, and shows the
    /// Undo toast.
    pub fn delete_image_selection(mut self) {
        let Some(sel) = self.image_selection.peek().clone() else {
            return;
        };
        let result = self.change(|| match sel.kind {
            Placed::Stamp | Placed::Drawing => {
                pdit_core::annotations::delete_annotation(sel.page, sel.object_index)
            }
            Placed::TextBox => page_ops::delete_text_box(sel.page, sel.object_index),
            Placed::Picture | Placed::Shape => page_ops::delete_image(sel.page, sel.object_index),
        });
        match result {
            Ok(undo) => {
                self.image_selection.set(None);
                let message = format!("{} deleted", sel.kind.name());
                self.show(message, ICON_TRASH, Some(undo));
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
        if let Ok(bounds) = move_selected(&sel, dx_pt, dy_pt) {
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
        let resized = match sel.kind {
            Placed::Stamp | Placed::Drawing => {
                pdit_core::annotations::resize_drawn(page, sel.object_index, width_pt)
            }
            Placed::TextBox => page_ops::resize_text_box(page, sel.object_index, width_pt),
            Placed::Picture | Placed::Shape => {
                page_ops::resize_image(page, sel.object_index, width_pt)
            }
        };
        let Ok([l, b, r, t]) = resized else {
            return;
        };
        let fx = if fixed_left { l } else { r };
        let fy = if fixed_top { t } else { b };
        if let Ok(bounds) = move_selected(&sel, anchor.0 - fx, anchor.1 - fy) {
            self.set_selection_bounds(bounds);
            self.redraw(page);
        }
    }

    /// Live step of a side-handle drag: stretches the selection to `width_pt` × `height_pt` (a text box instead
    /// re-wraps its `text` to the width, its letters kept) and keeps the held side put, as [`Self::drag_resize`].
    #[allow(clippy::too_many_arguments)]
    pub fn drag_stretch(
        self,
        page: u16,
        width_pt: f32,
        height_pt: f32,
        text: Option<&str>,
        fixed_left: bool,
        fixed_top: bool,
        anchor: (f32, f32),
    ) {
        let Some(sel) = self.image_selection.peek().clone() else {
            return;
        };
        let stretched = match (sel.kind, text) {
            (Placed::Stamp | Placed::Drawing, _) => {
                pdit_core::annotations::stretch_drawn(page, sel.object_index, width_pt, height_pt)
            }
            (Placed::TextBox, Some(text)) => {
                let Some(noto) = consume_context::<Editing>().fallback_font.peek().clone() else {
                    return;
                };
                pdit_core::rewrap_text_box(
                    page,
                    sel.object_index as i32,
                    text,
                    width_pt,
                    &pdit_core::Fonts::noto(&noto),
                )
            }
            (Placed::TextBox, None) => return,
            (Placed::Picture | Placed::Shape, _) => {
                page_ops::stretch_image(page, sel.object_index, width_pt, height_pt)
            }
        };
        let Ok([l, b, r, t]) = stretched else {
            return;
        };
        let fx = if fixed_left { l } else { r };
        let fy = if fixed_top { t } else { b };
        if let Ok(bounds) = move_selected(&sel, anchor.0 - fx, anchor.1 - fy) {
            self.set_selection_bounds(bounds);
            self.redraw(page);
        }
    }

    /// Ends an image drag: shows the Undo toast for the whole gesture. `resized`
    /// picks the message/icon (moved vs resized). The page canvas is already
    /// current from the live redraws, so we don't `refresh` here — that would
    /// remount the page and flash the skeleton. (The pages-panel thumbnail then
    /// lags until the next refresh-triggering edit; acceptable for a drag.)
    /// Ends a drag that isn't the selection's (a note, D-081): the Undo toast for the whole gesture.
    pub fn end_drag(mut self, message: &str) {
        let undo = self.drag_undo.peek().clone();
        if let Some(undo) = undo {
            self.drag_undo.set(None);
            self.show(message.to_owned(), ICON_MOVE, Some(undo));
        }
    }

    pub fn end_image_drag(mut self, resized: bool) {
        let Some(undo) = self.drag_undo.peek().clone() else {
            return;
        };
        self.drag_undo.set(None);
        let kind = self
            .image_selection
            .peek()
            .as_ref()
            .map_or(Placed::Picture, |s| s.kind);
        let (message, icon) = match (resized, kind) {
            (true, Placed::Picture) => (format!("{} resized", kind.name()), ICON_IMAGE),
            (true, _) => (format!("{} resized", kind.name()), ICON_MOVE),
            (false, _) => (format!("{} moved", kind.name()), ICON_MOVE),
        };
        self.show(message, icon, Some(undo));
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
            Ok(undo) if *self.merging.peek() => {
                let message = match count {
                    1 => "1 page added at the end".to_owned(),
                    n => format!("{n} pages added at the end"),
                };
                self.show(message, ICON_MERGE, Some(undo));
                // The first added page comes into view.
                let first = page + 1;
                next_frame(move || crate::thumbnails::scroll_to_page(first));
            }
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

    /// A press outside the toast hides it now (P-050 #7/#8), not only after its 6 s.
    pub fn hide_toast(self) {
        let open = self.toast.peek().as_ref().filter(|t| t.open).map(|t| t.id);
        if let Some(id) = open {
            self.hide(id);
        }
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

    /// A step can be undone / redone (the centre bar, D-062; the engine's history, P-050 #2).
    pub fn can_undo(&self) -> bool {
        self.undoable.read().0
    }

    pub fn can_redo(&self) -> bool {
        self.undoable.read().1
    }

    /// Reads the engine's history again (it changes from every tool, so the frame calls this often).
    pub fn sync_history(mut self) {
        let now = (
            pdit_core::history::can_undo(),
            pdit_core::history::can_redo(),
        );
        if *self.undoable.peek() != now {
            self.undoable.set(now);
        }
    }

    /// The centre bar's Undo, ⌘Z, and a toast's Undo: one step back.
    pub fn undo_last(self) {
        self.undo();
    }

    fn undo(self) {
        self.step(true);
    }

    /// The centre bar's Redo, ⇧⌘Z: one step forward again.
    pub fn redo(self) {
        self.step(false);
    }

    fn step(mut self, back: bool) {
        let toast = self.toast.peek().as_ref().map(|t| t.id);
        if let Some(id) = toast {
            self.hide(id);
        }
        self.toast.with_mut(|t| {
            if let Some(t) = t.as_mut() {
                t.undo = None;
            }
        });
        // Undoing a table step brings back the table it grew from.
        let table = match back {
            true => self
                .table_before
                .peek()
                .clone()
                .map(|t| TableSelection { shown: true, ..t }),
            false => None,
        };
        self.table.set(None);
        self.image_selection.set(None);
        consume_context::<Editing>().cancel();
        let result = match back {
            true => pdit_core::history::undo(),
            false => pdit_core::history::redo(),
        };
        match result {
            Ok(Some(sizes)) => {
                refresh(sizes);
                self.table.set(table);
                self.table_before.set(None);
            }
            Ok(None) => {}
            Err(error) => crate::log(&format!("pdit: could not undo / redo: {error}")),
        }
        self.sync_history();
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
    page_top: f64,
    /// A side handle ('l', 'r', 't', 'b'): the shape isn't kept; `None` for a corner.
    side: Option<char>,
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
    // A text box's text while a side handle re-wraps it (read once when the drag starts).
    let mut box_text = use_signal(|| None::<String>);
    // Cropping (D-073): the dragged part ("tl", "t", …, "move") and the pointer's last spot.
    let mut crop_drag = use_signal(|| None::<(&'static str, f64, f64)>);
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
    if let Some([fl, fb, fr, ft]) = tools.image_crop() {
        // The page Crop's frame (D-070) over the picture only; the part to cut away is dimmed.
        let (ix, iy) = (left * scale, (page_height_pt - top) * scale);
        let (iw, ih) = ((right - left) * scale, (top - bottom) * scale);
        let (fx, fy) = ((fl - left) * scale, (top - ft) * scale);
        let (fw, fh) = ((fr - fl) * scale, (ft - fb) * scale);
        let handles = [
            ("tl", 0.0, 0.0),
            ("t", 0.5, 0.0),
            ("tr", 1.0, 0.0),
            ("r", 1.0, 0.5),
            ("br", 1.0, 1.0),
            ("b", 0.5, 1.0),
            ("bl", 0.0, 1.0),
            ("l", 0.0, 0.5),
        ];
        let mut press = move |part: &'static str, event: Event<PointerData>| {
            event.stop_propagation();
            let p = event.client_coordinates();
            crop_drag.set(Some((part, p.x, p.y)));
            if let Some(el) = event
                .data()
                .downcast::<web_sys::PointerEvent>()
                .and_then(|e| e.target())
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                && let Some(pe) = event.data().downcast::<web_sys::PointerEvent>()
            {
                let _ = el.set_pointer_capture(pe.pointer_id());
            }
        };
        return rsx! {
            div {
                class: "sa-root pdit-crop-layer",
                style: "inset: 0;",
                tabindex: "-1",
                onmounted: move |e| async move {
                    let _ = e.set_focus(true).await;
                },
                onkeydown: move |e| match e.key() {
                    Key::Enter => tools.apply_image_crop(),
                    Key::Escape => tools.cancel_image_crop(),
                    _ => {}
                },
                onpointerdown: move |e| e.stop_propagation(),
                onclick: move |e| e.stop_propagation(),
                onpointermove: move |event| {
                    let Some((part, x0, y0)) = crop_drag() else { return };
                    let p = event.client_coordinates();
                    tools.drag_image_crop(
                        part,
                        ((p.x - x0) / f64::from(scale)) as f32,
                        -((p.y - y0) / f64::from(scale)) as f32,
                    );
                    crop_drag.set(Some((part, p.x, p.y)));
                },
                onpointerup: move |_| crop_drag.set(None),
                onpointercancel: move |_| crop_drag.set(None),
                div {
                    class: "pdit-crop-clip",
                    style: "inset: auto; left: {ix}px; top: {iy}px; width: {iw}px; height: {ih}px;",
                    div {
                        class: "pdit-crop-dim",
                        style: "left: {fx}px; top: {fy}px; width: {fw}px; height: {fh}px;",
                    }
                }
                div {
                    class: "pdit-img-ring pdit-crop-frame",
                    style: "left: {ix + fx}px; top: {iy + fy}px; width: {fw}px; height: {fh}px;",
                    onpointerdown: move |e| press("move", e),
                }
                for (part, hx, hy) in handles {
                    span {
                        key: "{part}",
                        class: "pdit-img-handle pdit-crop-handle {part}",
                        style: "left: {ix + fx + hx * fw}px; top: {iy + fy + hy * fh}px;",
                        onpointerdown: move |e| press(part, e),
                    }
                }
                div {
                    class: "sa-anchor",
                    style: "left: {bar_x}px; top: {bar_y}px; transform: translateX(-50%);",
                    div { class: "sa-bar",
                        button {
                            r#type: "button",
                            class: "sa-control",
                            onclick: move |e| {
                                e.stop_propagation();
                                tools.cancel_image_crop();
                            },
                            "Cancel"
                        }
                        button {
                            r#type: "button",
                            class: "sa-primary",
                            onclick: move |e| {
                                e.stop_propagation();
                                tools.apply_image_crop();
                            },
                            span { dangerous_inner_html: ICON_CROP, style: "display: contents" }
                            "Crop"
                        }
                    }
                }
            }
        };
    }
    // (css class, handle x px, handle y px, fixed_left, fixed_top, anchor pt, side).
    // A corner holds the one diagonally opposite; a side holds the opposite side.
    // A text box has no top/bottom handles: its height follows its text.
    let text_box = sel.kind == Placed::TextBox;
    let corners = [
        ("tl", x, y, false, false, (right, bottom), None),
        ("tr", x + width, y, true, false, (left, bottom), None),
        ("bl", x, y + height, false, true, (right, top), None),
        ("br", x + width, y + height, true, true, (left, top), None),
        (
            "t",
            x + width / 2.0,
            y,
            true,
            false,
            (left, bottom),
            Some('t'),
        ),
        (
            "r",
            x + width,
            y + height / 2.0,
            true,
            true,
            (left, top),
            Some('r'),
        ),
        (
            "b",
            x + width / 2.0,
            y + height,
            true,
            true,
            (left, top),
            Some('b'),
        ),
        (
            "l",
            x,
            y + height / 2.0,
            false,
            true,
            (right, top),
            Some('l'),
        ),
    ]
    .into_iter()
    .filter(|c| !(text_box && matches!(c.6, Some('t' | 'b'))));
    rsx! {
        div { class: "sa-root",
            div {
                class: "pdit-img-ring",
                style: "left: {x}px; top: {y}px; width: {width}px; height: {height}px;",
            }
            for (cls, cx, cy, fixed_left, fixed_top, anchor, side) in corners {
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
                        let page_rect = pe
                            .target()
                            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                            .and_then(|el| el.closest(".page").ok().flatten())
                            .map(|p| p.get_bounding_client_rect());
                        let page_left = page_rect.as_ref().map_or(0.0, |r| r.left());
                        let page_top = page_rect.as_ref().map_or(0.0, |r| r.top());
                        if let Some(el) = pe
                            .target()
                            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                        {
                            let _ = el.set_pointer_capture(pe.pointer_id());
                        }
                        tools.begin_image_drag();
                        box_text.set(
                            (text_box && side.is_some())
                                .then(|| pdit_core::text_box_text(page, sel.object_index as i32).ok())
                                .flatten(),
                        );
                        resize.set(Some(ResizeDrag {
                            fixed_left,
                            fixed_top,
                            anchor,
                            page_left,
                            page_top,
                            side,
                        }));
                    },
                    onpointermove: move |event| {
                        let Some(rd) = resize() else { return };
                        let pointer_x_pt =
                            ((event.client_coordinates().x - rd.page_left) / f64::from(scale)) as f32;
                        let width_pt = (pointer_x_pt - rd.anchor.0).abs().max(8.0);
                        let Some(side) = rd.side else {
                            tools.drag_resize(page, width_pt, rd.fixed_left, rd.fixed_top, rd.anchor);
                            return;
                        };
                        let pointer_y_pt = page_height_pt
                            - ((event.client_coordinates().y - rd.page_top) / f64::from(scale)) as f32;
                        let [l, b, r, t] = tools
                            .image_selection()
                            .map_or([0.0, 0.0, 1.0, 1.0], |s| s.bounds);
                        let (w, h) = match side {
                            'l' | 'r' => (width_pt, t - b),
                            _ => (r - l, (pointer_y_pt - rd.anchor.1).abs().max(8.0)),
                        };
                        tools.drag_stretch(
                            page,
                            w,
                            h,
                            box_text.peek().as_deref(),
                            rd.fixed_left,
                            rd.fixed_top,
                            rd.anchor,
                        );
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
                    // Rotate, crop and background are for pictures; the rest is moved, resized or deleted.
                    if sel.kind == Placed::Picture {
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
                            class: "sa-control",
                            "aria-label": "Crop",
                            title: "Crop",
                            onclick: move |event| {
                                event.stop_propagation();
                                tools.start_image_crop();
                            },
                            span { dangerous_inner_html: ICON_CROP, style: "display: contents" }
                        }
                        button {
                            r#type: "button",
                            class: "sa-control",
                            "aria-label": "Remove background",
                            title: "Remove background (keep only the ink)",
                            onclick: move |event| {
                                event.stop_propagation();
                                tools.remove_background_selection();
                            },
                            span { dangerous_inner_html: ICON_REMOVE_BACKGROUND, style: "display: contents" }
                        }
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

/// Moves the selection by (`dx`, `dy`) PDF points; returns its new bounds.
fn move_selected(sel: &ImageSelection, dx: f32, dy: f32) -> Result<[f32; 4], pdit_core::Error> {
    match sel.kind {
        Placed::Stamp | Placed::Drawing => {
            pdit_core::annotations::move_drawn(sel.page, sel.object_index, dx, dy)
        }
        Placed::TextBox => page_ops::move_text_box(sel.page, sel.object_index, dx, dy),
        Placed::Picture | Placed::Shape => page_ops::move_image(sel.page, sel.object_index, dx, dy),
    }
}

/// What can be moved and resized under (x, y) on `page`, the top one first: a stamp or pen drawing (they lie over
/// the page's content), then text or a shape added with pdit, then a picture.
fn placed_at(page: u16, x: f32, y: f32) -> Option<ImageSelection> {
    if let Ok(Some((index, kind, bounds))) = pdit_core::annotations::drawn_at(page, x, y) {
        return Some(ImageSelection {
            page,
            object_index: index,
            bounds,
            kind: match kind {
                pdit_core::annotations::AnnotationKind::Stamp => Placed::Stamp,
                _ => Placed::Drawing,
            },
        });
    }
    let (hit, kind) = if let Ok(Some(hit)) = page_ops::text_box_at(page, x, y) {
        (hit, Placed::TextBox)
    } else if let Ok(Some(hit)) = page_ops::shape_at(page, x, y) {
        (hit, Placed::Shape)
    } else {
        (page_ops::image_at(page, x, y).ok()??, Placed::Picture)
    };
    Some(ImageSelection {
        page,
        object_index: hit.object_index,
        bounds: hit.bounds,
        kind,
    })
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
