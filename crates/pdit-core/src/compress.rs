//! Compress PDF without MuPDF (the online version; D-069 is MuPDF's, on the desktop): pictures shown sharper than
//! `dpi` are made smaller and stored as JPEG, embedded TrueType fonts keep only the glyphs they can show
//! (`font_trim`, at every level), and the file is packed (object streams). Works on a copy: the open
//! document doesn't change. Pictures with transparency, masks, CMYK or uncommon formats are left as they are.

use crate::Error;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::HashMap;

/// A picture is made smaller only when it is at least this much sharper than asked for.
const MARGIN: f32 = 1.25;
/// Decoded pictures larger than this are left alone (a bomb guard).
const MAX_PIXELS: usize = 60_000_000;

/// Compresses `pdf`. `dpi`: `None` keeps every picture (the file is only packed); `Some(dpi)` makes pictures at
/// most that sharp where they are shown, as JPEG at `quality` (1–100).
pub fn compress_pdf(pdf: &[u8], dpi: Option<f32>, quality: u8) -> Result<Vec<u8>, Error> {
    let mut doc = Document::load_mem(pdf)
        .map_err(|e| Error::Pdfium(format!("could not read the PDF: {e}")))?;
    if doc.is_encrypted() {
        return Err(Error::Pdfium("the PDF is password-protected".into()));
    }
    if let Some(dpi) = dpi {
        for (id, (w_pt, h_pt)) in shown_sizes(&doc) {
            if let Some(stream) = smaller_picture(&doc, id, w_pt, h_pt, dpi, quality) {
                doc.objects.insert(id, Object::Stream(stream));
            }
        }
    }
    crate::font_trim::trim_fonts(&mut doc);
    doc.prune_objects();
    doc.compress();
    let mut out = Vec::new();
    doc.save_modern(&mut out)
        .map_err(|e| Error::Pdfium(format!("could not write the PDF: {e}")))?;
    Ok(out)
}

/// The largest size (points) each picture is drawn at on any page, from the pages' drawing commands. Pictures
/// drawn only inside form XObjects aren't found (they stay as they are).
// ponytail: page content only; follow Form XObjects too if PDFs with nested pictures stay big.
fn shown_sizes(doc: &Document) -> HashMap<ObjectId, (f32, f32)> {
    let mut sizes: HashMap<ObjectId, (f32, f32)> = HashMap::new();
    for page_id in doc.get_pages().into_values() {
        let names = page_images(doc, page_id);
        let Ok(content) = doc.get_and_decode_page_content(page_id) else {
            continue;
        };
        let mut ctm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0f32];
        let mut stack = Vec::new();
        for op in content.operations {
            match op.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => ctm = stack.pop().unwrap_or(ctm),
                "cm" if op.operands.len() == 6 => {
                    let m: Vec<f32> = op
                        .operands
                        .iter()
                        .map(|o| o.as_float().unwrap_or(0.0))
                        .collect();
                    ctm = multiply([m[0], m[1], m[2], m[3], m[4], m[5]], ctm);
                }
                "Do" => {
                    let name = op.operands.first().and_then(|o| o.as_name().ok());
                    if let Some(id) = name.and_then(|n| names.get(n)) {
                        let shown = (ctm[0].hypot(ctm[1]), ctm[2].hypot(ctm[3]));
                        let size = sizes.entry(*id).or_insert((0.0, 0.0));
                        size.0 = size.0.max(shown.0);
                        size.1 = size.1.max(shown.1);
                    }
                }
                _ => {}
            }
        }
    }
    sizes
}

/// `m` then `ctm` (PDF's row-vector order).
fn multiply(m: [f32; 6], ctm: [f32; 6]) -> [f32; 6] {
    [
        m[0] * ctm[0] + m[1] * ctm[2],
        m[0] * ctm[1] + m[1] * ctm[3],
        m[2] * ctm[0] + m[3] * ctm[2],
        m[2] * ctm[1] + m[3] * ctm[3],
        m[4] * ctm[0] + m[5] * ctm[2] + ctm[4],
        m[4] * ctm[1] + m[5] * ctm[3] + ctm[5],
    ]
}

/// The page's picture XObjects by resource name.
fn page_images(doc: &Document, page_id: ObjectId) -> HashMap<Vec<u8>, ObjectId> {
    let mut names = HashMap::new();
    let Ok((inline, ids)) = doc.get_page_resources(page_id) else {
        return names;
    };
    let dicts = inline
        .into_iter()
        .chain(ids.iter().filter_map(|id| doc.get_dictionary(*id).ok()));
    for resources in dicts {
        let xobjects = match resources.get(b"XObject") {
            Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok(),
            Ok(Object::Dictionary(d)) => Some(d),
            _ => None,
        };
        for (name, value) in xobjects.into_iter().flat_map(Dictionary::iter) {
            if let Ok(id) = value.as_reference()
                && is_image(doc, id)
            {
                names.entry(name.clone()).or_insert(id);
            }
        }
    }
    names
}

fn is_image(doc: &Document, id: ObjectId) -> bool {
    doc.get_object(id)
        .and_then(Object::as_stream)
        .is_ok_and(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image"))
}

/// The picture `id` made at most `dpi` sharp where shown (`w_pt` × `h_pt`) as a JPEG stream, when that is smaller
/// than it is now; `None` leaves it as it is.
fn smaller_picture(
    doc: &Document,
    id: ObjectId,
    w_pt: f32,
    h_pt: f32,
    dpi: f32,
    quality: u8,
) -> Option<lopdf::Stream> {
    let stream = doc.get_object(id).ok()?.as_stream().ok()?;
    let dict = &stream.dict;
    let int = |k: &[u8]| dict.get(k).ok().and_then(|o| o.as_i64().ok());
    let (width, height) = (int(b"Width")? as u32, int(b"Height")? as u32);
    // Left as they are: transparency, masks, decode arrays, non-8-bit, uncommon colours or filters.
    let skip = [b"SMask".as_slice(), b"Mask", b"Decode", b"ImageMask"];
    if skip.iter().any(|k| dict.has(k)) || int(b"BitsPerComponent").is_some_and(|b| b != 8) {
        return None;
    }
    if (width as usize) * (height as usize) > MAX_PIXELS || w_pt <= 0.0 || h_pt <= 0.0 {
        return None;
    }
    let target = (
        (w_pt / 72.0 * dpi).round().max(1.0) as u32,
        (h_pt / 72.0 * dpi).round().max(1.0) as u32,
    );
    let shrink =
        width as f32 > target.0 as f32 * MARGIN && height as f32 > target.1 as f32 * MARGIN;
    let filter = filter(dict)?;
    if !shrink && filter == "DCTDecode" {
        return None;
    }
    let channels = channels(doc, dict)?;
    let picture = match filter.as_str() {
        "DCTDecode" if channels != 4 => {
            image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg).ok()?
        }
        "FlateDecode" | "" => {
            let raw = match filter.as_str() {
                "" => stream.content.clone(),
                _ => stream
                    .decompressed_content_with_limit(width as usize * height as usize * 4)
                    .ok()?,
            };
            let pixels = width as usize * height as usize;
            match channels {
                1 if raw.len() == pixels => {
                    image::GrayImage::from_raw(width, height, raw).map(image::DynamicImage::from)?
                }
                3 if raw.len() == pixels * 3 => {
                    image::RgbImage::from_raw(width, height, raw).map(image::DynamicImage::from)?
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let picture = match shrink {
        true => picture.resize_exact(target.0, target.1, image::imageops::FilterType::Triangle),
        false => picture,
    };
    let gray = channels == 1;
    let mut jpeg = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality);
    match gray {
        true => encoder.encode_image(&picture.to_luma8()).ok()?,
        false => encoder.encode_image(&picture.to_rgb8()).ok()?,
    }
    if jpeg.len() >= stream.content.len() {
        return None;
    }
    let mut new = dict.clone();
    new.set("Filter", Object::Name(b"DCTDecode".to_vec()));
    new.set("Width", picture.width() as i64);
    new.set("Height", picture.height() as i64);
    new.set("BitsPerComponent", 8);
    new.set(
        "ColorSpace",
        Object::Name(if gray {
            b"DeviceGray".to_vec()
        } else {
            b"DeviceRGB".to_vec()
        }),
    );
    new.remove(b"DecodeParms");
    new.set("Length", jpeg.len() as i64);
    // Already JPEG: not to be packed again.
    Some(lopdf::Stream::new(new, jpeg).with_compression(false))
}

/// The picture's one filter ("" for none); `None` for a chain of several.
fn filter(dict: &Dictionary) -> Option<String> {
    let name = |o: &Object| {
        o.as_name()
            .ok()
            .map(|n| String::from_utf8_lossy(n).into_owned())
    };
    match dict.get(b"Filter") {
        Err(_) => Some(String::new()),
        Ok(Object::Array(a)) if a.len() == 1 => name(&a[0]),
        Ok(Object::Array(_)) => None,
        Ok(o) => name(o),
    }
}

/// Colour channels of the picture's colour space: 1 gray, 3 RGB, 4 CMYK; `None` for others (indexed, Lab…).
fn channels(doc: &Document, dict: &Dictionary) -> Option<u8> {
    let resolve = |o: &Object| match o {
        Object::Reference(id) => doc.get_object(*id).ok().cloned(),
        o => Some(o.clone()),
    };
    let space = resolve(dict.get(b"ColorSpace").ok()?)?;
    match &space {
        Object::Name(n) => match n.as_slice() {
            b"DeviceGray" => Some(1),
            b"DeviceRGB" => Some(3),
            b"DeviceCMYK" => Some(4),
            _ => None,
        },
        // [/ICCBased stream]: its N.
        Object::Array(a) if a.first().and_then(|o| o.as_name().ok()) == Some(b"ICCBased") => {
            let profile = resolve(a.get(1)?)?;
            let n = profile
                .as_stream()
                .ok()?
                .dict
                .get(b"N")
                .ok()?
                .as_i64()
                .ok()?;
            matches!(n, 1 | 3 | 4).then_some(n as u8)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::{Content, Operation};
    use lopdf::dictionary;

    /// A one-page PDF showing a `px`×`px` RGB picture (flate) at 2 × 2 inches.
    fn pdf_with_picture(px: u32) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pixels: Vec<u8> = (0..px * px)
            .flat_map(|i| [(i % 251) as u8, (i / px % 241) as u8, 128])
            .collect();
        let mut image = lopdf::Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => px as i64, "Height" => px as i64,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8 },
            pixels,
        );
        image.compress().unwrap();
        let image_id = doc.add_object(image);
        let content = Content {
            operations: vec![
                Operation::new("q", vec![]),
                Operation::new(
                    "cm",
                    vec![
                        144.into(),
                        0.into(),
                        0.into(),
                        144.into(),
                        72.into(),
                        500.into(),
                    ],
                ),
                Operation::new("Do", vec![Object::Name(b"Im1".to_vec())]),
                Operation::new("Q", vec![]),
            ],
        };
        let content_id = doc.add_object(lopdf::Stream::new(
            dictionary! {},
            content.encode().unwrap(),
        ));
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(
            dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im1" => image_id } } },
        );
        doc.objects.insert(
            pages_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 },
            ),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    fn picture_width(pdf: &[u8]) -> (i64, String) {
        let doc = Document::load_mem(pdf).unwrap();
        let (_, stream) = doc
            .objects
            .iter()
            .find_map(|(id, o)| {
                o.as_stream()
                    .ok()
                    .filter(|_| is_image(&doc, *id))
                    .map(|s| (id, s))
            })
            .unwrap();
        (
            stream.dict.get(b"Width").unwrap().as_i64().unwrap(),
            filter(&stream.dict).unwrap(),
        )
    }

    #[test]
    fn a_sharp_picture_shrinks_to_the_asked_dpi() {
        let pdf = pdf_with_picture(1200); // 600 dpi at 2 inches
        let medium = compress_pdf(&pdf, Some(150.0), 75).unwrap();
        assert_eq!(picture_width(&medium), (300, "DCTDecode".into()));
        assert!(
            medium.len() < pdf.len() / 4,
            "{} → {}",
            pdf.len(),
            medium.len()
        );
        // Best quality keeps the picture.
        let best = compress_pdf(&pdf, None, 75).unwrap();
        assert_eq!(picture_width(&best), (1200, "FlateDecode".into()));
    }

    #[test]
    fn a_picture_already_small_enough_keeps_its_size() {
        let pdf = pdf_with_picture(280); // 140 dpi at 2 inches
        let out = compress_pdf(&pdf, Some(150.0), 75).unwrap();
        assert_eq!(picture_width(&out).0, 280);
    }
}
