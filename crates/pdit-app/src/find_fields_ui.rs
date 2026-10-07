//! Find fields (D-053): leafmind's field finder suggests the form fields on
//! every page; the form editor's bar shows the progress and then "N fields
//! found" with Add / Cancel, and the suggestions sit on the pages in the form
//! editor's dashed outline (click one to leave it out). Add makes them real
//! fields (one Undo) and opens the form editor. Look: form-fill.css.

use crate::page_tools::PageTools;
use dioxus::prelude::*;
use pdit_core::fields::{self, FieldFinder, FoundField, FoundKind};
use pdit_core::form_edit::{self as fe, NewField};
use std::cell::RefCell;
use std::rc::Rc;

const MODEL: Asset = asset!("/assets/models/form-field-v1-nano.onnx");
const STATE_MODEL: Asset = asset!("/assets/models/form-field-v1-state.onnx");

thread_local! {
    /// Loaded on first use, then kept.
    static FINDER: RefCell<Option<Rc<FieldFinder>>> = const { RefCell::new(None) };
}

/// Suggestions with their pages.
type Found = Rc<Vec<(u16, FoundField)>>;

#[derive(Clone, PartialEq)]
enum Stage {
    Idle,
    Loading,
    Finding {
        page: u16,
        total: u16,
    },
    /// Suggestions, and which are left out.
    Review {
        found: Rc<Vec<(u16, FoundField)>>,
        off: Vec<bool>,
    },
    /// Added; `before` is the document before, for Undo.
    Added {
        count: usize,
        before: Rc<Vec<u8>>,
        found: Rc<Vec<(u16, FoundField)>>,
    },
}

/// Shared state.
#[derive(Clone, Copy)]
pub struct FindFields {
    stage: Signal<Stage>,
    /// Suggestions the AI analysis already found (D-055), for the AI menu's
    /// "Find form fields".
    prepared: Signal<Option<Found>>,
}

impl FindFields {
    pub fn provide() -> Self {
        use_context_provider(|| FindFields {
            stage: Signal::new(Stage::Idle),
            prepared: Signal::new(None),
        })
    }

    /// Fields were just added: the form editor is open under this bar.
    pub fn added(&self) -> bool {
        matches!(*self.stage.read(), Stage::Added { .. })
    }

    /// Rail → Form → Find fields: looks at every page.
    pub fn start(mut self) {
        if !matches!(*self.stage.peek(), Stage::Idle | Stage::Review { .. }) {
            return;
        }
        let forms = consume_context::<crate::form_edit_ui::FormEdit>();
        if forms.editing() {
            forms.finish();
        }
        self.stage.set(Stage::Loading);
        spawn(async move {
            let Some(finder) = finder().await else {
                self.stage.set(Stage::Idle);
                return consume_context::<PageTools>().show(
                    "Find fields could not start".into(),
                    crate::form_edit_ui::ICON_FORM,
                    None,
                );
            };
            let total = pdit_core::page_ops::page_sizes().map_or(0, |s| s.len()) as u16;
            let mut found = Vec::new();
            for page in 0..total {
                // Cancelled (Esc or Cancel) between pages.
                if !matches!(*self.stage.peek(), Stage::Loading | Stage::Finding { .. }) {
                    return;
                }
                self.stage.set(Stage::Finding {
                    page: page + 1,
                    total,
                });
                // Let the status show before the (blocking) look at the page.
                crate::print_ui::pause(30).await;
                match fields::find_fields(&finder, page) {
                    Ok(on_page) => {
                        let existing = pdit_core::form_fields(page).unwrap_or_default();
                        found.extend(
                            on_page
                                .into_iter()
                                .filter(|f| {
                                    !existing.iter().any(|e| {
                                        covers([e.rect.0, e.rect.1, e.rect.2, e.rect.3], f.rect)
                                    })
                                })
                                .map(|f| (page, f)),
                        );
                    }
                    Err(error) => {
                        crate::log(&format!("pdit: find fields, page {}: {error}", page + 1))
                    }
                }
            }
            if matches!(*self.stage.peek(), Stage::Finding { .. }) {
                let off = vec![false; found.len()];
                self.stage.set(Stage::Review {
                    found: Rc::new(found),
                    off,
                });
            }
        });
    }

    /// The analysis's suggestions (D-055), kept for the AI menu.
    pub fn set_prepared(mut self, found: Option<Vec<(u16, FoundField)>>) {
        self.prepared.set(found.map(Rc::new));
    }

    /// After the analysis (D-060): its suggestions open for review, if any.
    pub fn review_prepared_quietly(self) {
        if self.prepared.peek().is_some() {
            self.review_prepared();
        }
    }

    /// The analysis's suggestions to review.
    pub fn review_prepared(mut self) {
        let Some(found) = self.prepared.peek().clone() else {
            return consume_context::<PageTools>().show(
                "No new form fields to suggest".into(),
                crate::form_edit_ui::ICON_FORM,
                None,
            );
        };
        if !matches!(*self.stage.peek(), Stage::Idle | Stage::Review { .. }) {
            return;
        }
        let off = vec![false; found.len()];
        self.stage.set(Stage::Review { found, off });
    }

    pub fn cancel(mut self) {
        if matches!(
            *self.stage.peek(),
            Stage::Loading | Stage::Finding { .. } | Stage::Review { .. }
        ) {
            self.stage.set(Stage::Idle);
        }
    }

    fn toggle(mut self, k: usize) {
        self.stage.with_mut(|s| {
            if let Stage::Review { off, .. } = s
                && let Some(o) = off.get_mut(k)
            {
                *o = !*o;
            }
        });
    }

    /// Makes the kept suggestions real fields, then opens the form editor.
    fn add(mut self) {
        let Stage::Review { found, off } = self.stage.peek().clone() else {
            return;
        };
        let tools = consume_context::<PageTools>();
        let Some(before) = tools.snapshot() else {
            return;
        };
        let mut taken = fe::field_names().unwrap_or_default();
        let mut count = 0;
        for ((page, f), _) in found.iter().zip(&off).filter(|(_, off)| !**off) {
            let name = fields::unique(&f.name, &taken);
            let kind = match f.kind {
                FoundKind::Checkbox => NewField::Checkbox,
                FoundKind::Text | FoundKind::Signature => NewField::Text,
            };
            if let Err(error) = fe::add_field(*page, &kind, f.rect, &name) {
                crate::log(&format!("pdit: could not add the found fields: {error}"));
                tools.restore_snapshot(&before);
                return;
            }
            taken.push(name);
            count += 1;
        }
        tools.refresh_pages();
        consume_context::<crate::form_edit_ui::FormEdit>().start(None);
        self.stage.set(Stage::Added {
            count,
            before,
            found,
        });
    }

    /// Undo: the document as before, back to the suggestions.
    fn undo(mut self) {
        let Stage::Added { before, found, .. } = self.stage.peek().clone() else {
            return;
        };
        consume_context::<crate::form_edit_ui::FormEdit>().finish();
        consume_context::<PageTools>().restore_snapshot(&before);
        let off = vec![false; found.len()];
        self.stage.set(Stage::Review { found, off });
    }

    fn done(mut self) {
        consume_context::<crate::form_edit_ui::FormEdit>().finish();
        self.stage.set(Stage::Idle);
    }
}

/// An existing field already covers most of a suggestion.
pub(crate) fn covers(existing: [f32; 4], found: [f32; 4]) -> bool {
    let w = existing[2].min(found[2]) - existing[0].max(found[0]);
    let h = existing[3].min(found[3]) - existing[1].max(found[1]);
    let area = (found[2] - found[0]) * (found[3] - found[1]);
    w > 0.0 && h > 0.0 && area > 0.0 && w * h / area > 0.5
}

/// The finder, loaded from the bundled models on first use.
pub(crate) async fn finder() -> Option<Rc<FieldFinder>> {
    if let Some(finder) = FINDER.with_borrow(|f| f.clone()) {
        return Some(finder);
    }
    let model = crate::fetch_bytes(&MODEL.to_string()).await.ok()?;
    let state = crate::fetch_bytes(&STATE_MODEL.to_string()).await.ok()?;
    let finder = FieldFinder::from_onnx(&model)
        .and_then(|f| f.with_state(&state))
        .map_err(|e| crate::log(&format!("pdit: find fields: {e}")))
        .ok()?;
    let finder = Rc::new(finder);
    FINDER.with_borrow_mut(|f| *f = Some(finder.clone()));
    Some(finder)
}

/// The bar (the form editor's), while finding and after.
#[component]
pub fn FindFieldsBar() -> Element {
    let ui = use_context::<FindFields>();
    let stage = (ui.stage)();
    let msg = match &stage {
        Stage::Idle => return rsx! {},
        Stage::Loading => rsx! { "Getting ready…" },
        Stage::Finding { page, total } => rsx! { "Finding fields… page {page} of {total}" },
        Stage::Review { found, off } if found.is_empty() => {
            let _ = off;
            rsx! { b { "No fields found" } }
        }
        Stage::Review { found, off } => {
            let left_out = off.iter().filter(|o| **o).count();
            rsx! {
                b { if found.len() == 1 { "1 field found" } else { "{found.len()} fields found" } }
                if left_out > 0 { " · {left_out} left out" } else { " · click one to leave it out" }
            }
        }
        Stage::Added { count, .. } => rsx! {
            b { if *count == 1 { "1 field added" } else { "{count} fields added" } }
            " · the form editor is open to rename or resize them"
        },
    };
    let kept = match &stage {
        Stage::Review { off, .. } => off.iter().filter(|o| !**o).count(),
        _ => 0,
    };
    let empty = matches!(&stage, Stage::Review { found, .. } if found.is_empty());
    rsx! {
        div { class: "pdit-shape-bar pdit-form-bar pdit-ff-bar",
            span { class: "tool", span { "Find fields" } }
            span { class: "sep" }
            span { class: "msg", {msg} }
            span { class: "sep" }
            match stage {
                Stage::Added { .. } => rsx! {
                    button { class: "act", r#type: "button", onclick: move |_| ui.undo(), "Undo" }
                    button { class: "act", r#type: "button", onclick: move |_| ui.done(), "Done" }
                },
                Stage::Review { .. } if !empty => rsx! {
                    button { class: "act", r#type: "button", onclick: move |_| ui.cancel(), "Cancel" }
                    button {
                        class: "act is-primary",
                        r#type: "button",
                        disabled: kept == 0,
                        onclick: move |_| ui.add(),
                        if kept == 1 { "Add 1 field" } else { "Add {kept} fields" }
                    }
                },
                Stage::Review { .. } => rsx! {
                    button { class: "act", r#type: "button", onclick: move |_| ui.cancel(), "Close" }
                },
                _ => rsx! {
                    button { class: "act", r#type: "button", onclick: move |_| ui.cancel(), "Cancel" }
                },
            }
        }
    }
}

/// Per page: the suggestions. `scale` is CSS px per PDF point.
#[component]
pub fn FindFieldsOverlay(page: u16, page_height_pt: f32, scale: f32) -> Element {
    let ui = use_context::<FindFields>();
    let Stage::Review { found, off } = (ui.stage)() else {
        return rsx! {};
    };
    rsx! {
        for (k, (p, f)) in found.iter().enumerate().filter(|(_, (p, _))| *p == page) {
            {
                let _ = p;
                let [l, b, r, t] = f.rect;
                let gone = off.get(k).copied().unwrap_or(false);
                let tag = match (f.kind, f.filled) {
                    (FoundKind::Checkbox, _) => format!("☐ {}", f.name),
                    (FoundKind::Signature, _) => format!("{} · signature", f.name),
                    (_, Some(true)) => format!("{} · already filled", f.name),
                    _ => f.name.clone(),
                };
                rsx! {
                    div {
                        key: "{k}",
                        class: if gone { "pdit-ff-sug is-off" } else { "pdit-ff-sug" },
                        title: if gone { "Left out — click to add it back" } else { "Click to leave out" },
                        style: "left: {l * scale}px; top: {(page_height_pt - t) * scale}px; width: {(r - l) * scale}px; height: {(t - b) * scale}px;",
                        onpointerdown: move |event| event.stop_propagation(),
                        onclick: move |event| {
                            event.stop_propagation();
                            ui.toggle(k);
                        },
                        span { class: "pdit-fe-tag", "{tag}" }
                    }
                }
            }
        }
    }
}
