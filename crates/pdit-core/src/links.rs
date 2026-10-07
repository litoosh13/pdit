//! Clickable links (D-043): web links on selected text, links to a page of
//! this file, finding written-out web addresses, and reading every link on a
//! page. PDFium can't write a page destination, so page links are appended
//! with lopdf (D-045). Every change first drops a previewed text edit.

use crate::annotations::{pdf_rect, quad, rect, union};
use crate::edit_open::discard_edit;
use crate::render::{open_document, with_open};
use crate::{Error, pdfium};
use pdfium_render::prelude::*;

/// Where a link goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// An `http(s)://` or `mailto:` address.
    Web(String),
    /// A page of this file (0-based).
    Page(u16),
    /// Anything else (other schemes, other files, scripts): shown, never followed.
    Other,
}

/// A link on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// Index in the page's annotation list.
    pub index: usize,
    /// [left, bottom, right, top] in points.
    pub bounds: [f32; 4],
    /// One box per linked line (empty when the link has only its box).
    pub rects: Vec<[f32; 4]>,
    pub target: LinkTarget,
}

/// A web address written out in the page's text.
#[derive(Clone, Debug, PartialEq)]
pub struct WebAddress {
    pub url: String,
    pub rects: Vec<[f32; 4]>,
}

/// What a person typed as a link address, made into an `https://` or
/// `mailto:` address; `None` for anything else (other schemes, spaces, no
/// domain). "example.org" → "https://example.org", "a@b.org" → "mailto:a@b.org".
pub fn normalize_url(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    let url = if ["http://", "https://", "mailto:"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        s.to_owned()
    } else if let Some((scheme, _)) = s.split_once(':')
        && !scheme.is_empty()
        && scheme.chars().all(|c| c.is_ascii_alphabetic())
    {
        return None; // javascript:, file:, ...
    } else if s.contains('@') {
        format!("mailto:{s}")
    } else {
        format!("https://{s}")
    };
    let (scheme, rest) = url.split_once(':')?;
    let ok = if scheme.eq_ignore_ascii_case("mailto") {
        let (name, domain) = rest.split_once('@')?;
        !name.is_empty() && domain.contains('.') && !domain.starts_with('.')
    } else {
        let host = rest.strip_prefix("//")?.split(['/', '?', '#']).next()?;
        host.contains('.') && !host.starts_with('.') && !host.ends_with('.')
    };
    ok.then_some(url)
}

/// Links `rects` (one box per line, [left, bottom, right, top]) on `page` to
/// `url`, which must be an address [`normalize_url`] accepts. The link has no
/// border, so the page looks unchanged. Returns its index.
pub fn add_web_link(page: u16, rects: &[[f32; 4]], url: &str) -> Result<usize, Error> {
    let Some(&first) = rects.first() else {
        return Err(Error::Pdfium("nothing selected to link".into()));
    };
    let url = checked(url)?;
    discard_edit()?;
    let bounds = rects.iter().fold(first, |a, &b| union(a, b));
    let index = with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let mut link = pdf_page.annotations_mut().create_link_annotation(&url)?;
        for &r in rects {
            link.attachment_points_mut()
                .create_attachment_point_at_end(quad(r))?;
        }
        link.set_bounds(pdf_rect(bounds))?;
        Ok(pdf_page.annotations().len() - 1)
    })?;
    with_link(page, index, |annot| unsafe {
        // Without /Border viewers may draw a 1 pt black frame (the default).
        pdfium()
            .bindings()
            .FPDFAnnot_SetBorder(annot, 0.0, 0.0, 0.0);
        Ok(())
    })?;
    Ok(index)
}

/// Points the link at `index` to `url` (an address [`normalize_url`] accepts).
pub fn set_web_link(page: u16, index: usize, url: &str) -> Result<(), Error> {
    let url = checked(url)?;
    discard_edit()?;
    with_link(page, index, |annot| unsafe {
        let bindings = pdfium().bindings();
        if bindings.is_true(bindings.FPDFAnnot_SetURI(annot, &url)) {
            Ok(())
        } else {
            Err(Error::Pdfium("could not change the link".into()))
        }
    })
}

/// Links `rects` (one box per line, [left, bottom, right, top]) on `page` to
/// page `target` (0-based) of this file. PDFium has no call for a page
/// destination, so the open document is saved, the link is appended with
/// lopdf as an incremental update (D-045, RESEARCH §15), and the result is
/// opened again — the same reload Undo uses. Returns the link's index.
pub fn add_page_link(page: u16, rects: &[[f32; 4]], target: u16) -> Result<usize, Error> {
    if rects.is_empty() {
        return Err(Error::Pdfium("nothing selected to link".into()));
    }
    let bytes = crate::page_ops::snapshot()?;
    let linked = append_page_link(bytes, page, rects, target)
        .map_err(|e| Error::Pdfium(format!("could not add the page link: {e}")))?;
    open_document(linked)?;
    with_open(|document| Ok(document.pages().get(page.into())?.annotations().len() - 1))
}

/// The lopdf part of [`add_page_link`]: only the new link, the page (its
/// /Annots) and, if /Annots is its own object, that array are appended; the
/// bytes before stay as they are.
fn append_page_link(
    bytes: Vec<u8>,
    page: u16,
    rects: &[[f32; 4]],
    target: u16,
) -> Result<Vec<u8>, String> {
    use lopdf::{Dictionary, Document, IncrementalDocument, Object};
    let prev = Document::load_mem(&bytes).map_err(|e| e.to_string())?;
    let pages: Vec<lopdf::ObjectId> = prev.get_pages().into_values().collect();
    let (Some(&on), Some(&to)) = (pages.get(page as usize), pages.get(target as usize)) else {
        return Err("no such page".into());
    };
    let mut inc = IncrementalDocument::create_from(bytes, prev);
    let real = |v: f32| Object::Real(v);
    let bounds = rects.iter().fold(rects[0], |a, &b| union(a, b));
    let mut link = Dictionary::new();
    link.set("Type", "Annot");
    link.set("Subtype", "Link");
    link.set("Rect", bounds.map(real).to_vec());
    // Viewer order per line: top-left, top-right, bottom-left, bottom-right.
    let quads: Vec<Object> = rects
        .iter()
        .flat_map(|&[l, b, r, t]| [l, t, r, t, l, b, r, b])
        .map(real)
        .collect();
    link.set("QuadPoints", quads);
    link.set("Border", vec![Object::Integer(0); 3]);
    link.set("Dest", vec![Object::Reference(to), "Fit".into()]);
    link.set("P", Object::Reference(on));
    let link_id = inc.new_document.add_object(link);
    inc.opt_clone_object_to_new_document(on)
        .map_err(|e| e.to_string())?;
    let annots = inc
        .new_document
        .get_object(on)
        .and_then(Object::as_dict)
        .map_err(|e| e.to_string())?
        .get(b"Annots")
        .ok()
        .cloned();
    match annots {
        Some(Object::Reference(list)) => {
            inc.opt_clone_object_to_new_document(list)
                .map_err(|e| e.to_string())?;
            inc.new_document
                .get_object_mut(list)
                .and_then(Object::as_array_mut)
                .map_err(|e| e.to_string())?
                .push(Object::Reference(link_id));
        }
        other => {
            let mut list = other
                .and_then(|o| o.as_array().ok().cloned())
                .unwrap_or_default();
            list.push(Object::Reference(link_id));
            inc.new_document
                .get_object_mut(on)
                .and_then(Object::as_dict_mut)
                .map_err(|e| e.to_string())?
                .set("Annots", list);
        }
    }
    let mut out = Vec::new();
    inc.save_to(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Removes the link at `index`.
pub fn delete_link(page: u16, index: usize) -> Result<(), Error> {
    discard_edit()?;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let annotations = pdf_page.annotations_mut();
        let annotation = annotations.get(index as PdfPageAnnotationIndex)?;
        if annotation.annotation_type() != PdfPageAnnotationType::Link {
            return Err(Error::Pdfium("that link has changed".into()));
        }
        annotations.delete_annotation(annotation)?;
        Ok(())
    })
}

/// The links on `page`.
pub fn links(page: u16) -> Result<Vec<Link>, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let mut out = Vec::new();
        for (index, annotation) in pdf_page.annotations().iter().enumerate() {
            let Some(link_annot) = annotation.as_link_annotation() else {
                continue;
            };
            let target = match link_annot.link() {
                Ok(link) => target_of(&link),
                Err(_) => LinkTarget::Other,
            };
            out.push(Link {
                index,
                bounds: rect(&annotation.bounds()?),
                rects: annotation
                    .attachment_points()
                    .iter()
                    .map(crate::inspect::rect)
                    .collect(),
                target,
            });
        }
        Ok(out)
    })
}

fn target_of(link: &PdfLink) -> LinkTarget {
    if let Some(uri) = link
        .action()
        .and_then(|a| a.as_uri_action().and_then(|u| u.uri().ok()))
    {
        let lower = uri.to_ascii_lowercase();
        return if ["http://", "https://", "mailto:"]
            .iter()
            .any(|p| lower.starts_with(p))
        {
            LinkTarget::Web(uri)
        } else {
            LinkTarget::Other
        };
    }
    match link.destination().map(|d| d.page_index()) {
        Some(Ok(i)) => u16::try_from(i).map_or(LinkTarget::Other, LinkTarget::Page),
        _ => LinkTarget::Other,
    }
}

/// Web addresses written out on `page` (found by PDFium: `http(s)://…`,
/// `www.…` and e-mail addresses) that no link covers yet, as addresses
/// [`normalize_url`] accepts.
pub fn web_addresses(page: u16) -> Result<Vec<WebAddress>, Error> {
    let existing = links(page)?;
    let words = crate::annotations::words(page)?;
    let mut out = with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let text = pdf_page.text()?;
        let chars = text.chars();
        let bindings = pdfium().bindings();
        let mut out = Vec::new();
        unsafe {
            let text_page = bindings.FPDFText_LoadPage(pdf_page.page_handle());
            if text_page.is_null() {
                return Ok(out);
            }
            let web = bindings.FPDFLink_LoadWebLinks(text_page);
            if !web.is_null() {
                for i in 0..bindings.FPDFLink_CountWebLinks(web) {
                    // The address is read from its characters: the wasm bridge's
                    // FPDFLink_GetURL returns half of it (P-039).
                    let (mut start, mut count) = (0, 0);
                    if !bindings
                        .is_true(bindings.FPDFLink_GetTextRange(web, i, &mut start, &mut count))
                    {
                        continue;
                    }
                    let written: String = (start..start + count)
                        .filter_map(|c| chars.get(c as usize).ok()?.unicode_char())
                        .collect();
                    let Some(url) = normalize_url(&written) else {
                        continue;
                    };
                    let rects: Vec<[f32; 4]> =
                        (0..bindings.FPDFLink_CountRects(web, i))
                            .filter_map(|r| {
                                let (mut l, mut t, mut rr, mut b) = (0.0, 0.0, 0.0, 0.0);
                                bindings
                                    .is_true(bindings.FPDFLink_GetRect(
                                        web, i, r, &mut l, &mut t, &mut rr, &mut b,
                                    ))
                                    .then_some([l as f32, b as f32, rr as f32, t as f32])
                            })
                            .collect();
                    let covered = rects.iter().all(|r| {
                        let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
                        existing.iter().any(|l| {
                            let b = l.bounds;
                            cx >= b[0] && cx <= b[2] && cy >= b[1] && cy <= b[3]
                        })
                    });
                    if !rects.is_empty() && !covered {
                        out.push(WebAddress { url, rects });
                    }
                }
                bindings.FPDFLink_CloseWebLinks(web);
            }
            bindings.FPDFText_ClosePage(text_page);
        }
        Ok(out)
    })?;
    // PDFium's finder can miss e-mail addresses; take whole words that are one.
    for word in words {
        let written = word.text.trim_end_matches(['.', ',', ';', ':', ')']);
        let taken = |r: &[f32; 4]| {
            let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
            let inside = |b: [f32; 4]| cx >= b[0] && cx <= b[2] && cy >= b[1] && cy <= b[3];
            existing.iter().any(|l| inside(l.bounds))
                || out.iter().any(|a| a.rects.iter().any(|&b| inside(b)))
        };
        if written.contains('@')
            && let Some(url) = normalize_url(written)
            && url.starts_with("mailto:")
            && !taken(&word.bounds)
        {
            out.push(WebAddress {
                url,
                rects: vec![word.bounds],
            });
        }
    }
    Ok(out)
}

/// Turns every written-out web address in the document into a link.
/// Returns how many were made.
pub fn link_web_addresses() -> Result<usize, Error> {
    let pages = with_open(|document| Ok(document.pages().len()))?;
    let pages = u16::try_from(pages).unwrap_or(0);
    let mut made = 0;
    for page in 0..pages {
        for address in web_addresses(page)? {
            add_web_link(page, &address.rects, &address.url)?;
            made += 1;
        }
    }
    Ok(made)
}

fn checked(url: &str) -> Result<String, Error> {
    normalize_url(url).ok_or_else(|| Error::Pdfium("not a web or e-mail address".into()))
}

/// Runs `f` on the raw handle of the link annotation at `index`.
fn with_link(
    page: u16,
    index: usize,
    f: impl FnOnce(FPDF_ANNOTATION) -> Result<(), Error>,
) -> Result<(), Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let bindings = pdfium().bindings();
        unsafe {
            let annot = bindings.FPDFPage_GetAnnot(pdf_page.page_handle(), index as i32);
            if annot.is_null() {
                return Err(Error::Pdfium("that link has changed".into()));
            }
            let result =
                if bindings.FPDFAnnot_GetSubtype(annot) == PdfPageAnnotationType::Link as i32 {
                    f(annot)
                } else {
                    Err(Error::Pdfium("that link has changed".into()))
                };
            bindings.FPDFPage_CloseAnnot(annot);
            result
        }
    })
}
