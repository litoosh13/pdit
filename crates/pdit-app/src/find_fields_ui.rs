//! Find fields (D-053): leafmind's field finder looks at every page; the form
//! editor's bar shows the progress. What it finds is added right away (one
//! Undo) and the form editor opens, to move, resize or delete a wrong one
//! (P-050 #5). Look: form-fill.css.

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

#[derive(Clone, PartialEq)]
enum Stage {
    Idle,
    Loading,
    Finding {
        page: u16,
        total: u16,
    },
    /// Nothing new was found.
    NoneFound,
    /// Added; `before` is the document before, for Undo.
    Added {
        count: usize,
        before: Rc<Vec<u8>>,
    },
}

/// Shared state.
#[derive(Clone, Copy)]
pub struct FindFields {
    stage: Signal<Stage>,
}

impl FindFields {
    pub fn provide() -> Self {
        use_context_provider(|| FindFields {
            stage: Signal::new(Stage::Idle),
        })
    }

    /// Fields were just added: the form editor is open under this bar.
    pub fn added(&self) -> bool {
        matches!(*self.stage.read(), Stage::Added { .. })
    }

    /// Rail → Form → Find fields: looks at every page.
    pub fn start(mut self) {
        if !matches!(*self.stage.peek(), Stage::Idle | Stage::NoneFound) {
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
                match found.is_empty() {
                    true => self.stage.set(Stage::NoneFound),
                    false => self.add(&found),
                }
            }
        });
    }

    pub fn cancel(mut self) {
        if matches!(
            *self.stage.peek(),
            Stage::Loading | Stage::Finding { .. } | Stage::NoneFound
        ) {
            self.stage.set(Stage::Idle);
        }
    }

    /// Makes the found fields real fields, then opens the form editor.
    fn add(mut self, found: &[(u16, FoundField)]) {
        let tools = consume_context::<PageTools>();
        let Some(before) = tools.snapshot() else {
            return self.stage.set(Stage::Idle);
        };
        let mut taken = fe::field_names().unwrap_or_default();
        let mut count = 0;
        for (page, f) in found {
            let name = fields::unique(&f.name, &taken);
            let kind = match f.kind {
                FoundKind::Checkbox => NewField::Checkbox,
                FoundKind::Text | FoundKind::Signature => NewField::Text,
            };
            if let Err(error) = fe::add_field(*page, &kind, f.rect, &name) {
                crate::log(&format!("pdit: could not add the found fields: {error}"));
                tools.restore_snapshot(&before);
                return self.stage.set(Stage::Idle);
            }
            taken.push(name);
            count += 1;
        }
        tools.refresh_pages();
        consume_context::<crate::form_edit_ui::FormEdit>().start(None);
        self.stage.set(Stage::Added { count, before });
    }

    /// Undo: the document as before.
    fn undo(mut self) {
        let Stage::Added { before, .. } = self.stage.peek().clone() else {
            return;
        };
        consume_context::<crate::form_edit_ui::FormEdit>().finish();
        consume_context::<PageTools>().restore_snapshot(&before);
        self.stage.set(Stage::Idle);
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
        Stage::NoneFound => rsx! { b { "No fields found" } },
        Stage::Added { count, .. } => rsx! {
            b { if *count == 1 { "1 field added" } else { "{count} fields added" } }
            " · move, resize or delete a wrong one"
        },
    };
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
                Stage::NoneFound => rsx! {
                    button { class: "act", r#type: "button", onclick: move |_| ui.cancel(), "Close" }
                },
                _ => rsx! {
                    button { class: "act", r#type: "button", onclick: move |_| ui.cancel(), "Cancel" }
                },
            }
        }
    }
}
