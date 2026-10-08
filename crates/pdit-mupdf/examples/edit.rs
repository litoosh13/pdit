//! Try a paragraph edit by hand: `cargo run -p pdit-mupdf --example edit -- in.pdf PAGE X Y "old" "new" out.pdf
//! [extra-font.ttf]` (X, Y in points from the page's top-left). Writes out.pdf and out.png (page at 96 dpi).
use pdit_mupdf::{Editor, FallbackFont};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 8 {
        eprintln!("usage: edit in.pdf PAGE X Y old new out.pdf [extra-font.ttf]");
        std::process::exit(2);
    }
    let bytes = std::fs::read(&a[1]).unwrap();
    let (page, x, y): (i32, f32, f32) = (
        a[2].parse().unwrap(),
        a[3].parse().unwrap(),
        a[4].parse().unwrap(),
    );
    let noto: &[u8] = include_bytes!("../../pdit-app/assets/fonts/NotoSans-Regular.ttf");
    let extra = a.get(8).map(|p| std::fs::read(p).unwrap());
    let mut fallbacks = vec![FallbackFont { data: noto }];
    if let Some(extra) = extra.as_deref() {
        fallbacks.push(FallbackFont { data: extra });
    }
    let mut ed = Editor::open(&bytes).unwrap();
    let p = ed
        .paragraph_at(page, x, y)
        .unwrap()
        .expect("no paragraph there");
    println!(
        "paragraph: {} lines, font {:?} {} pt, pitch {:.2}",
        p.lines.len(),
        p.font,
        p.size,
        p.line_pitch
    );
    let new = if a[5].is_empty() {
        a[6].clone()
    } else {
        p.text.replacen(&a[5], &a[6], 1)
    };
    let t = std::time::Instant::now();
    ed.replace_paragraph(&p, &new, &fallbacks).unwrap();
    println!("replaced in {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    let out = ed.save().unwrap();
    std::fs::write(&a[7], &out).unwrap();
    let doc = mupdf::Document::from_bytes(&out, "pdf").unwrap();
    let pix = doc
        .load_page(page)
        .unwrap()
        .to_pixmap(
            &mupdf::Matrix::new_scale(96.0 / 72.0, 96.0 / 72.0),
            &mupdf::Colorspace::device_rgb(),
            false,
            true,
        )
        .unwrap();
    pix.save_as(&a[7].replace(".pdf", ".png"), mupdf::ImageFormat::PNG)
        .unwrap();
    println!(
        "reads back: {:?}",
        Editor::open(&out)
            .unwrap()
            .paragraph_at(page, x, y)
            .unwrap()
            .map(|p| p.text)
    );
}
