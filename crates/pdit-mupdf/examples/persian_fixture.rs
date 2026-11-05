//! Writes fixtures/synthetic-persian.pdf: made-up Persian lines on a blank page, in Vazirmatn through MuPDF's Story.
//! `cargo run -p pdit-mupdf --example persian_fixture -- fixtures/synthetic-persian.pdf`
const BLANK: &[u8] = b"%PDF-1.7\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>endobj\ntrailer<</Root 1 0 R>>\n";
const VAZIRMATN: &[u8] = include_bytes!("../../../desktop/fonts/vazirmatn/Vazirmatn-Regular.ttf");

fn main() {
    let out = std::env::args().nth(1).expect("the output file");
    let mut editor = pdit_mupdf::Editor::open(BLANK).expect("the blank page");
    let fonts = [pdit_mupdf::FallbackFont { data: VAZIRMATN }];
    let text = "این یک متن آزمایشی است و هیچ اطلاعات شخصی ندارد\nنام ساختگی: باغ سیب\nشماره ساختگی ۱۲۳۴۵ برای آزمایش است";
    editor
        .add_text(0, 72.0, 90.0, 460.0, text, 16.0, [0, 0, 0], &fonts)
        .expect("the text");
    std::fs::write(out, editor.save().expect("the file")).expect("written");
}
