//! Find & replace (D-051): the Beautiful UI "Search" card at the top right,
//! opened with ⌘F / Ctrl+F or the tools rail's Find. Matches are highlighted
//! on the pages; Replace edits the line through the text-edit engine, and
//! Replace all edits every matching line under one Undo.
//! Look: assets/css/search.css.

use crate::page_tools::{PageTools, next_frame, set_timeout};
use dioxus::prelude::*;
use pdit_core::search::{self, FindOptions, Match};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const SEARCH_CSS: Asset = asset!("/assets/css/search.css");
/// The find bar's icons: Devigner Icons (D-062; were the Search component's own, Beautiful UI).
pub(crate) const ICON_FIND: &str = include_str!("../assets/icons/devigner/SearchNormal.svg");
const ICON_CLEAR: &str = include_str!("../assets/icons/devigner/Close.svg");
const ICON_UP: &str = include_str!("../assets/icons/devigner/ChevronUp.svg");
const ICON_DOWN: &str = include_str!("../assets/icons/devigner/ChevronDown.svg");
const INPUT_ID: &str = "pdit-find-q";
/// The card's pop-out time (ms).
const CLOSE_MS: i32 = 120;

/// Shared find state.
#[derive(Clone, Copy)]
pub struct Find {
    open: Signal<bool>,
    closing: Signal<bool>,
    query: Signal<String>,
    with: Signal<String>,
    replace: Signal<bool>,
    options: Signal<FindOptions>,
    results: Signal<Vec<Match>>,
    cur: Signal<usize>,
}

impl Find {
    pub fn provide() -> Self {
        use_context_provider(|| Find {
            open: Signal::new(false),
            closing: Signal::new(false),
            query: Signal::new(String::new()),
            with: Signal::new(String::new()),
            replace: Signal::new(false),
            options: Signal::new(FindOptions::default()),
            results: Signal::new(Vec::new()),
            cur: Signal::new(0),
        })
    }

    /// Opens the card (or focuses it) and selects the query.
    pub fn open(mut self) {
        self.closing.set(false);
        self.open.set(true);
        self.search();
        next_frame(|| {
            let input = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id(INPUT_ID))
                .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok());
            if let Some(input) = input {
                let _ = input.focus();
                input.select();
            }
        });
    }

    /// Esc: plays the pop-out, then removes the card and the highlights.
    pub fn close(mut self) {
        if !*self.open.peek() {
            // A marked answer sentence (no card): Esc takes the mark away.
            if !self.results.peek().is_empty() {
                self.results.set(Vec::new());
            }
            return;
        }
        if *self.closing.peek() {
            return;
        }
        self.closing.set(true);
        let (mut open, mut closing, mut results) = (self.open, self.closing, self.results);
        set_timeout(CLOSE_MS, move || {
            if *closing.peek() {
                open.set(false);
                closing.set(false);
                results.set(Vec::new());
            }
        });
    }

    /// Marks a sentence on `page` (an answer's "Show on page", D-056): its
    /// first words, fewer if the sentence runs onto the next line (matches stay
    /// within one line), become the highlighted match.
    pub fn mark(mut self, page: u16, sentence: &str) {
        let words: Vec<&str> = sentence.split_whitespace().collect();
        for n in (2..=words.len().min(10)).rev() {
            let prefix = words[..n].join(" ");
            let found = search::find(page, &prefix, FindOptions::default()).unwrap_or_default();
            if let Some(first) = found.into_iter().next() {
                self.results.set(vec![first]);
                self.cur.set(0);
                scroll_to_current();
                return;
            }
        }
        crate::thumbnails::scroll_to_page(page);
    }

    /// Searches every page again (after typing, an option or a replace).
    // ponytail: searches the whole document on each keystroke; debounce or
    // search page by page if big documents feel slow.
    fn search(mut self) {
        let query = self.query.peek().clone();
        let options = *self.options.peek();
        let mut found = Vec::new();
        if !query.is_empty() {
            let pages = pdit_core::page_ops::page_sizes().map_or(0, |s| s.len());
            for page in 0..pages {
                found.extend(search::find(page as u16, &query, options).unwrap_or_default());
            }
        }
        let last = found.len().saturating_sub(1);
        if *self.cur.peek() > last {
            self.cur.set(last);
        }
        self.results.set(found);
    }

    fn step(mut self, forward: bool) {
        let n = self.results.peek().len();
        if n == 0 {
            return;
        }
        let cur = *self.cur.peek();
        self.cur.set(if forward {
            (cur + 1) % n
        } else {
            (cur + n - 1) % n
        });
        scroll_to_current();
    }

    fn show(mut self, k: usize) {
        self.cur.set(k);
        scroll_to_current();
    }

    /// Replace (one match) or Replace all, with Undo.
    fn replace(self, all: bool) {
        let results = self.results.peek().clone();
        let Some(current) = results.get(*self.cur.peek()).cloned() else {
            return;
        };
        let editing = consume_context::<crate::editing::Editing>();
        let Some(noto) = editing.fallback_font.peek().clone() else {
            return crate::log("pdit: the fallback font is still loading; try again in a moment");
        };
        // An edit waiting for Keep/Discard is kept first.
        editing.keep_waiting();
        let (query, with, options) = (
            self.query.peek().clone(),
            self.with.peek().clone(),
            *self.options.peek(),
        );
        if all {
            // Over the joined lines, so a phrase split into pieces is replaced too.
            if let Err(error) = replace_all_in_lines(&query, &with, options, &noto) {
                crate::log(&format!("pdit: replace all: {error}"));
            }
        } else {
            let line = (
                current.page,
                current.object_index,
                current.line.clone(),
                Some(current.nth),
            );
            replace_lines(vec![line], &query, &with, options, 1, &noto);
        }
        self.search();
    }
}

/// Edits `lines` (page, text object, its text, `Some(n)`: only the n-th match)
/// as one step with Undo: one edit per line, highest index first on each page so
/// earlier indices stay valid.
fn replace_lines(
    mut lines: Vec<(u16, usize, String, Option<usize>)>,
    query: &str,
    with: &str,
    options: search::FindOptions,
    count: usize,
    noto: &[u8],
) {
    lines.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let message = if count == 1 {
        "Replaced 1".to_owned()
    } else {
        format!("Replaced {count}")
    };
    consume_context::<PageTools>().apply(&message, ICON_FIND, || {
        let fonts = pdit_core::Fonts {
            look_alike: None,
            noto_sans: noto,
        };
        for (page, index, line, only) in &lines {
            let new = search::replace_in(line, query, with, options, *only);
            pdit_core::preview_edit(*page, *index, &new, &fonts)?;
            pdit_core::keep_edit();
        }
        Ok(())
    });
}

/// The AI chat's `change all "old" to "new"` (D-062): every `query` in the
/// document (any letter case, any part of a word) becomes `with`, as one step
/// with Undo. Returns how many were changed.
pub(crate) fn replace_everywhere(query: &str, with: &str) -> Result<usize, String> {
    let editing = consume_context::<crate::editing::Editing>();
    let noto = editing
        .fallback_font
        .peek()
        .clone()
        .ok_or("the fallback font is still loading; try again in a moment")?;
    editing.keep_waiting();
    replace_all_in_lines(query, with, search::FindOptions::default(), &noto)
}

/// Every match of `query` in the document's visual lines (pieces on one
/// baseline joined, so a phrase the PDF split into several text objects is
/// found) becomes `with`, as one step with Undo; nothing changes if any line
/// fails. A line with a match is rewritten as one text object (one piece: an
/// ordinary line edit). Returns how many were replaced.
fn replace_all_in_lines(
    query: &str,
    with: &str,
    options: search::FindOptions,
    noto: &[u8],
) -> Result<usize, String> {
    let pages = pdit_core::page_ops::page_sizes().map_or(0, |s| s.len()) as u16;
    // A point inside each line with a match: object numbers change with every
    // edit, so each line is found again by where it is (lines don't overlap).
    let mut targets = Vec::new();
    let mut count = 0;
    for page in 0..pages {
        for line in pdit_core::visual_text_lines(page).map_err(|e| e.to_string())? {
            let n = search::find_in(&line.text, query, options).len();
            if n > 0 {
                count += n;
                let [l, b, _, t] = line.bounds;
                targets.push((page, l + 0.5, (b + t) / 2.0));
            }
        }
    }
    if count == 0 {
        return Ok(0);
    }
    let message = if count == 1 {
        "Replaced 1".to_owned()
    } else {
        format!("Replaced {count}")
    };
    let mut failure = None;
    consume_context::<PageTools>().apply(&message, ICON_FIND, || {
        let fonts = pdit_core::Fonts {
            look_alike: None,
            noto_sans: noto,
        };
        let before = pdit_core::page_ops::snapshot()?;
        let result: Result<(), pdit_core::Error> = (|| {
            for &(page, x, y) in &targets {
                let lines = pdit_core::visual_text_lines(page)?;
                let Some(line) = lines.iter().find(|ln| {
                    let [l, b, r, t] = ln.bounds;
                    x >= l && x <= r && y >= b && y <= t
                }) else {
                    continue;
                };
                let new = search::replace_in(&line.text, query, with, options, None);
                pdit_core::preview_line(page, &line.object_indices, &new, &fonts)?;
                pdit_core::keep_edit();
            }
            Ok(())
        })();
        if let Err(error) = &result {
            failure = Some(error.to_string());
            let _ = pdit_core::discard_edit();
            let _ = pdit_core::page_ops::restore(before);
        }
        result
    });
    match failure {
        Some(error) => Err(error),
        None => Ok(count),
    }
}

/// Brings the current match into the middle of the window.
fn scroll_to_current() {
    next_frame(|| {
        let hit = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.query_selector(".pdit-find-hit.is-cur").ok().flatten());
        if let Some(hit) = hit {
            let options = web_sys::ScrollIntoViewOptions::new();
            options.set_block(web_sys::ScrollLogicalPosition::Center);
            options.set_behavior(web_sys::ScrollBehavior::Smooth);
            hit.scroll_into_view_with_scroll_into_view_options(&options);
        }
    });
}

/// The card, and ⌘F / Ctrl+F.
#[component]
pub fn FindCard() -> Element {
    let mut find = use_context::<Find>();
    let document = use_context::<Signal<Option<crate::pages::OpenDocument>>>();
    // The window listener runs outside Dioxus; it only counts the key and the
    // effect opens the card inside Dioxus (as AnnotationKeys).
    let mut opens = use_signal(|| 0u32);
    use_effect(move || {
        if opens() > 0 && document.peek().is_some() {
            find.open();
        }
    });
    // A different document: search it again.
    let doc_id = document.read().as_ref().map(|d| d.id);
    use_effect(use_reactive!(|doc_id| {
        let _ = doc_id;
        if *find.open.peek() {
            find.search();
        }
    }));
    use_hook(move || {
        let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                if (event.meta_key() || event.ctrl_key()) && event.key().eq_ignore_ascii_case("f") {
                    event.prevent_default();
                    opens += 1;
                }
            },
        );
        if let Some(window) = web_sys::window() {
            let _ =
                window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
        }
        // The app lives as long as the page.
        on_key.forget();
    });

    let results = find.results.read().clone();
    let query = find.query.read().clone();
    let options = *find.options.read();
    let replace = (find.replace)();
    let cur = (find.cur)();
    let count = match (query.is_empty(), results.len()) {
        (true, _) => String::new(),
        (false, 0) => "0 of 0".to_owned(),
        (false, n) => format!("{} of {n}", cur + 1),
    };
    let none = results.is_empty();
    rsx! {
        document::Stylesheet { href: SEARCH_CSS }
        if (find.open)() {
            div {
                class: if (find.closing)() { "pdit-find is-closing" } else { "pdit-find" },
                role: "dialog",
                "aria-label": "Find",
                div { class: "pdit-find-row",
                    span { class: "pdit-find-mag", dangerous_inner_html: ICON_FIND }
                    input {
                        id: INPUT_ID,
                        placeholder: "Find in document…",
                        "aria-label": "Find in document",
                        value: "{query}",
                        oninput: move |event| {
                            find.query.set(event.value());
                            find.cur.set(0);
                            find.search();
                            scroll_to_current();
                        },
                        onkeydown: move |event| {
                            if event.key() == Key::Enter {
                                event.prevent_default();
                                find.step(!event.modifiers().shift());
                            }
                        },
                    }
                    span { class: "pdit-find-count", "{count}" }
                    button {
                        class: "pdit-find-icon",
                        r#type: "button",
                        "aria-label": "Previous match (Shift+Enter)",
                        disabled: none,
                        onclick: move |_| find.step(false),
                        span { dangerous_inner_html: ICON_UP, style: "display: contents" }
                    }
                    button {
                        class: "pdit-find-icon",
                        r#type: "button",
                        "aria-label": "Next match (Enter)",
                        disabled: none,
                        onclick: move |_| find.step(true),
                        span { dangerous_inner_html: ICON_DOWN, style: "display: contents" }
                    }
                    if !query.is_empty() {
                        button {
                            class: "pdit-find-icon pdit-find-clear",
                            r#type: "button",
                            "aria-label": "Clear search",
                            onclick: move |_| {
                                find.query.set(String::new());
                                find.search();
                            },
                            span { dangerous_inner_html: ICON_CLEAR, style: "display: contents" }
                        }
                    }
                }
                if replace {
                    div { class: "pdit-find-row",
                        span { class: "pdit-find-mag" }
                        input {
                            placeholder: "Replace with…",
                            "aria-label": "Replace with",
                            value: "{find.with}",
                            oninput: move |event| find.with.set(event.value()),
                            onkeydown: move |event| {
                                if event.key() == Key::Enter {
                                    event.prevent_default();
                                    find.replace(false);
                                }
                            },
                        }
                    }
                }
                div { class: "pdit-find-bar",
                    button {
                        class: if options.match_case { "pdit-find-qb is-active" } else { "pdit-find-qb" },
                        r#type: "button",
                        title: "Match case",
                        onclick: move |_| {
                            find.options.with_mut(|o| o.match_case = !o.match_case);
                            find.cur.set(0);
                            find.search();
                        },
                        "Aa"
                    }
                    button {
                        class: if options.whole_word { "pdit-find-qb is-active" } else { "pdit-find-qb" },
                        r#type: "button",
                        onclick: move |_| {
                            find.options.with_mut(|o| o.whole_word = !o.whole_word);
                            find.cur.set(0);
                            find.search();
                        },
                        "Whole words"
                    }
                    span { class: "pdit-find-grow" }
                    button {
                        class: if replace { "pdit-find-qb is-active" } else { "pdit-find-qb" },
                        r#type: "button",
                        onclick: move |_| find.replace.toggle(),
                        "Replace"
                    }
                    if replace {
                        button {
                            class: "pdit-find-qb",
                            r#type: "button",
                            disabled: none,
                            onclick: move |_| find.replace(false),
                            "Replace"
                        }
                        button {
                            class: "pdit-find-qb is-primary",
                            r#type: "button",
                            disabled: none,
                            onclick: move |_| find.replace(true),
                            "Replace all"
                        }
                    }
                }
                if query.is_empty() {
                    div { class: "pdit-find-note",
                        "Searches the text of every page. Text inside pictures (scans) needs OCR, which comes later."
                    }
                } else if none {
                    div { class: "pdit-find-empty",
                        span { class: "ic", dangerous_inner_html: ICON_FIND }
                        span { class: "t1", "No results found" }
                        span { class: "t2", "Adjust your search to try again" }
                    }
                } else {
                    div { class: "pdit-find-results",
                        for (k, m) in results.iter().enumerate() {
                            {
                                let (before, hit, after) = context(m);
                                rsx! {
                                    button {
                                        key: "{k}",
                                        class: if k == cur { "pdit-find-res is-cur" } else { "pdit-find-res" },
                                        r#type: "button",
                                        onclick: move |_| find.show(k),
                                        span { class: "pg", "p. {m.page + 1}" }
                                        span { class: "ctx",
                                            "{before}"
                                            b { "{hit}" }
                                            "{after}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The match with a little of its line on each side.
fn context(m: &Match) -> (String, String, String) {
    let chars: Vec<char> = m.line.chars().collect();
    let (start, len) = m.span;
    let from = start.saturating_sub(22);
    let to = (start + len + 30).min(chars.len());
    let text = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    (
        format!("{}{}", if from > 0 { "…" } else { "" }, text(from, start)),
        text(start, start + len),
        format!(
            "{}{}",
            text(start + len, to),
            if to < chars.len() { "…" } else { "" }
        ),
    )
}

/// Per page: the match highlights. `scale` is CSS px per PDF point.
#[component]
pub fn FindOverlay(page: u16, page_height_pt: f32, scale: f32) -> Element {
    let find = use_context::<Find>();
    let cur = (find.cur)();
    let results = find.results.read();
    rsx! {
        for (k, m) in results.iter().enumerate().filter(|(_, m)| m.page == page) {
            {
                let [l, b, r, t] = m.rect;
                rsx! {
                    div {
                        key: "{k}",
                        class: if k == cur { "pdit-find-hit is-cur" } else { "pdit-find-hit" },
                        style: "left: {l * scale - 1.0}px; top: {(page_height_pt - t) * scale - 1.0}px; width: {(r - l) * scale + 2.0}px; height: {(t - b) * scale + 2.0}px;",
                    }
                }
            }
        }
    }
}
