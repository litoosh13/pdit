//! Development-only check that the PDF engine works inside the app. Runs the two
//! synthetic fixtures through `replace_text_verified`, then an edit session
//! (preview / discard / keep) on the open-document API, and logs the outcome to
//! the browser console. Not compiled into release builds.

use crate::log;
use pdit_core::{EditMethod, Error, FontChoice, TextStyle, replace_text_verified};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

const RENTAL: &[u8] = include_bytes!("../../../fixtures/synthetic-rental.pdf");
const SUBSET: &[u8] = include_bytes!("../../../fixtures/synthetic-subset-font.pdf");
const IMAGE: &[u8] = include_bytes!("../../../fixtures/synthetic-image.png");
const WRAPPED: &[u8] = include_bytes!("../../../fixtures/synthetic-wrapped.pdf");
const FORM: &[u8] = include_bytes!("../../../fixtures/synthetic-form.pdf");
const SPLIT: &[u8] = include_bytes!("../../../fixtures/synthetic-split-lines.pdf");

pub async fn run(font_url: &str) {
    let font = match fetch_bytes(font_url).await {
        Ok(font) => font,
        Err(error) => {
            return log(&format!(
                "pdit self-check: could not load fallback font: {error:?}"
            ));
        }
    };
    let cases = [
        ("standard font", RENTAL, "890", "999", EditMethod::Native),
        (
            "subset font, new characters",
            SUBSET,
            "890",
            "750",
            EditMethod::FallbackFont,
        ),
    ];
    for (name, pdf, find, replacement, expected) in cases {
        let outcome = replace_text_verified(pdf.to_vec(), find, replacement, &font);
        let message = match outcome {
            Ok(result) => {
                let report = &result.reports[0];
                let ok = report.method == expected && report.final_text.contains(replacement);
                format!(
                    "pdit self-check {}: {name}: {:?}, reads {:?}, {} bytes",
                    if ok { "PASS" } else { "FAIL" },
                    report.method,
                    report.final_text,
                    result.bytes.len()
                )
            }
            Err(error) => format!("pdit self-check FAIL: {name}: {error}"),
        };
        log(&message);
    }
    edit_session(&font);
    style_session(&font);
    paragraph_session(&font);
    form_session();
    shape_session();
    table_session(&font);
    signature_session();
    annotation_session();
    annotation2_session();
    doc_content_session();
    links_session(&font);
    bookmarks_session(&font);
    form_edit_session();
    menus_session(&font);
    find_session(&font);
    print_session();
    save_names();
    bundled_fonts().await;
    look_alike_session(&font).await;
    fields_session().await;
    scan_session().await;
    ask_session().await;
    page_session();
    expose_page_ops();
}

/// Paragraph grouping + reflow (D-035 later item): on the synthetic wrapped
/// fixture, clicking a line of a wrapped paragraph groups its whole block, a
/// short standalone line stays on its own, and editing a block re-wraps the new
/// text across lines at the block's column width.
fn paragraph_session(font: &[u8]) {
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: paragraph: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if let Err(error) = pdit_core::open_document(WRAPPED.to_vec()) {
        return check("open", false, error.to_string());
    }
    // Paragraph A: three wrapped lines (baselines 740/724/708), ends "period."
    let a = pdit_core::paragraph_at(0, 100.0, 743.0);
    check(
        "wrapped block groups its 3 lines",
        matches!(&a, Ok(Some(p)) if p.object_indices.len() == 3
            && p.text.starts_with("This wrapped clause")
            && p.text.trim_end().ends_with("period.")),
        format!("{a:?}"),
    );
    // Standalone short line (baseline 672): its own block, not joined to A or C.
    let b = pdit_core::paragraph_at(0, 100.0, 675.0);
    check(
        "standalone line stays alone",
        matches!(&b, Ok(Some(p)) if p.object_indices.len() == 1
            && p.text == "A separate short note stands alone."),
        format!("{b:?}"),
    );
    // Paragraph C: two wrapped lines (baselines 636/620).
    let c = pdit_core::paragraph_at(0, 100.0, 639.0);
    check(
        "second wrapped block groups its 2 lines",
        matches!(&c, Ok(Some(p)) if p.object_indices.len() == 2),
        format!("{c:?}"),
    );

    // Reflow A to a long paragraph: it should re-wrap to several lines.
    let fonts = pdit_core::Fonts::noto(font);
    let long = "This considerably longer replacement paragraph is written on purpose so that when it is reflowed to the original column width it wraps onto three separate lines on the page.";
    let _ = pdit_core::preview_reflow(0, &[0, 1, 2], long, None, &fonts);
    pdit_core::keep_edit();
    let after_long = pdit_core::paragraph_at(0, 100.0, 743.0);
    check(
        "reflow to a longer paragraph wraps to several lines",
        matches!(&after_long, Ok(Some(p)) if p.object_indices.len() >= 2 && p.text == long),
        format!("{after_long:?}"),
    );

    // Fresh copy: reflow A to a short sentence collapses it to one line.
    if pdit_core::open_document(WRAPPED.to_vec()).is_ok() {
        let short = "Short replacement sentence.";
        let _ = pdit_core::preview_reflow(0, &[0, 1, 2], short, None, &fonts);
        pdit_core::keep_edit();
        let after_short = pdit_core::paragraph_at(0, 100.0, 743.0);
        check(
            "reflow to a short sentence collapses to one line",
            matches!(&after_short, Ok(Some(p)) if p.object_indices.len() == 1 && p.text == short),
            format!("{after_short:?}"),
        );
    }

    // Lines split into pieces, even inside words, with a shape between two
    // pieces (fixtures/synthetic-split-lines.pdf): grouped as they look.
    if let Err(error) = pdit_core::open_document(SPLIT.to_vec()) {
        return check("split: open", false, error.to_string());
    }
    let sentence = "The garden club keeps a shared shed where members borrow tools, provided each tool is cleaned and hung on its hook before dusk.";
    let p = pdit_core::paragraph_at(0, 420.0, 742.0);
    check(
        "split: a click on a piece picks the whole wrapped sentence",
        matches!(&p, Ok(Some(p)) if p.object_indices == [0, 2, 3, 4] && p.text == sentence),
        format!("{p:?}"),
    );
    let v = pdit_core::paragraph_at(0, 150.0, 692.0);
    check(
        "split: a line in three pieces is one line",
        matches!(&v, Ok(Some(p)) if p.object_indices == [5, 6, 7] && p.text == "Visitors sign the book."),
        format!("{v:?}"),
    );
    let objects = || pdit_core::text_lines(0).map(|l| l.len()).unwrap_or(0);
    let before = objects();
    let edited = "Members borrow tools for one day.";
    let preview = pdit_core::preview_reflow(0, &[0, 2, 3, 4], edited, None, &fonts);
    let _ = pdit_core::discard_edit();
    let back = pdit_core::paragraph_at(0, 420.0, 742.0);
    check(
        "split: discard puts every piece back",
        preview.is_ok()
            && objects() == before
            && matches!(&back, Ok(Some(p)) if p.object_indices == [0, 2, 3, 4] && p.text == sentence),
        format!("{preview:?}; {before} → {} objects; {back:?}", objects()),
    );
    let _ = pdit_core::preview_reflow(0, &[0, 2, 3, 4], edited, None, &fonts);
    pdit_core::keep_edit();
    let kept = pdit_core::paragraph_at(0, 100.0, 742.0);
    let visitors = pdit_core::paragraph_at(0, 150.0, 692.0);
    check(
        "split: keep leaves one line at the top, the next line untouched",
        matches!(&kept, Ok(Some(p)) if p.text == edited && (p.bounds[3] - 746.0).abs() < 3.0)
            && matches!(&visitors, Ok(Some(p)) if p.text == "Visitors sign the book."),
        format!("{kept:?}; {visitors:?}"),
    );
    pdit_core::close_document();
}

/// Shapes (D-023, feature parity): draw a rectangle, ellipse, line and arrow on
/// a blank page and confirm they embed (the saved file grows).
fn shape_session() {
    use pdit_core::ShapeKind;
    use pdit_core::page_ops as pages;
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: shapes: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if pdit_core::open_document(pages::blank_document()).is_err() {
        return check("open", false, "blank".into());
    }
    let base = pdit_core::save_document().map(|b| b.len()).unwrap_or(0);
    let drawn = [
        pages::add_shape(
            0,
            ShapeKind::Rectangle,
            100.0,
            700.0,
            300.0,
            760.0,
            [31, 33, 36],
            2.0,
            Some([207, 227, 255]),
        ),
        pages::add_shape(
            0,
            ShapeKind::Ellipse,
            100.0,
            600.0,
            300.0,
            660.0,
            [31, 33, 36],
            2.0,
            None,
        ),
        pages::add_shape(
            0,
            ShapeKind::Line,
            100.0,
            560.0,
            300.0,
            560.0,
            [31, 33, 36],
            2.0,
            None,
        ),
        pages::add_shape(
            0,
            ShapeKind::Arrow,
            100.0,
            520.0,
            300.0,
            480.0,
            [211, 58, 44],
            3.0,
            None,
        ),
    ];
    let after = pdit_core::save_document().map(|b| b.len()).unwrap_or(0);
    check(
        "draw rectangle, ellipse, line, arrow",
        drawn.iter().all(|r| r.is_ok()) && after > base,
        format!("{base} -> {after} bytes; {drawn:?}"),
    );
    pdit_core::close_document();
}

/// Table tool (D-023, D-038): a 3 × 4 grid drawn on a blank page, bad sizes
/// (0 rows) clamped instead of failing; a 1 × 1 table grown to 1 × 2 keeps one
/// grid (the new one lands at the same index); widening its first column moves
/// the text in the second cell with it; a stale index is refused instead of
/// deleting other content.
fn table_session(font: &[u8]) {
    use pdit_core::TableLayout;
    use pdit_core::page_ops as pages;
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: table: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if pdit_core::open_document(pages::blank_document()).is_err() {
        return check("open", false, "blank".into());
    }
    let ink = [31, 33, 36];
    let base = pdit_core::save_document().map(|b| b.len()).unwrap_or(0);
    let grid = pages::add_table(0, 72.0, 500.0, 520.0, 700.0, 3, 4, ink, 1.0);
    let clamped = pages::add_table(0, 72.0, 300.0, 520.0, 400.0, 0, 2, ink, 1.0);
    let after = pdit_core::save_document().map(|b| b.len()).unwrap_or(0);
    check(
        "draw 3x4 grid, clamp 0 rows",
        grid.is_ok() && clamped.is_ok() && after > base,
        format!("{base} -> {after} bytes; {grid:?} {clamped:?}"),
    );
    let layout = |cols: Vec<f32>| TableLayout {
        left: 72.0,
        top: 200.0,
        col_widths: cols,
        row_heights: vec![50.0],
    };
    let one = pages::add_table(0, 72.0, 150.0, 272.0, 200.0, 1, 1, ink, 1.0);
    let grown = one.as_ref().ok().map(|&i| {
        pages::replace_table(
            0,
            i,
            &layout(vec![200.0]),
            &layout(vec![100.0, 100.0]),
            ink,
            1.0,
        )
    });
    check(
        "grow 1x1 to 1x2 replaces the grid",
        matches!((&one, &grown), (Ok(i), Some(Ok(j))) if i == j),
        format!("{one:?} -> {grown:?}"),
    );
    // Text in the second cell, then widen the first column by 40 pt.
    let _ = pdit_core::preview_add(0, 180.0, 186.0, "Cell", None, &pdit_core::Fonts::noto(font));
    pdit_core::keep_edit();
    let left_of = || {
        pdit_core::text_lines(0).ok().and_then(|l| {
            l.into_iter()
                .find(|l| l.text == "Cell")
                .map(|l| l.bounds[0])
        })
    };
    let before = left_of();
    let resized = match &grown {
        Some(Ok(i)) => pages::replace_table(
            0,
            *i,
            &layout(vec![100.0, 100.0]),
            &layout(vec![140.0, 60.0]),
            ink,
            1.0,
        ),
        _ => Err(pdit_core::Error::Pdfium("no table".into())),
    };
    let after = left_of();
    check(
        "resize moves cell text with its cell",
        resized.is_ok()
            && matches!((before, after), (Some(b), Some(a)) if (a - b - 40.0).abs() < 0.5),
        format!("{resized:?}; text left {before:?} -> {after:?}"),
    );
    // A stale index (object 0 is the 3x4 grid, not this table) is refused.
    let stale = pages::replace_table(
        0,
        0,
        &layout(vec![140.0, 60.0]),
        &layout(vec![70.0; 3]),
        ink,
        1.0,
    );
    check(
        "stale index is refused",
        stale.is_err(),
        format!("{stale:?}"),
    );
    pdit_core::close_document();
}

/// Signature (D-039): the pad's exporter makes a transparent PNG, and placing
/// it keeps the transparency (PDFium writes a soft mask), so a signature never
/// covers the page with a white box.
fn signature_session() {
    use pdit_core::page_ops as pages;
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: signature: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    let Some(png) = crate::signature_ui::sample_png() else {
        return check("export PNG", false, "no PNG".into());
    };
    if pdit_core::open_document(pages::blank_document()).is_err() {
        return check("open", false, "blank".into());
    }
    let added = pages::add_image(0, png.clone(), 100.0, 700.0, 120.0);
    let saved = pdit_core::save_document().unwrap_or_default();
    let soft_mask = saved.windows(6).any(|w| w == b"/SMask");
    check(
        "transparent PNG keeps its transparency",
        added.is_ok() && soft_mask,
        format!(
            "{} PNG bytes; {added:?}; /SMask in PDF: {soft_mask}",
            png.len()
        ),
    );
    pdit_core::close_document();
}

/// Annotations part 1 (D-040) on the synthetic rental fixture: the page's
/// words; highlight, underline and strikeout over a word; a sticky note; edit
/// the note and recolour a mark; delete one; a stale index is refused; the
/// marks survive saving and reopening.
fn annotation_session() {
    use pdit_core::annotations::{self as annots, AnnotationKind, MarkupKind};
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: annotations: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if let Err(error) = pdit_core::open_document(RENTAL.to_vec()) {
        return check("open", false, error.to_string());
    }
    let words = annots::words(0).unwrap_or_default();
    let monthly = words.iter().find(|w| w.text == "Monthly").cloned();
    check(
        "words with boxes",
        monthly
            .as_ref()
            .is_some_and(|w| w.bounds[2] > w.bounds[0] && w.bounds[3] > w.bounds[1]),
        format!("{} words; {monthly:?}", words.len()),
    );
    let Some(word) = monthly else {
        return pdit_core::close_document();
    };
    let yellow = [255, 225, 77];
    let added = [
        annots::add_markup(0, MarkupKind::Highlight, &[word.bounds], yellow),
        annots::add_markup(0, MarkupKind::Underline, &[word.bounds], [31, 157, 85]),
        annots::add_markup(0, MarkupKind::Strikeout, &[word.bounds], [214, 51, 127]),
        annots::add_note(0, 400.0, 760.0, "Check this figure"),
    ];
    let listed = annots::annotations(0).unwrap_or_default();
    let kinds: Vec<_> = listed.iter().map(|a| a.kind).collect();
    check(
        "add highlight, underline, strikeout, note",
        added.iter().all(|r| r.is_ok())
            && kinds
                == [
                    AnnotationKind::Markup(MarkupKind::Highlight),
                    AnnotationKind::Markup(MarkupKind::Underline),
                    AnnotationKind::Markup(MarkupKind::Strikeout),
                    AnnotationKind::Note,
                ]
            && listed[0].rects.len() == 1
            && listed[3].text == "Check this figure",
        format!("{added:?}; {listed:?}"),
    );
    let note = annots::set_note_text(0, 3, "Checked");
    let recolour = annots::set_markup_color(0, 0, [124, 196, 255]);
    let after = annots::annotations(0).unwrap_or_default();
    check(
        "edit note, recolour highlight",
        note.is_ok()
            && recolour.is_ok()
            && after.get(3).is_some_and(|a| a.text == "Checked")
            && after.first().is_some_and(|a| a.color == [124, 196, 255]),
        format!("{note:?} {recolour:?}"),
    );
    let stale = annots::set_note_text(0, 0, "not a note");
    check(
        "stale index is refused",
        stale.is_err(),
        format!("{stale:?}"),
    );
    let deleted = annots::delete_annotation(0, 1);
    let left = annots::annotations(0).map(|a| a.len()).unwrap_or(0);
    check(
        "delete the underline",
        deleted.is_ok() && left == 3,
        format!("{deleted:?}; {left} left"),
    );
    let saved = pdit_core::save_document().unwrap_or_default();
    let has = |needle: &[u8]| saved.windows(needle.len()).any(|w| w == needle);
    // For looking at the result: `window.__pditAnnotPdf` (debug builds only).
    let array = js_sys::Uint8Array::from(saved.as_slice());
    let _ = js_sys::Reflect::set(&js_sys::global(), &"__pditAnnotPdf".into(), &array);
    pdit_core::close_document();
    let reopened = pdit_core::open_document(saved.clone())
        .ok()
        .and_then(|_| annots::annotations(0).ok())
        .map(|a| a.len());
    check(
        "saved and reopened",
        reopened == Some(3) && has(b"/Highlight") && has(b"/StrikeOut") && has(b"/Text"),
        format!(
            "{reopened:?} annotations; appearance streams (/AP) in file: {}",
            has(b"/AP")
        ),
    );
    pdit_core::close_document();
}

/// Annotations part 2 (D-041): a pen stroke and a stamp on a blank page are
/// listed, saved with appearance streams (`/AP`) and the stroke's `/InkList`,
/// and survive reopening. `window.__pditAnnot2Pdf` holds the result.
fn annotation2_session() {
    use pdit_core::annotations::{self as annots, AnnotationKind};
    use pdit_core::page_ops as pages;
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: annotations 2: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if pdit_core::open_document(pages::blank_document()).is_err() {
        return check("open", false, "blank".into());
    }
    let stroke: Vec<(f32, f32)> = (0..=24)
        .map(|i| {
            let a = i as f32 / 24.0 * std::f32::consts::TAU;
            (200.0 + 80.0 * a.cos(), 600.0 + 30.0 * a.sin())
        })
        .collect();
    let ink = annots::add_ink(0, &stroke, [211, 58, 44], 2.0);
    let stamp = annots::add_stamp(0, 72.0, 760.0, "Approved", "2 Oct 2026", [31, 157, 85]);
    let listed = annots::annotations(0).unwrap_or_default();
    check(
        "add pen stroke and stamp",
        ink.is_ok()
            && stamp.is_ok()
            && listed.iter().map(|a| a.kind).collect::<Vec<_>>()
                == [AnnotationKind::Ink, AnnotationKind::Stamp]
            && listed[1].text == "APPROVED · 2 Oct 2026",
        format!("{ink:?} {stamp:?}; {listed:?}"),
    );
    let saved = pdit_core::save_document().unwrap_or_default();
    let has = |needle: &[u8]| saved.windows(needle.len()).any(|w| w == needle);
    let array = js_sys::Uint8Array::from(saved.as_slice());
    let _ = js_sys::Reflect::set(&js_sys::global(), &"__pditAnnot2Pdf".into(), &array);
    pdit_core::close_document();
    let reopened = pdit_core::open_document(saved.clone())
        .ok()
        .and_then(|_| annots::annotations(0).ok())
        .map(|a| a.len());
    check(
        "saved with /AP and /InkList, reopened",
        reopened == Some(2) && has(b"/AP") && has(b"/InkList") && has(b"/Stamp"),
        format!(
            "{reopened:?}; /AP {} /InkList {}",
            has(b"/AP"),
            has(b"/InkList")
        ),
    );
    pdit_core::close_document();
    // As the app does it: a text page, an Undo snapshot first, then the stamp
    // and a pen stroke, then the overlay's listing.
    if pdit_core::open_document(RENTAL.to_vec()).is_err() {
        return check("open rental", false, String::new());
    }
    let snap = pages::snapshot().map(|s| s.len());
    log(&format!(
        "pdit self-check: annotations 2: snapshot {snap:?}"
    ));
    let stamp = annots::add_stamp(0, 100.0, 500.0, "Draft", "2 Oct 2026", [29, 111, 216]);
    let ink = annots::add_ink(0, &stroke, [31, 33, 36], 2.0);
    let listed = annots::annotations(0).map(|a| a.len());
    check(
        "app path: text page, snapshot, stamp, pen, list",
        snap.is_ok() && stamp.is_ok() && ink.is_ok() && matches!(listed, Ok(2)),
        format!("{snap:?} {stamp:?} {ink:?} {listed:?}"),
    );
    pdit_core::close_document();
}

/// Header & footer + watermark (D-042) on a 3-page document: the footer reads
/// "Page n of 3" per page, settings read back from pdit's marks, applying again
/// replaces (no duplicates), the marks survive saving and reopening, and Remove
/// takes them away. `window.__pditDocPdf` holds the saved result.
fn doc_content_session() {
    use pdit_core::doc_content::{self as doc, HeaderFooter, Watermark};
    use pdit_core::page_ops as pages;
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: document: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if pdit_core::open_document(RENTAL.to_vec()).is_err() {
        return check("open", false, String::new());
    }
    let _ = pages::insert_blank_page(0);
    let _ = pages::insert_blank_page(1);
    // Page 2 is turned a quarter clockwise: its header & footer must still
    // sit at the top / bottom as the reader sees it.
    let _ = pages::rotate_page(1, true);
    let lines = |page: u16| -> Vec<String> {
        pdit_core::text_lines(page)
            .map(|l| l.into_iter().map(|l| l.text).collect())
            .unwrap_or_default()
    };
    let hf = HeaderFooter {
        header: [
            "Synthetic lease · {date}".into(),
            String::new(),
            String::new(),
        ],
        footer: [
            String::new(),
            "Page {page} of {pages}".into(),
            String::new(),
        ],
        size_pt: 9.0,
        pages: None,
    };
    let set = doc::set_header_footer(&hf, "2 Oct 2026");
    let page2 = lines(1);
    check(
        "header & footer on every page",
        set.is_ok()
            && page2.iter().any(|l| l == "Page 2 of 3")
            && page2.iter().any(|l| l == "Synthetic lease · 2 Oct 2026")
            && lines(2).iter().any(|l| l == "Page 3 of 3"),
        format!("{set:?}; page 2: {page2:?}"),
    );
    // On the turned page (595 × 842 media, shown 842 wide), the footer at the
    // shown bottom lies near the media's right edge (x ≈ 595 − 27).
    let footer_left = pdit_core::text_lines(1).ok().and_then(|l| {
        l.into_iter()
            .find(|l| l.text == "Page 2 of 3")
            .map(|l| l.bounds[0])
    });
    check(
        "footer stays at the bottom of a turned page",
        footer_left.is_some_and(|x| x > 540.0),
        format!("footer's left edge in page coordinates: {footer_left:?}"),
    );
    let read = doc::header_footer();
    check(
        "settings read back",
        matches!(&read, Ok(Some(r)) if *r == hf),
        format!("{read:?}"),
    );
    let again = doc::set_header_footer(&hf, "2 Oct 2026");
    let count = lines(1).iter().filter(|l| l.starts_with("Page ")).count();
    check(
        "applying again replaces",
        again.is_ok() && count == 1,
        format!("{again:?}; {count} footer line(s)"),
    );
    let wm = Watermark {
        text: "CONFIDENTIAL".into(),
        color: [211, 58, 44],
        opacity: 0.25,
        size_pt: 54.0,
        diagonal: true,
        pages: Some((1, 2)),
    };
    let set_wm = doc::set_watermark(&wm);
    let on = |page: u16| lines(page).iter().any(|l| l == "CONFIDENTIAL");
    check(
        "watermark on pages 1-2 only",
        set_wm.is_ok() && on(0) && on(1) && !on(2),
        format!("{set_wm:?}; {:?}", doc::watermark()),
    );
    let saved = pdit_core::save_document().unwrap_or_default();
    let array = js_sys::Uint8Array::from(saved.as_slice());
    let _ = js_sys::Reflect::set(&js_sys::global(), &"__pditDocPdf".into(), &array);
    pdit_core::close_document();
    let reopened = pdit_core::open_document(saved).is_ok();
    let (hf2, wm2) = (doc::header_footer(), doc::watermark());
    check(
        "settings survive save + reopen",
        reopened
            && matches!(&hf2, Ok(Some(r)) if *r == hf)
            && matches!(&wm2, Ok(Some(w)) if w.text == "CONFIDENTIAL" && w.pages == Some((1, 2))),
        format!("{hf2:?} {wm2:?}"),
    );
    let removed = (doc::remove_header_footer(), doc::remove_watermark());
    let left = lines(1);
    check(
        "remove both",
        matches!(removed, (Ok(6), Ok(2)))
            && !left
                .iter()
                .any(|l| l.starts_with("Page ") || l == "CONFIDENTIAL"),
        format!("{removed:?}; page 2 now: {left:?}"),
    );
    pdit_core::close_document();
}

/// Links (D-043): address checks, written-out addresses found by PDFium, a web
/// link on chosen words, edit / refuse / delete, auto-linking, save + reopen.
/// Item-only right-click menus (D-050): on a synthetic page with a text line,
/// a link, a note and a form field, each point gets its own item's menu and
/// empty space the page's.
fn menus_session(font: &[u8]) {
    use pdit_core::form_edit::{self as fe, NewField};
    use pdit_core::{annotations, links};
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: menus: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if pdit_core::open_document(pdit_core::page_ops::blank_document()).is_err() {
        return check("open", false, String::new());
    }
    let fonts = pdit_core::Fonts::noto(font);
    let _ = pdit_core::preview_add(0, 72.0, 700.0, "Visit the page today", None, &fonts);
    pdit_core::keep_edit();
    let word = |text: &str| {
        annotations::words(0)
            .ok()
            .and_then(|w| w.into_iter().find(|w| w.text == text))
            .map(|w| w.bounds)
    };
    let (Some(visit), Some(today)) = (word("Visit"), word("today")) else {
        return check("text placed", false, String::new());
    };
    let linked = links::add_web_link(0, &[today], "example.net");
    let note = annotations::add_note(0, 400.0, 500.0, "Synthetic note");
    let field = fe::add_field(0, &NewField::Text, [72.0, 300.0, 300.0, 322.0], "name");
    let centre = |[l, b, r, t]: [f32; 4]| ((l + r) / 2.0, (b + t) / 2.0);
    let menu = |point: (f32, f32)| {
        crate::context_menu::item_sections(0, point)
            .first()
            .map(|(label, actions)| format!("{label}: {}", actions.len()))
            .unwrap_or_default()
    };
    let cases = [
        ("text line", centre(visit), "Text: 2"),
        ("link", centre(today), "Link: 3"),
        ("note", (405.0, 495.0), "Note: 2"),
        ("form field", (150.0, 310.0), "Form field: 2"),
        ("empty space", (300.0, 150.0), "Page: 5"),
    ];
    for (name, point, want) in cases {
        let got = menu(point);
        check(
            name,
            got == want,
            format!("{got} (setup: {linked:?} {note:?} {field:?})"),
        );
    }
}

/// Find & replace (D-051) on a synthetic page: matches with per-word boxes,
/// whole words and case, then Replace all through the line-edit path the app
/// uses.
fn find_session(font: &[u8]) {
    use pdit_core::search::{FindOptions, find, replace_in};
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: find: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if pdit_core::open_document(pdit_core::page_ops::blank_document()).is_err() {
        return check("open", false, String::new());
    }
    let fonts = pdit_core::Fonts::noto(font);
    for (y, text) in [
        (700.0, "Rent is 890 Euro; rental rent"),
        (680.0, "Pay the Rent early"),
    ] {
        let _ = pdit_core::preview_add(0, 72.0, y, text, None, &fonts);
        pdit_core::keep_edit();
    }
    let any = FindOptions::default();
    let whole = FindOptions {
        whole_word: true,
        ..any
    };
    let found = find(0, "rent", any).unwrap_or_default();
    let lines = pdit_core::text_lines(0).unwrap_or_default();
    let inside = found.iter().all(|m| {
        lines.iter().any(|l| {
            l.object_index == m.object_index
                && m.rect[0] >= l.bounds[0] - 2.0
                && m.rect[2] <= l.bounds[2] + 2.0
                && m.rect[2] - m.rect[0] < (l.bounds[2] - l.bounds[0]) / 2.0
        })
    });
    check(
        "4 matches over 2 lines, each boxed inside its line",
        found.len() == 4 && inside,
        format!(
            "{:?}",
            found
                .iter()
                .map(|m| (m.object_index, m.rect))
                .collect::<Vec<_>>()
        ),
    );
    let counts = (
        find(0, "rent", whole).map_or(usize::MAX, |f| f.len()),
        find(
            0,
            "Rent",
            FindOptions {
                match_case: true,
                ..whole
            },
        )
        .map_or(usize::MAX, |f| f.len()),
        find(0, "xyz", any).map_or(usize::MAX, |f| f.len()),
    );
    check(
        "whole words 3, + match case 2, none 0",
        counts == (3, 2, 0),
        format!("{counts:?}"),
    );
    // Replace all, as search_ui does: one line edit per line, highest index first.
    let mut targets: Vec<_> = found
        .iter()
        .map(|m| (m.object_index, m.line.clone()))
        .collect();
    targets.dedup();
    targets.sort_by_key(|t| std::cmp::Reverse(t.0));
    let edits: Vec<_> = targets
        .iter()
        .map(|(index, line)| {
            let new = replace_in(line, "rent", "fee", whole, None);
            let result = pdit_core::preview_edit(0, *index, &new, &fonts).map(|p| p.text);
            pdit_core::keep_edit();
            result
        })
        .collect();
    let text: Vec<String> = pdit_core::text_lines(0)
        .unwrap_or_default()
        .into_iter()
        .map(|l| l.text)
        .collect();
    check(
        "replace all (whole words): lines read back",
        text.iter().any(|t| t == "fee is 890 Euro; rental fee")
            && text.iter().any(|t| t == "Pay the fee early")
            && find(0, "rent", whole).is_ok_and(|f| f.is_empty()),
        format!("{edits:?} → {text:?}"),
    );
    // The same on a document's own lines (the synthetic rental fixture).
    if pdit_core::open_document(RENTAL.to_vec()).is_err() {
        return check("open rental", false, String::new());
    }
    let found = find(0, "Euro", any).unwrap_or_default();
    let mut targets: Vec<_> = found
        .iter()
        .map(|m| (m.object_index, m.line.clone()))
        .collect();
    targets.dedup();
    targets.sort_by_key(|t| std::cmp::Reverse(t.0));
    let edits: Vec<_> = targets
        .iter()
        .map(|(index, line)| {
            let new = replace_in(line, "Euro", "EUR", any, None);
            let result = pdit_core::preview_edit(0, *index, &new, &fonts).map(|p| p.text);
            pdit_core::keep_edit();
            result
        })
        .collect();
    let text: Vec<String> = pdit_core::text_lines(0)
        .unwrap_or_default()
        .into_iter()
        .map(|l| l.text)
        .collect();
    check(
        "replace all on the fixture's own lines",
        !found.is_empty() && find(0, "Euro", any).is_ok_and(|f| f.is_empty()),
        format!("{} found; {edits:?} → {text:?}", found.len()),
    );
}

/// Print (D-052): a page drawn for paper is 200 dpi wide, and "Leave out"
/// really leaves comments and marks out (a stamp on a blank page).
fn print_session() {
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: print: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if pdit_core::open_document(pdit_core::page_ops::blank_document()).is_err() {
        return check("open", false, String::new());
    }
    let stamp =
        pdit_core::annotations::add_stamp(0, 100.0, 500.0, "Draft", "6 Oct 2026", [29, 111, 216]);
    let ink = |comments: bool| {
        pdit_core::render_page_for_print(0, 1653, comments).map(|image| {
            let data = image.data();
            let dark = data
                .chunks(4)
                .filter(|px| px[0] < 200 || px[1] < 200 || px[2] < 200)
                .count();
            (image.width(), image.height(), dark)
        })
    };
    let (with, without) = (ink(true), ink(false));
    let ok = matches!((&with, &without), (Ok((1653, h, a)), Ok((_, _, 0))) if *h > 2300 && *a > 0);
    check(
        "200 dpi, comments printed or left out",
        ok,
        format!("{stamp:?}; with {with:?}, without {without:?}"),
    );
}

/// Find fields (D-053): leafmind's finder, run on the synthetic form's page,
/// finds boxes where the form's 4 fields are, with names from their labels.
async fn fields_session() {
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: find fields: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    let Some(finder) = crate::find_fields_ui::finder().await else {
        return check("load the models", false, String::new());
    };
    if pdit_core::open_document(FORM.to_vec()).is_err() {
        return check("open", false, String::new());
    }
    let started = js_sys::Date::now();
    let found = pdit_core::fields::find_fields(&finder, 0);
    let ms = js_sys::Date::now() - started;
    let existing = pdit_core::form_fields(0).unwrap_or_default();
    let found = found.unwrap_or_default();
    // Each real field has a suggestion over most of it.
    let hit = existing
        .iter()
        .filter(|e| {
            found.iter().any(|f| {
                let w = e.rect.2.min(f.rect[2]) - e.rect.0.max(f.rect[0]);
                let h = e.rect.3.min(f.rect[3]) - e.rect.1.max(f.rect[1]);
                w > 0.0 && h > 0.0 && w * h > 0.5 * (e.rect.2 - e.rect.0) * (e.rect.3 - e.rect.1)
            })
        })
        .count();
    check(
        "suggestions over the form's own fields",
        !existing.is_empty() && hit >= existing.len() - 1,
        format!(
            "{hit} of {} in {ms:.0} ms; {:?}",
            existing.len(),
            found
                .iter()
                .map(|f| (f.kind, f.name.as_str(), f.filled))
                .collect::<Vec<_>>()
        ),
    );
}

/// The AI analysis (D-055): a text page is "text"; the same page turned into a
/// picture is a "scan"; in the desktop app OCR reads it, the invisible text
/// layer goes on, and Find finds a word where it really is on the page.
async fn scan_session() {
    use pdit_core::analysis::{PageKind, page_kind};
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: scans: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if pdit_core::open_document(RENTAL.to_vec()).is_err() {
        return check("open", false, String::new());
    }
    let before = page_kind(0);
    let original = pdit_core::annotations::words(0)
        .unwrap_or_default()
        .into_iter()
        .find(|w| w.text == "deposit");
    let made = pdit_core::page_ops::rasterize_page(0, 1240);
    let after = page_kind(0);
    check(
        "text page, then the same page as a picture is a scan",
        matches!(before, Ok(PageKind::Text)) && matches!(after, Ok(PageKind::Scan)),
        format!("{before:?} → {made:?} {after:?}"),
    );
    if !crate::ai_ui::desktop::available() {
        return log("pdit self-check: scans: OCR skipped (not the desktop app)");
    }
    let language = crate::ai_ui::desktop::language(0).await;
    let words = crate::ai_ui::desktop::read(0, language.unwrap_or("eng")).await;
    let font = crate::fetch_bytes(&crate::FALLBACK_FONT.to_string())
        .await
        .unwrap_or_default();
    let layered = words
        .as_ref()
        .map(|w| pdit_core::add_text_layer(0, w, &font));
    let hit = pdit_core::search::find(0, "deposit", pdit_core::search::FindOptions::default())
        .unwrap_or_default()
        .into_iter()
        .next();
    let near = match (&original, &hit) {
        (Some(o), Some(h)) => {
            (o.bounds[0] - h.rect[0]).abs() < 6.0 && (o.bounds[1] - h.rect[1]).abs() < 6.0
        }
        _ => false,
    };
    check(
        "OCR → text layer → Find finds \"deposit\" where it is",
        near && page_kind(0).is_ok_and(|k| k == PageKind::Text),
        format!(
            "language {language:?}; {} words; layer {layered:?}; original {:?}, found {:?}",
            words.as_ref().map_or(0, |w| w.len()),
            original.map(|o| o.bounds),
            hit.map(|h| h.rect)
        ),
    );
}

/// Ask a question (D-056), desktop app only: the models' download finishes
/// (files already there are kept), the synthetic rental PDF is read, and
/// questions get the document's own sentence, or none.
async fn ask_session() {
    use crate::ai_ui::desktop;
    use js_sys::{Object, Reflect};
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: ask: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if !desktop::available() {
        return log("pdit self-check: ask: skipped (not the desktop app)");
    }
    let get = |v: &wasm_bindgen::JsValue, k: &str| Reflect::get(v, &k.into()).unwrap_or_default();
    let _ = desktop::invoke("qa_download", &Object::new()).await;
    let mut status = wasm_bindgen::JsValue::NULL;
    for _ in 0..1200 {
        status = desktop::invoke("qa_status", &Object::new())
            .await
            .unwrap_or_default();
        if get(&status, "downloading").as_bool() != Some(true) {
            break;
        }
        crate::print_ui::pause(500).await;
    }
    check(
        "models ready",
        get(&status, "ready").as_bool() == Some(true),
        format!("{:?}", js_sys::JSON::stringify(&status).ok()),
    );
    let started = js_sys::Date::now();
    let indexed = desktop::invoke_raw("qa_index", RENTAL).await;
    check(
        "the document is read",
        indexed.is_ok(),
        format!("{indexed:?} in {:.0} ms", js_sys::Date::now() - started),
    );
    for (question, want) in [
        ("When does the lease begin?", Some("1 March 2027")),
        ("How much is the monthly rent?", Some("890")),
        ("Is there a swimming pool?", None),
    ] {
        let args = Object::new();
        let _ = Reflect::set(&args, &"question".into(), &question.into());
        let started = js_sys::Date::now();
        let reply = desktop::invoke("qa_ask", &args).await.unwrap_or_default();
        let text = js_sys::JSON::stringify(&reply)
            .ok()
            .and_then(|s| s.as_string())
            .unwrap_or_default();
        let ok = match want {
            Some(w) => text.contains("\"found\"") && text.contains(w),
            None => text.contains("not_found"),
        };
        check(
            question,
            ok,
            format!("{text} in {:.0} ms", js_sys::Date::now() - started),
        );
    }
}

fn links_session(font: &[u8]) {
    use pdit_core::annotations;
    use pdit_core::links::{self, LinkTarget, normalize_url};
    use pdit_core::page_ops as pages;
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: links: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    let cases = [
        ("example.org", Some("https://example.org")),
        ("HTTP://Example.org/a?b", Some("HTTP://Example.org/a?b")),
        ("help@example.com", Some("mailto:help@example.com")),
        ("javascript:alert(1)", None),
        ("file:///etc/passwd", None),
        ("localhost:3000", None),
        ("hello", None),
        ("https://exa mple.org", None),
        ("https://", None),
    ];
    let wrong: Vec<_> = cases
        .iter()
        .filter(|(i, want)| normalize_url(i).as_deref() != *want)
        .collect();
    check(
        "addresses accepted / refused",
        wrong.is_empty(),
        format!("wrong: {wrong:?}"),
    );
    if pdit_core::open_document(RENTAL.to_vec()).is_err() {
        return check("open", false, String::new());
    }
    let _ = pages::insert_blank_page(0); // after page 1
    let fonts = pdit_core::Fonts::noto(font);
    let _ = pdit_core::preview_add(
        1,
        72.0,
        700.0,
        "Visit www.example.org or help@example.com today",
        None,
        &fonts,
    );
    pdit_core::keep_edit();
    let found = links::web_addresses(1);
    let urls: Vec<String> = found
        .as_ref()
        .map(|f| f.iter().map(|a| a.url.clone()).collect())
        .unwrap_or_default();
    check(
        "written-out addresses found",
        urls.len() == 2
            && urls.iter().any(|u| u.contains("www.example.org"))
            && urls.iter().any(|u| u == "mailto:help@example.com"),
        format!(
            "{urls:?}; words: {:?}",
            annotations::words(1).map(|w| w.into_iter().map(|w| w.text).collect::<Vec<_>>())
        ),
    );
    let word = annotations::words(1)
        .ok()
        .and_then(|w| w.into_iter().find(|w| w.text == "today"));
    let added = word
        .as_ref()
        .map(|w| links::add_web_link(1, &[w.bounds], "example.net"));
    let target_of = |i: usize| {
        links::links(1)
            .ok()
            .and_then(|l| l.into_iter().find(|l| l.index == i))
            .map(|l| l.target)
    };
    let index = match &added {
        Some(Ok(i)) => *i,
        _ => usize::MAX,
    };
    check(
        "web link on a chosen word",
        target_of(index) == Some(LinkTarget::Web("https://example.net".into())),
        format!("{added:?} → {:?}", target_of(index)),
    );
    let changed = links::set_web_link(1, index, "https://example.com/x");
    let refused = links::set_web_link(1, index, "javascript:alert(1)");
    check(
        "edit, and a script address refused",
        changed.is_ok()
            && refused.is_err()
            && target_of(index) == Some(LinkTarget::Web("https://example.com/x".into())),
        format!("{changed:?} {refused:?} → {:?}", target_of(index)),
    );
    let made = links::link_web_addresses();
    let left = links::web_addresses(1).map(|f| f.len());
    let count = links::links(1).map(|l| l.len());
    check(
        "link web addresses (skips linked text)",
        matches!(made, Ok(2)) && matches!(left, Ok(0)) && matches!(count, Ok(3)),
        format!("made {made:?}, still unlinked {left:?}, links {count:?}"),
    );
    let in_comments = annotations::annotations(1).map(|a| a.len());
    check(
        "links stay out of the comment list",
        matches!(in_comments, Ok(0)),
        format!("{in_comments:?}"),
    );
    // Page links (D-045): written with lopdf, the document reopened.
    let before: Vec<LinkTarget> = links::links(1)
        .map(|l| l.into_iter().map(|l| l.target).collect())
        .unwrap_or_default();
    let or_word = annotations::words(1)
        .ok()
        .and_then(|w| w.into_iter().find(|w| w.text == "or"));
    let page_link = or_word
        .as_ref()
        .map(|w| links::add_page_link(1, &[w.bounds], 0));
    let after: Vec<LinkTarget> = links::links(1)
        .map(|l| l.into_iter().map(|l| l.target).collect())
        .unwrap_or_default();
    check(
        "page link (lopdf) goes to page 1, other links kept",
        matches!(page_link, Some(Ok(i)) if after.get(i) == Some(&LinkTarget::Page(0)))
            && after.len() == before.len() + 1
            && after[..before.len()] == before[..],
        format!("{page_link:?}; {after:?}"),
    );
    let bad = links::add_page_link(1, &[[72.0, 690.0, 90.0, 705.0]], 9);
    let web_after = or_word
        .as_ref()
        .map(|w| links::add_web_link(1, &[w.bounds], "example.com/after"));
    let web_ok = matches!(&web_after, Some(Ok(i)) if links::links(1).ok().and_then(|l| l.into_iter().find(|l| l.index == *i)).map(|l| l.target) == Some(LinkTarget::Web("https://example.com/after".into())));
    if let Some(Ok(i)) = web_after {
        let _ = links::delete_link(1, i);
    }
    check(
        "page 10 of 2 refused; PDFium still edits after the reload",
        bad.is_err() && web_ok,
        format!("{bad:?}; web link after reload ok: {web_ok}"),
    );
    let saved = pdit_core::save_document().unwrap_or_default();
    let array = js_sys::Uint8Array::from(saved.as_slice());
    let _ = js_sys::Reflect::set(&js_sys::global(), &"__pditLinksPdf".into(), &array);
    pdit_core::close_document();
    let reopened = pdit_core::open_document(saved).is_ok();
    let after: Vec<LinkTarget> = links::links(1)
        .map(|l| l.into_iter().map(|l| l.target).collect())
        .unwrap_or_default();
    check(
        "links survive save + reopen",
        reopened && after.len() == 4 && after.contains(&LinkTarget::Page(0)),
        format!("{after:?}"),
    );
    let deleted = links::delete_link(1, index);
    let not_link = links::delete_link(0, 0);
    check(
        "delete (and refuse a non-link)",
        deleted.is_ok() && not_link.is_err() && links::links(1).map(|l| l.len()).ok() == Some(3),
        format!("{deleted:?} {not_link:?}"),
    );
    pdit_core::close_document();
}

/// Bookmarks (D-043, D-045): find headings, write a list (with a non-Latin
/// title and a too-deep level), read it back through PDFium, rename, survive
/// save + reopen, remove all.
fn bookmarks_session(font: &[u8]) {
    use pdit_core::bookmarks::{self, Bookmark};
    use pdit_core::page_ops as pages;
    use pdit_core::{FontChoice, TextStyle};
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: bookmarks: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if pdit_core::open_document(RENTAL.to_vec()).is_err() {
        return check("open", false, String::new());
    }
    let _ = pages::insert_blank_page(0);
    let _ = pages::insert_blank_page(1);
    let fonts = pdit_core::Fonts::noto(font);
    let style = |size: f32| TextStyle {
        font: FontChoice::NotoSans,
        size,
        color: [0, 0, 0],
        bold: false,
        italic: false,
        underline: false,
    };
    for (page, y, text, size) in [
        (1, 760.0, "Chapter One", 22.0),
        (1, 720.0, "Section 1.1", 16.0),
        (
            1,
            690.0,
            "Body text on the second page, long enough to count as the body size.",
            11.0,
        ),
        (
            1,
            670.0,
            "More body text so the body size clearly wins over the headings here.",
            11.0,
        ),
        (2, 760.0, "Chapter Two", 22.0),
        (
            2,
            720.0,
            "Closing body text on the third page of this synthetic file.",
            11.0,
        ),
    ] {
        let _ = pdit_core::preview_add(page, 72.0, y, text, Some(&style(size)), &fonts);
        pdit_core::keep_edit();
    }
    let none = bookmarks::bookmarks();
    check(
        "none at first",
        matches!(&none, Ok(l) if l.is_empty()),
        format!("{none:?}"),
    );
    let found = bookmarks::heading_bookmarks().unwrap_or_default();
    let short: Vec<(String, Option<u16>, u8)> = found
        .iter()
        .map(|b| (b.title.clone(), b.page, b.level))
        .collect();
    let want = [
        ("Chapter One", 1, 0),
        ("Section 1.1", 1, 1),
        ("Chapter Two", 2, 0),
    ];
    let ours: Vec<_> = short
        .iter()
        .filter(|(t, _, _)| want.iter().any(|(w, _, _)| w == t))
        .cloned()
        .collect();
    check(
        "from headings: chapters level 1, section under it",
        ours == want
            .iter()
            .map(|(t, p, l)| (t.to_string(), Some(*p), *l))
            .collect::<Vec<_>>(),
        format!("{short:?}"),
    );
    let list = vec![
        Bookmark {
            title: "Chapter One".into(),
            page: Some(1),
            top: Some(780.0),
            level: 0,
        },
        Bookmark {
            title: "Résumé ★ فارسی".into(),
            page: Some(1),
            top: None,
            level: 1,
        },
        Bookmark {
            title: "Too deep".into(),
            page: Some(1),
            top: None,
            level: 4,
        },
        Bookmark {
            title: "Chapter Two".into(),
            page: Some(2),
            top: None,
            level: 0,
        },
    ];
    let set = bookmarks::set_bookmarks(&list);
    let read: Vec<(String, Option<u16>, u8)> = bookmarks::bookmarks()
        .map(|l| l.into_iter().map(|b| (b.title, b.page, b.level)).collect())
        .unwrap_or_default();
    let expected: Vec<(String, Option<u16>, u8)> = vec![
        ("Chapter One".into(), Some(1), 0),
        ("Résumé ★ فارسی".into(), Some(1), 1),
        ("Too deep".into(), Some(1), 2),
        ("Chapter Two".into(), Some(2), 0),
    ];
    check(
        "write + read back (non-Latin title; level 4 → 2)",
        set.is_ok() && read == expected,
        format!("{set:?}; {read:?}"),
    );
    let top = bookmarks::bookmarks()
        .ok()
        .and_then(|l| l.first().and_then(|b| b.top));
    check(
        "a bookmark keeps its place on the page",
        top.is_some_and(|t| (t - 780.0).abs() < 0.5),
        format!("{top:?}"),
    );
    let mut renamed = list.clone();
    renamed[3].title = "Chapter 2".into();
    renamed.remove(2);
    let set2 = bookmarks::set_bookmarks(&renamed);
    let titles: Vec<String> = bookmarks::bookmarks()
        .map(|l| l.into_iter().map(|b| b.title).collect())
        .unwrap_or_default();
    check(
        "rename + delete one (whole list written again)",
        set2.is_ok() && titles == ["Chapter One", "Résumé ★ فارسی", "Chapter 2"],
        format!("{set2:?}; {titles:?}"),
    );
    let saved = pdit_core::save_document().unwrap_or_default();
    let array = js_sys::Uint8Array::from(saved.as_slice());
    let _ = js_sys::Reflect::set(&js_sys::global(), &"__pditBookmarksPdf".into(), &array);
    pdit_core::close_document();
    let reopened = pdit_core::open_document(saved).is_ok();
    let again = bookmarks::bookmarks().map(|l| l.len());
    let lines = pdit_core::text_lines(1).map(|l| l.len());
    check(
        "survive save + reopen; text still readable",
        reopened && matches!(again, Ok(3)) && matches!(lines, Ok(n) if n >= 4),
        format!("{again:?} bookmarks, page 2 lines {lines:?}"),
    );
    let cleared = bookmarks::set_bookmarks(&[]);
    let left = bookmarks::bookmarks().map(|l| l.len());
    check(
        "remove all",
        cleared.is_ok() && matches!(left, Ok(0)),
        format!("{cleared:?} → {left:?}"),
    );
    pdit_core::close_document();
}

/// Creating and editing form fields (D-047): add all four kinds (two radios
/// in one group), refuse a taken name, fill them as today, move keeping the
/// value, change settings, refuse a stale selection, delete, save + reopen;
/// and add to a document that already has a form.
fn form_edit_session() {
    use pdit_core::FieldKind;
    use pdit_core::form_edit::{self as fe, FieldSettings, NewField};
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: form edit: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if pdit_core::open_document(RENTAL.to_vec()).is_err() {
        return check("open", false, String::new());
    }
    let rect = |f: &pdit_core::FormField| [f.rect.0, f.rect.1, f.rect.2, f.rect.3];
    let fields = || pdit_core::form_fields(0).unwrap_or_default();
    let find = |name: &str| {
        fields()
            .into_iter()
            .find(|f| f.name.as_deref() == Some(name))
    };
    let by_choice = |c: &str| {
        fields()
            .into_iter()
            .find(|f| f.choice.as_deref() == Some(c))
    };
    let missing = |what: &str| check(&format!("{what} found"), false, format!("{:?}", fields()));
    let added = [
        fe::add_field(0, &NewField::Text, [72.0, 560.0, 300.0, 582.0], "full_name"),
        fe::add_field(0, &NewField::Checkbox, [72.0, 530.0, 86.0, 544.0], "agree"),
        fe::add_field(
            0,
            &NewField::Radio {
                choice: "Small".into(),
            },
            [72.0, 500.0, 86.0, 514.0],
            "size",
        ),
        fe::add_field(
            0,
            &NewField::Radio {
                choice: "Large".into(),
            },
            [152.0, 500.0, 166.0, 514.0],
            "size",
        ),
        fe::add_field(
            0,
            &NewField::Dropdown {
                options: vec!["Germany".into(), "France".into(), "Italy".into()],
            },
            [72.0, 460.0, 220.0, 482.0],
            "country",
        ),
    ];
    let listed: Vec<(FieldKind, Option<String>, Option<String>)> = fields()
        .into_iter()
        .map(|f| (f.kind, f.name, f.choice))
        .collect();
    check(
        "add text, checkbox, 2 radios in one group, dropdown",
        added.iter().all(Result::is_ok)
            && listed
                == vec![
                    (FieldKind::Text, Some("full_name".into()), None),
                    (
                        FieldKind::Checkbox,
                        Some("agree".into()),
                        Some("Yes".into()),
                    ),
                    (FieldKind::Radio, Some("size".into()), Some("Small".into())),
                    (FieldKind::Radio, Some("size".into()), Some("Large".into())),
                    (FieldKind::ComboBox, Some("country".into()), None),
                ],
        format!("{added:?}; {listed:?}"),
    );
    let dup = fe::add_field(0, &NewField::Text, [72.0, 400.0, 200.0, 420.0], "agree");
    check("a taken name is refused", dup.is_err(), format!("{dup:?}"));
    let (Some(text), Some(agree), Some(large), Some(combo)) = (
        find("full_name"),
        find("agree"),
        by_choice("Large"),
        find("country"),
    ) else {
        return missing("the new fields");
    };
    let filled = [
        pdit_core::fill_text(0, text.annotation, "Jordan Rivera"),
        pdit_core::toggle_choice(0, agree.annotation),
        pdit_core::toggle_choice(0, large.annotation),
        pdit_core::select_option(0, combo.annotation, 1),
    ];
    let state = |name: &str| find(name).map(|f| (f.value, f.checked));
    check(
        "fill them as today",
        filled.iter().all(Result::is_ok)
            && state("full_name") == Some((Some("Jordan Rivera".into()), false))
            && state("agree").is_some_and(|s| s.1)
            && by_choice("Large").is_some_and(|f| f.checked)
            && find("country").and_then(|f| f.value).as_deref() == Some("France"),
        format!("{filled:?}"),
    );
    let Some(text) = find("full_name") else {
        return missing("full_name");
    };
    let moved = fe::set_field_rect(0, text.annotation, rect(&text), [72.0, 600.0, 340.0, 630.0]);
    let after = find("full_name");
    check(
        "move + resize keeps the value",
        moved.is_ok()
            && after.as_ref().is_some_and(|f| {
                f.value.as_deref() == Some("Jordan Rivera") && (f.rect.2 - 340.0).abs() < 1.0
            }),
        format!("{moved:?}; {after:?}"),
    );
    let (Some(text), Some(large), Some(combo)) =
        (find("full_name"), by_choice("Large"), find("country"))
    else {
        return missing("fields for settings");
    };
    let renamed = fe::set_field_settings(
        0,
        text.annotation,
        rect(&text),
        &FieldSettings {
            name: "name".into(),
            required: true,
            options: None,
            choice: None,
        },
    );
    let rechoiced = fe::set_field_settings(
        0,
        large.annotation,
        rect(&large),
        &FieldSettings {
            name: "size".into(),
            required: false,
            options: None,
            choice: Some("Big".into()),
        },
    );
    let reoptioned = fe::set_field_settings(
        0,
        combo.annotation,
        rect(&combo),
        &FieldSettings {
            name: "country".into(),
            required: false,
            options: Some(vec!["Germany".into(), "Spain".into()]),
            choice: None,
        },
    );
    check(
        "settings: rename + required, radio choice, dropdown options",
        renamed.is_ok()
            && rechoiced.is_ok()
            && reoptioned.is_ok()
            && find("name")
                .is_some_and(|f| f.required && f.value.as_deref() == Some("Jordan Rivera"))
            && by_choice("Big").is_some_and(|f| f.checked)
            && find("country")
                .is_some_and(|f| f.options == ["Germany", "Spain"] && f.value.is_none()),
        format!("{renamed:?} {rechoiced:?} {reoptioned:?}; {:?}", fields()),
    );
    let Some(agree) = find("agree") else {
        return missing("agree");
    };
    let stale = fe::delete_field(0, agree.annotation, [1.0, 1.0, 2.0, 2.0]);
    check(
        "a stale selection is refused",
        stale.is_err(),
        format!("{stale:?}"),
    );
    let Some(small) = by_choice("Small") else {
        return missing("Small");
    };
    let d1 = fe::delete_field(0, small.annotation, rect(&small));
    let left_in_group = fields()
        .iter()
        .filter(|f| f.name.as_deref() == Some("size"))
        .count();
    let Some(big) = by_choice("Big") else {
        return missing("Big");
    };
    let d2 = fe::delete_field(0, big.annotation, rect(&big));
    let names = fe::field_names().unwrap_or_default();
    check(
        "delete: radio leaves its group; the group goes with its last button",
        d1.is_ok() && d2.is_ok() && left_in_group == 1 && !names.contains(&"size".to_string()),
        format!("{d1:?} {d2:?}; {left_in_group} left; names {names:?}"),
    );
    let saved = pdit_core::save_document().unwrap_or_default();
    let array = js_sys::Uint8Array::from(saved.as_slice());
    let _ = js_sys::Reflect::set(&js_sys::global(), &"__pditFormEditPdf".into(), &array);
    pdit_core::close_document();
    let reopened = pdit_core::open_document(saved).is_ok();
    let mut names: Vec<_> = fields().into_iter().filter_map(|f| f.name).collect();
    names.sort();
    check(
        "survive save + reopen",
        reopened && names == ["agree", "country", "name"],
        format!("{names:?}"),
    );
    pdit_core::close_document();
    // A document that already has a form keeps its fields.
    if pdit_core::open_document(FORM.to_vec()).is_ok() {
        let before = fields().len();
        let add = fe::add_field(0, &NewField::Text, [72.0, 100.0, 250.0, 120.0], "notes");
        let names: Vec<_> = fields().into_iter().filter_map(|f| f.name).collect();
        check(
            "add to an existing form",
            add.is_ok() && names.len() == before + 1 && names.contains(&"full_name".to_string()),
            format!("{add:?}; {names:?}"),
        );
        pdit_core::close_document();
    }
}

/// Real form fields (D-036) on the synthetic AcroForm fixture: list them, fill a
/// text field, toggle a checkbox, choose a combo option, and read the values back.
fn form_session() {
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: form: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if let Err(error) = pdit_core::open_document(FORM.to_vec()) {
        return check("open", false, error.to_string());
    }
    let value_of = |field: &str| {
        pdit_core::form_fields(0).ok().and_then(|fs| {
            fs.into_iter()
                .find(|f| f.name.as_deref() == Some(field))
                .and_then(|f| f.value)
        })
    };
    let annot_of = |field: &str| {
        pdit_core::form_fields(0).ok().and_then(|fs| {
            fs.into_iter()
                .find(|f| f.name.as_deref() == Some(field))
                .map(|f| f.annotation)
        })
    };
    let fields = pdit_core::form_fields(0);
    check(
        "lists the 4 fields",
        matches!(&fields, Ok(f) if f.len() == 4),
        format!(
            "{:?}",
            fields.as_ref().map(|f| f
                .iter()
                .map(|x| (x.kind, x.name.clone()))
                .collect::<Vec<_>>())
        ),
    );
    if let Some(annot) = annot_of("full_name") {
        let r = pdit_core::fill_text(0, annot, "Jordan Rivera");
        check(
            "fill a text field",
            r.is_ok() && value_of("full_name").as_deref() == Some("Jordan Rivera"),
            format!("{r:?} -> {:?}", value_of("full_name")),
        );
    }
    if let Some(annot) = annot_of("agree") {
        let before = pdit_core::form_fields(0).ok().and_then(|f| {
            f.into_iter()
                .find(|x| x.name.as_deref() == Some("agree"))
                .map(|x| x.checked)
        });
        let r = pdit_core::toggle_choice(0, annot);
        let after = pdit_core::form_fields(0).ok().and_then(|f| {
            f.into_iter()
                .find(|x| x.name.as_deref() == Some("agree"))
                .map(|x| x.checked)
        });
        check(
            "toggle a checkbox",
            r.is_ok() && before == Some(false) && after == Some(true),
            format!("{r:?}: {before:?} -> {after:?}"),
        );
    }
    if let Some(annot) = annot_of("country") {
        let r = pdit_core::select_option(0, annot, 1); // France
        check(
            "choose a combo option",
            r.is_ok(),
            format!("{r:?} -> {:?}", value_of("country")),
        );
    }
    pdit_core::close_document();
}

/// Text styles (D-027) on the rental fixture (standard Helvetica): read, restyle,
/// keep, read back, un-italic, plain edit keeps bold, Noto Sans choice.
fn style_session(font: &[u8]) {
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: style session: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    if let Err(error) = pdit_core::open_document(RENTAL.to_vec()) {
        return check("open", false, error.to_string());
    }
    let lines = pdit_core::text_lines(0).unwrap_or_default();
    let Some(line) = lines.iter().find(|l| l.text.contains("890")) else {
        return check("find line", false, format!("{} lines", lines.len()));
    };
    let (index, text) = (line.object_index, line.text.clone());
    let text_of = |index: usize| {
        pdit_core::text_lines(0)
            .ok()
            .and_then(|lines| lines.into_iter().find(|l| l.object_index == index))
            .map(|l| l.text)
            .unwrap_or_default()
    };
    let before = pdit_core::text_style(0, index);
    check(
        "read style",
        matches!(&before, Ok(s) if !s.bold && !s.italic && s.size > 4.0),
        format!("{before:?}"),
    );
    let Ok(before) = before else { return };
    let styled = TextStyle {
        size: before.size + 6.0,
        color: [211, 58, 44],
        bold: true,
        italic: true,
        ..before
    };
    let preview =
        pdit_core::preview_styled(0, index, &text, &styled, &pdit_core::Fonts::noto(font));
    check(
        "preview styled (own font)",
        matches!(&preview, Ok(p) if p.method == EditMethod::Native),
        format!("{preview:?}"),
    );
    pdit_core::keep_edit();
    let after = pdit_core::text_style(0, index);
    let close = |a: f32, b: f32| (a - b).abs() < 0.05;
    check(
        "keep and read back",
        matches!(&after, Ok(s) if s.bold && s.italic && close(s.size, styled.size) && s.color == styled.color),
        format!("{after:?}"),
    );
    let upright = TextStyle {
        italic: false,
        ..styled
    };
    let _ = pdit_core::preview_styled(0, index, &text, &upright, &pdit_core::Fonts::noto(font));
    pdit_core::keep_edit();
    let after = pdit_core::text_style(0, index);
    check(
        "italic off (no double slant)",
        matches!(&after, Ok(s) if !s.italic && s.bold),
        format!("{after:?}"),
    );
    let _ = pdit_core::preview_edit(
        0,
        index,
        "Monthly rent is 950 Euro cold.",
        &pdit_core::Fonts::noto(font),
    );
    pdit_core::keep_edit();
    let after = pdit_core::text_style(0, index);
    check(
        "plain edit keeps bold and size",
        matches!(&after, Ok(s) if s.bold && close(s.size, styled.size)),
        format!("{after:?}"),
    );
    // Underline: a pdit bar tied to its line (found again by shape).
    let underlined = TextStyle {
        underline: true,
        ..upright
    };
    let _ = pdit_core::preview_styled(
        0,
        index,
        "Monthly rent is 950 Euro cold.",
        &underlined,
        &pdit_core::Fonts::noto(font),
    );
    pdit_core::keep_edit();
    let after = pdit_core::text_style(0, index);
    check(
        "underline on",
        matches!(&after, Ok(s) if s.underline),
        format!("{after:?}"),
    );
    let _ = pdit_core::preview_edit(
        0,
        index,
        "Monthly rent is 1,050 Euro, cold.",
        &pdit_core::Fonts::noto(font),
    );
    pdit_core::keep_edit();
    let after = pdit_core::text_style(0, index);
    check(
        "underline follows a text edit (resized)",
        matches!(&after, Ok(s) if s.underline)
            && text_of(index) == "Monthly rent is 1,050 Euro, cold.",
        format!("{after:?}"),
    );
    let plain = TextStyle {
        underline: false,
        ..underlined
    };
    let _ = pdit_core::preview_styled(
        0,
        index,
        "Monthly rent is 1,050 Euro, cold.",
        &plain,
        &pdit_core::Fonts::noto(font),
    );
    let _ = pdit_core::discard_edit();
    let after = pdit_core::text_style(0, index);
    check(
        "discard keeps the underline",
        matches!(&after, Ok(s) if s.underline),
        format!("{after:?}"),
    );
    let _ = pdit_core::preview_styled(
        0,
        index,
        "Monthly rent is 1,050 Euro, cold.",
        &plain,
        &pdit_core::Fonts::noto(font),
    );
    pdit_core::keep_edit();
    let after = pdit_core::text_style(0, index);
    check(
        "underline off",
        matches!(&after, Ok(s) if !s.underline),
        format!("{after:?}"),
    );
    let _ = pdit_core::preview_styled(
        0,
        index,
        "Monthly rent is 950 Euro cold.",
        &underlined,
        &pdit_core::Fonts::noto(font),
    );
    pdit_core::keep_edit();
    let noto = TextStyle {
        font: FontChoice::NotoSans,
        ..upright
    };
    let preview = pdit_core::preview_styled(
        0,
        index,
        "Monthly rent is 950 Euro cold.",
        &noto,
        &pdit_core::Fonts::noto(font),
    );
    check(
        "Noto Sans choice",
        matches!(&preview, Ok(p) if p.method == EditMethod::FallbackFont),
        format!("{preview:?}"),
    );
    pdit_core::keep_edit();
    match pdit_core::save_document() {
        Ok(bytes) => {
            check("save", true, format!("{} bytes", bytes.len()));
            let array = js_sys::Uint8Array::from(bytes.as_slice());
            let _ = js_sys::Reflect::set(&js_sys::global(), &"__pditStylePdf".into(), &array);
        }
        Err(error) => check("save", false, error.to_string()),
    }
    pdit_core::close_document();
}

/// The bundled look-alike fonts (D-028): each of the 20 files is served,
/// parses, and subsets to a small font for a sample text.
async fn bundled_fonts() {
    use crate::font_catalog::{Family, load};
    let mut failures = Vec::new();
    let mut sizes = Vec::new();
    for family in Family::ALL {
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let name = format!("{family:?} bold={bold} italic={italic}");
            match load(family, bold, italic).await {
                Ok(bytes) => match pdit_core::subset::subset_for_text(&bytes, "Rent 890 €") {
                    Ok(subset) if subset.len() < bytes.len() / 4 => {
                        sizes.push(format!(
                            "{family:?}{}{}: {}→{}",
                            if bold { "B" } else { "" },
                            if italic { "I" } else { "" },
                            bytes.len(),
                            subset.len()
                        ));
                    }
                    Ok(subset) => {
                        failures.push(format!("{name}: subset not smaller ({})", subset.len()))
                    }
                    Err(error) => failures.push(format!("{name}: {error}")),
                },
                Err(error) => failures.push(format!("{name}: {error}")),
            }
        }
    }
    log(&format!(
        "pdit self-check {}: bundled fonts: {} ok; {}; failures: {failures:?}",
        if failures.is_empty() { "PASS" } else { "FAIL" },
        sizes.len(),
        sizes.join(", ")
    ));
}

/// Font matching (D-028): names map to the right family, and text a subset
/// font can't show goes to the real look-alike face, not simulated styles.
async fn look_alike_session(noto: &[u8]) {
    use crate::font_catalog::{Family, family_for, load};
    let traits = |name: &str, serif: bool, fixed_pitch: bool| pdit_core::FontTraits {
        name: name.to_owned(),
        bold: false,
        italic: false,
        serif,
        fixed_pitch,
        symbolic: false,
    };
    let cases = [
        (traits("Arial-BoldMT", false, false), Some(Family::Sans)),
        (traits("Helvetica", false, false), Some(Family::Sans)),
        (
            traits("TimesNewRomanPSMT", true, false),
            Some(Family::Serif),
        ),
        (traits("CourierNewPSMT", false, true), Some(Family::Mono)),
        (traits("Calibri-Light", false, false), Some(Family::Carlito)),
        (traits("Cambria", true, false), Some(Family::Caladea)),
        (traits("Garamond", true, false), Some(Family::Serif)),
        (traits("Consolas", false, true), Some(Family::Mono)),
        (traits("NotoSans-Regular", false, false), None),
    ];
    let wrong: Vec<_> = cases
        .iter()
        .filter(|(t, want)| family_for(t) != *want)
        .map(|(t, _)| t.name.clone())
        .collect();
    log(&format!(
        "pdit self-check {}: look-alike names: wrong {wrong:?}",
        if wrong.is_empty() { "PASS" } else { "FAIL" }
    ));

    let Ok(serif_bold) = load(Family::Serif, true, false).await else {
        return log("pdit self-check FAIL: look-alike: could not load Liberation Serif Bold");
    };
    if let Err(error) = pdit_core::open_document(SUBSET.to_vec()) {
        return log(&format!("pdit self-check FAIL: look-alike: open: {error}"));
    }
    let fonts = pdit_core::Fonts {
        look_alike: Some(pdit_core::LookAlike {
            bytes: &serif_bold,
            bold: true,
            italic: false,
        }),
        noto_sans: noto,
    };
    let added = pdit_core::style_near(0, 300.0, 744.0).and_then(|near| {
        let style = pdit_core::TextStyle { bold: true, ..near };
        pdit_core::preview_add(0, 300.0, 744.0, "Paid in full", Some(&style), &fonts)
    });
    let ok = matches!(&added, Ok(a) if a.preview.method == pdit_core::EditMethod::LookAlike);
    pdit_core::keep_edit();
    let index = added.as_ref().map(|a| a.object_index).unwrap_or(usize::MAX);
    let read = pdit_core::text_style(0, index);
    let traits = pdit_core::font_traits(0, index);
    log(&format!(
        "pdit self-check {}: look-alike: new text in Liberation Serif Bold, reads bold from the face: {added:?} / {read:?} / {traits:?}",
        if ok && matches!(&read, Ok(s) if s.bold) {
            "PASS"
        } else {
            "FAIL"
        }
    ));
    pdit_core::close_document();
}

/// Page tools (D-029) on the rental fixture: insert blank, duplicate, rotate,
/// extract, insert another PDF, delete with undo, and the last page kept.
fn page_session() {
    use pdit_core::page_ops as pages;
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: pages: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    let first_line = |page: u16| {
        pdit_core::text_lines(page)
            .ok()
            .and_then(|lines| lines.into_iter().next())
            .map(|l| l.text)
            .unwrap_or_default()
    };
    if let Err(error) = pdit_core::open_document(RENTAL.to_vec()) {
        return check("open", false, error.to_string());
    }
    let title = first_line(0);

    let blank = pages::insert_blank_page(0).and_then(|()| pages::page_sizes());
    check(
        "insert blank",
        matches!(&blank, Ok(s) if s.len() == 2 && s[1] == s[0])
            && pdit_core::text_lines(1)
                .map(|l| l.is_empty())
                .unwrap_or(false),
        format!("{blank:?}"),
    );
    let duplicated = pages::duplicate_page(0).and_then(|()| pages::page_sizes());
    check(
        "duplicate",
        matches!(&duplicated, Ok(s) if s.len() == 3) && first_line(1) == title,
        format!("{duplicated:?}, page 2 starts {:?}", first_line(1)),
    );
    let rotated = pages::rotate_page(0, true).and_then(|()| pages::page_sizes());
    check(
        "rotate clockwise",
        matches!(&rotated, Ok(s) if s[0].0 > s[0].1),
        format!("{rotated:?}"),
    );
    let _ = pages::rotate_page(0, false);
    let extracted = pages::extract_page(0);
    check(
        "extract",
        matches!(&extracted, Ok(b) if b.starts_with(b"%PDF") && b.len() > 500),
        format!("{:?} bytes", extracted.as_ref().map(|b| b.len())),
    );
    let inserted = pages::insert_pdf(SUBSET.to_vec(), 2);
    check(
        "insert another PDF",
        matches!(inserted, Ok(1))
            && pages::page_sizes().map(|s| s.len()).ok() == Some(4)
            && first_line(3).contains("Synthetic lease"),
        format!("{inserted:?}, page 4 starts {:?}", first_line(3)),
    );
    let undo = pages::snapshot();
    let deleted = pages::delete_page(1).and_then(|()| pages::page_sizes());
    let restored = undo.and_then(pages::restore);
    check(
        "delete, then undo",
        matches!(&deleted, Ok(s) if s.len() == 3) && matches!(&restored, Ok(s) if s.len() == 4),
        format!(
            "{:?} → {:?}",
            deleted.map(|s| s.len()),
            restored.map(|s| s.len())
        ),
    );
    for _ in 0..3 {
        let _ = pages::delete_page(0);
    }
    let last = pages::delete_page(0);
    check(
        "the last page stays",
        last.is_err() && pages::page_sizes().map(|s| s.len()).ok() == Some(1),
        format!("{last:?}"),
    );
    // move: a page keeps its content at its new index, both directions (P-031).
    if pdit_core::open_document(RENTAL.to_vec())
        .and_then(|_| pages::insert_pdf(SUBSET.to_vec(), 0))
        .is_ok()
    {
        let (a, b) = (first_line(0), first_line(1)); // subset, then rental
        let before = pdit_core::save_document().map(|b| b.len()).unwrap_or(0);
        let fwd = pages::move_page(0, 1);
        let after_fwd = (first_line(0), first_line(1));
        let after_move = pdit_core::save_document().map(|b| b.len()).unwrap_or(0);
        let back = pages::move_page(1, 0);
        let after_back = (first_line(0), first_line(1));
        check(
            "move a page and back",
            fwd.is_ok()
                && back.is_ok()
                && after_fwd == (b.clone(), a.clone())
                && after_back == (a.clone(), b.clone())
                && pages::page_sizes().map(|s| s.len()).ok() == Some(2),
            format!(
                "{after_fwd:?} then {after_back:?}; saved {before} → {after_move} bytes after one move"
            ),
        );
    }
    // add image: an image object embeds and the saved file grows (D-023).
    if pdit_core::open_document(RENTAL.to_vec()).is_ok() {
        let before = pdit_core::save_document().map(|b| b.len()).unwrap_or(0);
        let added = pages::add_image(0, IMAGE.to_vec(), 72.0, 720.0, 120.0);
        let after = pdit_core::save_document().map(|b| b.len()).unwrap_or(0);
        check(
            "add image",
            added.is_ok() && after > before,
            format!("saved {before} -> {after} bytes; {added:?}"),
        );
        // image select/edit (D-023a): find the image, rotate it (stays put via
        // re-centre), then delete it.
        let hit = pages::image_at(0, 132.0, 710.0).ok().flatten();
        check(
            "image_at finds the added image",
            matches!(&hit, Some(h) if h.bounds[0] < 132.0 && h.bounds[2] > 132.0),
            format!("{hit:?}"),
        );
        if let Some(h) = hit {
            let [l, b, r, t] = h.bounds;
            let (cx, cy) = ((l + r) / 2.0, (b + t) / 2.0);
            let rotated = pages::rotate_image(0, h.object_index, 90.0).and_then(|rb| {
                let (ncx, ncy) = ((rb[0] + rb[2]) / 2.0, (rb[1] + rb[3]) / 2.0);
                pages::move_image(0, h.object_index, cx - ncx, cy - ncy)
            });
            let centred = |bb: &[f32; 4]| {
                ((bb[0] + bb[2]) / 2.0 - cx).abs() < 1.0 && ((bb[1] + bb[3]) / 2.0 - cy).abs() < 1.0
            };
            check(
                "rotate image stays centred",
                matches!(&rotated, Ok(bb) if centred(bb)),
                format!("{rotated:?} vs centre ({cx:.0},{cy:.0})"),
            );
            let before = pages::page_sizes().ok();
            let deleted = pages::delete_image(0, h.object_index);
            check(
                "delete image, then no image there",
                deleted.is_ok()
                    && pages::image_at(0, 132.0, 710.0)
                        .map(|h| h.is_none())
                        .unwrap_or(false)
                    && pages::page_sizes().ok() == before,
                format!("{deleted:?}"),
            );
        }
    }
    if let Ok(bytes) = pdit_core::open_document(RENTAL.to_vec())
        .and_then(|_| pages::insert_pdf(SUBSET.to_vec(), 0))
        .and_then(|_| pages::rotate_page(1, true))
        .and_then(|()| pdit_core::save_document())
    {
        let array = js_sys::Uint8Array::from(bytes.as_slice());
        let _ = js_sys::Reflect::set(&js_sys::global(), &"__pditPagesPdf".into(), &array);
    }
    pdit_core::close_document();
}

/// Debug builds only: `window.__pditPages(op, a)` runs a page operation on
/// the open document, for reproducing page-tool problems from the console.
/// ops: open-rental, open-subset, count, snapshot, restore, rotate, blank,
/// dup, delete, extract, insert-rental, insert-subset.
fn expose_page_ops() {
    use pdit_core::page_ops as pages;
    use std::cell::RefCell;
    thread_local! {
        static SNAPSHOT: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    }
    let f = wasm_bindgen::closure::Closure::<dyn Fn(String, f64) -> String>::new(
        |op: String, a: f64| {
            let i = a as u16;
            let result: Result<String, pdit_core::Error> = match op.as_str() {
                "open-rental" => {
                    pdit_core::open_document(RENTAL.to_vec()).map(|s| s.len().to_string())
                }
                "open-subset" => {
                    pdit_core::open_document(SUBSET.to_vec()).map(|s| s.len().to_string())
                }
                "count" => pages::page_sizes().map(|s| s.len().to_string()),
                "snapshot" => pages::snapshot().map(|b| {
                    let n = b.len();
                    SNAPSHOT.with_borrow_mut(|s| *s = Some(b));
                    n.to_string()
                }),
                "restore" => match SNAPSHOT.with_borrow(|s| s.clone()) {
                    Some(b) => pages::restore(b).map(|s| s.len().to_string()),
                    None => Ok("no snapshot".into()),
                },
                "rotate" => pages::rotate_page(i, true).map(|()| "ok".into()),
                "blank" => pages::insert_blank_page(i).map(|()| "ok".into()),
                "dup" => pages::duplicate_page(i).map(|()| "ok".into()),
                "delete" => pages::delete_page(i).map(|()| "ok".into()),
                "extract" => pages::extract_page(i).map(|b| b.len().to_string()),
                "insert-rental" => pages::insert_pdf(RENTAL.to_vec(), i).map(|n| n.to_string()),
                "insert-subset" => pages::insert_pdf(SUBSET.to_vec(), i).map(|n| n.to_string()),
                // Positions only (never the text), so a user's own PDF can be
                // debugged without reading it.
                "line-bounds" => pdit_core::text_lines(i).map(|lines| {
                    let bounds: Vec<String> = lines
                        .iter()
                        .map(|l| {
                            let [a, b, c, d] = l.bounds;
                            format!("[{a:.1},{b:.1},{c:.1},{d:.1}]")
                        })
                        .collect();
                    format!("[{}]", bounds.join(","))
                }),
                other => Ok(format!("unknown op {other}")),
            };
            match result {
                Ok(v) => v,
                Err(e) => format!("error: {e}"),
            }
        },
    );
    let _ = js_sys::Reflect::set(&js_sys::global(), &"__pditPages".into(), f.as_ref());
    f.forget();
}

/// The saved file's name (D-020).
fn save_names() {
    let cases = [
        ("lease.pdf", "lease-edited.pdf"),
        ("Lease.PDF", "Lease-edited.pdf"),
        ("notes", "notes-edited.pdf"),
        (".pdf", "document-edited.pdf"),
        ("مستند.pdf", "مستند-edited.pdf"),
    ];
    let wrong: Vec<_> = cases
        .iter()
        .map(|(from, to)| (crate::save::edited_name(from), *to))
        .filter(|(got, want)| got != want)
        .collect();
    log(&format!(
        "pdit self-check {}: save names: {wrong:?}",
        if wrong.is_empty() { "PASS" } else { "FAIL" }
    ));
}

/// Preview → discard → preview → keep → unsupported, then add text, on the
/// subset-font fixture.
fn edit_session(font: &[u8]) {
    let check = |name: &str, ok: bool, detail: String| {
        log(&format!(
            "pdit self-check {}: edit session: {name}: {detail}",
            if ok { "PASS" } else { "FAIL" }
        ))
    };
    let text_of = |index: usize| {
        pdit_core::text_lines(0)
            .ok()
            .and_then(|lines| lines.into_iter().find(|l| l.object_index == index))
            .map(|l| l.text)
            .unwrap_or_default()
    };
    if let Err(error) = pdit_core::open_document(SUBSET.to_vec()) {
        return check("open", false, error.to_string());
    }
    let lines = pdit_core::text_lines(0).unwrap_or_default();
    let Some(rent) = lines.iter().find(|l| l.text.contains("890")) else {
        return check(
            "find line",
            false,
            format!("{} lines, none with 890", lines.len()),
        );
    };
    let index = rent.object_index;
    let original = rent.text.clone();
    check(
        "find line",
        true,
        format!("{} lines, object {index}", lines.len()),
    );

    let preview = pdit_core::preview_edit(
        0,
        index,
        "Monthly rent: 750 Euro",
        &pdit_core::Fonts::noto(font),
    );
    check(
        "preview with fallback",
        matches!(&preview, Ok(p) if p.method == EditMethod::FallbackFont && p.text == "Monthly rent: 750 Euro"),
        format!("{preview:?}"),
    );
    let discarded = pdit_core::discard_edit();
    let after = text_of(index);
    check(
        "discard restores",
        discarded.is_ok()
            && after == original
            && pdit_core::text_lines(0).map(|l| l.len()).ok() == Some(lines.len()),
        format!("{after:?}"),
    );

    let preview = pdit_core::preview_edit(
        0,
        index,
        "Monthly rent: 980 Euro",
        &pdit_core::Fonts::noto(font),
    );
    check(
        "preview native",
        matches!(&preview, Ok(p) if p.method == EditMethod::Native),
        format!("{preview:?}"),
    );
    pdit_core::keep_edit();
    check(
        "keep",
        text_of(index) == "Monthly rent: 980 Euro",
        text_of(index),
    );

    let bad = pdit_core::preview_edit(
        0,
        index,
        "Monthly rent: ۷۵۰ Euro",
        &pdit_core::Fonts::noto(font),
    );
    check(
        "unsupported characters leave the page unchanged",
        matches!(bad, Err(Error::UnsupportedCharacters { .. }))
            && text_of(index) == "Monthly rent: 980 Euro",
        text_of(index),
    );

    // Add text (D-026): next to the rent line, in its style when possible.
    let count = pdit_core::text_lines(0).map(|l| l.len()).unwrap_or(0);
    let (x, y) = (300.0, 744.0);
    let added =
        pdit_core::preview_add(0, x, y, "Paid in full", None, &pdit_core::Fonts::noto(font));
    check(
        "add text preview",
        matches!(&added, Ok(a) if a.preview.text == "Paid in full" && a.preview.bounds[0] >= x - 0.5),
        format!("{added:?}"),
    );
    let discarded = pdit_core::discard_edit();
    check(
        "add text discard removes it",
        discarded.is_ok() && pdit_core::text_lines(0).map(|l| l.len()).ok() == Some(count),
        format!(
            "{} lines",
            pdit_core::text_lines(0).map(|l| l.len()).unwrap_or(0)
        ),
    );
    let added =
        pdit_core::preview_add(0, x, y, "Paid in full", None, &pdit_core::Fonts::noto(font));
    pdit_core::keep_edit();
    let index = added.as_ref().map(|a| a.object_index).unwrap_or(usize::MAX);
    check(
        "add text keep",
        text_of(index) == "Paid in full",
        text_of(index),
    );
    let bad = pdit_core::preview_add(0, x, 700.0, "۷۵۰", None, &pdit_core::Fonts::noto(font));
    check(
        "add text unsupported adds nothing",
        matches!(bad, Err(Error::UnsupportedCharacters { .. }))
            && pdit_core::text_lines(0).map(|l| l.len()).ok() == Some(count + 1),
        format!("{bad:?}"),
    );

    match pdit_core::save_document() {
        Ok(bytes) => {
            check("save", true, format!("{} bytes", bytes.len()));
            // Exposed for external verification of the saved file (debug builds only).
            let array = js_sys::Uint8Array::from(bytes.as_slice());
            let _ = js_sys::Reflect::set(&js_sys::global(), &"__pditSelfCheckPdf".into(), &array);
        }
        Err(error) => check("save", false, error.to_string()),
    }
    pdit_core::close_document();
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url))
        .await?
        .dyn_into()?;
    let buffer = JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}
