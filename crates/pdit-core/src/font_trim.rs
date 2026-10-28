//! Font trimming for Compress online (D-078): an embedded TrueType font keeps only the glyphs the PDF can show.
//! Glyph numbers stay the same (only the other glyphs' outlines go), so the font's cmap and the PDF's codes still
//! match, as in MuPDF's subsetting. A simple font keeps every glyph its 256 codes can reach (through the cmap, Latin
//! and symbol ranges); a two-byte font (Identity-H/V) keeps the glyphs its text uses. A font used in a way not
//! understood here stays whole.

use allsorts::binary::read::ReadScope;
use allsorts::tables::cmap::{Cmap, CmapSubtable};
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Unicode a simple font's 256 codes can stand for (WinAnsi, MacRoman, Standard, PDFDoc and symbol fonts):
/// Latin, spacing marks, Greek, punctuation to math, the lozenge, symbol-font codes (fi/fl there too), Apple's logo, fi/fl.
const SIMPLE_RANGES: [(u32, u32); 6] = [
    (0x0000, 0x03FF),
    (0x2000, 0x22FF),
    (0x25CA, 0x25CA),
    (0xF000, 0xF0FF),
    (0xF8FF, 0xF8FF),
    (0xFB01, 0xFB02),
];
/// Tables kept in a trimmed font (the ones PDF viewers read); layout tables (GSUB, GPOS, kern…) go.
const KEPT_TABLES: [&[u8; 4]; 15] = [
    b"OS/2", b"cmap", b"cvt ", b"fpgm", b"glyf", b"head", b"hhea", b"hmtx", b"loca", b"maxp",
    b"name", b"post", b"prep", b"vhea", b"vmtx",
];

/// Trims the document's embedded TrueType fonts in place.
pub(crate) fn trim_fonts(doc: &mut Document) {
    let shown = shown_codes(doc);
    let form_fonts = form_fonts(doc);
    let mut fonts = Vec::new();
    for (id, object) in &doc.objects {
        find_fonts(Some(*id), object, &mut fonts);
    }
    // Font file → glyphs to keep; None: keep it whole.
    let mut keep: HashMap<ObjectId, Option<BTreeSet<u16>>> = HashMap::new();
    for (font_id, font) in fonts {
        let Some((file, glyphs)) = font_glyphs(doc, font_id, &font, shown.as_ref(), &form_fonts)
        else {
            continue;
        };
        match (
            keep.entry(file).or_insert_with(|| Some(BTreeSet::new())),
            glyphs,
        ) {
            (Some(all), Some(glyphs)) => all.extend(glyphs),
            (all, _) => *all = None,
        }
    }
    for (file, glyphs) in keep {
        let Some(glyphs) = glyphs else { continue };
        let Some(stream) = doc.get_object(file).ok().and_then(|o| o.as_stream().ok()) else {
            continue;
        };
        let Some(trimmed) = stream_bytes(stream).and_then(|font| trim_truetype(&font, &glyphs))
        else {
            continue;
        };
        let mut dict = stream.dict.clone();
        dict.remove(b"Filter");
        dict.remove(b"DecodeParms");
        dict.set("Length1", trimmed.len() as i64);
        doc.objects
            .insert(file, Object::Stream(Stream::new(dict, trimmed)));
    }
}

/// Every font dictionary in the document (with its id when it is an object of its own).
fn find_fonts(
    id: Option<ObjectId>,
    object: &Object,
    out: &mut Vec<(Option<ObjectId>, Dictionary)>,
) {
    let dict = match object {
        Object::Dictionary(d) => d,
        Object::Stream(s) => &s.dict,
        Object::Array(a) => return a.iter().for_each(|o| find_fonts(None, o, out)),
        _ => return,
    };
    if dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Font") {
        out.push((id, dict.clone()));
    }
    for (_, value) in dict.iter() {
        find_fonts(None, value, out);
    }
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    match object {
        Object::Reference(id) => doc.get_object(*id).ok(),
        o => Some(o),
    }
}

fn get<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    resolve(doc, dict.get(key).ok()?)
}

fn stream_bytes(stream: &Stream) -> Option<Vec<u8>> {
    match stream.dict.has(b"Filter") {
        true => stream.decompressed_content().ok(),
        false => Some(stream.content.clone()),
    }
}

/// The font's embedded TrueType file and the glyphs it needs (`None`: all of them); `None` when it has no such file.
fn font_glyphs(
    doc: &Document,
    font_id: Option<ObjectId>,
    font: &Dictionary,
    shown: Option<&HashMap<ObjectId, HashSet<u16>>>,
    form_fonts: &HashSet<ObjectId>,
) -> Option<(ObjectId, Option<BTreeSet<u16>>)> {
    let name =
        |d: &Dictionary, k: &[u8]| d.get(k).and_then(Object::as_name).ok().map(<[u8]>::to_vec);
    let file_of = |d: &Dictionary| {
        let descriptor = get(doc, d, b"FontDescriptor")?.as_dict().ok()?;
        descriptor.get(b"FontFile2").ok()?.as_reference().ok()
    };
    match name(font, b"Subtype")?.as_slice() {
        b"TrueType" => {
            let file = file_of(font)?;
            let differences = get(doc, font, b"Encoding")
                .and_then(|e| e.as_dict().ok())
                .is_some_and(|e| e.has(b"Differences"));
            let glyphs = (!differences)
                .then(|| {
                    doc.get_object(file)
                        .ok()?
                        .as_stream()
                        .ok()
                        .and_then(stream_bytes)
                })
                .flatten()
                .and_then(|data| simple_glyphs(&data));
            Some((file, glyphs))
        }
        b"Type0" => {
            let descendants = get(doc, font, b"DescendantFonts")?.as_array().ok()?;
            let cid_font = resolve(doc, descendants.first()?)?.as_dict().ok()?;
            let file = file_of(cid_font)?;
            let identity = matches!(
                name(font, b"Encoding").as_deref(),
                Some(b"Identity-H" | b"Identity-V")
            );
            let shown = font_id
                .filter(|id| identity && !form_fonts.contains(id))
                .and_then(|id| Some(shown?.get(&id).cloned().unwrap_or_default()));
            let glyphs = shown.and_then(|cids| {
                match cid_font.get(b"CIDToGIDMap").ok().map(|m| resolve(doc, m)) {
                    None => Some(cids.into_iter().collect()),
                    Some(Some(Object::Name(n))) if n == b"Identity" => {
                        Some(cids.into_iter().collect())
                    }
                    Some(Some(Object::Stream(map))) => {
                        let map = stream_bytes(map)?;
                        let gid = |cid: u16| {
                            let i = cid as usize * 2;
                            Some(u16::from_be_bytes([*map.get(i)?, *map.get(i + 1)?]))
                        };
                        cids.into_iter().map(gid).collect()
                    }
                    _ => None,
                }
            });
            Some((file, glyphs))
        }
        _ => None,
    }
}

/// The glyphs a simple TrueType font's codes can reach through its cmap; `None` without a usable cmap.
fn simple_glyphs(font: &[u8]) -> Option<BTreeSet<u16>> {
    let cmap = ReadScope::new(table(font, b"cmap")?)
        .read::<Cmap<'_>>()
        .ok()?;
    let mut glyphs = BTreeSet::new();
    for record in cmap.encoding_records() {
        let unicode = match (record.platform_id.0, record.encoding_id.0) {
            (0, 0..=4) | (3, 1) | (3, 10) => true,
            (1, 0) | (3, 0) => false,
            _ => continue,
        };
        let subtable = cmap
            .scope
            .offset(record.offset as usize)
            .read::<CmapSubtable<'_>>()
            .ok()?;
        subtable
            .mappings_fn(|ch, gid| {
                let wanted = match unicode {
                    true => SIMPLE_RANGES.iter().any(|(a, b)| (*a..=*b).contains(&ch)),
                    false => ch < 0x100 || (0xF000..0xF100).contains(&ch),
                };
                if wanted {
                    glyphs.insert(gid);
                }
            })
            .ok()?;
    }
    (!glyphs.is_empty()).then_some(glyphs)
}

/// Fonts in the interactive form's default resources: a viewer may draw any text with them.
fn form_fonts(doc: &Document) -> HashSet<ObjectId> {
    let fonts = || {
        let catalog = doc.catalog().ok()?;
        let form = get(doc, catalog, b"AcroForm")?.as_dict().ok()?;
        let resources = get(doc, form, b"DR")?.as_dict().ok()?;
        get(doc, resources, b"Font")?.as_dict().ok()
    };
    fonts()
        .into_iter()
        .flat_map(Dictionary::iter)
        .filter_map(|(_, f)| f.as_reference().ok())
        .collect()
}

/// The two-byte codes each font (by object id) shows anywhere: pages, form XObjects, appearance streams,
/// patterns. `None` when some text's font can't be told (then two-byte fonts stay whole).
fn shown_codes(doc: &Document) -> Option<HashMap<ObjectId, HashSet<u16>>> {
    let mut codes = HashMap::new();
    for page_id in doc.get_pages().into_values() {
        let (inline, ids) = doc.get_page_resources(page_id).ok()?;
        let mut fonts = HashMap::new();
        for resources in inline
            .into_iter()
            .chain(ids.iter().filter_map(|id| doc.get_dictionary(*id).ok()))
        {
            for (name, font) in font_names(doc, resources) {
                fonts.entry(name).or_insert(font);
            }
        }
        collect(
            &doc.get_and_decode_page_content(page_id).ok()?,
            &fonts,
            &mut codes,
        )?;
    }
    for object in doc.objects.values() {
        let (dict, stream) = match object {
            Object::Stream(s) => (&s.dict, Some(s)),
            Object::Dictionary(d) => (d, None),
            _ => continue,
        };
        let subtype = dict.get(b"Subtype").and_then(Object::as_name).ok();
        // Type 3 glyphs are drawn with their font's resources: not followed.
        if subtype == Some(b"Type3") {
            return None;
        }
        let Some(stream) = stream else { continue };
        if subtype != Some(b"Form") && !dict.has(b"PatternType") && !dict.has(b"Resources") {
            continue;
        }
        let fonts = get(doc, dict, b"Resources")
            .and_then(|r| r.as_dict().ok())
            .map(|r| font_names(doc, r))
            .unwrap_or_default();
        let content = Content::decode(&stream_bytes(stream)?).ok()?;
        collect(&content, &fonts, &mut codes)?;
    }
    Some(codes)
}

/// A resources dictionary's fonts by name (`None` for a font that isn't an object of its own).
fn font_names(doc: &Document, resources: &Dictionary) -> HashMap<Vec<u8>, Option<ObjectId>> {
    get(doc, resources, b"Font")
        .and_then(|f| f.as_dict().ok())
        .into_iter()
        .flat_map(Dictionary::iter)
        .map(|(name, font)| (name.clone(), font.as_reference().ok()))
        .collect()
}

fn collect(
    content: &Content,
    fonts: &HashMap<Vec<u8>, Option<ObjectId>>,
    codes: &mut HashMap<ObjectId, HashSet<u16>>,
) -> Option<()> {
    // None: no font chosen yet (it would come from the caller); Some(None): a font without an id.
    let mut font: Option<Option<ObjectId>> = None;
    let mut saved = Vec::new();
    for op in &content.operations {
        let strings: Vec<&[u8]> = match op.operator.as_str() {
            "q" => {
                saved.push(font);
                continue;
            }
            "Q" => {
                font = saved.pop().unwrap_or(font);
                continue;
            }
            "Tf" => {
                let name = op.operands.first()?.as_name().ok()?;
                font = Some(*fonts.get(name)?);
                continue;
            }
            "Tj" | "'" | "\"" => op
                .operands
                .last()
                .and_then(|o| o.as_str().ok())
                .into_iter()
                .collect(),
            "TJ" => op
                .operands
                .first()
                .and_then(|o| o.as_array().ok())
                .into_iter()
                .flatten()
                .filter_map(|o| o.as_str().ok())
                .collect(),
            _ => continue,
        };
        if let Some(id) = font? {
            let set = codes.entry(id).or_default();
            for s in strings {
                set.extend(s.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)));
            }
        }
    }
    Some(())
}

/// The font's table `tag`.
fn table<'a>(font: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    tables(font)?
        .into_iter()
        .find(|(t, _)| t == tag)
        .map(|(_, data)| data)
}

fn tables(font: &[u8]) -> Option<Vec<([u8; 4], &[u8])>> {
    let u16_at = |i: usize| Some(u16::from_be_bytes(font.get(i..i + 2)?.try_into().ok()?));
    let u32_at = |i: usize| Some(u32::from_be_bytes(font.get(i..i + 4)?.try_into().ok()?));
    if !matches!(font.get(0..4)?, [0, 1, 0, 0] | b"true") {
        return None;
    }
    (0..u16_at(4)? as usize)
        .map(|i| {
            let record = 12 + 16 * i;
            let (offset, length) = (u32_at(record + 8)? as usize, u32_at(record + 12)? as usize);
            Some((
                font.get(record..record + 4)?.try_into().ok()?,
                font.get(offset..offset.checked_add(length)?)?,
            ))
        })
        .collect()
}

/// `font` with only `keep`'s glyph outlines (plus .notdef and the parts of composite glyphs), glyph numbers
/// unchanged; `None` when it can't be read or isn't smaller.
fn trim_truetype(font: &[u8], keep: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let tables = tables(font)?;
    let find = |tag: &[u8; 4]| tables.iter().find(|(t, _)| t == tag).map(|(_, d)| *d);
    let (head, maxp, loca, glyf) = (
        find(b"head")?,
        find(b"maxp")?,
        find(b"loca")?,
        find(b"glyf")?,
    );
    let count = u16::from_be_bytes(maxp.get(4..6)?.try_into().ok()?) as usize;
    let long = i16::from_be_bytes(head.get(50..52)?.try_into().ok()?) == 1;
    let offsets: Vec<usize> = (0..=count)
        .map(|g| match long {
            true => Some(u32::from_be_bytes(loca.get(g * 4..g * 4 + 4)?.try_into().ok()?) as usize),
            false => {
                Some(u16::from_be_bytes(loca.get(g * 2..g * 2 + 2)?.try_into().ok()?) as usize * 2)
            }
        })
        .collect::<Option<_>>()?;
    let glyph = |g: usize| glyf.get(offsets[g]..offsets[g + 1].max(offsets[g]));

    // The glyphs to keep, with the parts of composite glyphs.
    let mut kept = vec![false; count];
    let mut todo: Vec<usize> = std::iter::once(0)
        .chain(keep.iter().map(|g| *g as usize))
        .collect();
    while let Some(g) = todo.pop() {
        if g >= count || std::mem::replace(&mut kept[g], true) {
            continue;
        }
        let data = glyph(g)?;
        if data.len() < 10 || i16::from_be_bytes([data[0], data[1]]) >= 0 {
            continue;
        }
        let mut at = 10;
        loop {
            let flags = u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?);
            todo.push(u16::from_be_bytes(data.get(at + 2..at + 4)?.try_into().ok()?) as usize);
            at += 4 + if flags & 0x1 != 0 { 4 } else { 2 };
            at += match flags {
                f if f & 0x8 != 0 => 2,
                f if f & 0x40 != 0 => 4,
                f if f & 0x80 != 0 => 8,
                _ => 0,
            };
            if flags & 0x20 == 0 {
                break;
            }
        }
    }

    let (mut new_glyf, mut new_loca) = (Vec::new(), Vec::new());
    for (g, keep) in kept.iter().enumerate() {
        new_loca.extend((new_glyf.len() as u32).to_be_bytes());
        if *keep {
            new_glyf.extend(glyph(g)?);
            new_glyf.resize(new_glyf.len().next_multiple_of(4), 0);
        }
    }
    new_loca.extend((new_glyf.len() as u32).to_be_bytes());
    let mut new_head = head.to_vec();
    new_head[8..12].fill(0);
    new_head[50..52].copy_from_slice(&1i16.to_be_bytes());

    let mut out_tables: Vec<([u8; 4], &[u8])> = tables
        .iter()
        .filter(|(t, _)| KEPT_TABLES.contains(&t))
        .map(|(t, d)| match t {
            b"glyf" => (*t, new_glyf.as_slice()),
            b"loca" => (*t, new_loca.as_slice()),
            b"head" => (*t, new_head.as_slice()),
            _ => (*t, *d),
        })
        .collect();
    out_tables.sort_by_key(|(t, _)| *t);
    let out = write_sfnt(&out_tables);
    (out.len() < font.len()).then_some(out)
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut word = [0; 4];
        word[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

fn write_sfnt(tables: &[([u8; 4], &[u8])]) -> Vec<u8> {
    let n = tables.len() as u16;
    let selector = 15 - n.leading_zeros() as u16;
    let range = (1u16 << selector) * 16;
    let mut out = vec![0, 1, 0, 0];
    for v in [n, range, selector, n * 16 - range] {
        out.extend(v.to_be_bytes());
    }
    let mut offset = 12 + 16 * tables.len();
    let mut head_at = None;
    for (tag, data) in tables {
        out.extend(tag);
        out.extend(checksum(data).to_be_bytes());
        out.extend((offset as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        if tag == b"head" {
            head_at = Some(offset);
        }
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in tables {
        out.extend(*data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    if let Some(at) = head_at {
        let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[at + 8..at + 12].copy_from_slice(&adjust.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    const FONT: &[u8] = include_bytes!("../../pdit-app/assets/fonts/NotoSans-Regular.ttf");

    fn gid(font: &[u8], ch: char) -> u16 {
        let cmap = ReadScope::new(table(font, b"cmap").unwrap())
            .read::<Cmap<'_>>()
            .unwrap();
        let record = cmap.find_subtable(
            allsorts::tables::cmap::PlatformId(3),
            allsorts::tables::cmap::EncodingId(1),
        );
        let sub = cmap
            .scope
            .offset(record.unwrap().offset as usize)
            .read::<CmapSubtable<'_>>()
            .unwrap();
        sub.map_glyph(ch as u32).unwrap().unwrap()
    }

    fn outline(font: &[u8], g: u16) -> usize {
        let (head, loca) = (table(font, b"head").unwrap(), table(font, b"loca").unwrap());
        let long = i16::from_be_bytes([head[50], head[51]]) == 1;
        let at = |i: usize| match long {
            true => u32::from_be_bytes(loca[i * 4..i * 4 + 4].try_into().unwrap()) as usize,
            false => u16::from_be_bytes([loca[i * 2], loca[i * 2 + 1]]) as usize * 2,
        };
        at(g as usize + 1) - at(g as usize)
    }

    /// A page showing "Hi" with the font twice: as a simple WinAnsi font and as a two-byte Identity-H font.
    fn pdf() -> (Document, ObjectId, ObjectId) {
        let mut doc = Document::with_version("1.7");
        let simple_file = doc.add_object(Stream::new(
            dictionary! { "Length1" => FONT.len() as i64 },
            FONT.to_vec(),
        ));
        let cid_file = doc.add_object(Stream::new(
            dictionary! { "Length1" => FONT.len() as i64 },
            FONT.to_vec(),
        ));
        let descriptor = |file| {
            dictionary! { "Type" => "FontDescriptor", "FontName" => "NotoSans", "Flags" => 32,
            "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 1000.into()], "ItalicAngle" => 0, "Ascent" => 1000,
            "Descent" => 0, "CapHeight" => 700, "StemV" => 80, "FontFile2" => file }
        };
        let simple = doc.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "NotoSans",
            "Encoding" => "WinAnsiEncoding", "FontDescriptor" => descriptor(simple_file) },
        );
        let cid = doc.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "NotoSans",
            "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"),
                "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
            "FontDescriptor" => descriptor(cid_file), "CIDToGIDMap" => "Identity" },
        );
        let type0 = doc.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "NotoSans",
            "Encoding" => "Identity-H", "DescendantFonts" => vec![cid.into()] },
        );
        let shown: Vec<u8> = "Hi"
            .chars()
            .flat_map(|c| gid(FONT, c).to_be_bytes())
            .collect();
        let content = format!(
            "BT /F1 12 Tf 72 720 Td (Hi) Tj /F2 12 Tf 0 -20 Td <{}> Tj ET",
            shown.iter().map(|b| format!("{b:02X}")).collect::<String>()
        );
        let content = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let pages = doc.new_object_id();
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => simple, "F2" => type0 } } });
        doc.objects.insert(
            pages,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
            ),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        (doc, simple_file, cid_file)
    }

    #[test]
    fn fonts_keep_the_glyphs_they_can_show() {
        let (mut doc, simple_file, cid_file) = pdf();
        trim_fonts(&mut doc);
        let file = |id| {
            doc.get_object(id)
                .unwrap()
                .as_stream()
                .unwrap()
                .content
                .clone()
        };
        let (simple, cid) = (file(simple_file), file(cid_file));
        assert!(
            simple.len() < FONT.len() / 2 && cid.len() < simple.len(),
            "{} {}",
            simple.len(),
            cid.len()
        );
        // The same glyph numbers; outlines only where they can be shown.
        for font in [&simple, &cid] {
            assert_eq!(gid(font, 'H'), gid(FONT, 'H'));
            assert!(outline(font, gid(font, 'H')) > 0);
            assert_eq!(outline(font, gid(FONT, 'Ж')), 0);
            ReadScope::new(font.as_slice())
                .read::<allsorts::font_data::FontData<'_>>()
                .unwrap();
        }
        assert!(outline(&simple, gid(FONT, 'é')) > 0);
        assert_eq!(outline(&cid, gid(FONT, 'é')), 0);
    }

    #[test]
    fn a_two_byte_font_of_an_unknown_text_stays_whole() {
        let (mut doc, _, cid_file) = pdf();
        // A form XObject drawing text without choosing a font: whose font it is can't be told.
        doc.add_object(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form" },
            b"BT (x) Tj ET".to_vec(),
        ));
        trim_fonts(&mut doc);
        assert_eq!(
            doc.get_object(cid_file)
                .unwrap()
                .as_stream()
                .unwrap()
                .content
                .len(),
            FONT.len()
        );
    }
}
