//! Split into files (D-070): which pages go in each file.

/// How to split.
#[derive(Clone, Debug, PartialEq)]
pub enum Split {
    EveryPage,
    /// Every n pages.
    Every(u16),
    /// After these page numbers (from 1), e.g. "3, 5".
    After(String),
}

/// The files for a document of `pages` pages: (first, last) page indices (from 0), in order.
pub fn groups(pages: u16, how: &Split) -> Vec<(u16, u16)> {
    if pages == 0 {
        return Vec::new();
    }
    let step = match how {
        Split::EveryPage => 1,
        Split::Every(n) => (*n).max(1),
        Split::After(list) => {
            let mut cuts: Vec<u16> = list
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter_map(|n| n.trim().parse().ok())
                .filter(|&n| n >= 1 && n < pages)
                .collect();
            cuts.sort_unstable();
            cuts.dedup();
            let mut out = Vec::new();
            let mut first = 0;
            for cut in cuts {
                out.push((first, cut - 1));
                first = cut;
            }
            out.push((first, pages - 1));
            return out;
        }
    };
    (0..pages)
        .step_by(usize::from(step))
        .map(|first| (first, (first + step - 1).min(pages - 1)))
        .collect()
}

/// "pages 1–2" / "page 3" for a group.
pub fn describe((first, last): (u16, u16)) -> String {
    if first == last {
        format!("page {}", first + 1)
    } else {
        format!("pages {}–{}", first + 1, last + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits() {
        assert_eq!(groups(3, &Split::EveryPage), [(0, 0), (1, 1), (2, 2)]);
        assert_eq!(groups(5, &Split::Every(2)), [(0, 1), (2, 3), (4, 4)]);
        assert_eq!(groups(5, &Split::Every(0)), groups(5, &Split::EveryPage));
        assert_eq!(
            groups(6, &Split::After("3, 5".into())),
            [(0, 2), (3, 4), (5, 5)]
        );
        // Out of range, repeated and junk numbers are ignored; no cut is one file.
        assert_eq!(
            groups(4, &Split::After("9 0 x 2 2".into())),
            [(0, 1), (2, 3)]
        );
        assert_eq!(groups(4, &Split::After(String::new())), [(0, 3)]);
        assert_eq!(describe((0, 1)), "pages 1–2");
        assert_eq!(describe((2, 2)), "page 3");
    }
}
