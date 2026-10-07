//! Bookmarks (D-043): the document's outline as a flat list in reading order,
//! each with its level. PDFium reads it; PDFium can't write one, so
//! [`set_bookmarks`] writes the whole list with lopdf as an incremental
//! update and opens the result again (D-045, RESEARCH §15).

use crate::Error;
use crate::render::{open_document, with_open};
use pdfium_render::prelude::*;

/// One bookmark. `level` 0 is top level; 1 sits under the bookmark before it.
#[derive(Clone, Debug, PartialEq)]
pub struct Bookmark {
    pub title: String,
    /// The page it opens (0-based); `None` when it opens something else
    /// (another file, a web address) — such a bookmark keeps only its title
    /// when the list is written again.
    pub page: Option<u16>,
    /// Where on the page, in points from the bottom; `None` shows the whole page.
    pub top: Option<f32>,
    pub level: u8,
}

/// How deep a bookmark list goes (deeper ones are written at this level).
const MAX_LEVEL: u8 = 7;

/// The document's bookmarks, depth first.
pub fn bookmarks() -> Result<Vec<Bookmark>, Error> {
    with_open(|document| {
        let mut out = Vec::new();
        let bookmarks = document.bookmarks();
        // PDFium's first top-level bookmark; walk siblings and children.
        let mut stack: Vec<(PdfBookmark, u8)> = Vec::new();
        if let Some(first) = bookmarks.root() {
            stack.push((first, 0));
        }
        while let Some((bookmark, level)) = stack.pop() {
            let dest = bookmark.destination();
            let page = dest
                .as_ref()
                .and_then(|d| d.page_index().ok())
                .and_then(|i| u16::try_from(i).ok());
            let top = dest.and_then(|d| match d.view_settings().ok()? {
                PdfDestinationViewSettings::SpecificCoordinatesAndZoom(_, y, _) => {
                    y.map(|y| y.value)
                }
                PdfDestinationViewSettings::FitPageHorizontallyToWindow(y) => y.map(|y| y.value),
                _ => None,
            });
            out.push(Bookmark {
                title: bookmark.title().unwrap_or_default(),
                page,
                top,
                level,
            });
            // Next sibling after this bookmark's children: push it first.
            if let Some(next) = bookmark.next_sibling() {
                stack.push((next, level));
            }
            if let Some(child) = bookmark.first_child() {
                stack.push((child, level.saturating_add(1).min(MAX_LEVEL)));
            }
        }
        Ok(out)
    })
}

/// Replaces the document's bookmarks with `list` (an empty list removes
/// them). A level more than one deeper than the bookmark before it is
/// treated as one deeper.
pub fn set_bookmarks(list: &[Bookmark]) -> Result<(), Error> {
    let bytes = crate::page_ops::snapshot()?;
    let written = write_outline(bytes, list)
        .map_err(|e| Error::Pdfium(format!("could not write the bookmarks: {e}")))?;
    open_document(written).map(|_| ())
}

fn write_outline(bytes: Vec<u8>, list: &[Bookmark]) -> Result<Vec<u8>, String> {
    use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId};
    let prev = Document::load_mem(&bytes).map_err(|e| e.to_string())?;
    let pages: Vec<ObjectId> = prev.get_pages().into_values().collect();
    let root = prev
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| e.to_string())?;
    let mut inc = IncrementalDocument::create_from(bytes, prev);
    inc.opt_clone_object_to_new_document(root)
        .map_err(|e| e.to_string())?;
    if list.is_empty() {
        inc.new_document
            .get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .map_err(|e| e.to_string())?
            .remove(b"Outlines");
    } else {
        let doc = &mut inc.new_document;
        let outlines_id = doc.new_object_id();
        let ids: Vec<ObjectId> = list.iter().map(|_| doc.new_object_id()).collect();
        // Parent of each item (None = the outline root), levels clamped.
        let mut parent: Vec<Option<usize>> = Vec::with_capacity(list.len());
        let mut path: Vec<usize> = Vec::new(); // open items, one per level
        for (i, b) in list.iter().enumerate() {
            let level = usize::from(b.level.min(MAX_LEVEL)).min(path.len());
            path.truncate(level);
            parent.push(path.last().copied());
            path.push(i);
        }
        let children = |p: Option<usize>| -> Vec<usize> {
            (0..list.len()).filter(|&i| parent[i] == p).collect()
        };
        let descendants = |i: usize| -> usize {
            // Items after i while deeper than it (they're in order).
            let depth = |mut k: usize| {
                let mut d = 0;
                while let Some(p) = parent[k] {
                    d += 1;
                    k = p;
                }
                d
            };
            let d = depth(i);
            list[i + 1..]
                .iter()
                .enumerate()
                .take_while(|(k, _)| depth(i + 1 + k) > d)
                .count()
        };
        for (i, b) in list.iter().enumerate() {
            let mut d = Dictionary::new();
            d.set("Title", text_string(&b.title));
            d.set("Parent", parent[i].map_or(outlines_id, |p| ids[p]));
            let siblings = children(parent[i]);
            let at = siblings.iter().position(|&s| s == i).unwrap_or(0);
            if at > 0 {
                d.set("Prev", ids[siblings[at - 1]]);
            }
            if let Some(&next) = siblings.get(at + 1) {
                d.set("Next", ids[next]);
            }
            let kids = children(Some(i));
            if let (Some(&first), Some(&last)) = (kids.first(), kids.last()) {
                d.set("First", ids[first]);
                d.set("Last", ids[last]);
                // Positive: shown open.
                d.set("Count", descendants(i) as i64);
            }
            if let Some(&page) = b.page.and_then(|p| pages.get(usize::from(p))) {
                let dest = match b.top {
                    Some(top) => vec![
                        page.into(),
                        "XYZ".into(),
                        Object::Null,
                        Object::Real(top),
                        Object::Null,
                    ],
                    None => vec![page.into(), "Fit".into()],
                };
                d.set("Dest", dest);
            }
            doc.set_object(ids[i], d);
        }
        let top_level = children(None);
        let mut outlines = Dictionary::new();
        outlines.set("Type", "Outlines");
        outlines.set("First", ids[top_level[0]]);
        outlines.set("Last", ids[*top_level.last().unwrap_or(&top_level[0])]);
        outlines.set("Count", list.len() as i64);
        doc.set_object(outlines_id, outlines);
        inc.new_document
            .get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .map_err(|e| e.to_string())?
            .set("Outlines", outlines_id);
    }
    let mut out = Vec::new();
    inc.save_to(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// A PDF text string: plain bytes when ASCII, else UTF-16BE with its mark.
fn text_string(s: &str) -> lopdf::Object {
    use lopdf::{Object, StringFormat};
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

/// Bookmarks made from the document's headings: lines set clearly larger than
/// the body text (the size most of the text uses). The largest heading size
/// is level 0, the next level 1, and so on. Empty when nothing stands out.
pub fn heading_bookmarks() -> Result<Vec<Bookmark>, Error> {
    let pages = with_open(|document| Ok(document.pages().len()))?;
    let pages = u16::try_from(pages).unwrap_or(0);
    // (page, title, top, size, characters)
    let mut lines: Vec<(u16, String, f32, f32)> = Vec::new();
    for page in 0..pages {
        for line in crate::text_lines(page)? {
            let title = line.text.split_whitespace().collect::<Vec<_>>().join(" ");
            if title.is_empty() {
                continue;
            }
            let size = crate::text_style(page, line.object_index)
                .map(|s| s.size)
                .unwrap_or(0.0);
            lines.push((page, title, line.bounds[3], size));
        }
    }
    // The body size: the one carrying the most characters (to 0.5 pt).
    let mut weight: Vec<(i32, usize)> = Vec::new();
    for (_, title, _, size) in &lines {
        let key = (size * 2.0).round() as i32;
        match weight.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += title.chars().count(),
            None => weight.push((key, title.chars().count())),
        }
    }
    let Some(&(body, _)) = weight.iter().max_by_key(|(_, n)| *n) else {
        return Ok(Vec::new());
    };
    let body = body as f32 / 2.0;
    // ponytail: size-only heuristic — no bold or numbering cues; add them if
    // real documents need it.
    let is_heading = |(_, title, _, size): &&(u16, String, f32, f32)| {
        *size >= body * 1.15 && title.chars().count() <= 120
    };
    let mut sizes: Vec<i32> = lines
        .iter()
        .filter(is_heading)
        .map(|l| (l.3 * 2.0).round() as i32)
        .collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();
    Ok(lines
        .iter()
        .filter(is_heading)
        .map(|(page, title, top, size)| Bookmark {
            title: title.clone(),
            page: Some(*page),
            // A little above the line, so it isn't cut off at the window's top.
            top: Some(top + 8.0),
            level: sizes
                .iter()
                .position(|&s| s == (size * 2.0).round() as i32)
                .map_or(0, |p| p.min(usize::from(MAX_LEVEL)) as u8),
        })
        .collect())
}
