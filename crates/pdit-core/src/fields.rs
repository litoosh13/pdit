//! Finding form fields on a page (D-053): leafmind's field finder looks at
//! the rendered page and suggests text boxes, checkboxes and signature areas;
//! each gets a name from the label beside it. Turning suggestions into real
//! fields is `form_edit::add_field`.

pub use leafmind_fields::FieldFinder;

/// What a suggestion becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundKind {
    Text,
    Checkbox,
    /// A signature line: becomes a text field.
    Signature,
}

/// One suggested field.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundField {
    pub kind: FoundKind,
    /// [left, bottom, right, top] in PDF points.
    pub rect: [f32; 4],
    /// Already filled in on the page (by hand or typed), when the finder knows.
    pub filled: Option<bool>,
    /// From the label beside it, e.g. "full_name"; unique on the page.
    pub name: String,
}

/// Page width the finder looks at, in pixels (its model works at 640; the
/// finder also reads the page's top and bottom halves).
#[cfg(target_arch = "wasm32")]
const FIND_WIDTH_PX: u32 = 1280;

/// The fields `finder` sees on `page`, top to bottom.
#[cfg(target_arch = "wasm32")]
pub fn find_fields(finder: &FieldFinder, page: u16) -> Result<Vec<FoundField>, crate::Error> {
    let image = crate::render_page(page, FIND_WIDTH_PX)?;
    let (width, height) = (image.width(), image.height());
    let found = finder
        .find(&image.data(), width, height)
        .map_err(|e| crate::Error::Pdfium(format!("finding fields: {e}")))?;
    let (width_pt, height_pt) = crate::page_ops::page_sizes()?
        .get(usize::from(page))
        .copied()
        .ok_or_else(|| crate::Error::Pdfium("no such page".into()))?;
    let k = width_pt / width as f32;
    let words: Vec<(String, [f32; 4])> = crate::annotations::words(page)
        .unwrap_or_default()
        .into_iter()
        .map(|w| (w.text, w.bounds))
        .collect();
    let mut fields: Vec<FoundField> = found
        .into_iter()
        .map(|f| {
            let [l, t, r, b] = f.bounds;
            let rect = [l * k, height_pt - b * k, r * k, height_pt - t * k];
            let kind = match f.kind {
                leafmind_fields::FieldKind::Text => FoundKind::Text,
                leafmind_fields::FieldKind::Choice => FoundKind::Checkbox,
                leafmind_fields::FieldKind::Signature => FoundKind::Signature,
            };
            FoundField {
                kind,
                rect,
                filled: f.filled,
                name: String::new(),
            }
        })
        .collect();
    fields.sort_by(|a, b| {
        b.rect[3]
            .total_cmp(&a.rect[3])
            .then(a.rect[0].total_cmp(&b.rect[0]))
    });
    let mut taken = Vec::new();
    for field in &mut fields {
        let base = match field.kind {
            FoundKind::Signature => "signature".to_owned(),
            kind => label_for(&words, field.rect, kind == FoundKind::Checkbox),
        };
        field.name = unique(&base, &taken);
        taken.push(field.name.clone());
    }
    Ok(fields)
}

/// The words labelling a field at `rect`: on its line, just left of it (a
/// checkbox's label is usually just right of it, so that is tried first).
pub fn label_for(words: &[(String, [f32; 4])], rect: [f32; 4], label_right: bool) -> String {
    let [l, b, r, t] = rect;
    let middle = (b + t) / 2.0;
    let on_line = |w: &&(String, [f32; 4])| w.1[1] <= middle + 4.0 && w.1[3] >= middle - 4.0;
    let mut left: Vec<_> = words
        .iter()
        .filter(on_line)
        .filter(|w| w.1[2] <= l + 2.0 && l - w.1[2] < 160.0)
        .collect();
    left.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
    let mut right: Vec<_> = words
        .iter()
        .filter(on_line)
        .filter(|w| w.1[0] >= r - 2.0 && w.1[0] - r < 40.0)
        .collect();
    right.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
    // The last words before the field, as long as they run together.
    let mut label: Vec<&str> = Vec::new();
    let mut edge = l;
    for w in left.iter().rev() {
        if edge - w.1[2] > 12.0 && !label.is_empty() || label.len() == 4 {
            break;
        }
        label.insert(0, &w.0);
        edge = w.1[0];
    }
    let words: Vec<&str> = if label_right && !right.is_empty() {
        right.iter().take(4).map(|w| w.0.as_str()).collect()
    } else if !label.is_empty() {
        label
    } else {
        right.iter().take(4).map(|w| w.0.as_str()).collect()
    };
    let name = slug(&words.join(" "));
    if name.is_empty() {
        "field".into()
    } else {
        name
    }
}

/// "Full name:" → "full_name".
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_owned()
}

/// `base`, or `base_2`, `base_3`, … if taken.
pub fn unique(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, l: f32, r: f32) -> (String, [f32; 4]) {
        (text.into(), [l, 700.0, r, 712.0])
    }

    #[test]
    fn names_from_labels() {
        assert_eq!(slug("Full name:"), "full_name");
        assert_eq!(slug("E-Mail (work)"), "e_mail_work");
        assert_eq!(slug("Straße Nr."), "straße_nr");
        let taken = vec!["email".to_owned(), "email_2".to_owned()];
        assert_eq!(unique("email", &taken), "email_3");
        assert_eq!(unique("phone", &taken), "phone");
        let words = [
            word("Section", 20.0, 45.0),
            word("Full", 60.0, 80.0),
            word("name", 83.0, 110.0),
            word("Basic", 178.0, 205.0),
        ];
        // Text box after "Full name" (the far "Section" is a separate label).
        assert_eq!(
            label_for(&words, [140.0, 698.0, 300.0, 714.0], false),
            "full_name"
        );
        // Checkbox with its label on the right.
        assert_eq!(
            label_for(&words, [160.0, 700.0, 173.0, 713.0], true),
            "basic"
        );
        assert_eq!(label_for(&[], [0.0, 0.0, 10.0, 10.0], false), "field");
    }
}
