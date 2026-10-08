//! Font subsetting (D-028): a fallback font is embedded with only the glyphs a
//! text needs, instead of the whole file (P-029). allsorts keeps a Unicode
//! cmap in the subset, which PDFium needs to map the text to glyphs; subsetter
//! (the first candidate) drops the cmap, so PDFium could not use its output.

use crate::Error;
use allsorts::binary::read::ReadScope;
use allsorts::font::{Font, MatchingPresentation};
use allsorts::font_data::FontData;
use allsorts::subset::{CmapTarget, SubsetProfile, subset};

/// A copy of `font` holding only the glyphs for the characters in `text`
/// (plus .notdef), with a Unicode cmap for them.
pub fn subset_for_text(font: &[u8], text: &str) -> Result<Vec<u8>, Error> {
    let fail = |what: &str, error: &dyn std::fmt::Debug| {
        Error::Pdfium(format!("font subsetting: {what}: {error:?}"))
    };
    let data = ReadScope::new(font)
        .read::<FontData<'_>>()
        .map_err(|e| fail("read", &e))?;
    let provider = data.table_provider(0).map_err(|e| fail("tables", &e))?;
    let mut parsed = Font::new(provider).map_err(|e| fail("parse", &e))?;
    let mut glyphs = vec![0_u16];
    for ch in text.chars() {
        let (glyph, _) = parsed.lookup_glyph_index(ch, MatchingPresentation::NotRequired, None);
        if glyph != 0 && !glyphs.contains(&glyph) {
            glyphs.push(glyph);
        }
    }
    let mut bytes = subset(
        &parsed.font_table_provider,
        &glyphs,
        &SubsetProfile::Minimal,
        CmapTarget::Unicode,
    )
    .map_err(|e| fail("subset", &e))?;
    tag_postscript_name(&mut bytes);
    Ok(bytes)
}

/// Whether `font` has a glyph for every character of `text` (spaces aside).
pub fn covers(font: &[u8], text: &str) -> bool {
    has_all_glyphs(font, text) == Some(true)
}

/// Whether `font` can be read and lacks a glyph for some character of `text`
/// (spaces aside). An unreadable font (CFF, a bare CID font) says nothing.
pub fn lacks_glyph(font: &[u8], text: &str) -> bool {
    has_all_glyphs(font, text) == Some(false)
}

/// `None` when the font's tables can't be read.
fn has_all_glyphs(font: &[u8], text: &str) -> Option<bool> {
    let data = ReadScope::new(font).read::<FontData<'_>>().ok()?;
    let provider = data.table_provider(0).ok()?;
    let mut parsed = Font::new(provider).ok()?;
    Some(text.chars().filter(|c| !c.is_whitespace()).all(|c| {
        parsed
            .lookup_glyph_index(c, MatchingPresentation::NotRequired, None)
            .0
            != 0
    }))
}

/// Gives a subset its own PostScript name, as PDF subset tags do
/// ("ABCDEF+NotoSans-Regular"): PDFium's content writer shares one font
/// resource between fonts of the same name, so two subsets both called
/// "NotoSans-Regular" made the second line show the first one's glyphs
/// (P-040). The tag comes from the subset's bytes (same glyphs, same tag);
/// readers drop it (edit_open strips "TAG+"). The font is returned unchanged
/// if its tables can't be read.
fn tag_postscript_name(font: &mut Vec<u8>) {
    if let Some(tagged) = with_tagged_name(font) {
        *font = tagged;
    }
}

fn with_tagged_name(font: &[u8]) -> Option<Vec<u8>> {
    let u16_at = |at: usize| {
        font.get(at..at + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
    };
    let u32_at = |at: usize| {
        font.get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    // FNV-1a over the subset: the same glyphs give the same tag.
    let hash = font.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    });
    let tag: String = (0..6)
        .map(|i| char::from(b'A' + ((hash >> (i * 5)) % 26) as u8))
        .chain(['+'])
        .collect();
    // The table directory: (tag, data).
    let count = usize::from(u16_at(4)?);
    let mut tables = Vec::with_capacity(count);
    for i in 0..count {
        let rec = 12 + i * 16;
        let name: [u8; 4] = font.get(rec..rec + 4)?.try_into().ok()?;
        let (offset, length) = (u32_at(rec + 8)? as usize, u32_at(rec + 12)? as usize);
        tables.push((name, font.get(offset..offset + length)?.to_vec()));
    }
    // The name table, with the tag before each PostScript name (format 0).
    let name = &mut tables.iter_mut().find(|(t, _)| t == b"name")?.1;
    let n = name.as_slice();
    let n16 = |at: usize| n.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    if n16(0)? != 0 {
        return None;
    }
    let records = usize::from(n16(2)?);
    let strings_at = usize::from(n16(4)?);
    let mut head = Vec::new();
    let mut strings = Vec::new();
    head.extend_from_slice(&n[0..4]);
    head.extend_from_slice(&((6 + records * 12) as u16).to_be_bytes());
    for r in 0..records {
        let rec = 6 + r * 12;
        let (platform, id) = (n16(rec)?, n16(rec + 6)?);
        let (len, off) = (usize::from(n16(rec + 8)?), usize::from(n16(rec + 10)?));
        let text = n.get(strings_at + off..strings_at + off + len)?;
        let mut value = Vec::new();
        if id == 6 {
            // UTF-16BE on the Unicode and Windows platforms, one byte per char on Mac.
            for c in tag.chars() {
                if platform == 1 {
                    value.push(c as u8);
                } else {
                    value.extend_from_slice(&(c as u16).to_be_bytes());
                }
            }
        }
        value.extend_from_slice(text);
        head.extend_from_slice(&n[rec..rec + 8]);
        head.extend_from_slice(&u16::try_from(value.len()).ok()?.to_be_bytes());
        head.extend_from_slice(&u16::try_from(strings.len()).ok()?.to_be_bytes());
        strings.extend_from_slice(&value);
    }
    head.extend_from_slice(&strings);
    *name = head;
    // The font again: same header, new directory, tables 4-byte aligned.
    let mut out = font.get(0..12)?.to_vec();
    let mut offset = 12 + count * 16;
    let mut body = Vec::new();
    for (name, data) in &tables {
        let mut padded = data.clone();
        padded.resize(data.len().div_ceil(4) * 4, 0);
        let sum = padded.chunks(4).fold(0u32, |s, w| {
            s.wrapping_add(u32::from_be_bytes([w[0], w[1], w[2], w[3]]))
        });
        out.extend_from_slice(name);
        out.extend_from_slice(&sum.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += padded.len();
        body.extend_from_slice(&padded);
    }
    out.extend_from_slice(&body);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allsorts::tables::NameTable;
    use allsorts::tag;

    fn postscript_name(font: &[u8]) -> Option<String> {
        let data = ReadScope::new(font).read::<FontData<'_>>().ok()?;
        let provider = data.table_provider(0).ok()?;
        use allsorts::tables::FontTableProvider;
        let name = provider.table_data(tag::NAME).ok()??;
        let table = ReadScope::new(&name).read::<NameTable<'_>>().ok()?;
        table.string_for_id(NameTable::POSTSCRIPT_NAME)
    }

    #[test]
    fn subsets_get_their_own_names() {
        let noto = include_bytes!("../../pdit-app/assets/fonts/NotoSans-Regular.ttf");
        let a = subset_for_text(noto, "Rent is 890").unwrap();
        let b = subset_for_text(noto, "Pay the fee").unwrap();
        let again = subset_for_text(noto, "Rent is 890").unwrap();
        let (na, nb) = (postscript_name(&a).unwrap(), postscript_name(&b).unwrap());
        assert_ne!(na, nb);
        assert_eq!(na, postscript_name(&again).unwrap());
        assert!(na.ends_with("+NotoSans-Regular") && na.len() == 23, "{na}");
        // Still a font allsorts (and so PDFium) can read.
        assert!(subset_for_text(&a, "Rent").is_ok());
    }
}
