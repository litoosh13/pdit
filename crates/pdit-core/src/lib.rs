//! pdit's PDF engine layer, built on PDFium through `pdfium-render`.
//!
//! In the browser, call [`engine::start`] once before using anything else.

mod edit;
mod error;
mod inspect;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod lines;
pub mod subset;

#[cfg(target_arch = "wasm32")]
pub mod analysis;
#[cfg(target_arch = "wasm32")]
pub mod annotations;
#[cfg(target_arch = "wasm32")]
pub mod bookmarks;
#[cfg(target_arch = "wasm32")]
pub mod doc_content;
#[cfg(target_arch = "wasm32")]
mod edit_open;
#[cfg(target_arch = "wasm32")]
pub mod engine;
pub mod fields;
#[cfg(target_arch = "wasm32")]
pub mod form_edit;
#[cfg(target_arch = "wasm32")]
mod forms;
#[cfg(target_arch = "wasm32")]
pub mod links;
#[cfg(target_arch = "wasm32")]
pub mod page_ops;
#[cfg(target_arch = "wasm32")]
mod render;
pub mod search;

pub use edit::{EditMethod, EditReport, EditResult, replace_text_verified};
#[cfg(target_arch = "wasm32")]
pub use edit_open::{
    AddPreview, EditPreview, FontChoice, FontTraits, Fonts, LayerWord, LookAlike, Paragraph,
    TextLine, TextStyle, VisualLine, add_text_layer, discard_edit, font_traits, font_traits_near,
    keep_edit, paragraph_at, preview_add, preview_edit, preview_line, preview_reflow,
    preview_styled, save_document, style_near, text_lines, text_style, visual_text_lines,
};
pub use error::Error;
#[cfg(target_arch = "wasm32")]
pub use forms::{FieldKind, FormField, fill_text, form_fields, select_option, toggle_choice};
pub use inspect::{PageInfo, TextObjectInfo, inspect};
#[cfg(target_arch = "wasm32")]
pub use page_ops::{
    ImageHit, ShapeKind, TableLayout, add_shape, add_table, delete_image, image_at, move_image,
    replace_table, resize_image, rotate_image,
};
#[cfg(target_arch = "wasm32")]
pub use render::{close_document, open_document, render_page, render_page_for_print};

use pdfium_render::prelude::Pdfium;
use std::sync::OnceLock;

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

/// Workaround for pdfium-render 0.9.4 on WASM (wasm_bindings.rs,
/// copy_ptr_with_len_to_pdfium): when copying our data into PDFium it creates a
/// view of our memory, then allocates a Vec of the same size. If that allocation
/// grows WASM memory, the view detaches and the copy throws. Call this before
/// handing `len` bytes to PDFium (fonts, whole documents): reserving and freeing
/// the headroom first avoids the growth; black_box stops the optimiser from
/// removing the unused allocation.
pub(crate) fn reserve_for_pdfium_copy(len: usize) {
    drop(std::hint::black_box(Vec::<u8>::with_capacity(len * 2)));
}

/// The shared PDFium instance. In the browser, [`engine::start`] must have completed first.
pub(crate) fn pdfium() -> &'static Pdfium {
    PDFIUM.get_or_init(Pdfium::default)
}
