//! The look-alike fonts bundled for fallback text (D-028): Liberation Sans,
//! Serif and Mono, Carlito and Caladea, each in four styles. They are fetched
//! from pdit's own site only when first needed, then kept in memory.
//! Sources and licences: assets/fonts/*/SOURCE.md.

use dioxus::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const SANS_REGULAR: Asset = asset!("/assets/fonts/liberation/LiberationSans-Regular.ttf");
const SANS_BOLD: Asset = asset!("/assets/fonts/liberation/LiberationSans-Bold.ttf");
const SANS_ITALIC: Asset = asset!("/assets/fonts/liberation/LiberationSans-Italic.ttf");
const SANS_BOLDITALIC: Asset = asset!("/assets/fonts/liberation/LiberationSans-BoldItalic.ttf");
const SERIF_REGULAR: Asset = asset!("/assets/fonts/liberation/LiberationSerif-Regular.ttf");
const SERIF_BOLD: Asset = asset!("/assets/fonts/liberation/LiberationSerif-Bold.ttf");
const SERIF_ITALIC: Asset = asset!("/assets/fonts/liberation/LiberationSerif-Italic.ttf");
const SERIF_BOLDITALIC: Asset = asset!("/assets/fonts/liberation/LiberationSerif-BoldItalic.ttf");
const MONO_REGULAR: Asset = asset!("/assets/fonts/liberation/LiberationMono-Regular.ttf");
const MONO_BOLD: Asset = asset!("/assets/fonts/liberation/LiberationMono-Bold.ttf");
const MONO_ITALIC: Asset = asset!("/assets/fonts/liberation/LiberationMono-Italic.ttf");
const MONO_BOLDITALIC: Asset = asset!("/assets/fonts/liberation/LiberationMono-BoldItalic.ttf");
const CARLITO_REGULAR: Asset = asset!("/assets/fonts/carlito/Carlito-Regular.ttf");
const CARLITO_BOLD: Asset = asset!("/assets/fonts/carlito/Carlito-Bold.ttf");
const CARLITO_ITALIC: Asset = asset!("/assets/fonts/carlito/Carlito-Italic.ttf");
const CARLITO_BOLDITALIC: Asset = asset!("/assets/fonts/carlito/Carlito-BoldItalic.ttf");
const CALADEA_REGULAR: Asset = asset!("/assets/fonts/caladea/Caladea-Regular.ttf");
const CALADEA_BOLD: Asset = asset!("/assets/fonts/caladea/Caladea-Bold.ttf");
const CALADEA_ITALIC: Asset = asset!("/assets/fonts/caladea/Caladea-Italic.ttf");
const CALADEA_BOLDITALIC: Asset = asset!("/assets/fonts/caladea/Caladea-BoldItalic.ttf");

/// A bundled look-alike family.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Family {
    /// Liberation Sans: Arial, Helvetica.
    Sans,
    /// Liberation Serif: Times New Roman, Times.
    Serif,
    /// Liberation Mono: Courier New, Courier.
    Mono,
    /// Carlito: Calibri.
    Carlito,
    /// Caladea: Cambria.
    Caladea,
}

impl Family {
    /// Every family (used by the debug self-check).
    #[cfg(debug_assertions)]
    pub const ALL: [Family; 5] = [
        Family::Sans,
        Family::Serif,
        Family::Mono,
        Family::Carlito,
        Family::Caladea,
    ];
}

fn asset(family: Family, bold: bool, italic: bool) -> Asset {
    match (family, bold, italic) {
        (Family::Sans, false, false) => SANS_REGULAR,
        (Family::Sans, true, false) => SANS_BOLD,
        (Family::Sans, false, true) => SANS_ITALIC,
        (Family::Sans, true, true) => SANS_BOLDITALIC,
        (Family::Serif, false, false) => SERIF_REGULAR,
        (Family::Serif, true, false) => SERIF_BOLD,
        (Family::Serif, false, true) => SERIF_ITALIC,
        (Family::Serif, true, true) => SERIF_BOLDITALIC,
        (Family::Mono, false, false) => MONO_REGULAR,
        (Family::Mono, true, false) => MONO_BOLD,
        (Family::Mono, false, true) => MONO_ITALIC,
        (Family::Mono, true, true) => MONO_BOLDITALIC,
        (Family::Carlito, false, false) => CARLITO_REGULAR,
        (Family::Carlito, true, false) => CARLITO_BOLD,
        (Family::Carlito, false, true) => CARLITO_ITALIC,
        (Family::Carlito, true, true) => CARLITO_BOLDITALIC,
        (Family::Caladea, false, false) => CALADEA_REGULAR,
        (Family::Caladea, true, false) => CALADEA_BOLD,
        (Family::Caladea, false, true) => CALADEA_ITALIC,
        (Family::Caladea, true, true) => CALADEA_BOLDITALIC,
    }
}

/// A family in one style: (family, bold, italic).
type Face = (Family, bool, bool);

thread_local! {
    static LOADED: RefCell<HashMap<Face, Rc<Vec<u8>>>> = RefCell::new(HashMap::new());
}

/// The look-alike for a PDF font (D-028): by name first (the common fonts and
/// their known clones), otherwise by the font's flags. None for symbol fonts.
pub fn family_for(traits: &pdit_core::FontTraits) -> Option<Family> {
    if traits.symbolic {
        return None;
    }
    let name: String = traits
        .name
        .to_ascii_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&["notosans"]) {
        // Noto Sans itself is the general fallback.
        return None;
    }
    Some(if has(&["calibri", "carlito"]) {
        Family::Carlito
    } else if has(&["cambria", "caladea"]) {
        Family::Caladea
    } else if has(&["courier", "cousine", "liberationmono", "nimbusmono"]) {
        Family::Mono
    } else if has(&["times", "tinos", "liberationserif", "nimbusrom"]) {
        Family::Serif
    } else if has(&[
        "arial",
        "helvetica",
        "arimo",
        "liberationsans",
        "nimbussans",
    ]) {
        Family::Sans
    } else if traits.fixed_pitch {
        Family::Mono
    } else if traits.serif {
        Family::Serif
    } else {
        Family::Sans
    })
}

/// The font file for `family` in the given style, fetched on first use.
pub async fn load(family: Family, bold: bool, italic: bool) -> Result<Rc<Vec<u8>>, String> {
    let key = (family, bold, italic);
    if let Some(bytes) = LOADED.with_borrow(|fonts| fonts.get(&key).cloned()) {
        return Ok(bytes);
    }
    let bytes = crate::fetch_bytes(&asset(family, bold, italic).to_string())
        .await
        .map_err(|e| format!("{e:?}"))?;
    let bytes = Rc::new(bytes);
    LOADED.with_borrow_mut(|fonts| fonts.insert(key, bytes.clone()));
    Ok(bytes)
}
