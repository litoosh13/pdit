//! PDF → Word / OpenDocument with pdit-mupdf (D-071), for checking files by hand:
//! `cargo run -p pdit-mupdf --example office -- <in.pdf> <out.docx|out.odt>`
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (input, output) = (&args[1], &args[2]);
    let pdf = std::fs::read(input).expect("the PDF");
    let pages = mupdf::Document::open(input.as_str())
        .and_then(|d| d.page_count())
        .expect("pages");
    let file =
        pdit_mupdf::to_office(&pdf, 0, pages - 1, output.ends_with(".odt")).expect("converted");
    std::fs::write(output, &file).expect("written");
    println!("{input} → {output}: {} bytes", file.len());
}
