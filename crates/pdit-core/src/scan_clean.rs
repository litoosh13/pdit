//! Clean scan (D-068): a scanned or photographed page becomes white paper with
//! dark text — the paper's light is estimated and divided out (shadows, tint and
//! uneven light go), then levels make the paper white and the ink dark. A photo
//! with the table around the page is cut out and straightened first, when the
//! page has no text layer yet (an OCR layer would no longer line up).
//! Tested against DocRes on synthetic pages: RESEARCH.md "Test 2026-10-09".

#[cfg(target_arch = "wasm32")]
use crate::{Error, render::with_open};
use image::{RgbImage, imageops};
#[cfg(target_arch = "wasm32")]
use pdfium_render::prelude::*;

#[cfg(target_arch = "wasm32")]
/// A page whose largest image covers this much of it is a scan.
const SCAN_SHARE: f32 = 0.7;
#[cfg(target_arch = "wasm32")]
/// Fewer characters than this: no text layer yet (as analysis.rs).
const MIN_CHARS: usize = 20;
/// Levels after dividing out the paper: at or above `WHITE` is paper, at or below `BLACK` is ink.
const BLACK: f32 = 0.35;
const WHITE: f32 = 0.90;
const GAMMA: f32 = 1.3;

#[cfg(target_arch = "wasm32")]
/// Whether `page` is mainly one picture (a scan or a photo of a page).
pub fn is_scan_page(page: u16) -> Result<bool, Error> {
    with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        Ok(largest_image(&pdf_page)?.is_some())
    })
}

#[cfg(target_arch = "wasm32")]
/// Cleans the scan on `page`; false when the page isn't one.
pub fn clean_scan(page: u16) -> Result<bool, Error> {
    crate::edit_open::discard_edit()?;
    with_open(|document| {
        let mut pdf_page = document.pages().get(page.into())?;
        let Some(index) = largest_image(&pdf_page)? else {
            return Ok(false);
        };
        let has_text = pdf_page
            .text()?
            .all()
            .chars()
            .filter(|c| !c.is_whitespace())
            .count()
            >= MIN_CHARS;
        let mut object = pdf_page.objects_mut().get(index)?;
        let image = object
            .as_image_object_mut()
            .ok_or_else(|| Error::Pdfium("not an image".into()))?;
        let mut rgb = image.get_raw_image()?.to_rgb8();
        if !has_text && let Some(corners) = page_corners(&rgb) {
            rgb = straighten(&rgb, corners);
        }
        clean(&mut rgb);
        image.set_image(&image::DynamicImage::ImageRgb8(rgb))?;
        Ok(true)
    })
}

#[cfg(target_arch = "wasm32")]
/// The index of the image covering at least `SCAN_SHARE` of the page, if any.
fn largest_image(page: &PdfPage) -> Result<Option<usize>, Error> {
    let area = (page.width().value * page.height().value).max(1.0);
    let mut best = None;
    for (i, object) in page.objects().iter().enumerate() {
        if object.as_image_object().is_none() {
            continue;
        }
        let b = object.bounds()?;
        let share = b.width().value * b.height().value / area;
        if share >= SCAN_SHARE && best.is_none_or(|(_, s)| share > s) {
            best = Some((i, share));
        }
    }
    Ok(best.map(|(i, _)| i))
}

/// Divides out the paper's light, per channel (coloured ink keeps its colour), then levels.
fn clean(img: &mut RgbImage) {
    let (w, h) = img.dimensions();
    // The paper's light: shrink, take the brightest nearby pixel (text disappears), blur, grow back.
    let small = imageops::resize(
        img,
        (w / 4).max(1),
        (h / 4).max(1),
        imageops::FilterType::Triangle,
    );
    let small = imageops::blur(&max_filter(&small, 4), 6.0);
    let light = imageops::resize(&small, w, h, imageops::FilterType::Triangle);
    for (p, l) in img.pixels_mut().zip(light.pixels()) {
        for c in 0..3 {
            let ratio = (f32::from(p[c]) / f32::from(l[c]).max(1.0)).min(1.0);
            let v = ((ratio - BLACK) / (WHITE - BLACK))
                .clamp(0.0, 1.0)
                .powf(GAMMA);
            p[c] = (v * 255.0).round() as u8;
        }
    }
}

/// The brightest value within `r` pixels (a square), per channel: rows, then columns.
fn max_filter(img: &RgbImage, r: u32) -> RgbImage {
    let (w, h) = img.dimensions();
    let pass = |src: &RgbImage, horizontal: bool| {
        RgbImage::from_fn(w, h, |x, y| {
            let mut m = [0u8; 3];
            let (lo, hi) = if horizontal {
                (x.saturating_sub(r), (x + r).min(w - 1))
            } else {
                (y.saturating_sub(r), (y + r).min(h - 1))
            };
            for i in lo..=hi {
                let p = if horizontal {
                    src.get_pixel(i, y)
                } else {
                    src.get_pixel(x, i)
                };
                for c in 0..3 {
                    m[c] = m[c].max(p[c]);
                }
            }
            image::Rgb(m)
        })
    };
    pass(&pass(img, true), false)
}

/// A photographed page's corners (top-left, top-right, bottom-right, bottom-left), when the
/// image shows something around the page (a table): the largest bright region's extreme points.
fn page_corners(img: &RgbImage) -> Option<[(f32, f32); 4]> {
    const S: u32 = 8;
    let (w, h) = (img.width() / S, img.height() / S);
    if w < 16 || h < 16 {
        return None;
    }
    let grey = imageops::grayscale(&imageops::resize(img, w, h, imageops::FilterType::Triangle));
    let grey = imageops::blur(&grey, 1.5);
    let t = otsu(&grey);
    // The largest bright region (flood fill).
    let bright = |x: u32, y: u32| grey.get_pixel(x, y)[0] > t;
    let mut seen = vec![false; (w * h) as usize];
    let mut best: Vec<(u32, u32)> = Vec::new();
    for start in 0..w * h {
        let (sx, sy) = (start % w, start / w);
        if seen[start as usize] || !bright(sx, sy) {
            continue;
        }
        let mut region = Vec::new();
        let mut stack = vec![(sx, sy)];
        seen[start as usize] = true;
        while let Some((x, y)) = stack.pop() {
            region.push((x, y));
            let near = [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ];
            for (nx, ny) in near {
                if nx < w && ny < h && !seen[(ny * w + nx) as usize] && bright(nx, ny) {
                    seen[(ny * w + nx) as usize] = true;
                    stack.push((nx, ny));
                }
            }
        }
        if region.len() > best.len() {
            best = region;
        }
    }
    // Only a page with a clear frame around it: not the whole image, not a speck, and
    // (almost) not touching the image's edges — a flat scan's unshadowed part does.
    let share = best.len() as f32 / (w * h) as f32;
    let on_edge = best
        .iter()
        .filter(|&&(x, y)| x == 0 || y == 0 || x == w - 1 || y == h - 1)
        .count() as f32
        / (2 * (w + h)) as f32;
    if !(0.3..=0.9).contains(&share) || on_edge > 0.05 {
        return None;
    }
    let pick = |f: fn(f32, f32) -> f32, max: bool| {
        let key = |&&(x, y): &&(u32, u32)| (f(x as f32, y as f32) * 16.0) as i64;
        let p = if max {
            best.iter().max_by_key(key)
        } else {
            best.iter().min_by_key(key)
        };
        p.map(|&(x, y)| ((x * S) as f32, (y * S) as f32))
    };
    Some([
        pick(|x, y| x + y, false)?,
        pick(|x, y| x - y, true)?,
        pick(|x, y| x + y, true)?,
        pick(|x, y| x - y, false)?,
    ])
}

/// Otsu's threshold of a grey image.
fn otsu(grey: &image::GrayImage) -> u8 {
    let mut hist = [0u64; 256];
    for p in grey.pixels() {
        hist[usize::from(p[0])] += 1;
    }
    let total: u64 = hist.iter().sum();
    let sum: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &n)| i as f64 * n as f64)
        .sum();
    let (mut wb, mut sb, mut best, mut t) = (0u64, 0.0f64, 0.0f64, 0u8);
    for (i, &n) in hist.iter().enumerate() {
        wb += n;
        if wb == 0 || wb == total {
            continue;
        }
        sb += i as f64 * n as f64;
        let (mb, mf) = (sb / wb as f64, (sum - sb) / (total - wb) as f64);
        let between = wb as f64 * (total - wb) as f64 * (mb - mf).powi(2);
        if between > best {
            best = between;
            t = i as u8;
        }
    }
    t
}

/// The page between `corners` warped to fill an image the size of `img` (perspective, bilinear).
fn straighten(img: &RgbImage, corners: [(f32, f32); 4]) -> RgbImage {
    let (w, h) = img.dimensions();
    let target = [
        (0.0, 0.0),
        (w as f32, 0.0),
        (w as f32, h as f32),
        (0.0, h as f32),
    ];
    // Maps output points to the photo.
    let m = homography(target, corners);
    RgbImage::from_fn(w, h, |x, y| {
        let (u, v) = (x as f32 + 0.5, y as f32 + 0.5);
        let d = m[6] * u + m[7] * v + 1.0;
        let sx = (m[0] * u + m[1] * v + m[2]) / d - 0.5;
        let sy = (m[3] * u + m[4] * v + m[5]) / d - 0.5;
        bilinear(img, sx, sy)
    })
}

fn bilinear(img: &RgbImage, x: f32, y: f32) -> image::Rgb<u8> {
    let (w, h) = (img.width() as f32 - 1.0, img.height() as f32 - 1.0);
    let (x, y) = (x.clamp(0.0, w), y.clamp(0.0, h));
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |dx: f32, dy: f32| *img.get_pixel((x0 + dx).min(w) as u32, (y0 + dy).min(h) as u32);
    let (a, b, c, d) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0));
    image::Rgb(std::array::from_fn(|i| {
        let top = f32::from(a[i]) * (1.0 - fx) + f32::from(b[i]) * fx;
        let bottom = f32::from(c[i]) * (1.0 - fx) + f32::from(d[i]) * fx;
        (top * (1.0 - fy) + bottom * fy).round() as u8
    }))
}

/// The perspective transform taking `from[i]` to `to[i]` (8 unknowns, Gaussian elimination).
fn homography(from: [(f32, f32); 4], to: [(f32, f32); 4]) -> [f32; 8] {
    let mut a = [[0f64; 9]; 8];
    for i in 0..4 {
        let ((x, y), (u, v)) = (from[i], to[i]);
        let (x, y, u, v) = (f64::from(x), f64::from(y), f64::from(u), f64::from(v));
        a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for col in 0..8 {
        let pivot = (col..8)
            .max_by(|&r, &s| a[r][col].abs().total_cmp(&a[s][col].abs()))
            .unwrap_or(col);
        a.swap(col, pivot);
        for r in 0..8 {
            if r != col && a[col][col] != 0.0 {
                let (f, pivot_row) = (a[r][col] / a[col][col], a[col]);
                for (v, p) in a[r].iter_mut().zip(pivot_row).skip(col) {
                    *v -= f * p;
                }
            }
        }
    }
    std::array::from_fn(|i| (a[i][8] / a[i][i]) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grey, shadowed paper with dark "text" comes out white paper with dark text.
    #[test]
    fn clean_whitens_paper_and_keeps_text() {
        let mut img = RgbImage::from_fn(200, 200, |x, y| {
            // darker towards the lower right, as a soft shadow (a very steep one leaves the paper
            // slightly cream: the brightest-nearby estimate runs ahead of the slope)
            let light = 200 - (x + y) / 8;
            if (60..140).contains(&x) && (95..105).contains(&y) {
                image::Rgb([30, 30, 30])
            } else {
                image::Rgb([light as u8, (light - 10) as u8, (light - 30) as u8])
            }
        });
        clean(&mut img);
        assert!(
            img.get_pixel(10, 10).0.iter().all(|&c| c >= 250),
            "paper {:?}",
            img.get_pixel(10, 10)
        );
        assert!(
            img.get_pixel(190, 190).0.iter().all(|&c| c >= 250),
            "shadowed paper {:?}",
            img.get_pixel(190, 190)
        );
        assert!(
            img.get_pixel(100, 100).0.iter().all(|&c| c <= 40),
            "text {:?}",
            img.get_pixel(100, 100)
        );
    }

    /// A flat scan with a big shadow (the bright part touches the edges) isn't cut.
    #[test]
    fn shadowed_flat_scan_is_not_cut() {
        let img = RgbImage::from_fn(400, 560, |x, y| {
            let shade = (x as f32 / 400.0 - 0.5).max(0.0) * 2.0 * (1.0 - y as f32 / 560.0);
            let v = (230.0 - 150.0 * shade) as u8;
            image::Rgb([v, v, v - 20])
        });
        assert_eq!(page_corners(&img), None);
    }

    /// A bright page on a dark table: its corners are found and the cut-out is all paper.
    #[test]
    fn photographed_page_is_cut_out() {
        let page = [(80.0, 60.0), (420.0, 90.0), (440.0, 560.0), (50.0, 530.0)];
        let inside = |x: f32, y: f32| {
            (0..4).all(|i| {
                let ((ax, ay), (bx, by)) = (page[i], page[(i + 1) % 4]);
                (bx - ax) * (y - ay) - (by - ay) * (x - ax) >= 0.0
            })
        };
        let img = RgbImage::from_fn(500, 640, |x, y| {
            if inside(x as f32, y as f32) {
                image::Rgb([230, 225, 210])
            } else {
                image::Rgb([70, 50, 35])
            }
        });
        let corners = page_corners(&img).expect("corners");
        for (found, real) in corners.iter().zip(page) {
            assert!(
                (found.0 - real.0).abs() < 20.0 && (found.1 - real.1).abs() < 20.0,
                "{found:?} vs {real:?}"
            );
        }
        let flat = straighten(&img, corners);
        let dark = flat.pixels().filter(|p| p[0] < 150).count();
        assert!(dark * 100 < flat.pixels().len(), "table left: {dark} px");
    }
}
