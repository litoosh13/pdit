//! Paragraph edits with MuPDF (D-065, step 2): the web app sends the document and the new text of the paragraph
//! under a point; pdit-mupdf replaces it (Story: re-wrapped in the PDF's own font, fallbacks Noto Sans and
//! Vazirmatn) and the new document goes back. MuPDF's context is per thread, so each call opens, edits and
//! saves in one go.

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
