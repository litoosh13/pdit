//! Page numbers (page labels, D-070): the numbers a PDF reader shows for each page — "i, ii, iii, 1, 2, 3" —
//! as ranges: from a page on, a style, a prefix and a starting number (PDF 1.7, 12.4.2).

/// How a range numbers its pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// 1, 2, 3
    Decimal,
    /// i, ii, iii
    RomanLower,
    /// I, II, III
    RomanUpper,
    /// a, b, … z, aa, bb
    LettersLower,
    /// A, B, … Z, AA, BB
    LettersUpper,
    /// The prefix only.
    None,
}

impl Style {
    pub const ALL: [Style; 6] = [
        Style::Decimal,
        Style::RomanLower,
        Style::RomanUpper,
        Style::LettersLower,
        Style::LettersUpper,
        Style::None,
    ];

    /// "1, 2, 3", for the style menu.
    pub fn name(self) -> &'static str {
        match self {
            Style::Decimal => "1, 2, 3",
            Style::RomanLower => "i, ii, iii",
            Style::RomanUpper => "I, II, III",
            Style::LettersLower => "a, b, c",
            Style::LettersUpper => "A, B, C",
            Style::None => "No number",
        }
    }

    /// The PDF's /S letter (MuPDF's pdf_page_label_style), or 0 for none.
    pub fn code(self) -> u8 {
        match self {
            Style::Decimal => b'D',
            Style::RomanLower => b'r',
            Style::RomanUpper => b'R',
            Style::LettersLower => b'a',
            Style::LettersUpper => b'A',
            Style::None => 0,
        }
    }

    pub fn from_code(code: u8) -> Style {
        Style::ALL
            .into_iter()
            .find(|s| s.code() == code)
            .unwrap_or(Style::None)
    }

    fn number(self, n: u32) -> String {
        match self {
            Style::Decimal => n.to_string(),
            Style::RomanLower => roman(n),
            Style::RomanUpper => roman(n).to_uppercase(),
            Style::LettersLower => letters(n),
            Style::LettersUpper => letters(n).to_uppercase(),
            Style::None => String::new(),
        }
    }
}

/// From page `first` (from 0) on: `prefix` + the number, counting from `start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    pub first: u16,
    pub style: Style,
    pub prefix: String,
    pub start: u32,
}

impl Range {
    /// The ordinary numbering: 1, 2, 3 from the first page.
    pub fn plain() -> Range {
        Range {
            first: 0,
            style: Style::Decimal,
            prefix: String::new(),
            start: 1,
        }
    }
}

/// Just the ordinary numbering (no labels needed).
pub fn is_plain(ranges: &[Range]) -> bool {
    ranges.is_empty() || ranges == [Range::plain()]
}

/// Page `page`'s label (pages before the first range: their own number).
pub fn label(ranges: &[Range], page: u16) -> String {
    match ranges
        .iter()
        .filter(|r| r.first <= page)
        .max_by_key(|r| r.first)
    {
        Some(r) => format!(
            "{}{}",
            r.prefix,
            r.style.number(r.start + u32::from(page - r.first))
        ),
        None => (u32::from(page) + 1).to_string(),
    }
}

/// The ranges behind each page's label as a reader reports it (None: no label): consecutive pages with the
/// same prefix and style and the next number share a range.
pub fn ranges_from_labels(labels: &[Option<String>]) -> Vec<Range> {
    if labels.iter().all(Option::is_none) {
        return vec![Range::plain()];
    }
    let mut out: Vec<Range> = Vec::new();
    let mut expected: Option<(Style, String, u32)> = None;
    for (page, label) in labels.iter().enumerate() {
        let (prefix, style, n) = label
            .as_deref()
            .map_or_else(|| (String::new(), Style::Decimal, page as u32 + 1), parse);
        let continues = expected.as_ref().is_some_and(|(s, p, e)| {
            *s == style && *p == prefix && (*e == n || style == Style::None)
        });
        if !continues {
            out.push(Range {
                first: page as u16,
                style,
                prefix: prefix.clone(),
                start: n.max(1),
            });
        }
        expected = Some((style, prefix, n + 1));
    }
    out
}

/// A label's prefix, style and number.
fn parse(label: &str) -> (String, Style, u32) {
    let digits = label.len() - label.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0
        && let Ok(n) = label[label.len() - digits..].parse()
    {
        return (label[..label.len() - digits].to_owned(), Style::Decimal, n);
    }
    // Roman numerals and letters count only after nothing or a non-letter ("Cover", "Index" are words).
    let tail = |f: fn(char) -> bool| {
        let n = label.len() - label.trim_end_matches(f).len();
        let before = label[..label.len() - n].chars().last();
        if before.is_some_and(|c| c.is_alphabetic()) {
            0
        } else {
            n
        }
    };
    let lower = tail(|c| "ivxlcdm".contains(c));
    if lower > 0
        && let Some(n) = from_roman(&label[label.len() - lower..])
    {
        return (
            label[..label.len() - lower].to_owned(),
            Style::RomanLower,
            n,
        );
    }
    let upper = tail(|c| "IVXLCDM".contains(c));
    if upper > 0
        && let Some(n) = from_roman(&label[label.len() - upper..].to_lowercase())
    {
        return (
            label[..label.len() - upper].to_owned(),
            Style::RomanUpper,
            n,
        );
    }
    if let Some(last) = label.chars().last().filter(char::is_ascii_alphabetic) {
        let run = label.len() - label.trim_end_matches(last).len();
        if label[..label.len() - run]
            .chars()
            .last()
            .is_some_and(|c| c.is_alphabetic())
        {
            return (label.to_owned(), Style::None, 1);
        }
        let n = u32::from(last.to_ascii_lowercase() as u8 - b'a') + 1 + 26 * (run as u32 - 1);
        let style = if last.is_ascii_lowercase() {
            Style::LettersLower
        } else {
            Style::LettersUpper
        };
        return (label[..label.len() - run].to_owned(), style, n);
    }
    (label.to_owned(), Style::None, 1)
}

fn roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut s = String::new();
    for (value, digits) in TABLE {
        while n >= value {
            s += digits;
            n -= value;
        }
    }
    s
}

/// A lowercase roman numeral's value, only when written the standard way.
fn from_roman(s: &str) -> Option<u32> {
    (1..4000).find(|&n| roman(n) == s)
}

/// a, b, … z, aa, bb, … (PDF letter numbering: the letter repeated).
fn letters(n: u32) -> String {
    let n = n.max(1) - 1;
    let c = (b'a' + (n % 26) as u8) as char;
    c.to_string().repeat(n as usize / 26 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(first: u16, style: Style, prefix: &str, start: u32) -> Range {
        Range {
            first,
            style,
            prefix: prefix.into(),
            start,
        }
    }

    #[test]
    fn labels_from_ranges() {
        let front = [r(0, Style::RomanLower, "", 1), r(3, Style::Decimal, "", 1)];
        let got: Vec<String> = (0..6).map(|p| label(&front, p)).collect();
        assert_eq!(got, ["i", "ii", "iii", "1", "2", "3"]);
        assert_eq!(
            label(&[r(0, Style::LettersUpper, "App. ", 26)], 1),
            "App. AA"
        );
        assert_eq!(label(&[r(0, Style::None, "Cover", 1)], 0), "Cover");
        assert_eq!(label(&[r(2, Style::Decimal, "", 1)], 0), "1");
        assert_eq!(roman(1994), "mcmxciv");
        assert!(is_plain(&[Range::plain()]) && !is_plain(&front));
    }

    #[test]
    fn ranges_read_back_from_labels() {
        let ranges = vec![
            r(0, Style::None, "Cover", 1),
            r(1, Style::RomanLower, "", 1),
            r(4, Style::Decimal, "", 1),
            r(7, Style::Decimal, "A-", 1),
            r(9, Style::LettersLower, "", 27),
        ];
        let labels: Vec<Option<String>> = (0..10).map(|p| Some(label(&ranges, p))).collect();
        assert_eq!(ranges_from_labels(&labels), ranges, "{labels:?}");
        assert_eq!(ranges_from_labels(&[None, None]), [Range::plain()]);
        // A jump in numbering starts a new range.
        let jump = [Some("1".into()), Some("2".into()), Some("10".into())];
        assert_eq!(
            ranges_from_labels(&jump),
            [r(0, Style::Decimal, "", 1), r(2, Style::Decimal, "", 10)]
        );
    }
}
