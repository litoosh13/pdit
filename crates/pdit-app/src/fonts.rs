//! Registers the bundled Inter font (OFL-1.1, The Inter Project Authors; see
//! assets/fonts/inter/SOURCE.md). The @font-face rule is fontsource's
//! `inter-latin-wght-normal` rule (@fontsource-variable/inter 5.3.0, wght.css),
//! emitted from Rust so its URL is the bundler's fingerprinted asset path.

use dioxus::prelude::*;

const INTER: Asset = asset!("/assets/fonts/inter/inter-latin-wght-normal.woff2");

/// Latin subset, as in fontsource and the Google Fonts `latin` subset.
const LATIN: &str = "U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+0304,U+0308,U+0329,U+2000-206F,U+20AC,U+2122,U+2191,U+2193,U+2212,U+2215,U+FEFF,U+FFFD";

#[component]
pub fn Fonts() -> Element {
    let css = format!(
        "@font-face {{ font-family: 'Inter Variable'; font-style: normal; font-display: swap; \
         font-weight: 100 900; src: url({INTER}) format('woff2-variations'); unicode-range: {LATIN}; }}"
    );
    rsx! {
        document::Style { {css} }
    }
}
