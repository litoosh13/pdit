//! Text lines as they look, for click-to-edit: a PDF line is often several
//! text objects (split even inside a word), so pieces on one baseline are
//! joined into visual lines, and wrapped lines into paragraphs. Geometry only,
//! so it's tested natively; edit_open reads the pieces from PDFium.

use serde::Serialize;

/// A paragraph block: consecutive text lines that share a left margin, font size
/// and line spacing, joined into one editable unit for reflow (D-035 later item).
#[derive(Debug, Clone, Serialize)]
pub struct Paragraph {
    /// The block's text-object indices, in reading order (top to bottom).
    pub object_indices: Vec<usize>,
    /// The lines joined into one string (a space at each wrap; a trailing hyphen
    /// joins with none).
    pub text: String,
    /// Combined bounds [left, bottom, right, top] in PDF points.
    pub bounds: [f32; 4],
}

/// One text object, for grouping into visual lines.
#[derive(Debug, Clone)]
pub(crate) struct Piece {
    pub index: usize,
    pub text: String,
    /// Left, bottom, right, top in PDF points.
    pub bounds: [f32; 4],
    pub baseline: f32,
    pub size: f32,
}

/// A visual line: the pieces on one baseline that touch, left to right. Many
/// PDF writers split a line into several text objects, even inside a word
/// ("…been a" + "dmitted…"), so a line is not one object.
#[derive(Debug, Clone)]
pub(crate) struct Line {
    indices: Vec<usize>,
    text: String,
    l: f32,
    b: f32,
    r: f32,
    t: f32,
    size: f32,
}

impl Line {
    /// Its text objects, left to right.
    pub(crate) fn indices(&self) -> &[usize] {
        &self.indices
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Left, bottom, right, top in PDF points.
    pub(crate) fn bounds(&self) -> [f32; 4] {
        [self.l, self.b, self.r, self.t]
    }
}

/// Groups `pieces` into visual lines: pieces whose baselines are within a
/// third of the size of each other, left to right, split where the gap is over
/// 1.5 × the size (a table column). The text joins the pieces as they are, with
/// a space where they are apart and neither has one; control characters
/// (glyphs a font maps to nothing readable) are dropped.
pub(crate) fn visual_lines(mut pieces: Vec<Piece>) -> Vec<Line> {
    pieces.sort_by(|a, b| b.baseline.total_cmp(&a.baseline));
    let mut lines = Vec::new();
    let mut rest = pieces.as_slice();
    while let Some(first) = rest.first() {
        let near = rest
            .iter()
            .take_while(|p| first.baseline - p.baseline <= first.size.max(1.0) / 3.0)
            .count();
        let mut row = rest[..near].to_vec();
        rest = &rest[near..];
        row.sort_by(|a, b| a.bounds[0].total_cmp(&b.bounds[0]));
        let mut line: Option<Line> = None;
        for piece in row {
            let [l, b, r, t] = piece.bounds;
            let size = piece.size.max(1.0);
            let text: String = piece.text.chars().filter(|c| !c.is_control()).collect();
            match &mut line {
                Some(line) if l - line.r <= 1.5 * size.max(line.size) => {
                    let apart = l - line.r > 0.2 * size;
                    if apart
                        && !text.is_empty()
                        && !line.text.ends_with(' ')
                        && !text.starts_with(' ')
                    {
                        line.text.push(' ');
                    }
                    line.text.push_str(&text);
                    line.indices.push(piece.index);
                    line.l = line.l.min(l);
                    line.b = line.b.min(b);
                    line.r = line.r.max(r);
                    line.t = line.t.max(t);
                    line.size = line.size.max(size);
                }
                _ => {
                    lines.extend(line.take());
                    line = Some(Line {
                        indices: vec![piece.index],
                        text,
                        l,
                        b,
                        r,
                        t,
                        size,
                    });
                }
            }
        }
        lines.extend(line);
    }
    lines
}

/// `paragraph_at` over visual lines (top to bottom).
pub(crate) fn paragraph_in(lines: Vec<Line>, x: f32, y: f32) -> Option<Paragraph> {
    // The clicked line: the last whose bounds hold the point.
    let click = lines
        .iter()
        .rposition(|ln| x >= ln.l && x <= ln.r && y >= ln.b && y <= ln.t)?;
    // Reading order: top to bottom (descending top).
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&a, &b| lines[b].t.total_cmp(&lines[a].t));
    let pos = order.iter().position(|&i| i == click).unwrap();
    let base = &lines[click];
    let size = base.size.max(1.0);
    let aligned = |a: &Line| (a.l - base.l).abs() <= 3.0 && (a.size - size).abs() <= 0.2 * size;
    let spaced = |upper: &Line, lower: &Line| {
        let d = upper.t - lower.t;
        d > 0.6 * size && d < 2.2 * size
    };
    // The raw run of consecutive aligned, text-spaced lines around the click.
    let mut run_start = pos;
    while run_start > 0
        && aligned(&lines[order[run_start - 1]])
        && spaced(&lines[order[run_start - 1]], &lines[order[run_start]])
    {
        run_start -= 1;
    }
    let mut run_end = pos;
    while run_end + 1 < order.len()
        && aligned(&lines[order[run_end + 1]])
        && spaced(&lines[order[run_end]], &lines[order[run_end + 1]])
    {
        run_end += 1;
    }
    // Within the run, group by sentence: a line continues onto the next only
    // when it is "full" (reaches near the run's right margin, so it wrapped
    // for lack of room) and does not already finish a sentence (. ! ?). So a
    // wrapped sentence groups down to its period, while separate sentences or
    // a short line each stand alone.
    let run_max = order[run_start..=run_end]
        .iter()
        .map(|&i| lines[i].r)
        .fold(f32::MIN, f32::max);
    let slack = (2.5 * size).max(0.12 * (run_max - base.l));
    // Also wrapped: the next line's first word would not have fit after it
    // (a long word or web address leaves a short line). Its width is guessed
    // from the next line's width per character.
    let first_word_width = |next: &Line| {
        let text = next.text.trim();
        let word = text.split_whitespace().next().unwrap_or("");
        (next.r - next.l) * word.chars().count() as f32 / text.chars().count().max(1) as f32
    };
    let wraps = |a: &Line, next: &Line| {
        !a.text.trim_end().ends_with(['.', '!', '?'])
            && (a.r >= run_max - slack || a.r + 0.3 * size + first_word_width(next) > run_max)
    };
    let mut start = pos;
    while start > run_start && wraps(&lines[order[start - 1]], &lines[order[start]]) {
        start -= 1;
    }
    let mut end = pos;
    while end < run_end && wraps(&lines[order[end]], &lines[order[end + 1]]) {
        end += 1;
    }
    let mut text = String::new();
    let (mut l, mut b, mut r, mut t) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (k, &oi) in order[start..=end].iter().enumerate() {
        let ln = &lines[oi];
        if k > 0 {
            if text.ends_with('-') {
                text.pop();
            } else {
                text.push(' ');
            }
        }
        text.push_str(ln.text.trim());
        l = l.min(ln.l);
        b = b.min(ln.b);
        r = r.max(ln.r);
        t = t.max(ln.t);
    }
    // ponytail: collapses doubled spaces from pieces that each carry one.
    let text = text
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let object_indices = order[start..=end]
        .iter()
        .flat_map(|&oi| lines[oi].indices.iter().copied())
        .collect();
    Some(Paragraph {
        object_indices,
        text,
        bounds: [l, b, r, t],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(index: usize, text: &str, l: f32, r: f32, baseline: f32) -> Piece {
        Piece {
            index,
            text: text.into(),
            bounds: [l, baseline - 2.0, r, baseline + 7.0],
            baseline,
            size: 10.0,
        }
    }

    #[test]
    fn split_lines_group_into_one_paragraph() {
        // Like fixtures/synthetic-split-lines.pdf: lines split mid-word, out of
        // object order, a space-only piece, a control-character piece.
        let pieces = vec![
            piece(
                3,
                "provided each tool is cleaned and hung on its hook be",
                72.0,
                389.0,
                727.0,
            ),
            piece(
                0,
                "The garden club keeps a shared shed where members bor",
                72.0,
                389.0,
                740.0,
            ),
            piece(2, "row tools,", 390.0, 447.0, 740.3),
            piece(4, "fore dusk.", 390.0, 447.0, 727.0),
            piece(9, "\u{2}", 448.0, 450.0, 727.0),
            piece(5, "Visitors ", 72.0, 119.0, 690.0),
            piece(6, "", 120.0, 120.0, 690.0),
            piece(7, "sign the book.", 127.0, 207.0, 690.0),
            // A table cell far right on the same baseline: its own line.
            piece(8, "12.00", 500.0, 530.0, 690.0),
        ];
        let lines = visual_lines(pieces);
        let p = paragraph_in(lines.clone(), 420.0, 742.0).unwrap();
        assert_eq!(p.object_indices, vec![0, 2, 3, 4, 9]);
        assert_eq!(
            p.text,
            "The garden club keeps a shared shed where members borrow tools, provided each tool is cleaned and hung on its hook before dusk."
        );
        let v = paragraph_in(lines.clone(), 150.0, 692.0).unwrap();
        assert_eq!(v.object_indices, vec![5, 6, 7]);
        assert_eq!(v.text, "Visitors sign the book.");
        assert_eq!(
            paragraph_in(lines.clone(), 510.0, 692.0).unwrap().text,
            "12.00"
        );
        // Between pieces of a line still hits the line; empty space doesn't.
        assert!(paragraph_in(lines.clone(), 123.0, 692.0).is_some());
        assert!(paragraph_in(lines, 300.0, 600.0).is_none());
    }

    #[test]
    fn a_long_first_word_on_the_next_line_counts_as_a_wrap() {
        let lines = visual_lines(vec![
            piece(
                0,
                "Read the guide for every step, and for more information please visit",
                72.0,
                500.0,
                740.0,
            ),
            piece(
                1,
                "https://example.org/a/very/long/address/for/the/guide.",
                72.0,
                400.0,
                727.0,
            ),
            piece(2, "A short closing line.", 72.0, 200.0, 714.0),
            piece(
                3,
                "And a last full line that reaches across the column to the right edge.",
                72.0,
                560.0,
                701.0,
            ),
        ]);
        let p = paragraph_in(lines.clone(), 100.0, 742.0).unwrap();
        assert_eq!(p.object_indices, vec![0, 1]);
        // A short line whose next word would have fit stays alone.
        assert_eq!(
            paragraph_in(lines, 100.0, 716.0).unwrap().object_indices,
            vec![2]
        );
    }
}
