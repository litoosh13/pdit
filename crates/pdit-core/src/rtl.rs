//! Persian / Arabic text read from older PDFs (P-050 #18). Many Persian books store each line in display order
//! (left to right on the page) as Arabic presentation forms (one code per letter shape): PDFium then reads a line
//! mirrored letter by letter, MuPDF mostly with its words in mirrored order. [`logical_line`] turns such a line into
//! reading order with plain letters, so it can be edited; text without presentation forms is left as it is.

/// Whether `c` is a right-to-left letter (Hebrew, Arabic and their presentation forms).
pub fn is_rtl(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Whether `text` has Arabic presentation forms: an older PDF that stores lines in display order (see the module).
pub fn in_display_order(text: &str) -> bool {
    text.chars().any(|c| form(c).is_some())
}

/// The letters a presentation form stands for, and its position in a word (0 alone, 1 end, 2 start, 3 middle).
fn form(c: char) -> Option<(&'static str, u8)> {
    FORMS
        .binary_search_by_key(&(c as u32), |f| f.0)
        .ok()
        .map(|i| (FORMS[i].1, FORMS[i].2))
}

/// `text` with plain letters: presentation forms become the letters they stand for; the stretching line (tatweel,
/// U+0640) goes.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match form(c) {
            Some((letters, _)) => out.push_str(letters),
            None if c == '\u{0640}' => {}
            None => out.push(c),
        }
    }
    out
}

/// One line of text as a PDF stores it, in reading order with plain letters. A line with presentation forms is in
/// display order: when most of its words begin with an end form (or end with a start form) it is mirrored letter
/// by letter and is reversed (numbers and Latin words kept left to right); otherwise only its words are in mirrored
/// order and they are reversed. A line without presentation forms is only cleaned ([`plain`]).
pub fn logical_line(line: &str) -> String {
    if !line.chars().any(|c| form(c).is_some()) {
        return plain(line);
    }
    let (mut forward, mut backward) = (0, 0);
    for word in line.split_whitespace() {
        let shapes: Vec<u8> = word.chars().filter_map(|c| form(c).map(|f| f.1)).collect();
        let (Some(&first), Some(&last)) = (shapes.first(), shapes.last()) else {
            continue;
        };
        if shapes.len() < 2 {
            continue;
        }
        if first == 2 || last == 1 {
            forward += 1;
        }
        if first == 1 || last == 2 {
            backward += 1;
        }
    }
    let ordered = if backward > forward {
        mirror(line)
    } else {
        line.split_whitespace()
            .rev()
            .map(reverse_parts)
            .collect::<Vec<_>>()
            .join(" ")
    };
    plain(&ordered)
}

/// Several lines (top to bottom) of one paragraph, each [`logical_line`], joined by spaces.
pub fn logical_paragraph<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    lines
        .into_iter()
        .map(logical_line)
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Characters that keep left-to-right order inside right-to-left text: digits (Latin, Arabic, Persian) and Latin
/// letters, with the separators of numbers.
fn keeps_ltr(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c as u32, 0x0660..=0x0669 | 0x06F0..=0x06F9)
}

/// A line mirrored letter by letter, reversed: runs of numbers / Latin back to left to right, brackets turned.
fn mirror(line: &str) -> String {
    let reversed: Vec<char> = line.chars().rev().map(turn_bracket).collect();
    let mut out = String::with_capacity(line.len());
    let mut run: Vec<char> = Vec::new();
    let flush = |run: &mut Vec<char>, out: &mut String| {
        out.extend(run.iter().rev());
        run.clear();
    };
    for (i, &c) in reversed.iter().enumerate() {
        // A number's "." or "," stays inside its run.
        let inside = matches!(c, '.' | ',' | '/' | ':')
            && i > 0
            && keeps_ltr(reversed[i - 1])
            && reversed.get(i + 1).is_some_and(|n| keeps_ltr(*n));
        if keeps_ltr(c) || (inside && !run.is_empty()) {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// A word whose parts, glued by punctuation ("a:b", "a،b"), are in mirrored order: the parts reversed.
fn reverse_parts(word: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in word.chars() {
        if matches!(c, ':' | '،' | '؛' | '.' | '!' | '؟' | '?' | ',' | ';') && !current.is_empty()
        {
            parts.push(std::mem::take(&mut current));
            parts.push(c.to_string());
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    if parts.len() < 2 {
        return word.to_owned();
    }
    parts.reverse();
    parts.concat()
}

fn turn_bracket(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '«' => '»',
        '»' => '«',
        other => other,
    }
}

/// Arabic presentation forms (U+FB50–FDFF, U+FE70–FEFF): (form, the letters it stands for, its position: 0 alone,
/// 1 end, 2 start, 3 middle of a word). Generated from Unicode's decompositions (Python unicodedata), sorted.
pub(crate) const FORMS: [(u32, &str, u8); 731] = [
    (0xFB50, "\u{0671}", 0),
    (0xFB51, "\u{0671}", 1),
    (0xFB52, "\u{067B}", 0),
    (0xFB53, "\u{067B}", 1),
    (0xFB54, "\u{067B}", 2),
    (0xFB55, "\u{067B}", 3),
    (0xFB56, "\u{067E}", 0),
    (0xFB57, "\u{067E}", 1),
    (0xFB58, "\u{067E}", 2),
    (0xFB59, "\u{067E}", 3),
    (0xFB5A, "\u{0680}", 0),
    (0xFB5B, "\u{0680}", 1),
    (0xFB5C, "\u{0680}", 2),
    (0xFB5D, "\u{0680}", 3),
    (0xFB5E, "\u{067A}", 0),
    (0xFB5F, "\u{067A}", 1),
    (0xFB60, "\u{067A}", 2),
    (0xFB61, "\u{067A}", 3),
    (0xFB62, "\u{067F}", 0),
    (0xFB63, "\u{067F}", 1),
    (0xFB64, "\u{067F}", 2),
    (0xFB65, "\u{067F}", 3),
    (0xFB66, "\u{0679}", 0),
    (0xFB67, "\u{0679}", 1),
    (0xFB68, "\u{0679}", 2),
    (0xFB69, "\u{0679}", 3),
    (0xFB6A, "\u{06A4}", 0),
    (0xFB6B, "\u{06A4}", 1),
    (0xFB6C, "\u{06A4}", 2),
    (0xFB6D, "\u{06A4}", 3),
    (0xFB6E, "\u{06A6}", 0),
    (0xFB6F, "\u{06A6}", 1),
    (0xFB70, "\u{06A6}", 2),
    (0xFB71, "\u{06A6}", 3),
    (0xFB72, "\u{0684}", 0),
    (0xFB73, "\u{0684}", 1),
    (0xFB74, "\u{0684}", 2),
    (0xFB75, "\u{0684}", 3),
    (0xFB76, "\u{0683}", 0),
    (0xFB77, "\u{0683}", 1),
    (0xFB78, "\u{0683}", 2),
    (0xFB79, "\u{0683}", 3),
    (0xFB7A, "\u{0686}", 0),
    (0xFB7B, "\u{0686}", 1),
    (0xFB7C, "\u{0686}", 2),
    (0xFB7D, "\u{0686}", 3),
    (0xFB7E, "\u{0687}", 0),
    (0xFB7F, "\u{0687}", 1),
    (0xFB80, "\u{0687}", 2),
    (0xFB81, "\u{0687}", 3),
    (0xFB82, "\u{068D}", 0),
    (0xFB83, "\u{068D}", 1),
    (0xFB84, "\u{068C}", 0),
    (0xFB85, "\u{068C}", 1),
    (0xFB86, "\u{068E}", 0),
    (0xFB87, "\u{068E}", 1),
    (0xFB88, "\u{0688}", 0),
    (0xFB89, "\u{0688}", 1),
    (0xFB8A, "\u{0698}", 0),
    (0xFB8B, "\u{0698}", 1),
    (0xFB8C, "\u{0691}", 0),
    (0xFB8D, "\u{0691}", 1),
    (0xFB8E, "\u{06A9}", 0),
    (0xFB8F, "\u{06A9}", 1),
    (0xFB90, "\u{06A9}", 2),
    (0xFB91, "\u{06A9}", 3),
    (0xFB92, "\u{06AF}", 0),
    (0xFB93, "\u{06AF}", 1),
    (0xFB94, "\u{06AF}", 2),
    (0xFB95, "\u{06AF}", 3),
    (0xFB96, "\u{06B3}", 0),
    (0xFB97, "\u{06B3}", 1),
    (0xFB98, "\u{06B3}", 2),
    (0xFB99, "\u{06B3}", 3),
    (0xFB9A, "\u{06B1}", 0),
    (0xFB9B, "\u{06B1}", 1),
    (0xFB9C, "\u{06B1}", 2),
    (0xFB9D, "\u{06B1}", 3),
    (0xFB9E, "\u{06BA}", 0),
    (0xFB9F, "\u{06BA}", 1),
    (0xFBA0, "\u{06BB}", 0),
    (0xFBA1, "\u{06BB}", 1),
    (0xFBA2, "\u{06BB}", 2),
    (0xFBA3, "\u{06BB}", 3),
    (0xFBA4, "\u{06C0}", 0),
    (0xFBA5, "\u{06C0}", 1),
    (0xFBA6, "\u{06C1}", 0),
    (0xFBA7, "\u{06C1}", 1),
    (0xFBA8, "\u{06C1}", 2),
    (0xFBA9, "\u{06C1}", 3),
    (0xFBAA, "\u{06BE}", 0),
    (0xFBAB, "\u{06BE}", 1),
    (0xFBAC, "\u{06BE}", 2),
    (0xFBAD, "\u{06BE}", 3),
    (0xFBAE, "\u{06D2}", 0),
    (0xFBAF, "\u{06D2}", 1),
    (0xFBB0, "\u{06D3}", 0),
    (0xFBB1, "\u{06D3}", 1),
    (0xFBD3, "\u{06AD}", 0),
    (0xFBD4, "\u{06AD}", 1),
    (0xFBD5, "\u{06AD}", 2),
    (0xFBD6, "\u{06AD}", 3),
    (0xFBD7, "\u{06C7}", 0),
    (0xFBD8, "\u{06C7}", 1),
    (0xFBD9, "\u{06C6}", 0),
    (0xFBDA, "\u{06C6}", 1),
    (0xFBDB, "\u{06C8}", 0),
    (0xFBDC, "\u{06C8}", 1),
    (0xFBDD, "\u{0677}", 0),
    (0xFBDE, "\u{06CB}", 0),
    (0xFBDF, "\u{06CB}", 1),
    (0xFBE0, "\u{06C5}", 0),
    (0xFBE1, "\u{06C5}", 1),
    (0xFBE2, "\u{06C9}", 0),
    (0xFBE3, "\u{06C9}", 1),
    (0xFBE4, "\u{06D0}", 0),
    (0xFBE5, "\u{06D0}", 1),
    (0xFBE6, "\u{06D0}", 2),
    (0xFBE7, "\u{06D0}", 3),
    (0xFBE8, "\u{0649}", 2),
    (0xFBE9, "\u{0649}", 3),
    (0xFBEA, "\u{0626}\u{0627}", 0),
    (0xFBEB, "\u{0626}\u{0627}", 1),
    (0xFBEC, "\u{0626}\u{06D5}", 0),
    (0xFBED, "\u{0626}\u{06D5}", 1),
    (0xFBEE, "\u{0626}\u{0648}", 0),
    (0xFBEF, "\u{0626}\u{0648}", 1),
    (0xFBF0, "\u{0626}\u{06C7}", 0),
    (0xFBF1, "\u{0626}\u{06C7}", 1),
    (0xFBF2, "\u{0626}\u{06C6}", 0),
    (0xFBF3, "\u{0626}\u{06C6}", 1),
    (0xFBF4, "\u{0626}\u{06C8}", 0),
    (0xFBF5, "\u{0626}\u{06C8}", 1),
    (0xFBF6, "\u{0626}\u{06D0}", 0),
    (0xFBF7, "\u{0626}\u{06D0}", 1),
    (0xFBF8, "\u{0626}\u{06D0}", 2),
    (0xFBF9, "\u{0626}\u{0649}", 0),
    (0xFBFA, "\u{0626}\u{0649}", 1),
    (0xFBFB, "\u{0626}\u{0649}", 2),
    (0xFBFC, "\u{06CC}", 0),
    (0xFBFD, "\u{06CC}", 1),
    (0xFBFE, "\u{06CC}", 2),
    (0xFBFF, "\u{06CC}", 3),
    (0xFC00, "\u{0626}\u{062C}", 0),
    (0xFC01, "\u{0626}\u{062D}", 0),
    (0xFC02, "\u{0626}\u{0645}", 0),
    (0xFC03, "\u{0626}\u{0649}", 0),
    (0xFC04, "\u{0626}\u{064A}", 0),
    (0xFC05, "\u{0628}\u{062C}", 0),
    (0xFC06, "\u{0628}\u{062D}", 0),
    (0xFC07, "\u{0628}\u{062E}", 0),
    (0xFC08, "\u{0628}\u{0645}", 0),
    (0xFC09, "\u{0628}\u{0649}", 0),
    (0xFC0A, "\u{0628}\u{064A}", 0),
    (0xFC0B, "\u{062A}\u{062C}", 0),
    (0xFC0C, "\u{062A}\u{062D}", 0),
    (0xFC0D, "\u{062A}\u{062E}", 0),
    (0xFC0E, "\u{062A}\u{0645}", 0),
    (0xFC0F, "\u{062A}\u{0649}", 0),
    (0xFC10, "\u{062A}\u{064A}", 0),
    (0xFC11, "\u{062B}\u{062C}", 0),
    (0xFC12, "\u{062B}\u{0645}", 0),
    (0xFC13, "\u{062B}\u{0649}", 0),
    (0xFC14, "\u{062B}\u{064A}", 0),
    (0xFC15, "\u{062C}\u{062D}", 0),
    (0xFC16, "\u{062C}\u{0645}", 0),
    (0xFC17, "\u{062D}\u{062C}", 0),
    (0xFC18, "\u{062D}\u{0645}", 0),
    (0xFC19, "\u{062E}\u{062C}", 0),
    (0xFC1A, "\u{062E}\u{062D}", 0),
    (0xFC1B, "\u{062E}\u{0645}", 0),
    (0xFC1C, "\u{0633}\u{062C}", 0),
    (0xFC1D, "\u{0633}\u{062D}", 0),
    (0xFC1E, "\u{0633}\u{062E}", 0),
    (0xFC1F, "\u{0633}\u{0645}", 0),
    (0xFC20, "\u{0635}\u{062D}", 0),
    (0xFC21, "\u{0635}\u{0645}", 0),
    (0xFC22, "\u{0636}\u{062C}", 0),
    (0xFC23, "\u{0636}\u{062D}", 0),
    (0xFC24, "\u{0636}\u{062E}", 0),
    (0xFC25, "\u{0636}\u{0645}", 0),
    (0xFC26, "\u{0637}\u{062D}", 0),
    (0xFC27, "\u{0637}\u{0645}", 0),
    (0xFC28, "\u{0638}\u{0645}", 0),
    (0xFC29, "\u{0639}\u{062C}", 0),
    (0xFC2A, "\u{0639}\u{0645}", 0),
    (0xFC2B, "\u{063A}\u{062C}", 0),
    (0xFC2C, "\u{063A}\u{0645}", 0),
    (0xFC2D, "\u{0641}\u{062C}", 0),
    (0xFC2E, "\u{0641}\u{062D}", 0),
    (0xFC2F, "\u{0641}\u{062E}", 0),
    (0xFC30, "\u{0641}\u{0645}", 0),
    (0xFC31, "\u{0641}\u{0649}", 0),
    (0xFC32, "\u{0641}\u{064A}", 0),
    (0xFC33, "\u{0642}\u{062D}", 0),
    (0xFC34, "\u{0642}\u{0645}", 0),
    (0xFC35, "\u{0642}\u{0649}", 0),
    (0xFC36, "\u{0642}\u{064A}", 0),
    (0xFC37, "\u{0643}\u{0627}", 0),
    (0xFC38, "\u{0643}\u{062C}", 0),
    (0xFC39, "\u{0643}\u{062D}", 0),
    (0xFC3A, "\u{0643}\u{062E}", 0),
    (0xFC3B, "\u{0643}\u{0644}", 0),
    (0xFC3C, "\u{0643}\u{0645}", 0),
    (0xFC3D, "\u{0643}\u{0649}", 0),
    (0xFC3E, "\u{0643}\u{064A}", 0),
    (0xFC3F, "\u{0644}\u{062C}", 0),
    (0xFC40, "\u{0644}\u{062D}", 0),
    (0xFC41, "\u{0644}\u{062E}", 0),
    (0xFC42, "\u{0644}\u{0645}", 0),
    (0xFC43, "\u{0644}\u{0649}", 0),
    (0xFC44, "\u{0644}\u{064A}", 0),
    (0xFC45, "\u{0645}\u{062C}", 0),
    (0xFC46, "\u{0645}\u{062D}", 0),
    (0xFC47, "\u{0645}\u{062E}", 0),
    (0xFC48, "\u{0645}\u{0645}", 0),
    (0xFC49, "\u{0645}\u{0649}", 0),
    (0xFC4A, "\u{0645}\u{064A}", 0),
    (0xFC4B, "\u{0646}\u{062C}", 0),
    (0xFC4C, "\u{0646}\u{062D}", 0),
    (0xFC4D, "\u{0646}\u{062E}", 0),
    (0xFC4E, "\u{0646}\u{0645}", 0),
    (0xFC4F, "\u{0646}\u{0649}", 0),
    (0xFC50, "\u{0646}\u{064A}", 0),
    (0xFC51, "\u{0647}\u{062C}", 0),
    (0xFC52, "\u{0647}\u{0645}", 0),
    (0xFC53, "\u{0647}\u{0649}", 0),
    (0xFC54, "\u{0647}\u{064A}", 0),
    (0xFC55, "\u{064A}\u{062C}", 0),
    (0xFC56, "\u{064A}\u{062D}", 0),
    (0xFC57, "\u{064A}\u{062E}", 0),
    (0xFC58, "\u{064A}\u{0645}", 0),
    (0xFC59, "\u{064A}\u{0649}", 0),
    (0xFC5A, "\u{064A}\u{064A}", 0),
    (0xFC5B, "\u{0630}\u{0670}", 0),
    (0xFC5C, "\u{0631}\u{0670}", 0),
    (0xFC5D, "\u{0649}\u{0670}", 0),
    (0xFC5E, "\u{0020}\u{064C}\u{0651}", 0),
    (0xFC5F, "\u{0020}\u{064D}\u{0651}", 0),
    (0xFC60, "\u{0020}\u{064E}\u{0651}", 0),
    (0xFC61, "\u{0020}\u{064F}\u{0651}", 0),
    (0xFC62, "\u{0020}\u{0650}\u{0651}", 0),
    (0xFC63, "\u{0020}\u{0651}\u{0670}", 0),
    (0xFC64, "\u{0626}\u{0631}", 1),
    (0xFC65, "\u{0626}\u{0632}", 1),
    (0xFC66, "\u{0626}\u{0645}", 1),
    (0xFC67, "\u{0626}\u{0646}", 1),
    (0xFC68, "\u{0626}\u{0649}", 1),
    (0xFC69, "\u{0626}\u{064A}", 1),
    (0xFC6A, "\u{0628}\u{0631}", 1),
    (0xFC6B, "\u{0628}\u{0632}", 1),
    (0xFC6C, "\u{0628}\u{0645}", 1),
    (0xFC6D, "\u{0628}\u{0646}", 1),
    (0xFC6E, "\u{0628}\u{0649}", 1),
    (0xFC6F, "\u{0628}\u{064A}", 1),
    (0xFC70, "\u{062A}\u{0631}", 1),
    (0xFC71, "\u{062A}\u{0632}", 1),
    (0xFC72, "\u{062A}\u{0645}", 1),
    (0xFC73, "\u{062A}\u{0646}", 1),
    (0xFC74, "\u{062A}\u{0649}", 1),
    (0xFC75, "\u{062A}\u{064A}", 1),
    (0xFC76, "\u{062B}\u{0631}", 1),
    (0xFC77, "\u{062B}\u{0632}", 1),
    (0xFC78, "\u{062B}\u{0645}", 1),
    (0xFC79, "\u{062B}\u{0646}", 1),
    (0xFC7A, "\u{062B}\u{0649}", 1),
    (0xFC7B, "\u{062B}\u{064A}", 1),
    (0xFC7C, "\u{0641}\u{0649}", 1),
    (0xFC7D, "\u{0641}\u{064A}", 1),
    (0xFC7E, "\u{0642}\u{0649}", 1),
    (0xFC7F, "\u{0642}\u{064A}", 1),
    (0xFC80, "\u{0643}\u{0627}", 1),
    (0xFC81, "\u{0643}\u{0644}", 1),
    (0xFC82, "\u{0643}\u{0645}", 1),
    (0xFC83, "\u{0643}\u{0649}", 1),
    (0xFC84, "\u{0643}\u{064A}", 1),
    (0xFC85, "\u{0644}\u{0645}", 1),
    (0xFC86, "\u{0644}\u{0649}", 1),
    (0xFC87, "\u{0644}\u{064A}", 1),
    (0xFC88, "\u{0645}\u{0627}", 1),
    (0xFC89, "\u{0645}\u{0645}", 1),
    (0xFC8A, "\u{0646}\u{0631}", 1),
    (0xFC8B, "\u{0646}\u{0632}", 1),
    (0xFC8C, "\u{0646}\u{0645}", 1),
    (0xFC8D, "\u{0646}\u{0646}", 1),
    (0xFC8E, "\u{0646}\u{0649}", 1),
    (0xFC8F, "\u{0646}\u{064A}", 1),
    (0xFC90, "\u{0649}\u{0670}", 1),
    (0xFC91, "\u{064A}\u{0631}", 1),
    (0xFC92, "\u{064A}\u{0632}", 1),
    (0xFC93, "\u{064A}\u{0645}", 1),
    (0xFC94, "\u{064A}\u{0646}", 1),
    (0xFC95, "\u{064A}\u{0649}", 1),
    (0xFC96, "\u{064A}\u{064A}", 1),
    (0xFC97, "\u{0626}\u{062C}", 2),
    (0xFC98, "\u{0626}\u{062D}", 2),
    (0xFC99, "\u{0626}\u{062E}", 2),
    (0xFC9A, "\u{0626}\u{0645}", 2),
    (0xFC9B, "\u{0626}\u{0647}", 2),
    (0xFC9C, "\u{0628}\u{062C}", 2),
    (0xFC9D, "\u{0628}\u{062D}", 2),
    (0xFC9E, "\u{0628}\u{062E}", 2),
    (0xFC9F, "\u{0628}\u{0645}", 2),
    (0xFCA0, "\u{0628}\u{0647}", 2),
    (0xFCA1, "\u{062A}\u{062C}", 2),
    (0xFCA2, "\u{062A}\u{062D}", 2),
    (0xFCA3, "\u{062A}\u{062E}", 2),
    (0xFCA4, "\u{062A}\u{0645}", 2),
    (0xFCA5, "\u{062A}\u{0647}", 2),
    (0xFCA6, "\u{062B}\u{0645}", 2),
    (0xFCA7, "\u{062C}\u{062D}", 2),
    (0xFCA8, "\u{062C}\u{0645}", 2),
    (0xFCA9, "\u{062D}\u{062C}", 2),
    (0xFCAA, "\u{062D}\u{0645}", 2),
    (0xFCAB, "\u{062E}\u{062C}", 2),
    (0xFCAC, "\u{062E}\u{0645}", 2),
    (0xFCAD, "\u{0633}\u{062C}", 2),
    (0xFCAE, "\u{0633}\u{062D}", 2),
    (0xFCAF, "\u{0633}\u{062E}", 2),
    (0xFCB0, "\u{0633}\u{0645}", 2),
    (0xFCB1, "\u{0635}\u{062D}", 2),
    (0xFCB2, "\u{0635}\u{062E}", 2),
    (0xFCB3, "\u{0635}\u{0645}", 2),
    (0xFCB4, "\u{0636}\u{062C}", 2),
    (0xFCB5, "\u{0636}\u{062D}", 2),
    (0xFCB6, "\u{0636}\u{062E}", 2),
    (0xFCB7, "\u{0636}\u{0645}", 2),
    (0xFCB8, "\u{0637}\u{062D}", 2),
    (0xFCB9, "\u{0638}\u{0645}", 2),
    (0xFCBA, "\u{0639}\u{062C}", 2),
    (0xFCBB, "\u{0639}\u{0645}", 2),
    (0xFCBC, "\u{063A}\u{062C}", 2),
    (0xFCBD, "\u{063A}\u{0645}", 2),
    (0xFCBE, "\u{0641}\u{062C}", 2),
    (0xFCBF, "\u{0641}\u{062D}", 2),
    (0xFCC0, "\u{0641}\u{062E}", 2),
    (0xFCC1, "\u{0641}\u{0645}", 2),
    (0xFCC2, "\u{0642}\u{062D}", 2),
    (0xFCC3, "\u{0642}\u{0645}", 2),
    (0xFCC4, "\u{0643}\u{062C}", 2),
    (0xFCC5, "\u{0643}\u{062D}", 2),
    (0xFCC6, "\u{0643}\u{062E}", 2),
    (0xFCC7, "\u{0643}\u{0644}", 2),
    (0xFCC8, "\u{0643}\u{0645}", 2),
    (0xFCC9, "\u{0644}\u{062C}", 2),
    (0xFCCA, "\u{0644}\u{062D}", 2),
    (0xFCCB, "\u{0644}\u{062E}", 2),
    (0xFCCC, "\u{0644}\u{0645}", 2),
    (0xFCCD, "\u{0644}\u{0647}", 2),
    (0xFCCE, "\u{0645}\u{062C}", 2),
    (0xFCCF, "\u{0645}\u{062D}", 2),
    (0xFCD0, "\u{0645}\u{062E}", 2),
    (0xFCD1, "\u{0645}\u{0645}", 2),
    (0xFCD2, "\u{0646}\u{062C}", 2),
    (0xFCD3, "\u{0646}\u{062D}", 2),
    (0xFCD4, "\u{0646}\u{062E}", 2),
    (0xFCD5, "\u{0646}\u{0645}", 2),
    (0xFCD6, "\u{0646}\u{0647}", 2),
    (0xFCD7, "\u{0647}\u{062C}", 2),
    (0xFCD8, "\u{0647}\u{0645}", 2),
    (0xFCD9, "\u{0647}\u{0670}", 2),
    (0xFCDA, "\u{064A}\u{062C}", 2),
    (0xFCDB, "\u{064A}\u{062D}", 2),
    (0xFCDC, "\u{064A}\u{062E}", 2),
    (0xFCDD, "\u{064A}\u{0645}", 2),
    (0xFCDE, "\u{064A}\u{0647}", 2),
    (0xFCDF, "\u{0626}\u{0645}", 3),
    (0xFCE0, "\u{0626}\u{0647}", 3),
    (0xFCE1, "\u{0628}\u{0645}", 3),
    (0xFCE2, "\u{0628}\u{0647}", 3),
    (0xFCE3, "\u{062A}\u{0645}", 3),
    (0xFCE4, "\u{062A}\u{0647}", 3),
    (0xFCE5, "\u{062B}\u{0645}", 3),
    (0xFCE6, "\u{062B}\u{0647}", 3),
    (0xFCE7, "\u{0633}\u{0645}", 3),
    (0xFCE8, "\u{0633}\u{0647}", 3),
    (0xFCE9, "\u{0634}\u{0645}", 3),
    (0xFCEA, "\u{0634}\u{0647}", 3),
    (0xFCEB, "\u{0643}\u{0644}", 3),
    (0xFCEC, "\u{0643}\u{0645}", 3),
    (0xFCED, "\u{0644}\u{0645}", 3),
    (0xFCEE, "\u{0646}\u{0645}", 3),
    (0xFCEF, "\u{0646}\u{0647}", 3),
    (0xFCF0, "\u{064A}\u{0645}", 3),
    (0xFCF1, "\u{064A}\u{0647}", 3),
    (0xFCF2, "\u{0640}\u{064E}\u{0651}", 3),
    (0xFCF3, "\u{0640}\u{064F}\u{0651}", 3),
    (0xFCF4, "\u{0640}\u{0650}\u{0651}", 3),
    (0xFCF5, "\u{0637}\u{0649}", 0),
    (0xFCF6, "\u{0637}\u{064A}", 0),
    (0xFCF7, "\u{0639}\u{0649}", 0),
    (0xFCF8, "\u{0639}\u{064A}", 0),
    (0xFCF9, "\u{063A}\u{0649}", 0),
    (0xFCFA, "\u{063A}\u{064A}", 0),
    (0xFCFB, "\u{0633}\u{0649}", 0),
    (0xFCFC, "\u{0633}\u{064A}", 0),
    (0xFCFD, "\u{0634}\u{0649}", 0),
    (0xFCFE, "\u{0634}\u{064A}", 0),
    (0xFCFF, "\u{062D}\u{0649}", 0),
    (0xFD00, "\u{062D}\u{064A}", 0),
    (0xFD01, "\u{062C}\u{0649}", 0),
    (0xFD02, "\u{062C}\u{064A}", 0),
    (0xFD03, "\u{062E}\u{0649}", 0),
    (0xFD04, "\u{062E}\u{064A}", 0),
    (0xFD05, "\u{0635}\u{0649}", 0),
    (0xFD06, "\u{0635}\u{064A}", 0),
    (0xFD07, "\u{0636}\u{0649}", 0),
    (0xFD08, "\u{0636}\u{064A}", 0),
    (0xFD09, "\u{0634}\u{062C}", 0),
    (0xFD0A, "\u{0634}\u{062D}", 0),
    (0xFD0B, "\u{0634}\u{062E}", 0),
    (0xFD0C, "\u{0634}\u{0645}", 0),
    (0xFD0D, "\u{0634}\u{0631}", 0),
    (0xFD0E, "\u{0633}\u{0631}", 0),
    (0xFD0F, "\u{0635}\u{0631}", 0),
    (0xFD10, "\u{0636}\u{0631}", 0),
    (0xFD11, "\u{0637}\u{0649}", 1),
    (0xFD12, "\u{0637}\u{064A}", 1),
    (0xFD13, "\u{0639}\u{0649}", 1),
    (0xFD14, "\u{0639}\u{064A}", 1),
    (0xFD15, "\u{063A}\u{0649}", 1),
    (0xFD16, "\u{063A}\u{064A}", 1),
    (0xFD17, "\u{0633}\u{0649}", 1),
    (0xFD18, "\u{0633}\u{064A}", 1),
    (0xFD19, "\u{0634}\u{0649}", 1),
    (0xFD1A, "\u{0634}\u{064A}", 1),
    (0xFD1B, "\u{062D}\u{0649}", 1),
    (0xFD1C, "\u{062D}\u{064A}", 1),
    (0xFD1D, "\u{062C}\u{0649}", 1),
    (0xFD1E, "\u{062C}\u{064A}", 1),
    (0xFD1F, "\u{062E}\u{0649}", 1),
    (0xFD20, "\u{062E}\u{064A}", 1),
    (0xFD21, "\u{0635}\u{0649}", 1),
    (0xFD22, "\u{0635}\u{064A}", 1),
    (0xFD23, "\u{0636}\u{0649}", 1),
    (0xFD24, "\u{0636}\u{064A}", 1),
    (0xFD25, "\u{0634}\u{062C}", 1),
    (0xFD26, "\u{0634}\u{062D}", 1),
    (0xFD27, "\u{0634}\u{062E}", 1),
    (0xFD28, "\u{0634}\u{0645}", 1),
    (0xFD29, "\u{0634}\u{0631}", 1),
    (0xFD2A, "\u{0633}\u{0631}", 1),
    (0xFD2B, "\u{0635}\u{0631}", 1),
    (0xFD2C, "\u{0636}\u{0631}", 1),
    (0xFD2D, "\u{0634}\u{062C}", 2),
    (0xFD2E, "\u{0634}\u{062D}", 2),
    (0xFD2F, "\u{0634}\u{062E}", 2),
    (0xFD30, "\u{0634}\u{0645}", 2),
    (0xFD31, "\u{0633}\u{0647}", 2),
    (0xFD32, "\u{0634}\u{0647}", 2),
    (0xFD33, "\u{0637}\u{0645}", 2),
    (0xFD34, "\u{0633}\u{062C}", 3),
    (0xFD35, "\u{0633}\u{062D}", 3),
    (0xFD36, "\u{0633}\u{062E}", 3),
    (0xFD37, "\u{0634}\u{062C}", 3),
    (0xFD38, "\u{0634}\u{062D}", 3),
    (0xFD39, "\u{0634}\u{062E}", 3),
    (0xFD3A, "\u{0637}\u{0645}", 3),
    (0xFD3B, "\u{0638}\u{0645}", 3),
    (0xFD3C, "\u{0627}\u{064B}", 1),
    (0xFD3D, "\u{0627}\u{064B}", 0),
    (0xFD50, "\u{062A}\u{062C}\u{0645}", 2),
    (0xFD51, "\u{062A}\u{062D}\u{062C}", 1),
    (0xFD52, "\u{062A}\u{062D}\u{062C}", 2),
    (0xFD53, "\u{062A}\u{062D}\u{0645}", 2),
    (0xFD54, "\u{062A}\u{062E}\u{0645}", 2),
    (0xFD55, "\u{062A}\u{0645}\u{062C}", 2),
    (0xFD56, "\u{062A}\u{0645}\u{062D}", 2),
    (0xFD57, "\u{062A}\u{0645}\u{062E}", 2),
    (0xFD58, "\u{062C}\u{0645}\u{062D}", 1),
    (0xFD59, "\u{062C}\u{0645}\u{062D}", 2),
    (0xFD5A, "\u{062D}\u{0645}\u{064A}", 1),
    (0xFD5B, "\u{062D}\u{0645}\u{0649}", 1),
    (0xFD5C, "\u{0633}\u{062D}\u{062C}", 2),
    (0xFD5D, "\u{0633}\u{062C}\u{062D}", 2),
    (0xFD5E, "\u{0633}\u{062C}\u{0649}", 1),
    (0xFD5F, "\u{0633}\u{0645}\u{062D}", 1),
    (0xFD60, "\u{0633}\u{0645}\u{062D}", 2),
    (0xFD61, "\u{0633}\u{0645}\u{062C}", 2),
    (0xFD62, "\u{0633}\u{0645}\u{0645}", 1),
    (0xFD63, "\u{0633}\u{0645}\u{0645}", 2),
    (0xFD64, "\u{0635}\u{062D}\u{062D}", 1),
    (0xFD65, "\u{0635}\u{062D}\u{062D}", 2),
    (0xFD66, "\u{0635}\u{0645}\u{0645}", 1),
    (0xFD67, "\u{0634}\u{062D}\u{0645}", 1),
    (0xFD68, "\u{0634}\u{062D}\u{0645}", 2),
    (0xFD69, "\u{0634}\u{062C}\u{064A}", 1),
    (0xFD6A, "\u{0634}\u{0645}\u{062E}", 1),
    (0xFD6B, "\u{0634}\u{0645}\u{062E}", 2),
    (0xFD6C, "\u{0634}\u{0645}\u{0645}", 1),
    (0xFD6D, "\u{0634}\u{0645}\u{0645}", 2),
    (0xFD6E, "\u{0636}\u{062D}\u{0649}", 1),
    (0xFD6F, "\u{0636}\u{062E}\u{0645}", 1),
    (0xFD70, "\u{0636}\u{062E}\u{0645}", 2),
    (0xFD71, "\u{0637}\u{0645}\u{062D}", 1),
    (0xFD72, "\u{0637}\u{0645}\u{062D}", 2),
    (0xFD73, "\u{0637}\u{0645}\u{0645}", 2),
    (0xFD74, "\u{0637}\u{0645}\u{064A}", 1),
    (0xFD75, "\u{0639}\u{062C}\u{0645}", 1),
    (0xFD76, "\u{0639}\u{0645}\u{0645}", 1),
    (0xFD77, "\u{0639}\u{0645}\u{0645}", 2),
    (0xFD78, "\u{0639}\u{0645}\u{0649}", 1),
    (0xFD79, "\u{063A}\u{0645}\u{0645}", 1),
    (0xFD7A, "\u{063A}\u{0645}\u{064A}", 1),
    (0xFD7B, "\u{063A}\u{0645}\u{0649}", 1),
    (0xFD7C, "\u{0641}\u{062E}\u{0645}", 1),
    (0xFD7D, "\u{0641}\u{062E}\u{0645}", 2),
    (0xFD7E, "\u{0642}\u{0645}\u{062D}", 1),
    (0xFD7F, "\u{0642}\u{0645}\u{0645}", 1),
    (0xFD80, "\u{0644}\u{062D}\u{0645}", 1),
    (0xFD81, "\u{0644}\u{062D}\u{064A}", 1),
    (0xFD82, "\u{0644}\u{062D}\u{0649}", 1),
    (0xFD83, "\u{0644}\u{062C}\u{062C}", 2),
    (0xFD84, "\u{0644}\u{062C}\u{062C}", 1),
    (0xFD85, "\u{0644}\u{062E}\u{0645}", 1),
    (0xFD86, "\u{0644}\u{062E}\u{0645}", 2),
    (0xFD87, "\u{0644}\u{0645}\u{062D}", 1),
    (0xFD88, "\u{0644}\u{0645}\u{062D}", 2),
    (0xFD89, "\u{0645}\u{062D}\u{062C}", 2),
    (0xFD8A, "\u{0645}\u{062D}\u{0645}", 2),
    (0xFD8B, "\u{0645}\u{062D}\u{064A}", 1),
    (0xFD8C, "\u{0645}\u{062C}\u{062D}", 2),
    (0xFD8D, "\u{0645}\u{062C}\u{0645}", 2),
    (0xFD8E, "\u{0645}\u{062E}\u{062C}", 2),
    (0xFD8F, "\u{0645}\u{062E}\u{0645}", 2),
    (0xFD92, "\u{0645}\u{062C}\u{062E}", 2),
    (0xFD93, "\u{0647}\u{0645}\u{062C}", 2),
    (0xFD94, "\u{0647}\u{0645}\u{0645}", 2),
    (0xFD95, "\u{0646}\u{062D}\u{0645}", 2),
    (0xFD96, "\u{0646}\u{062D}\u{0649}", 1),
    (0xFD97, "\u{0646}\u{062C}\u{0645}", 1),
    (0xFD98, "\u{0646}\u{062C}\u{0645}", 2),
    (0xFD99, "\u{0646}\u{062C}\u{0649}", 1),
    (0xFD9A, "\u{0646}\u{0645}\u{064A}", 1),
    (0xFD9B, "\u{0646}\u{0645}\u{0649}", 1),
    (0xFD9C, "\u{064A}\u{0645}\u{0645}", 1),
    (0xFD9D, "\u{064A}\u{0645}\u{0645}", 2),
    (0xFD9E, "\u{0628}\u{062E}\u{064A}", 1),
    (0xFD9F, "\u{062A}\u{062C}\u{064A}", 1),
    (0xFDA0, "\u{062A}\u{062C}\u{0649}", 1),
    (0xFDA1, "\u{062A}\u{062E}\u{064A}", 1),
    (0xFDA2, "\u{062A}\u{062E}\u{0649}", 1),
    (0xFDA3, "\u{062A}\u{0645}\u{064A}", 1),
    (0xFDA4, "\u{062A}\u{0645}\u{0649}", 1),
    (0xFDA5, "\u{062C}\u{0645}\u{064A}", 1),
    (0xFDA6, "\u{062C}\u{062D}\u{0649}", 1),
    (0xFDA7, "\u{062C}\u{0645}\u{0649}", 1),
    (0xFDA8, "\u{0633}\u{062E}\u{0649}", 1),
    (0xFDA9, "\u{0635}\u{062D}\u{064A}", 1),
    (0xFDAA, "\u{0634}\u{062D}\u{064A}", 1),
    (0xFDAB, "\u{0636}\u{062D}\u{064A}", 1),
    (0xFDAC, "\u{0644}\u{062C}\u{064A}", 1),
    (0xFDAD, "\u{0644}\u{0645}\u{064A}", 1),
    (0xFDAE, "\u{064A}\u{062D}\u{064A}", 1),
    (0xFDAF, "\u{064A}\u{062C}\u{064A}", 1),
    (0xFDB0, "\u{064A}\u{0645}\u{064A}", 1),
    (0xFDB1, "\u{0645}\u{0645}\u{064A}", 1),
    (0xFDB2, "\u{0642}\u{0645}\u{064A}", 1),
    (0xFDB3, "\u{0646}\u{062D}\u{064A}", 1),
    (0xFDB4, "\u{0642}\u{0645}\u{062D}", 2),
    (0xFDB5, "\u{0644}\u{062D}\u{0645}", 2),
    (0xFDB6, "\u{0639}\u{0645}\u{064A}", 1),
    (0xFDB7, "\u{0643}\u{0645}\u{064A}", 1),
    (0xFDB8, "\u{0646}\u{062C}\u{062D}", 2),
    (0xFDB9, "\u{0645}\u{062E}\u{064A}", 1),
    (0xFDBA, "\u{0644}\u{062C}\u{0645}", 2),
    (0xFDBB, "\u{0643}\u{0645}\u{0645}", 1),
    (0xFDBC, "\u{0644}\u{062C}\u{0645}", 1),
    (0xFDBD, "\u{0646}\u{062C}\u{062D}", 1),
    (0xFDBE, "\u{062C}\u{062D}\u{064A}", 1),
    (0xFDBF, "\u{062D}\u{062C}\u{064A}", 1),
    (0xFDC0, "\u{0645}\u{062C}\u{064A}", 1),
    (0xFDC1, "\u{0641}\u{0645}\u{064A}", 1),
    (0xFDC2, "\u{0628}\u{062D}\u{064A}", 1),
    (0xFDC3, "\u{0643}\u{0645}\u{0645}", 2),
    (0xFDC4, "\u{0639}\u{062C}\u{0645}", 2),
    (0xFDC5, "\u{0635}\u{0645}\u{0645}", 2),
    (0xFDC6, "\u{0633}\u{062E}\u{064A}", 1),
    (0xFDC7, "\u{0646}\u{062C}\u{064A}", 1),
    (0xFDF0, "\u{0635}\u{0644}\u{06D2}", 0),
    (0xFDF1, "\u{0642}\u{0644}\u{06D2}", 0),
    (0xFDF2, "\u{0627}\u{0644}\u{0644}\u{0647}", 0),
    (0xFDF3, "\u{0627}\u{0643}\u{0628}\u{0631}", 0),
    (0xFDF4, "\u{0645}\u{062D}\u{0645}\u{062F}", 0),
    (0xFDF5, "\u{0635}\u{0644}\u{0639}\u{0645}", 0),
    (0xFDF6, "\u{0631}\u{0633}\u{0648}\u{0644}", 0),
    (0xFDF7, "\u{0639}\u{0644}\u{064A}\u{0647}", 0),
    (0xFDF8, "\u{0648}\u{0633}\u{0644}\u{0645}", 0),
    (0xFDF9, "\u{0635}\u{0644}\u{0649}", 0),
    (
        0xFDFA,
        "\u{0635}\u{0644}\u{0649}\u{0020}\u{0627}\u{0644}\u{0644}\u{0647}\u{0020}\u{0639}\u{0644}\u{064A}\u{0647}\u{0020}\u{0648}\u{0633}\u{0644}\u{0645}",
        0,
    ),
    (
        0xFDFB,
        "\u{062C}\u{0644}\u{0020}\u{062C}\u{0644}\u{0627}\u{0644}\u{0647}",
        0,
    ),
    (0xFDFC, "\u{0631}\u{06CC}\u{0627}\u{0644}", 0),
    (0xFE70, "\u{0020}\u{064B}", 0),
    (0xFE71, "\u{0640}\u{064B}", 3),
    (0xFE72, "\u{0020}\u{064C}", 0),
    (0xFE74, "\u{0020}\u{064D}", 0),
    (0xFE76, "\u{0020}\u{064E}", 0),
    (0xFE77, "\u{0640}\u{064E}", 3),
    (0xFE78, "\u{0020}\u{064F}", 0),
    (0xFE79, "\u{0640}\u{064F}", 3),
    (0xFE7A, "\u{0020}\u{0650}", 0),
    (0xFE7B, "\u{0640}\u{0650}", 3),
    (0xFE7C, "\u{0020}\u{0651}", 0),
    (0xFE7D, "\u{0640}\u{0651}", 3),
    (0xFE7E, "\u{0020}\u{0652}", 0),
    (0xFE7F, "\u{0640}\u{0652}", 3),
    (0xFE80, "\u{0621}", 0),
    (0xFE81, "\u{0622}", 0),
    (0xFE82, "\u{0622}", 1),
    (0xFE83, "\u{0623}", 0),
    (0xFE84, "\u{0623}", 1),
    (0xFE85, "\u{0624}", 0),
    (0xFE86, "\u{0624}", 1),
    (0xFE87, "\u{0625}", 0),
    (0xFE88, "\u{0625}", 1),
    (0xFE89, "\u{0626}", 0),
    (0xFE8A, "\u{0626}", 1),
    (0xFE8B, "\u{0626}", 2),
    (0xFE8C, "\u{0626}", 3),
    (0xFE8D, "\u{0627}", 0),
    (0xFE8E, "\u{0627}", 1),
    (0xFE8F, "\u{0628}", 0),
    (0xFE90, "\u{0628}", 1),
    (0xFE91, "\u{0628}", 2),
    (0xFE92, "\u{0628}", 3),
    (0xFE93, "\u{0629}", 0),
    (0xFE94, "\u{0629}", 1),
    (0xFE95, "\u{062A}", 0),
    (0xFE96, "\u{062A}", 1),
    (0xFE97, "\u{062A}", 2),
    (0xFE98, "\u{062A}", 3),
    (0xFE99, "\u{062B}", 0),
    (0xFE9A, "\u{062B}", 1),
    (0xFE9B, "\u{062B}", 2),
    (0xFE9C, "\u{062B}", 3),
    (0xFE9D, "\u{062C}", 0),
    (0xFE9E, "\u{062C}", 1),
    (0xFE9F, "\u{062C}", 2),
    (0xFEA0, "\u{062C}", 3),
    (0xFEA1, "\u{062D}", 0),
    (0xFEA2, "\u{062D}", 1),
    (0xFEA3, "\u{062D}", 2),
    (0xFEA4, "\u{062D}", 3),
    (0xFEA5, "\u{062E}", 0),
    (0xFEA6, "\u{062E}", 1),
    (0xFEA7, "\u{062E}", 2),
    (0xFEA8, "\u{062E}", 3),
    (0xFEA9, "\u{062F}", 0),
    (0xFEAA, "\u{062F}", 1),
    (0xFEAB, "\u{0630}", 0),
    (0xFEAC, "\u{0630}", 1),
    (0xFEAD, "\u{0631}", 0),
    (0xFEAE, "\u{0631}", 1),
    (0xFEAF, "\u{0632}", 0),
    (0xFEB0, "\u{0632}", 1),
    (0xFEB1, "\u{0633}", 0),
    (0xFEB2, "\u{0633}", 1),
    (0xFEB3, "\u{0633}", 2),
    (0xFEB4, "\u{0633}", 3),
    (0xFEB5, "\u{0634}", 0),
    (0xFEB6, "\u{0634}", 1),
    (0xFEB7, "\u{0634}", 2),
    (0xFEB8, "\u{0634}", 3),
    (0xFEB9, "\u{0635}", 0),
    (0xFEBA, "\u{0635}", 1),
    (0xFEBB, "\u{0635}", 2),
    (0xFEBC, "\u{0635}", 3),
    (0xFEBD, "\u{0636}", 0),
    (0xFEBE, "\u{0636}", 1),
    (0xFEBF, "\u{0636}", 2),
    (0xFEC0, "\u{0636}", 3),
    (0xFEC1, "\u{0637}", 0),
    (0xFEC2, "\u{0637}", 1),
    (0xFEC3, "\u{0637}", 2),
    (0xFEC4, "\u{0637}", 3),
    (0xFEC5, "\u{0638}", 0),
    (0xFEC6, "\u{0638}", 1),
    (0xFEC7, "\u{0638}", 2),
    (0xFEC8, "\u{0638}", 3),
    (0xFEC9, "\u{0639}", 0),
    (0xFECA, "\u{0639}", 1),
    (0xFECB, "\u{0639}", 2),
    (0xFECC, "\u{0639}", 3),
    (0xFECD, "\u{063A}", 0),
    (0xFECE, "\u{063A}", 1),
    (0xFECF, "\u{063A}", 2),
    (0xFED0, "\u{063A}", 3),
    (0xFED1, "\u{0641}", 0),
    (0xFED2, "\u{0641}", 1),
    (0xFED3, "\u{0641}", 2),
    (0xFED4, "\u{0641}", 3),
    (0xFED5, "\u{0642}", 0),
    (0xFED6, "\u{0642}", 1),
    (0xFED7, "\u{0642}", 2),
    (0xFED8, "\u{0642}", 3),
    (0xFED9, "\u{0643}", 0),
    (0xFEDA, "\u{0643}", 1),
    (0xFEDB, "\u{0643}", 2),
    (0xFEDC, "\u{0643}", 3),
    (0xFEDD, "\u{0644}", 0),
    (0xFEDE, "\u{0644}", 1),
    (0xFEDF, "\u{0644}", 2),
    (0xFEE0, "\u{0644}", 3),
    (0xFEE1, "\u{0645}", 0),
    (0xFEE2, "\u{0645}", 1),
    (0xFEE3, "\u{0645}", 2),
    (0xFEE4, "\u{0645}", 3),
    (0xFEE5, "\u{0646}", 0),
    (0xFEE6, "\u{0646}", 1),
    (0xFEE7, "\u{0646}", 2),
    (0xFEE8, "\u{0646}", 3),
    (0xFEE9, "\u{0647}", 0),
    (0xFEEA, "\u{0647}", 1),
    (0xFEEB, "\u{0647}", 2),
    (0xFEEC, "\u{0647}", 3),
    (0xFEED, "\u{0648}", 0),
    (0xFEEE, "\u{0648}", 1),
    (0xFEEF, "\u{0649}", 0),
    (0xFEF0, "\u{0649}", 1),
    (0xFEF1, "\u{064A}", 0),
    (0xFEF2, "\u{064A}", 1),
    (0xFEF3, "\u{064A}", 2),
    (0xFEF4, "\u{064A}", 3),
    (0xFEF5, "\u{0644}\u{0622}", 0),
    (0xFEF6, "\u{0644}\u{0622}", 1),
    (0xFEF7, "\u{0644}\u{0623}", 0),
    (0xFEF8, "\u{0644}\u{0623}", 1),
    (0xFEF9, "\u{0644}\u{0625}", 0),
    (0xFEFA, "\u{0644}\u{0625}", 1),
    (0xFEFB, "\u{0644}\u{0627}", 0),
    (0xFEFC, "\u{0644}\u{0627}", 1),
];

#[cfg(test)]
mod tests {
    use super::*;

    // "سلام دنیا" as presentation forms: س initial, لا final, م alone · د alone, ن initial, ی middle, ا final.
    const SALAM: &str = "\u{FEB3}\u{FEFC}\u{FEE1}";
    const DONYA: &str = "\u{FEA9}\u{FEE7}\u{FBFF}\u{FE8E}";

    #[test]
    fn forms_are_sorted_for_lookup() {
        assert!(FORMS.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(plain(SALAM), "سلام");
        assert_eq!(plain("ب\u{0640}\u{0640}ه"), "به");
    }

    #[test]
    fn words_in_mirrored_order_are_reversed() {
        // MuPDF's way: each word right, the words in page order (left to right).
        assert_eq!(logical_line(&format!("{DONYA} {SALAM}")), "سلام دنیا");
    }

    #[test]
    fn a_line_mirrored_letter_by_letter_is_reversed() {
        // PDFium's way: the line as shown, left to right: the number (left-to-right itself) first, then the words'
        // letters backwards.
        let words: String = format!("{SALAM} {DONYA}").chars().rev().collect();
        let line = format!("2024 {words}");
        assert_eq!(logical_line(&line), "سلام دنیا 2024");
    }

    #[test]
    fn glued_parts_and_plain_text() {
        // "سلام:دنیا" in mirrored word order is stored "دنیا:سلام".
        assert_eq!(logical_line(&format!("{DONYA}:{SALAM}")), "سلام:دنیا");
        // Modern PDFs store plain letters in reading order: left as they are.
        assert_eq!(logical_line("سلام دنیا"), "سلام دنیا");
        assert_eq!(logical_line("Hello world"), "Hello world");
        assert_eq!(
            logical_paragraph([format!("{DONYA} {SALAM}").as_str(), ""]),
            "سلام دنیا"
        );
    }
}
