//! Creating and editing form fields (D-047). PDFium fills fields (forms.rs)
//! but can't create widgets, so every change here is written with lopdf as an
//! incremental update and the document opened again (D-045, RESEARCH §16).
//! Fields get their own appearance (white box, grey border; a check mark or a
//! dot when on), so every viewer shows them. A field is addressed like in
//! forms.rs: its page and its position among the page's annotations, plus its
//! rectangle, which must still match (a stale selection is refused).

use crate::Error;
use crate::render::{open_document, with_open};
use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream, StringFormat};

/// A field to add.
#[derive(Clone, Debug, PartialEq)]
pub enum NewField {
    Text,
    Checkbox,
    /// A radio button; joins the group `name` if it exists, with this choice.
    Radio {
        choice: String,
    },
    Dropdown {
        options: Vec<String>,
    },
}

/// What the Settings box changes.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldSettings {
    /// The field's name (a radio button's group name).
    pub name: String,
    pub required: bool,
    /// A dropdown's options; `None` leaves them.
    pub options: Option<Vec<String>>,
    /// A radio button's choice; `None` leaves it.
    pub choice: Option<String>,
}

/// Field flag bit 2: the field must be filled.
const REQUIRED: i64 = 2;
const RADIO_FLAGS: i64 = 49152; // radio + no toggle to off
const COMBO_FLAGS: i64 = 131072;

/// Adds a field on `page` at `rect` ([left, bottom, right, top] in points).
/// Names must be new, except a radio button joining its group.
pub fn add_field(page: u16, kind: &NewField, rect: [f32; 4], name: &str) -> Result<(), Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Pdfium("a field needs a name".into()));
    }
    edit(|inc, pages| {
        let page_id = *pages.get(usize::from(page)).ok_or("no such page")?;
        let form = acro_form(inc)?;
        let existing = top_field(inc, form, name)?;
        let mut widget = new_widget(page_id, rect);
        let field_id = match kind {
            NewField::Radio { choice } => {
                let group = match existing {
                    Some(id) if is_radio_group(inc, id) => id,
                    Some(_) => return Err(taken()),
                    None => {
                        let mut g = Dictionary::new();
                        g.set("FT", "Btn");
                        g.set("Ff", RADIO_FLAGS);
                        g.set("T", text(name));
                        g.set("Kids", Vec::<Object>::new());
                        let id = inc.new_document.add_object(g);
                        fields_mut(inc, form)?.push(Object::Reference(id));
                        id
                    }
                };
                widget.set("Parent", group);
                widget.set("AS", "Off");
                set_appearance(inc, &mut widget, Look::Radio(choice), rect);
                let id = inc.new_document.add_object(widget);
                inc.opt_clone_object_to_new_document(group)
                    .map_err(|e| e.to_string())?;
                dict_mut(inc, group)?
                    .get_mut(b"Kids")
                    .and_then(Object::as_array_mut)
                    .map_err(|e| e.to_string())?
                    .push(Object::Reference(id));
                id
            }
            _ if existing.is_some() => return Err(taken()),
            other => {
                widget.set("T", text(name));
                match other {
                    NewField::Text => {
                        widget.set("FT", "Tx");
                        widget.set("DA", text("/Helv 11 Tf 0 g"));
                        set_appearance(inc, &mut widget, Look::Box, rect);
                    }
                    NewField::Checkbox => {
                        widget.set("FT", "Btn");
                        widget.set("V", "Off");
                        widget.set("AS", "Off");
                        widget.set("DA", text("/ZaDb 0 Tf 0 g"));
                        set_appearance(inc, &mut widget, Look::Check("Yes"), rect);
                    }
                    NewField::Dropdown { options } => {
                        widget.set("FT", "Ch");
                        widget.set("Ff", COMBO_FLAGS);
                        widget.set("Opt", options.iter().map(|o| text(o)).collect::<Vec<_>>());
                        widget.set("DA", text("/Helv 11 Tf 0 g"));
                        set_appearance(inc, &mut widget, Look::Box, rect);
                    }
                    NewField::Radio { .. } => unreachable!(),
                }
                let id = inc.new_document.add_object(widget);
                fields_mut(inc, form)?.push(Object::Reference(id));
                id
            }
        };
        annots_mut(inc, page_id)?.push(Object::Reference(field_id));
        Ok(())
    })
}

/// Moves or resizes the field at `index` on `page` (now at `old`) to `rect`.
/// A move keeps the field's look as it is; a resize redraws it, and a value it
/// holds is filled in again (cleared first, so PDFium draws it anew).
pub fn set_field_rect(page: u16, index: usize, old: [f32; 4], rect: [f32; 4]) -> Result<(), Error> {
    let size = |r: [f32; 4]| ((r[2] - r[0]).abs(), (r[3] - r[1]).abs());
    let resized = {
        let ((w0, h0), (w1, h1)) = (size(old), size(rect));
        (w0 - w1).abs() > 0.5 || (h0 - h1).abs() > 0.5
    };
    let before = value_of(page, index)?;
    edit(|inc, pages| {
        let widget = widget_at(inc, pages, page, index, old)?;
        let look = look_of(inc, widget)?;
        let mut w = dict_mut(inc, widget)?.clone();
        w.set("Rect", rect.map(Object::Real).to_vec());
        if resized {
            set_appearance(inc, &mut w, look.as_look(), rect);
        }
        inc.new_document.set_object(widget, w);
        Ok(())
    })?;
    if !resized {
        return Ok(());
    }
    match before {
        Some((crate::FieldKind::Text, v)) => {
            crate::fill_text(page, index, "")?;
            crate::fill_text(page, index, &v)
        }
        Some((crate::FieldKind::ComboBox, v)) => {
            let options = crate::form_fields(page)?
                .into_iter()
                .find(|f| f.annotation == index)
                .map(|f| f.options)
                .unwrap_or_default();
            match options.iter().position(|o| *o == v) {
                // ponytail: a one-option dropdown keeps a plain box after a
                // resize until it's picked again.
                Some(i) if options.len() > 1 => {
                    crate::select_option(page, index, (i + 1) % options.len())?;
                    crate::select_option(page, index, i)
                }
                _ => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

/// Changes the name, required flag, options or radio choice of the field at
/// `index` on `page` (at `rect`).
pub fn set_field_settings(
    page: u16,
    index: usize,
    rect: [f32; 4],
    settings: &FieldSettings,
) -> Result<(), Error> {
    let name = settings.name.trim();
    if name.is_empty() {
        return Err(Error::Pdfium("a field needs a name".into()));
    }
    edit(|inc, pages| {
        let widget = widget_at(inc, pages, page, index, rect)?;
        let field = field_of(inc, widget);
        let form = acro_form(inc)?;
        if top_field(inc, form, name)?.is_some_and(|id| id != field) {
            return Err(taken());
        }
        inc.opt_clone_object_to_new_document(field)
            .map_err(|e| e.to_string())?;
        let f = dict_mut(inc, field)?;
        f.set("T", text(name));
        let flags = f.get(b"Ff").and_then(Object::as_i64).unwrap_or(0);
        f.set(
            "Ff",
            if settings.required {
                flags | REQUIRED
            } else {
                flags & !REQUIRED
            },
        );
        if let Some(options) = &settings.options {
            f.set("Opt", options.iter().map(|o| text(o)).collect::<Vec<_>>());
            let value = f
                .get(b"V")
                .ok()
                .and_then(|v| lopdf::decode_text_string(v).ok());
            // /I holds the chosen option's position, which PDFium reads too.
            match value.and_then(|v| options.iter().position(|o| *o == v)) {
                Some(i) => f.set("I", vec![Object::Integer(i as i64)]),
                None => {
                    // The chosen value is no longer offered: clear it, and its look.
                    f.remove(b"V");
                    f.remove(b"I");
                    let mut w = dict_mut(inc, widget)?.clone();
                    set_appearance(inc, &mut w, Look::Box, rect);
                    inc.new_document.set_object(widget, w);
                }
            }
        }
        if let Some(choice) = settings
            .choice
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            let LookOf::Radio(old) = look_of(inc, widget)? else {
                return Err("only a radio button has a choice".into());
            };
            if old != choice {
                let siblings = kids(inc, field);
                let taken = siblings
                    .iter()
                    .filter(|&&k| k != widget)
                    .any(|&k| matches!(look_of(inc, k), Ok(LookOf::Radio(c)) if c == choice));
                if taken {
                    return Err("another button in this group has that choice".into());
                }
                let mut w = dict_mut(inc, widget)?.clone();
                let on = w
                    .get(b"AS")
                    .and_then(Object::as_name)
                    .is_ok_and(|n| n == old.as_bytes());
                set_appearance(inc, &mut w, Look::Radio(choice), rect);
                w.set("AS", if on { choice } else { "Off" });
                inc.new_document.set_object(widget, w);
                let f = dict_mut(inc, field)?;
                if f.get(b"V")
                    .and_then(Object::as_name)
                    .is_ok_and(|n| n == old.as_bytes())
                {
                    f.set("V", Object::Name(choice.as_bytes().to_vec()));
                }
            }
        }
        Ok(())
    })
}

/// Deletes the field at `index` on `page` (at `rect`). A radio button leaves
/// its group; the group goes with its last button.
pub fn delete_field(page: u16, index: usize, rect: [f32; 4]) -> Result<(), Error> {
    edit(|inc, pages| {
        let page_id = pages[usize::from(page)];
        let widget = widget_at(inc, pages, page, index, rect)?;
        let field = field_of(inc, widget);
        annots_mut(inc, page_id)?.retain(|o| o.as_reference().ok() != Some(widget));
        let form = acro_form(inc)?;
        let gone = if field != widget {
            inc.opt_clone_object_to_new_document(field)
                .map_err(|e| e.to_string())?;
            let kids = dict_mut(inc, field)?
                .get_mut(b"Kids")
                .and_then(Object::as_array_mut)
                .map_err(|e| e.to_string())?;
            kids.retain(|o| o.as_reference().ok() != Some(widget));
            kids.is_empty().then_some(field)
        } else {
            Some(widget)
        };
        if let Some(id) = gone {
            fields_mut(inc, form)?.retain(|o| o.as_reference().ok() != Some(id));
        }
        Ok(())
    })
}

/// The field's kind and value, when it holds text or a chosen option.
fn value_of(page: u16, index: usize) -> Result<Option<(crate::FieldKind, String)>, Error> {
    Ok(crate::form_fields(page)?
        .into_iter()
        .find(|f| f.annotation == index)
        .and_then(|f| f.value.filter(|v| !v.is_empty()).map(|v| (f.kind, v))))
}

/// Saves, lets `change` append its objects, reopens. Undo is the caller's
/// snapshot, as for every other change.
fn edit(
    change: impl FnOnce(&mut IncrementalDocument, &[ObjectId]) -> Result<(), String>,
) -> Result<(), Error> {
    let bytes = crate::page_ops::snapshot()?;
    let fail = |e: String| Error::Pdfium(format!("could not change the form: {e}"));
    let prev = Document::load_mem(&bytes).map_err(|e| fail(e.to_string()))?;
    let pages: Vec<ObjectId> = prev.get_pages().into_values().collect();
    let mut inc = IncrementalDocument::create_from(bytes, prev);
    change(&mut inc, &pages).map_err(fail)?;
    let mut out = Vec::new();
    inc.save_to(&mut out).map_err(|e| fail(e.to_string()))?;
    open_document(out).map(|_| ())
}

fn taken() -> String {
    "another field already has this name".into()
}

fn text(s: &str) -> Object {
    if s.is_ascii() {
        Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut b = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            b.extend(u.to_be_bytes());
        }
        Object::String(b, StringFormat::Hexadecimal)
    }
}

/// A dictionary of the new revision, cloned from the old one first.
fn dict_mut(inc: &mut IncrementalDocument, id: ObjectId) -> Result<&mut Dictionary, String> {
    inc.opt_clone_object_to_new_document(id)
        .map_err(|e| e.to_string())?;
    inc.new_document
        .get_object_mut(id)
        .and_then(Object::as_dict_mut)
        .map_err(|e| e.to_string())
}

/// The catalog's AcroForm (made if missing, moved out if inline), with
/// Helvetica and ZapfDingbats in its default resources.
fn acro_form(inc: &mut IncrementalDocument) -> Result<ObjectId, String> {
    let root = inc
        .new_document
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| e.to_string())?;
    let catalog = dict_mut(inc, root)?;
    let form_id = match catalog.get(b"AcroForm").cloned() {
        Ok(Object::Reference(id)) => id,
        found => {
            let form = match found {
                Ok(Object::Dictionary(d)) => d,
                _ => {
                    let mut d = Dictionary::new();
                    d.set("Fields", Vec::<Object>::new());
                    d.set("DA", text("/Helv 0 Tf 0 g"));
                    d
                }
            };
            let id = inc.new_document.add_object(form);
            dict_mut(inc, root)?.set("AcroForm", id);
            id
        }
    };
    // Default resources: our appearances and DA strings name /Helv and /ZaDb.
    let dr = dict_mut(inc, form_id)?.get(b"DR").cloned().ok();
    let mut dr = match dr {
        Some(Object::Reference(id)) => {
            let d = dict_mut(inc, id)?.clone();
            inc.new_document.set_object(id, d.clone());
            (Some(id), d)
        }
        Some(Object::Dictionary(d)) => (None, d),
        _ => (None, Dictionary::new()),
    };
    let fonts = match dr.1.get(b"Font").cloned() {
        Ok(Object::Reference(id)) => dict_mut(inc, id)?.clone(),
        Ok(Object::Dictionary(d)) => d,
        _ => Dictionary::new(),
    };
    let mut fonts = fonts;
    for (key, base) in [("Helv", "Helvetica"), ("ZaDb", "ZapfDingbats")] {
        if !fonts.has(key.as_bytes()) {
            let mut f = Dictionary::new();
            f.set("Type", "Font");
            f.set("Subtype", "Type1");
            f.set("BaseFont", base);
            if key == "Helv" {
                f.set("Encoding", "WinAnsiEncoding");
            }
            fonts.set(key, inc.new_document.add_object(f));
        }
    }
    dr.1.set("Font", fonts);
    match dr.0 {
        Some(id) => inc.new_document.set_object(id, dr.1),
        None => {
            dict_mut(inc, form_id)?.set("DR", dr.1);
        }
    }
    let form = dict_mut(inc, form_id)?;
    if !form.has(b"Fields") {
        form.set("Fields", Vec::<Object>::new());
    }
    Ok(form_id)
}

/// The AcroForm's /Fields array (moved inline if it was its own object).
fn fields_mut(inc: &mut IncrementalDocument, form: ObjectId) -> Result<&mut Vec<Object>, String> {
    if let Ok(Object::Reference(id)) = dict_mut(inc, form)?.get(b"Fields").cloned() {
        let list = inc
            .new_document
            .get_object(id)
            .or_else(|_| inc.get_prev_documents().get_object(id))
            .and_then(Object::as_array)
            .map_err(|e| e.to_string())?
            .clone();
        dict_mut(inc, form)?.set("Fields", list);
    }
    dict_mut(inc, form)?
        .get_mut(b"Fields")
        .and_then(Object::as_array_mut)
        .map_err(|e| e.to_string())
}

/// The page's /Annots array (moved inline if it was its own object).
fn annots_mut(inc: &mut IncrementalDocument, page: ObjectId) -> Result<&mut Vec<Object>, String> {
    let page_dict = dict_mut(inc, page)?;
    match page_dict.get(b"Annots").cloned() {
        Ok(Object::Reference(id)) => {
            let list = inc
                .get_prev_documents()
                .get_object(id)
                .and_then(Object::as_array)
                .map_err(|e| e.to_string())?
                .clone();
            dict_mut(inc, page)?.set("Annots", list);
        }
        Ok(_) => {}
        Err(_) => {
            page_dict.set("Annots", Vec::<Object>::new());
        }
    }
    dict_mut(inc, page)?
        .get_mut(b"Annots")
        .and_then(Object::as_array_mut)
        .map_err(|e| e.to_string())
}

/// The top-level field called `name`, if any.
fn top_field(
    inc: &mut IncrementalDocument,
    form: ObjectId,
    name: &str,
) -> Result<Option<ObjectId>, String> {
    let ids: Vec<ObjectId> = fields_mut(inc, form)?
        .iter()
        .filter_map(|o| o.as_reference().ok())
        .collect();
    Ok(ids.into_iter().find(|&id| {
        dict(inc, id)
            .and_then(|d| d.get(b"T").ok().cloned())
            .and_then(|t| lopdf::decode_text_string(&t).ok())
            .is_some_and(|t| t == name)
    }))
}

/// The widget at `index` in the page's /Annots, if it still sits at `rect`.
fn widget_at(
    inc: &mut IncrementalDocument,
    pages: &[ObjectId],
    page: u16,
    index: usize,
    rect: [f32; 4],
) -> Result<ObjectId, String> {
    let page_id = *pages.get(usize::from(page)).ok_or("no such page")?;
    let stale = || "that field has changed; select it again".to_string();
    let id = annots_mut(inc, page_id)?
        .get(index)
        .and_then(|o| o.as_reference().ok())
        .ok_or_else(stale)?;
    let w = dict_mut(inc, id)?;
    let is_widget = w
        .get(b"Subtype")
        .and_then(Object::as_name)
        .is_ok_and(|n| n == b"Widget");
    let at: Vec<f32> = w
        .get(b"Rect")
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_float().ok()).collect())
        .unwrap_or_default();
    let same = at.len() == 4 && {
        let (l, r) = (at[0].min(at[2]), at[0].max(at[2]));
        let (b, t) = (at[1].min(at[3]), at[1].max(at[3]));
        [l, b, r, t]
            .iter()
            .zip(rect)
            .all(|(a, e)| (a - e).abs() < 1.0)
    };
    if is_widget && same {
        Ok(id)
    } else {
        Err(stale())
    }
}

/// An object of this edit or, if untouched, of the file before it.
fn get(inc: &IncrementalDocument, id: ObjectId) -> Option<&Object> {
    inc.new_document
        .get_object(id)
        .or_else(|_| inc.get_prev_documents().get_object(id))
        .ok()
}

fn dict(inc: &IncrementalDocument, id: ObjectId) -> Option<&Dictionary> {
    get(inc, id).and_then(|o| o.as_dict().ok())
}

/// The field a widget belongs to: its parent when it has no name of its own.
fn field_of(inc: &IncrementalDocument, widget: ObjectId) -> ObjectId {
    let w = dict(inc, widget);
    match w.and_then(|w| w.get(b"Parent").and_then(Object::as_reference).ok()) {
        Some(parent) if !w.is_some_and(|w| w.has(b"T")) => parent,
        _ => widget,
    }
}

fn kids(inc: &IncrementalDocument, field: ObjectId) -> Vec<ObjectId> {
    dict(inc, field)
        .and_then(|d| d.get(b"Kids").ok())
        .and_then(|k| k.as_array().ok())
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default()
}

fn is_radio_group(inc: &IncrementalDocument, id: ObjectId) -> bool {
    dict(inc, id).is_some_and(|d| {
        d.get(b"Ff")
            .and_then(Object::as_i64)
            .is_ok_and(|f| f & 32768 != 0)
    })
}

fn new_widget(page: ObjectId, rect: [f32; 4]) -> Dictionary {
    let mut w = Dictionary::new();
    w.set("Type", "Annot");
    w.set("Subtype", "Widget");
    w.set("Rect", rect.map(Object::Real).to_vec());
    w.set("F", 4); // print
    w.set("P", page);
    let grey = vec![Object::Real(0.6); 3];
    let mut mk = Dictionary::new();
    mk.set("BC", grey);
    mk.set("BG", vec![Object::Real(1.0); 3]);
    w.set("MK", mk);
    w
}

/// How a widget is drawn.
enum Look<'a> {
    Box,
    Check(&'a str),
    Radio(&'a str),
}

/// A widget's current look, read back (owned) for redrawing.
enum LookOf {
    Box,
    Check(String),
    Radio(String),
}

impl LookOf {
    fn as_look(&self) -> Look<'_> {
        match self {
            LookOf::Box => Look::Box,
            LookOf::Check(on) => Look::Check(on),
            LookOf::Radio(on) => Look::Radio(on),
        }
    }
}

fn look_of(inc: &IncrementalDocument, widget: ObjectId) -> Result<LookOf, String> {
    let field = field_of(inc, widget);
    let ft = dict(inc, field)
        .and_then(|d| d.get(b"FT").ok())
        .and_then(|n| n.as_name().ok())
        .ok_or("not a form field")?;
    if ft != b"Btn" {
        return Ok(LookOf::Box);
    }
    // The on-state is the appearance name that isn't Off.
    let normal = dict(inc, widget)
        .and_then(|w| w.get(b"AP").ok())
        .and_then(|ap| ap.as_dict().ok())
        .and_then(|ap| ap.get(b"N").ok())
        .and_then(|n| match n {
            Object::Reference(id) => get(inc, *id),
            other => Some(other),
        })
        .and_then(|n| n.as_dict().ok());
    let on = normal
        .and_then(|n| n.iter().map(|(k, _)| k).find(|k| k.as_slice() != b"Off"))
        .map(|k| String::from_utf8_lossy(k).into_owned())
        .unwrap_or_else(|| "Yes".into());
    Ok(if is_radio_group(inc, field) {
        LookOf::Radio(on)
    } else {
        LookOf::Check(on)
    })
}

/// Writes the widget's /AP for `look` at the size of `rect`.
fn set_appearance(
    inc: &mut IncrementalDocument,
    widget: &mut Dictionary,
    look: Look,
    rect: [f32; 4],
) {
    let (w, h) = ((rect[2] - rect[0]).abs(), (rect[3] - rect[1]).abs());
    let doc = &mut inc.new_document;
    let mut stream = |ops: String, zapf: bool| {
        let mut d = Dictionary::new();
        d.set("Type", "XObject");
        d.set("Subtype", "Form");
        d.set(
            "BBox",
            vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)],
        );
        if zapf {
            let mut f = Dictionary::new();
            f.set("Type", "Font");
            f.set("Subtype", "Type1");
            f.set("BaseFont", "ZapfDingbats");
            let mut fonts = Dictionary::new();
            fonts.set("ZaDb", doc.add_object(f));
            let mut res = Dictionary::new();
            res.set("Font", fonts);
            d.set("Resources", res);
        }
        Object::Reference(doc.add_object(Stream::new(d, ops.into_bytes())))
    };
    let frame = format!(
        "1 g 0 0 {w} {h} re f 0.6 G 1 w 0.5 0.5 {} {} re S",
        w - 1.0,
        h - 1.0
    );
    let n = match look {
        Look::Box => stream(frame, false),
        Look::Check(on) => {
            let size = (h * 0.75).max(4.0);
            let check = format!(
                "{frame} BT /ZaDb {size} Tf 0 g {} {} Td (4) Tj ET",
                (w - size * 0.8) / 2.0,
                (h - size * 0.7) / 2.0
            );
            let mut states = Dictionary::new();
            states.set(on, stream(check, true));
            states.set("Off", stream(frame, false));
            Object::Dictionary(states)
        }
        Look::Radio(on) => {
            let ring = |r: f32| {
                let (cx, cy, k) = (w / 2.0, h / 2.0, 0.5523 * r);
                format!(
                    "{} {cy} m {} {} {} {} {cx} {} c {} {} {} {} {} {cy} c {} {} {} {} {cx} {} c {} {} {} {} {} {cy} c",
                    cx + r,
                    cx + r,
                    cy + k,
                    cx + k,
                    cy + r,
                    cy + r,
                    cx - k,
                    cy + r,
                    cx - r,
                    cy + k,
                    cx - r,
                    cx - r,
                    cy - k,
                    cx - k,
                    cy - r,
                    cy - r,
                    cx + k,
                    cy - r,
                    cx + r,
                    cy - k,
                    cx + r
                )
            };
            let r = w.min(h) / 2.0 - 0.5;
            let off = format!("1 g {} f 0.6 G 1 w {} S", ring(r), ring(r));
            let dot = format!("{off} 0 g {} f", ring(r / 2.2));
            let mut states = Dictionary::new();
            states.set(on, stream(dot, false));
            states.set("Off", stream(off, false));
            Object::Dictionary(states)
        }
    };
    let mut ap = Dictionary::new();
    ap.set("N", n);
    widget.set("AP", ap);
}

/// The open document's field names (top-level), for unique new names.
pub fn field_names() -> Result<Vec<String>, Error> {
    let pages = with_open(|document| Ok(document.pages().len()))?;
    let mut names = Vec::new();
    for page in 0..u16::try_from(pages).unwrap_or(0) {
        for f in crate::form_fields(page)? {
            if let Some(n) = f.name
                && !names.contains(&n)
            {
                names.push(n);
            }
        }
    }
    Ok(names)
}
