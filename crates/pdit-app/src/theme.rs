//! Light mode only (D-062, user 2026-10-08: "only light mode is enough for
//! now"). The stylesheets still hold dark rules, which `data-theme="light"`
//! switches off whatever the system setting; they go as each piece is
//! redesigned.

/// Pins the light theme before the first render.
pub fn apply() {
    if let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = root.set_attribute("data-theme", "light");
    }
}
