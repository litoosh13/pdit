//! From images (D-071): pictures become a new PDF, one per page. A JPEG photo goes in as it is (DCTDecode: no
//! re-encoding, no growth); other pictures (PNG, a CMYK JPEG) are stored losslessly (FlateDecode), transparency on
//! white. Written with lopdf, so it works in the browser and the desktop app alike.

use crate::Error;
use lopdf::{Dictionary, Document, Object, Stream, dictionary};
use std::io::Write;

/// The page each picture gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSize {
    /// The picture's size at 96 dpi.
    Picture,
    /// A4 or US Letter, turned to the picture's orientation; the picture fitted inside a margin.
    A4,
    Letter,
}

/// Pixels per inch for "the picture's own size".
const OWN_DPI: f32 = 96.0;
/// Room around a picture on an A4 / Letter page (points).
const MARGIN_PT: f32 = 24.0;

fn err(e: impl std::fmt::Display) -> Error {
    Error::Pdfium(format!("could not make the PDF: {e}"))
}

/// A new PDF with each of `images` (PNG / JPEG bytes) on its own page, in order.
pub fn pdf_from_images(images: &[Vec<u8>], size: PageSize) -> Result<Vec<u8>, Error> {
    if images.is_empty() {
        return Err(err("no pictures"));
    }
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for bytes in images {
        let (stream, w, h) = image_stream(bytes)?;
        let image_id = doc.add_object(stream);
        let (pw, ph, x, y, iw, ih) = place(w as f32, h as f32, size);
        let content = format!("q {iw:.3} 0 0 {ih:.3} {x:.3} {y:.3} cm /Im0 Do Q");
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), Object::Real(pw), Object::Real(ph)],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
            "Contents" => content_id,
        });
        kids.push(Object::Reference(page_id));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(err)?;
    Ok(out)
}

/// Page width/height, then where the picture goes (x, y, width, height), all in points.
fn place(w_px: f32, h_px: f32, size: PageSize) -> (f32, f32, f32, f32, f32, f32) {
    let (w, h) = (w_px * 72.0 / OWN_DPI, h_px * 72.0 / OWN_DPI);
    let (short, long) = match size {
        PageSize::Picture => return (w, h, 0.0, 0.0, w, h),
        PageSize::A4 => (595.0, 842.0),
        PageSize::Letter => (612.0, 792.0),
    };
    let (pw, ph) = if w > h { (long, short) } else { (short, long) };
    let scale = ((pw - 2.0 * MARGIN_PT) / w).min((ph - 2.0 * MARGIN_PT) / h);
    let (iw, ih) = (w * scale, h * scale);
    (pw, ph, (pw - iw) / 2.0, (ph - ih) / 2.0, iw, ih)
}

/// The picture as a PDF image XObject, and its pixel size.
fn image_stream(bytes: &[u8]) -> Result<(Stream, u32, u32), Error> {
    if let Some((w, h, components)) = jpeg_header(bytes)
        && (components == 1 || components == 3)
    {
        let space = if components == 1 {
            "DeviceGray"
        } else {
            "DeviceRGB"
        };
        let dict = dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
            "ColorSpace" => space, "BitsPerComponent" => 8, "Filter" => "DCTDecode",
        };
        return Ok((
            Stream::new(dict, bytes.to_vec()).with_compression(false),
            w,
            h,
        ));
    }
    let img = image::load_from_memory(bytes)
        .map_err(|e| Error::Pdfium(format!("could not read a picture: {e}")))?;
    let (w, h) = (img.width(), img.height());
    let (space, raw) = if img.color().has_color() {
        // Transparency goes onto white paper.
        let rgba = img.to_rgba8();
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for p in rgba.pixels() {
            let a = u16::from(p[3]);
            for c in 0..3 {
                rgb.push(((u16::from(p[c]) * a + 255 * (255 - a)) / 255) as u8);
            }
        }
        ("DeviceRGB", rgb)
    } else {
        let la = img.to_luma_alpha8();
        let gray = la
            .pixels()
            .map(|p| {
                ((u16::from(p[0]) * u16::from(p[1]) + 255 * (255 - u16::from(p[1]))) / 255) as u8
            })
            .collect();
        ("DeviceGray", gray)
    };
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).map_err(err)?;
    let data = z.finish().map_err(err)?;
    let dict = dictionary! {
        "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
        "ColorSpace" => space, "BitsPerComponent" => 8, "Filter" => "FlateDecode",
    };
    Ok((Stream::new(dict, data).with_compression(false), w, h))
}

/// A baseline/progressive JPEG's width, height and colour components, from its frame header.
fn jpeg_header(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        let marker = bytes[i + 1];
        let len = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
        // SOF0–SOF15, except DHT (C4), JPG (C8) and DAC (CC).
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            let h = u32::from(u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]));
            let w = u32::from(u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]));
            return Some((w, h, bytes[i + 9]));
        }
        i += 2 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb, Rgba};

    fn encode(img: image::DynamicImage, format: image::ImageFormat) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    #[test]
    fn pictures_become_pages() {
        let photo = encode(
            ImageBuffer::from_fn(480, 320, |x, y| {
                Rgb([(x % 256) as u8, (y % 256) as u8, 128])
            })
            .into(),
            image::ImageFormat::Jpeg,
        );
        let drawing = encode(
            ImageBuffer::from_fn(200, 300, |x, _| {
                Rgba::<u8>([0, 0, 0, if x < 100 { 255 } else { 0 }])
            })
            .into(),
            image::ImageFormat::Png,
        );
        assert_eq!(jpeg_header(&photo), Some((480, 320, 3)));

        let pdf = pdf_from_images(&[photo.clone(), drawing], PageSize::A4).unwrap();
        let doc = Document::load_mem(&pdf).unwrap();
        let pages = doc.get_pages();
        assert_eq!(pages.len(), 2);
        let media = |n: u32| -> Vec<f32> {
            let page = doc.get_dictionary(pages[&n]).unwrap();
            page.get(b"MediaBox")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o.as_float().unwrap())
                .collect()
        };
        // A landscape photo turns the A4 page; a portrait drawing doesn't.
        assert_eq!(media(1), [0.0, 0.0, 842.0, 595.0]);
        assert_eq!(media(2), [0.0, 0.0, 595.0, 842.0]);
        // The JPEG went in untouched; the PNG is Flate, on white where it was transparent.
        let streams: Vec<&Stream> = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter(|s| {
                s.dict
                    .get(b"Subtype")
                    .is_ok_and(|v| v.as_name().is_ok_and(|n| n == b"Image"))
            })
            .collect();
        assert!(
            streams.iter().any(|s| s.content == photo),
            "the JPEG bytes as they were"
        );
        let png = streams
            .iter()
            .find(|s| s.dict.get(b"Filter").unwrap().as_name().unwrap() == b"FlateDecode")
            .unwrap();
        let raw = png.decompressed_content().unwrap();
        assert_eq!(raw.len(), 200 * 300 * 3, "RGB: three bytes a pixel");
        assert_eq!(
            (raw[0], raw[150 * 3]),
            (0, 255),
            "black, then white where it was transparent"
        );

        // Its own size: pixels at 96 dpi.
        let own =
            Document::load_mem(&pdf_from_images(&[photo], PageSize::Picture).unwrap()).unwrap();
        let page = own.get_dictionary(own.get_pages()[&1]).unwrap();
        let w = page.get(b"MediaBox").unwrap().as_array().unwrap()[2]
            .as_float()
            .unwrap();
        assert!((w - 360.0).abs() < 0.01, "{w}");
        assert!(pdf_from_images(&[], PageSize::A4).is_err());
    }
}
