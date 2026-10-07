//! Real form fields (AcroForm) of the open document: listing them, and filling them the way
//! a person typing does. Going through PDFium's form-fill layer (focus the field, replace its
//! text, leave it) makes PDFium write a fresh appearance, so the value shows in every viewer.
//! Setting the value directly (pdfium-render's `set_value`) stores it but leaves the old,
//! empty appearance (RESEARCH §14b). The handles this needs come from our patched copy of
//! pdfium-render (third_party/pdfium-render/PDIT_PATCH.md).

use crate::render::with_open;
use crate::{Error, pdfium};
use pdfium_render::prelude::*;

/// What kind of field a widget belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Checkbox,
    Radio,
    ComboBox,
    ListBox,
    Signature,
    PushButton,
    Unknown,
}

/// One field widget on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct FormField {
    /// Position among the page's annotations; pass it back to the fill functions.
    pub annotation: usize,
    pub kind: FieldKind,
    pub name: Option<String>,
    /// Left, bottom, right, top in PDF points.
    pub rect: (f32, f32, f32, f32),
    /// Text or chosen option; `None` when empty or not applicable.
    pub value: Option<String>,
    /// For checkboxes and radio buttons.
    pub checked: bool,
    /// Choices of a combo or list box, in order.
    pub options: Vec<String>,
    pub multiline: bool,
    pub read_only: bool,
    /// The field must be filled (D-047).
    pub required: bool,
    /// A checkbox's or radio button's value when on (a radio's choice).
    pub choice: Option<String>,
}

/// The form fields on page `page` of the open document (empty when it has none).
pub fn form_fields(page: u16) -> Result<Vec<FormField>, Error> {
    with_open(|document| {
        let form = document.form().map(|f| f.handle());
        let pdf_page = document.pages().get(page.into())?;
        let page_handle = pdf_page.page_handle();
        // PDFium knows a page's form controls (flags, export values) only
        // while the page is loaded into the form layer.
        if let Some(form) = form {
            // SAFETY: both handles belong to the open document and this page.
            unsafe { pdfium().bindings().FORM_OnAfterLoadPage(page_handle, form) };
        }
        let mut fields = Vec::new();
        for (annotation, item) in pdf_page.annotations().iter().enumerate() {
            let Some(field) = item.as_form_field() else {
                continue;
            };
            let bounds = item.bounds()?;
            let rect = (
                bounds.left().value,
                bounds.bottom().value,
                bounds.right().value,
                bounds.top().value,
            );
            let (kind, value, checked, options, multiline) = match field {
                PdfFormField::Text(text) => (
                    FieldKind::Text,
                    text.value(),
                    false,
                    vec![],
                    text.is_multiline(),
                ),
                PdfFormField::Checkbox(check) => (
                    FieldKind::Checkbox,
                    None,
                    check.is_checked().unwrap_or(false),
                    vec![],
                    false,
                ),
                PdfFormField::RadioButton(radio) => (
                    FieldKind::Radio,
                    None,
                    radio.is_checked().unwrap_or(false),
                    vec![],
                    false,
                ),
                PdfFormField::ComboBox(combo) => (
                    FieldKind::ComboBox,
                    combo.value(),
                    false,
                    labels(combo.options()),
                    false,
                ),
                PdfFormField::ListBox(list) => (
                    FieldKind::ListBox,
                    list.value(),
                    false,
                    labels(list.options()),
                    false,
                ),
                PdfFormField::Signature(_) => (FieldKind::Signature, None, false, vec![], false),
                PdfFormField::PushButton(_) => (FieldKind::PushButton, None, false, vec![], false),
                PdfFormField::Unknown(_) => (FieldKind::Unknown, None, false, vec![], false),
            };
            let (required, choice) = match form {
                Some(form) => flags_and_choice(form, page_handle, annotation, kind),
                None => (false, None),
            };
            fields.push(FormField {
                annotation,
                required,
                choice,
                kind,
                name: field.name(),
                rect,
                value,
                checked,
                options,
                multiline,
                read_only: field.is_read_only(),
            });
        }
        if let Some(form) = form {
            // SAFETY: as above; pairs FORM_OnAfterLoadPage.
            unsafe {
                pdfium()
                    .bindings()
                    .FORM_OnBeforeClosePage(page_handle, form)
            };
        }
        Ok(fields)
    })
}

/// The required flag and, for checkboxes and radio buttons, the on-value,
/// read through PDFium's raw calls (pdfium-render has no wrapper).
fn flags_and_choice(
    form: FPDF_FORMHANDLE,
    page: FPDF_PAGE,
    annotation: usize,
    kind: FieldKind,
) -> (bool, Option<String>) {
    let bindings = pdfium().bindings();
    let Ok(index) = i32::try_from(annotation) else {
        return (false, None);
    };
    // SAFETY: the handles belong to the open document and the loaded page; the
    // annotation is closed before we return. The wasm bridge copies into the
    // buffer it gets, so it is never null (as in doc_content.rs).
    unsafe {
        let annot = bindings.FPDFPage_GetAnnot(page, index);
        if annot.is_null() {
            return (false, None);
        }
        let required = bindings.FPDFAnnot_GetFormFieldFlags(form, annot) & 2 != 0;
        let choice = matches!(kind, FieldKind::Checkbox | FieldKind::Radio)
            .then(|| {
                let mut buf = vec![0u16; 256];
                let n = bindings.FPDFAnnot_GetFormFieldExportValue(
                    form,
                    annot,
                    buf.as_mut_ptr(),
                    (buf.len() * 2) as _,
                ) as usize;
                (n > 2 && n <= buf.len() * 2).then(|| {
                    buf.truncate(n / 2);
                    String::from_utf16_lossy(&buf)
                        .trim_end_matches('\0')
                        .to_owned()
                })
            })
            .flatten();
        bindings.FPDFPage_CloseAnnot(annot);
        (required, choice)
    }
}

fn labels(options: &PdfFormFieldOptions<'_>) -> Vec<String> {
    options
        .iter()
        .map(|option| option.label().cloned().unwrap_or_default())
        .collect()
}

/// Replaces the text of text or combo-box field `annotation` on `page` with `text`,
/// as if the user had selected all of it and typed `text`.
pub fn fill_text(page: u16, annotation: usize, text: &str) -> Result<(), Error> {
    let mut wide: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    wide.extend([0, 0]); // PDFium reads up to a 16-bit NUL
    with_field(page, annotation, |form, pdf_page| {
        // SAFETY: `wide` stays alive and NUL-terminated for the whole call.
        let bindings = pdfium().bindings();
        unsafe {
            bindings.FORM_SelectAllText(form, pdf_page);
            bindings.FORM_ReplaceSelection(form, pdf_page, wide.as_ptr() as FPDF_WIDESTRING);
        }
        Ok(())
    })
}

/// Toggles checkbox `annotation`, or selects radio button `annotation`, on `page`,
/// as if the user had focused it and pressed the space bar.
pub fn toggle_choice(page: u16, annotation: usize) -> Result<(), Error> {
    with_field(page, annotation, |form, pdf_page| {
        // SAFETY: handles are valid for the duration of `with_field`.
        let pressed = unsafe { pdfium().bindings().FORM_OnChar(form, pdf_page, 0x20, 0) };
        if pressed == 0 {
            return Err(Error::Pdfium("the field did not accept the key".into()));
        }
        Ok(())
    })
}

/// Chooses option `option` (0-based) of combo or list box `annotation` on `page`.
pub fn select_option(page: u16, annotation: usize, option: usize) -> Result<(), Error> {
    let index = i32::try_from(option).map_err(|_| Error::Pdfium("option out of range".into()))?;
    with_field(page, annotation, |form, pdf_page| {
        // SAFETY: handles are valid for the duration of `with_field`.
        let chosen = unsafe {
            pdfium()
                .bindings()
                .FORM_SetIndexSelected(form, pdf_page, index, 1)
        };
        if chosen == 0 {
            return Err(Error::Pdfium("the field did not accept that option".into()));
        }
        Ok(())
    })
}

/// Loads `page` into the form-fill layer, focuses field `annotation`, runs `f`, then takes
/// the focus away (which commits the value and rebuilds its appearance) and unloads the page.
fn with_field(
    page: u16,
    annotation: usize,
    f: impl FnOnce(FPDF_FORMHANDLE, FPDF_PAGE) -> Result<(), Error>,
) -> Result<(), Error> {
    let index =
        i32::try_from(annotation).map_err(|_| Error::Pdfium("annotation out of range".into()))?;
    with_open(|document| {
        let form = document
            .form()
            .ok_or_else(|| Error::Pdfium("this document has no form fields".into()))?
            .handle();
        let pdf_page = document.pages().get(page.into())?;
        let page_handle = pdf_page.page_handle();
        let bindings = pdfium().bindings();
        // SAFETY: the form and page handles belong to the open document and the loaded page,
        // both alive until the end of this closure; the annotation is closed before we return.
        unsafe {
            bindings.FORM_OnAfterLoadPage(page_handle, form);
            let widget = bindings.FPDFPage_GetAnnot(page_handle, index);
            let result = if widget.is_null() {
                Err(Error::Pdfium("no such field".into()))
            } else if bindings.FORM_SetFocusedAnnot(form, widget) == 0 {
                Err(Error::Pdfium("the field cannot be focused".into()))
            } else {
                let result = f(form, page_handle);
                bindings.FORM_ForceToKillFocus(form);
                result
            };
            if !widget.is_null() {
                bindings.FPDFPage_CloseAnnot(widget);
            }
            bindings.FORM_OnBeforeClosePage(page_handle, form);
            result
        }
    })
}
