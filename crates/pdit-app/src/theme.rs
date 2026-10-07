//! Light/dark theme (D-022). The theme button (web/gooey-island) stores the
//! user's pick in localStorage and on `<html data-theme>`; the stylesheets
//! follow `data-theme`, or the system setting when there is none.

/// Same key as `THEME_KEY` in web/gooey-island/src/index.jsx.
const THEME_KEY: &str = "pdit-theme";

/// Applies a theme the user picked on an earlier visit, before the first render.
pub fn apply_saved() {
    let Some(window) = web_sys::window() else {
        return;
    };
    // Storage can be unavailable (private windows); the system theme then applies.
    let saved = window
        .local_storage()
        .ok()
        .flatten()
        .and_then(|storage| storage.get_item(THEME_KEY).ok().flatten());
    if let Some(theme) = saved.filter(|t| t == "light" || t == "dark")
        && let Some(root) = window.document().and_then(|d| d.document_element())
    {
        let _ = root.set_attribute("data-theme", &theme);
    }
}
