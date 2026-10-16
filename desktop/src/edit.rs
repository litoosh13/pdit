//! Paragraph edits with MuPDF (D-065, step 2): the web app sends the document and the new text of the paragraph
//! under a point; pdit-mupdf replaces it (Story: re-wrapped in the PDF's own font, fallbacks Noto Sans and
//! Vazirmatn) and the new document goes back. MuPDF's context is per thread, so each call opens, edits and
//! saves in one go. Compress PDF (M-009, D-069): `compress_pdf`, MuPDF's cleaner at the chosen level.

use percent_encoding::percent_decode_str;
use tauri::ipc::{InvokeBody, Request, Response};

/// The new text's fallback fonts, in order: the web app's Noto Sans (Latin, Greek, Cyrillic), then Vazirmatn for
/// Persian and Arabic letters (fonts/vazirmatn/SOURCE.md).
const NOTO_SANS: &[u8] = include_bytes!("../../crates/pdit-app/assets/fonts/NotoSans-Regular.ttf");
const VAZIRMATN: &[u8] = include_bytes!("../fonts/vazirmatn/Vazirmatn-Regular.ttf");

/// Body: the PDF. Headers: x-page (from 0), x-x / x-y (a point in the paragraph, points from the page's
/// top-left), x-expected (the paragraph's text as the app read it) and x-text (the new text), both
/// percent-encoded. Fails with "different paragraph" when MuPDF's paragraph there isn't the app's, so the app
/// can edit its own way instead.
#[tauri::command]
pub async fn edit_paragraph(request: Request<'_>) -> Result<Response, String> {
    let InvokeBody::Raw(pdf) = request.body() else {
        return Err("expected the PDF as raw bytes".into());
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .ok_or(format!("missing {name}"))
    };
    let number = |name: &str| -> Result<f32, String> {
        header(name)?.parse().map_err(|_| format!("bad {name}"))
    };
    let text = |name: &str| -> Result<String, String> {
        Ok(percent_decode_str(header(name)?)
            .decode_utf8()
            .map_err(|_| format!("bad {name}"))?
            .into_owned())
    };
    let (page, x, y) = (number("x-page")? as i32, number("x-x")?, number("x-y")?);
    let (expected, new_text) = (text("x-expected")?, text("x-text")?);

    let mut editor = pdit_mupdf::Editor::open(pdf).map_err(|e| e.to_string())?;
    let paragraph = editor
        .paragraph_at(page, x, y)
        .map_err(|e| e.to_string())?
        .filter(|p| same_letters(&p.text, &expected))
        .ok_or("different paragraph")?;
    editor
        .replace_paragraph(
            &paragraph,
            &new_text,
            &[
                pdit_mupdf::FallbackFont { data: NOTO_SANS },
                pdit_mupdf::FallbackFont { data: VAZIRMATN },
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(Response::new(editor.save().map_err(|e| e.to_string())?))
}

/// A new text box with text the PDF's fonts can't shape (Persian, Arabic…; P-050 #16): body the PDF; headers
/// x-page, x-x, x-top (the box's top-left in MuPDF's page space), x-width, x-size (points), x-color ("r,g,b") and
/// x-text (percent-encoded). Returns the PDF with the text laid out by MuPDF in Noto Sans / Vazirmatn.
#[tauri::command]
pub async fn add_text(request: Request<'_>) -> Result<Response, String> {
    let InvokeBody::Raw(pdf) = request.body() else {
        return Err("expected the PDF as raw bytes".into());
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .ok_or(format!("missing {name}"))
    };
    let number = |name: &str| -> Result<f32, String> {
        header(name)?.parse().map_err(|_| format!("bad {name}"))
    };
    let text = percent_decode_str(header("x-text")?)
        .decode_utf8()
        .map_err(|_| "bad x-text")?
        .into_owned();
    let color: Vec<u8> = header("x-color")?
        .split(',')
        .filter_map(|c| c.trim().parse().ok())
        .collect();
    let color = [0, 1, 2].map(|i| color.get(i).copied().unwrap_or(0));
    let mut editor = pdit_mupdf::Editor::open(pdf).map_err(|e| e.to_string())?;
    editor
        .add_text(
            number("x-page")? as i32,
            number("x-x")?,
            number("x-top")?,
            number("x-width")?,
            &text,
            number("x-size")?,
            color,
            &[
                pdit_mupdf::FallbackFont { data: NOTO_SANS },
                pdit_mupdf::FallbackFont { data: VAZIRMATN },
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(Response::new(editor.save().map_err(|e| e.to_string())?))
}

/// The same letters and digits in the same order (spacing, punctuation and lost hyphens aside).
fn same_letters(a: &str, b: &str) -> bool {
    let key = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    key(a) == key(b)
}

#[cfg(test)]
mod tests {
    #[test]
    fn same_letters_ignores_spacing_and_lost_hyphens() {
        assert!(super::same_letters(
            "Texas A&M University-Corpus Christi.",
            "Texas A&M University Corpus  Christi"
        ));
        assert!(!super::same_letters(
            "Dear Yasin,",
            "April 23, 2025 Dear Yasin,"
        ));
    }
}

/// Body: the PDF; header x-level: 0 best quality, 1 medium, 2 smallest. Returns the compressed copy.
#[tauri::command]
pub async fn compress_pdf(request: Request<'_>) -> Result<Response, String> {
    let InvokeBody::Raw(pdf) = request.body() else {
        return Err("expected the PDF as raw bytes".into());
    };
    let level = match request.headers().get("x-level").and_then(|v| v.to_str().ok()) {
        Some("0") => pdit_mupdf::Level::BestQuality,
        Some("1") => pdit_mupdf::Level::Medium,
        Some("2") => pdit_mupdf::Level::Smallest,
        _ => return Err("bad x-level".into()),
    };
    let out = pdit_mupdf::compress(pdf, level).map_err(|e| e.to_string())?;
    Ok(Response::new(out))
}

/// Page numbers (D-070). Body: the PDF; header x-labels: percent-encoded ranges, one per line, each
/// "first page (from 0) TAB style code (D r R a A, or empty) TAB start TAB prefix"; none: plain 1, 2, 3.
#[tauri::command]
pub async fn set_page_labels(request: Request<'_>) -> Result<Response, String> {
    let InvokeBody::Raw(pdf) = request.body() else {
        return Err("expected the PDF as raw bytes".into());
    };
    let spec = request
        .headers()
        .get("x-labels")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| percent_decode_str(v).decode_utf8().ok())
        .ok_or("missing x-labels")?;
    let ranges = spec
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut f = line.splitn(4, '\t');
            let mut next = || f.next().ok_or(format!("bad range {line:?}"));
            let first = next()?.parse().map_err(|_| "bad first page")?;
            let style = next()?.bytes().next().unwrap_or(0);
            let start = next()?.parse().map_err(|_| "bad start")?;
            let prefix = next()?.to_owned();
            Ok(pdit_mupdf::LabelRange { first, style, prefix, start })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let out = pdit_mupdf::set_page_labels(pdf, &ranges).map_err(|e| e.to_string())?;
    Ok(Response::new(out))
}

/// Export as Word / OpenDocument (D-071). Body: the PDF; headers x-first / x-last (pages from 0) and x-format
/// ("docx" or "odt"). Returns the file.
#[tauri::command]
pub async fn to_office(request: Request<'_>) -> Result<Response, String> {
    let InvokeBody::Raw(pdf) = request.body() else {
        return Err("expected the PDF as raw bytes".into());
    };
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .ok_or(format!("missing {name}"))
    };
    let page = |name: &str| -> Result<i32, String> { header(name)?.parse().map_err(|_| format!("bad {name}")) };
    let odt = match header("x-format")? {
        "docx" => false,
        "odt" => true,
        _ => return Err("bad x-format".into()),
    };
    let out = pdit_mupdf::to_office(pdf, page("x-first")?, page("x-last")?, odt).map_err(|e| e.to_string())?;
    Ok(Response::new(out))
}
