//! Find and replace (D-051). Matching runs per text object (a "line" in
//! pdit), so a replacement is an ordinary line edit through the text-edit
//! engine. A match split across two objects is not found.

/// Search options.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
}

/// Where `query` occurs in `text`: (start, length) in chars, left to right,
/// not overlapping.
pub fn find_in(text: &str, query: &str, options: FindOptions) -> Vec<(usize, usize)> {
    let fold = |c: char| {
        if options.match_case {
            c
        } else {
            // One char in, one char out keeps the indices aligned.
            c.to_lowercase().next().unwrap_or(c)
        }
    };
    let hay: Vec<char> = text.chars().map(fold).collect();
    let needle: Vec<char> = query.chars().map(fold).collect();
    let word = |i: usize| hay.get(i).is_some_and(|c| c.is_alphanumeric());
    let mut found = Vec::new();
    if needle.is_empty() || needle.len() > hay.len() {
        return found;
    }
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        let end = i + needle.len();
        let whole = !options.whole_word || (!(i > 0 && word(i - 1)) && !word(end));
        if hay[i..end] == needle[..] && whole {
            found.push((i, needle.len()));
            i = end;
        } else {
            i += 1;
        }
    }
    found
}

/// `text` with matches of `query` replaced by `with`: all of them, or only
/// the `only`-th (0-based) when given.
pub fn replace_in(
    text: &str,
    query: &str,
    with: &str,
    options: FindOptions,
    only: Option<usize>,
) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    for (n, (start, len)) in find_in(text, query, options).into_iter().enumerate() {
        if only.is_some_and(|k| k != n) {
            continue;
        }
        out.extend(&chars[at..start]);
        out.push_str(with);
        at = start + len;
    }
    out.extend(&chars[at..]);
    out
}

/// One match on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub page: u16,
    /// The text object (line) it is in, and which match in that line it is.
    pub object_index: usize,
    pub nth: usize,
    /// The match's box [left, bottom, right, top] in PDF points.
    pub rect: [f32; 4],
    /// The whole line, and the match's (start, length) in chars within it.
    pub line: String,
    pub span: (usize, usize),
}

/// Every match of `query` on `page`, in object order.
#[cfg(target_arch = "wasm32")]
pub fn find(page: u16, query: &str, options: FindOptions) -> Result<Vec<Match>, crate::Error> {
    use crate::annotations::{rect, union};
    use pdfium_render::prelude::*;
    crate::render::with_open(|document| {
        let pdf_page = document.pages().get(page.into())?;
        let text_page = pdf_page.text()?;
        let mut found = Vec::new();
        for (object_index, object) in pdf_page.objects().iter().enumerate() {
            let Some(text_object) = object.as_text_object() else {
                continue;
            };
            let line = text_object.text();
            let spans = find_in(&line, query, options);
            if spans.is_empty() {
                continue;
            }
            let whole = crate::inspect::rect(object.bounds()?);
            // Per-char boxes when PDFium's chars line up with the object's
            // text; otherwise the whole line's box.
            let boxes: Vec<Option<[f32; 4]>> = text_page
                .chars_for_object(text_object)
                .map(|chars| {
                    chars
                        .iter()
                        .map(|c| {
                            let blank = c.unicode_char().is_none_or(char::is_whitespace);
                            (!blank)
                                .then(|| c.loose_bounds().ok().map(|r| rect(&r)))
                                .flatten()
                        })
                        .collect()
                })
                .unwrap_or_default();
            let aligned = boxes.len() == line.chars().count();
            for (nth, (start, len)) in spans.into_iter().enumerate() {
                let rect = if aligned {
                    boxes[start..start + len]
                        .iter()
                        .flatten()
                        .copied()
                        .reduce(union)
                        .unwrap_or(whole)
                } else {
                    whole
                };
                found.push(Match {
                    page,
                    object_index,
                    nth,
                    rect,
                    line: line.clone(),
                    span: (start, len),
                });
            }
        }
        Ok(found)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_replaces() {
        let any = FindOptions::default();
        let whole = FindOptions {
            whole_word: true,
            ..any
        };
        let case = FindOptions {
            match_case: true,
            ..whole
        };
        let text = "Rent is rent; rental rent";
        assert_eq!(find_in(text, "rent", any).len(), 4);
        assert_eq!(find_in(text, "rent", whole), vec![(0, 4), (8, 4), (21, 4)]);
        assert_eq!(find_in(text, "rent", case), vec![(8, 4), (21, 4)]);
        assert_eq!(find_in("aaa", "aa", any), vec![(0, 2)]);
        assert!(find_in("x", "", any).is_empty());
        assert_eq!(find_in("Größe größe", "GRÖSSE", any).len(), 0);
        assert_eq!(find_in("Größe größe", "grö", any), vec![(0, 3), (6, 3)]);
        assert_eq!(
            replace_in(text, "rent", "fee", whole, None),
            "fee is fee; rental fee"
        );
        assert_eq!(
            replace_in(text, "rent", "fee", whole, Some(1)),
            "Rent is fee; rental rent"
        );
        assert_eq!(
            replace_in("890 € cold", "€", "Euro", any, None),
            "890 Euro cold"
        );
    }
}
