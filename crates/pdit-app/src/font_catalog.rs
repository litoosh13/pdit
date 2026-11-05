//! The fonts bundled for the font menu (D-084, D-085) and for fallback text
//! (D-028): Liberation Sans, Serif and Mono, Carlito, Caladea, Roboto, Open Sans,
//! Montserrat and Lato, each in four styles. They are fetched from pdit's own
//! site only when first needed, then kept in memory.
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
const ROBOTO_REGULAR: Asset = asset!("/assets/fonts/roboto/Roboto-Regular.ttf");
const ROBOTO_BOLD: Asset = asset!("/assets/fonts/roboto/Roboto-Bold.ttf");
const ROBOTO_ITALIC: Asset = asset!("/assets/fonts/roboto/Roboto-Italic.ttf");
const ROBOTO_BOLDITALIC: Asset = asset!("/assets/fonts/roboto/Roboto-BoldItalic.ttf");
const OPENSANS_REGULAR: Asset = asset!("/assets/fonts/opensans/OpenSans-Regular.ttf");
const OPENSANS_BOLD: Asset = asset!("/assets/fonts/opensans/OpenSans-Bold.ttf");
const OPENSANS_ITALIC: Asset = asset!("/assets/fonts/opensans/OpenSans-Italic.ttf");
const OPENSANS_BOLDITALIC: Asset = asset!("/assets/fonts/opensans/OpenSans-BoldItalic.ttf");
const MONTSERRAT_REGULAR: Asset = asset!("/assets/fonts/montserrat/Montserrat-Regular.ttf");
const MONTSERRAT_BOLD: Asset = asset!("/assets/fonts/montserrat/Montserrat-Bold.ttf");
const MONTSERRAT_ITALIC: Asset = asset!("/assets/fonts/montserrat/Montserrat-Italic.ttf");
const MONTSERRAT_BOLDITALIC: Asset = asset!("/assets/fonts/montserrat/Montserrat-BoldItalic.ttf");
const LATO_REGULAR: Asset = asset!("/assets/fonts/lato/Lato-Regular.ttf");
const LATO_BOLD: Asset = asset!("/assets/fonts/lato/Lato-Bold.ttf");
const LATO_ITALIC: Asset = asset!("/assets/fonts/lato/Lato-Italic.ttf");
const LATO_BOLDITALIC: Asset = asset!("/assets/fonts/lato/Lato-BoldItalic.ttf");

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
    Roboto,
    OpenSans,
    Montserrat,
    Lato,
}

impl Family {
    /// Every family (used by the debug self-check).
    #[cfg(any(debug_assertions, feature = "self-check"))]
    pub const ALL: [Family; 9] = [
        Family::Sans,
        Family::Serif,
        Family::Mono,
        Family::Carlito,
        Family::Caladea,
        Family::Roboto,
        Family::OpenSans,
        Family::Montserrat,
        Family::Lato,
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
        (Family::Roboto, false, false) => ROBOTO_REGULAR,
        (Family::Roboto, true, false) => ROBOTO_BOLD,
        (Family::Roboto, false, true) => ROBOTO_ITALIC,
        (Family::Roboto, true, true) => ROBOTO_BOLDITALIC,
        (Family::OpenSans, false, false) => OPENSANS_REGULAR,
        (Family::OpenSans, true, false) => OPENSANS_BOLD,
        (Family::OpenSans, false, true) => OPENSANS_ITALIC,
        (Family::OpenSans, true, true) => OPENSANS_BOLDITALIC,
        (Family::Montserrat, false, false) => MONTSERRAT_REGULAR,
        (Family::Montserrat, true, false) => MONTSERRAT_BOLD,
        (Family::Montserrat, false, true) => MONTSERRAT_ITALIC,
        (Family::Montserrat, true, true) => MONTSERRAT_BOLDITALIC,
        (Family::Lato, false, false) => LATO_REGULAR,
        (Family::Lato, true, false) => LATO_BOLD,
        (Family::Lato, false, true) => LATO_ITALIC,
        (Family::Lato, true, true) => LATO_BOLDITALIC,
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
    Some(if has(&["roboto"]) {
        Family::Roboto
    } else if has(&["opensans"]) {
        Family::OpenSans
    } else if has(&["montserrat"]) {
        Family::Montserrat
    } else if name.starts_with("lato") {
        Family::Lato
    } else if has(&["calibri", "carlito"]) {
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

/// The family of a font picked in the format bar's menu.
pub fn family_of(font: pdit_core::FontChoice) -> Option<Family> {
    use pdit_core::FontChoice as F;
    match font {
        F::Document | F::NotoSans => None,
        F::LiberationSans => Some(Family::Sans),
        F::LiberationSerif => Some(Family::Serif),
        F::LiberationMono => Some(Family::Mono),
        F::Carlito => Some(Family::Carlito),
        F::Caladea => Some(Family::Caladea),
        F::Roboto => Some(Family::Roboto),
        F::OpenSans => Some(Family::OpenSans),
        F::Montserrat => Some(Family::Montserrat),
        F::Lato => Some(Family::Lato),
    }
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
