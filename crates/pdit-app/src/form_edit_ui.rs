//! Creating and editing form fields (D-047, approved mockup). Right-click →
//! Form → Edit form (or a field type) turns on edit mode: the shapes bar with
//! the four field types and Done; fields show a dashed outline and their name.
//! Drag to place a text field or dropdown (a click gives a default size),
//! click to place a checkbox or radio button (a radio joins the selected
//! radio's group). Click a field to select it: the image ring + corner
//! handles (drag the field to move it, a corner to resize) and a bar with
//! Settings · Delete. Done returns to filling. Every change goes through the
//! page tools' snapshot + Undo toast; the engine is pdit-core's form_edit.

use crate::page_tools::{PageTools, next_frame};
use dioxus::prelude::*;
use pdit_core::form_edit::{self as fe, FieldSettings, NewField};
use pdit_core::{FieldKind, FormField};

pub const ICON_FORM: &str = include_str!("../assets/icons/devigner/ClipboardList.svg");
const ICON_TEXT: &str = include_str!("../assets/icons/devigner/TextField.svg");
const ICON_CHECK: &str = include_str!("../assets/icons/devigner/CheckSquare.svg");
const ICON_RADIO: &str = include_str!("../assets/icons/devigner/RecordCircle.svg");
const ICON_DROPDOWN: &str = include_str!("../assets/icons/devigner/ChevronDownSquare.svg");
const ICON_SETTINGS: &str = include_str!("../assets/icons/devigner/Settings.svg");
const ICON_TRASH: &str = include_str!("../assets/icons/devigner/TrashBinMinimalistic.svg");
const SHAPES_CSS: Asset = asset!("/assets/css/shapes.css");
const IMAGE_SELECT_CSS: Asset = asset!("/assets/css/image-select.css");

/// A field type to place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Text,
    Checkbox,
    Radio,
    Dropdown,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Text, Tool::Checkbox, Tool::Radio, Tool::Dropdown];
    pub fn label(self) -> &'static str {
        match self {
            Tool::Text => "Text field",
            Tool::Checkbox => "Checkbox",
            Tool::Radio => "Radio button",
            Tool::Dropdown => "Dropdown",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Tool::Text => ICON_TEXT,
            Tool::Checkbox => ICON_CHECK,
            Tool::Radio => ICON_RADIO,
            Tool::Dropdown => ICON_DROPDOWN,
        }
    }
    /// Default size in points (a click, or a checkbox/radio).
    fn size(self) -> (f32, f32) {
        match self {
            Tool::Text => (180.0, 20.0),
            Tool::Dropdown => (140.0, 20.0),
            Tool::Checkbox | Tool::Radio => (12.0, 12.0),
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Tool::Text => "text",
            Tool::Checkbox => "checkbox",
            Tool::Radio => "group",
            Tool::Dropdown => "dropdown",
        }
    }
}

/// Corner handles (resize) only where they fit: text fields and dropdowns.
/// Checkboxes and radio buttons are too small; they are moved by dragging.
fn has_handles(kind: FieldKind) -> bool {
    !matches!(kind, FieldKind::Checkbox | FieldKind::Radio)
}

fn kind_icon(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::Checkbox => ICON_CHECK,
        FieldKind::Radio => ICON_RADIO,
        FieldKind::ComboBox | FieldKind::ListBox => ICON_DROPDOWN,
        _ => ICON_TEXT,
    }
}

/// What a press on the page started.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Drag {
    /// Placing a field: from this corner to `now`.
    Place {
        page: u16,
        from: (f32, f32),
        now: (f32, f32),
    },
    /// Moving (corner `None`) or resizing (corner 0 tl, 1 tr, 2 bl, 3 br) the
    /// selected field; `rect` is where it is now, `orig` where it was.
    Shape {
        page: u16,
        annotation: usize,
        orig: [f32; 4],
        rect: [f32; 4],
        corner: Option<u8>,
        from: (f32, f32),
    },
}

/// The Settings box's draft.
#[derive(Clone, PartialEq, Debug)]
struct Settings {
    page: u16,
    annotation: usize,
    kind: FieldKind,
    name: String,
    required: bool,
    choice: String,
    options: String,
    error: Option<String>,
    shown: bool,
}

/// Shared form-editing state.
#[derive(Clone, Copy)]
pub struct FormEdit {
    on: Signal<bool>,
    tool: Signal<Option<Tool>>,
    /// The selected field: (page, annotation).
    selected: Signal<Option<(u16, usize)>>,
    drag: Signal<Option<Drag>>,
    settings: Signal<Option<Settings>>,
}

fn rect_of(f: &FormField) -> [f32; 4] {
    [f.rect.0, f.rect.1, f.rect.2, f.rect.3]
}

fn field(page: u16, annotation: usize) -> Option<FormField> {
    pdit_core::form_fields(page)
        .ok()?
        .into_iter()
        .find(|f| f.annotation == annotation)
}

impl FormEdit {
    pub fn provide() -> Self {
        use_context_provider(|| FormEdit {
            on: Signal::new(false),
            tool: Signal::new(None),
            selected: Signal::new(None),
            drag: Signal::new(None),
            settings: Signal::new(None),
        })
    }

    /// Edit mode, for components that re-render when it changes.
    pub fn on_signal(&self) -> Signal<bool> {
        self.on
    }

    pub fn editing(&self) -> bool {
        *self.on.peek()
    }

    /// Right-click → Form: edit mode, with `tool` armed (or none).
    pub fn start(mut self, tool: Option<Tool>) {
        self.on.set(true);
        self.tool.set(tool);
    }

    /// Done (or Form → Done editing form): back to filling.
    pub fn finish(mut self) {
        self.on.set(false);
        self.tool.set(None);
        self.selected.set(None);
        self.drag.set(None);
        self.settings.set(None);
    }

    /// Escape: closes the Settings box, else deselects, else disarms.
    pub fn escape(mut self) {
        if self.settings.peek().is_some() {
            self.settings.set(None);
        } else if self.selected.peek().is_some() {
            self.selected.set(None);
        } else if self.tool.peek().is_some() {
            self.tool.set(None);
        }
    }

    pub fn busy(&self) -> bool {
        self.drag.peek().is_some()
    }

    /// A press on `page` at (x, y) points while editing; `per_pt` is CSS px per
    /// point (for the handles' grab size).
    pub fn press(mut self, page: u16, x: f32, y: f32, per_pt: f32) {
        self.settings.set(None);
        if self.tool.peek().is_some() {
            self.drag.set(Some(Drag::Place {
                page,
                from: (x, y),
                now: (x, y),
            }));
            return;
        }
        let grab = 8.0 / per_pt.max(0.01);
        let fields = pdit_core::form_fields(page).unwrap_or_default();
        // The selected field's corners first, then any field under the pointer.
        if let Some((p, a)) = *self.selected.peek()
            && p == page
            && let Some(f) = fields.iter().find(|f| f.annotation == a)
            && has_handles(f.kind)
        {
            let [l, b, r, t] = rect_of(f);
            let corners = [(l, t), (r, t), (l, b), (r, b)];
            if let Some(c) = corners
                .iter()
                .position(|&(cx, cy)| (cx - x).abs() <= grab && (cy - y).abs() <= grab)
            {
                self.drag.set(Some(Drag::Shape {
                    page,
                    annotation: a,
                    orig: rect_of(f),
                    rect: rect_of(f),
                    corner: Some(c as u8),
                    from: (x, y),
                }));
                return;
            }
        }
        let hit = fields.iter().rev().find(|f| {
            let [l, b, r, t] = rect_of(f);
            x >= l && x <= r && y >= b && y <= t
        });
        match hit {
            Some(f) => {
                self.selected.set(Some((page, f.annotation)));
                self.drag.set(Some(Drag::Shape {
                    page,
                    annotation: f.annotation,
                    orig: rect_of(f),
                    rect: rect_of(f),
                    corner: None,
                    from: (x, y),
                }));
            }
            None => self.selected.set(None),
        }
    }

    pub fn drag_to(mut self, x: f32, y: f32) {
        let Some(d) = *self.drag.peek() else { return };
        self.drag.set(Some(match d {
            Drag::Place { page, from, .. } => Drag::Place {
                page,
                from,
                now: (x, y),
            },
            Drag::Shape {
                page,
                annotation,
                orig,
                corner,
                from,
                ..
            } => {
                let (dx, dy) = (x - from.0, y - from.1);
                let [l, b, r, t] = orig;
                let rect = match corner {
                    None => [l + dx, b + dy, r + dx, t + dy],
                    Some(c) => {
                        let min = 8.0;
                        let (mut l2, mut b2, mut r2, mut t2) = (l, b, r, t);
                        if c % 2 == 0 {
                            l2 = (l + dx).min(r - min)
                        } else {
                            r2 = (r + dx).max(l + min)
                        }
                        if c < 2 {
                            t2 = (t + dy).max(b + min)
                        } else {
                            b2 = (b + dy).min(t - min)
                        }
                        [l2, b2, r2, t2]
                    }
                };
                Drag::Shape {
                    page,
                    annotation,
                    orig,
                    rect,
                    corner,
                    from,
                }
            }
        }));
    }

    pub fn release(mut self) {
        let Some(d) = self.drag.take() else { return };
        match d {
            Drag::Place { page, from, now } => self.place(page, from, now),
            Drag::Shape {
                page,
                annotation,
                orig,
                rect,
                corner,
                ..
            } => {
                let moved = orig.iter().zip(rect).any(|(a, b)| (a - b).abs() > 0.5);
                if moved {
                    let message = if corner.is_some() {
                        "Field resized"
                    } else {
                        "Field moved"
                    };
                    consume_context::<PageTools>().apply(message, ICON_FORM, || {
                        fe::set_field_rect(page, annotation, orig, rect)
                    });
                }
            }
        }
    }

    /// Places the armed field type: dragged size, or the default at a click.
    fn place(mut self, page: u16, from: (f32, f32), to: (f32, f32)) {
        let Some(tool) = *self.tool.peek() else {
            return;
        };
        let (w, h) = tool.size();
        let dragged = (to.0 - from.0).abs() > 4.0 || (to.1 - from.1).abs() > 4.0;
        let rect = match tool {
            Tool::Checkbox | Tool::Radio => [
                from.0 - w / 2.0,
                from.1 - h / 2.0,
                from.0 + w / 2.0,
                from.1 + h / 2.0,
            ],
            _ if dragged => [
                from.0.min(to.0),
                from.1.min(to.1),
                from.0.max(to.0).max(from.0.min(to.0) + 24.0),
                from.1.max(to.1).max(from.1.min(to.1) + 12.0),
            ],
            _ => [from.0, from.1 - h / 2.0, from.0 + w, from.1 + h / 2.0],
        };
        let names = fe::field_names().unwrap_or_default();
        let fresh = |prefix: &str| {
            (1..)
                .map(|n| format!("{prefix}{n}"))
                .find(|n| !names.contains(n))
                .unwrap_or_default()
        };
        // A radio button joins the selected radio's group.
        let group = self
            .selected
            .peek()
            .and_then(|(p, a)| field(p, a))
            .filter(|f| f.kind == FieldKind::Radio && tool == Tool::Radio)
            .and_then(|f| f.name);
        let (kind, name, message) = match tool {
            Tool::Text => (
                NewField::Text,
                fresh(tool.prefix()),
                "Text field added".to_owned(),
            ),
            Tool::Checkbox => (
                NewField::Checkbox,
                fresh(tool.prefix()),
                "Checkbox added".to_owned(),
            ),
            Tool::Dropdown => (
                NewField::Dropdown {
                    options: vec!["Option 1".into(), "Option 2".into()],
                },
                fresh(tool.prefix()),
                "Dropdown added".to_owned(),
            ),
            Tool::Radio => {
                let name = group.clone().unwrap_or_else(|| fresh(tool.prefix()));
                let taken: Vec<String> = all_fields()
                    .into_iter()
                    .filter(|(_, f)| f.name.as_deref() == Some(&name))
                    .filter_map(|(_, f)| f.choice)
                    .collect();
                let choice = (1..)
                    .map(|n| format!("Choice{n}"))
                    .find(|c| !taken.contains(c))
                    .unwrap_or_default();
                let message = match &group {
                    Some(g) => format!("Radio button added to {g}"),
                    None => "Radio button added".to_owned(),
                };
                (NewField::Radio { choice }, name, message)
            }
        };
        let added = consume_context::<PageTools>().apply(&message, ICON_FORM, || {
            fe::add_field(page, &kind, rect, &name)
        });
        if added {
            // The new widget is the page's last annotation.
            let newest = pdit_core::form_fields(page)
                .unwrap_or_default()
                .into_iter()
                .map(|f| f.annotation)
                .max();
            self.selected.set(newest.map(|a| (page, a)));
        }
    }

    fn open_settings(mut self, page: u16, annotation: usize) {
        let Some(f) = field(page, annotation) else {
            return;
        };
        self.settings.set(Some(Settings {
            page,
            annotation,
            kind: f.kind,
            name: f.name.unwrap_or_default(),
            required: f.required,
            choice: f.choice.unwrap_or_default(),
            options: f.options.join("\n"),
            error: None,
            shown: false,
        }));
        let mut s = self.settings;
        next_frame(move || {
            next_frame(move || {
                s.with_mut(|s| {
                    if let Some(s) = s.as_mut() {
                        s.shown = true;
                    }
                })
            })
        });
    }

    /// Changes the Settings box's draft (clearing its error).
    fn edit_settings(mut self, change: impl FnOnce(&mut Settings)) {
        self.settings.with_mut(|b| {
            if let Some(b) = b.as_mut() {
                change(b);
                b.error = None;
            }
        })
    }

    fn save_settings(mut self) {
        let Some(s) = self.settings.peek().clone() else {
            return;
        };
        let Some(f) = field(s.page, s.annotation) else {
            self.settings.set(None);
            return;
        };
        let name = s.name.trim().to_owned();
        let old = f.name.clone().unwrap_or_default();
        let error = if name.is_empty() {
            Some("A field needs a name.")
        } else if name != old && fe::field_names().unwrap_or_default().contains(&name) {
            Some("Another field already has this name.")
        } else if f.kind == FieldKind::Radio
            && s.choice.trim() != f.choice.as_deref().unwrap_or_default()
            && all_fields().iter().any(|(_, o)| {
                o.name.as_deref() == Some(&old) && o.choice.as_deref() == Some(s.choice.trim())
            })
        {
            Some("Another button in this group has that choice.")
        } else {
            None
        };
        if let Some(e) = error {
            self.settings.with_mut(|b| {
                if let Some(b) = b.as_mut() {
                    b.error = Some(e.into());
                }
            });
            return;
        }
        let settings = FieldSettings {
            name,
            required: s.required,
            options: matches!(f.kind, FieldKind::ComboBox | FieldKind::ListBox).then(|| {
                s.options
                    .lines()
                    .map(str::trim)
                    .filter(|o| !o.is_empty())
                    .map(String::from)
                    .collect()
            }),
            choice: (f.kind == FieldKind::Radio).then(|| s.choice.trim().to_owned()),
        };
        self.settings.set(None);
        consume_context::<PageTools>().apply("Field settings changed", ICON_FORM, || {
            fe::set_field_settings(s.page, s.annotation, rect_of(&f), &settings)
        });
    }

    /// Right-click → Field settings… (D-050): opens the form editor if needed,
    /// selects the field and opens its settings.
    pub(crate) fn settings_for(mut self, page: u16, annotation: usize) {
        if !self.editing() {
            self.start(None);
        }
        self.selected.set(Some((page, annotation)));
        self.open_settings(page, annotation);
    }

    pub(crate) fn delete(mut self, page: u16, annotation: usize) {
        let Some(f) = field(page, annotation) else {
            return;
        };
        self.selected.set(None);
        self.settings.set(None);
        consume_context::<PageTools>().apply("Field deleted", ICON_FORM, || {
            fe::delete_field(page, annotation, rect_of(&f))
        });
    }
}

/// Every field of the document with its page.
fn all_fields() -> Vec<(u16, FormField)> {
    let pages = pdit_core::page_ops::page_sizes()
        .map(|s| s.len())
        .unwrap_or(0);
    (0..u16::try_from(pages).unwrap_or(0))
        .flat_map(|p| {
            pdit_core::form_fields(p)
                .unwrap_or_default()
                .into_iter()
                .map(move |f| (p, f))
        })
        .collect()
}

/// The edit-mode bar (the shapes bar): the field types and Done.
#[component]
pub fn FormEditBar() -> Element {
    let mut fe_ui = use_context::<FormEdit>();
    let on = *fe_ui.on.read();
    let tool = *fe_ui.tool.read();
    // Find fields' bar stands in for this one right after adding (D-053).
    let found = use_context::<crate::find_fields_ui::FindFields>().added();
    rsx! {
        document::Stylesheet { href: SHAPES_CSS }
        document::Stylesheet { href: IMAGE_SELECT_CSS }
        if on && !found {
            div { class: "pdit-shape-bar pdit-form-bar",
                span { class: "tool",
                    span { dangerous_inner_html: ICON_FORM, style: "display: contents" }
                    span { "Edit form" }
                }
                span { class: "sep" }
                for t in Tool::ALL {
                    button {
                        class: "t",
                        r#type: "button",
                        title: t.label(),
                        "aria-pressed": if tool == Some(t) { "true" } else { "false" },
                        onclick: move |_| fe_ui.tool.set(if tool == Some(t) { None } else { Some(t) }),
                        span { dangerous_inner_html: t.icon(), style: "display: contents" }
                        span { "{t.label()}" }
                    }
                }
                span { class: "sep" }
                button { class: "act", r#type: "button", onclick: move |_| fe_ui.finish(), "Done" }
            }
        }
    }
}

/// Per page, in edit mode: outlines + names, the placing rectangle, the
/// selected field's ring, handles, bar and Settings box. Pointer input comes
/// from the page (pages.rs → `press` / `drag_to` / `release`).
#[component]
pub fn FormEditOverlay(page: u16, page_width_pt: f32, page_height_pt: f32, scale: f32) -> Element {
    let ui = use_context::<FormEdit>();
    let on = *ui.on.read();
    let drag = *ui.drag.read();
    let selected = *ui.selected.read();
    let tool = *ui.tool.read();
    let settings = ui.settings.read().clone().filter(|s| s.page == page);
    // Re-read after every change: the page refresh remounts the overlays.
    let _ = use_context::<Signal<Option<crate::pages::OpenDocument>>>().read();
    if !on {
        return rsx! {};
    }
    let fields = pdit_core::form_fields(page).unwrap_or_default();
    let px = move |[l, b, r, t]: [f32; 4]| {
        (
            l * scale,
            (page_height_pt - t) * scale,
            (r - l) * scale,
            (t - b) * scale,
        )
    };
    let live = |f: &FormField| match drag {
        Some(Drag::Shape {
            page: p,
            annotation,
            rect,
            ..
        }) if p == page && annotation == f.annotation => rect,
        _ => rect_of(f),
    };
    let sel = selected
        .filter(|(p, _)| *p == page)
        .and_then(|(_, a)| fields.iter().find(|f| f.annotation == a).cloned());
    rsx! {
        if tool.is_some() {
            div { class: "pdit-fe-arming" }
        }
        for f in fields.iter() {
            {
                let (x, y, w, h) = px(live(f));
                let tag = if f.kind == FieldKind::Radio { f.choice.clone() } else { f.name.clone() }
                    .unwrap_or_default();
                rsx! {
                    div {
                        key: "fe-{f.annotation}",
                        class: if f.required { "pdit-fe-field is-required" } else { "pdit-fe-field" },
                        style: "left: {x}px; top: {y}px; width: {w}px; height: {h}px;",
                        span { class: "pdit-fe-tag", "{tag}" }
                    }
                }
            }
        }
        if let Some(Drag::Place { page: p, from, now }) = drag
            && p == page
        {
            {
                let r = [from.0.min(now.0), from.1.min(now.1), from.0.max(now.0), from.1.max(now.1)];
                let (x, y, w, h) = px(r);
                rsx! { div { class: "pdit-fe-draft", style: "left: {x}px; top: {y}px; width: {w}px; height: {h}px;" } }
            }
        }
        if let Some(f) = sel {
            {
                let (x, y, w, h) = px(live(&f));
                let annotation = f.annotation;
                let label = match (f.kind, &f.choice) {
                    (FieldKind::Radio, Some(c)) => format!("{} · {c}", f.name.clone().unwrap_or_default()),
                    _ => f.name.clone().unwrap_or_default(),
                };
                let corners = [("tl", x, y), ("tr", x + w, y), ("bl", x, y + h), ("br", x + w, y + h)];
                let width_px = page_width_pt * scale;
                rsx! {
                    div {
                        class: "pdit-img-ring",
                        style: "left: {x - 3.0}px; top: {y - 3.0}px; width: {w + 6.0}px; height: {h + 6.0}px;",
                    }
                    for (c, hx, hy) in corners.into_iter().filter(|_| has_handles(f.kind)) {
                        div { key: "{c}", class: "pdit-img-handle {c}", style: "left: {hx}px; top: {hy}px;" }
                    }
                    if drag.is_none() {
                        div {
                            class: "sa-root sa-anchor pdit-markup-anchor",
                            style: "transform: translate({(x + w / 2.0).clamp(150.0, (width_px - 150.0).max(150.0))}px, {y + h + 12.0}px) translateX(-50%);",
                            onpointerdown: move |event| event.stop_propagation(),
                            onclick: move |event| event.stop_propagation(),
                            div { class: "sa-bar",
                                span { class: "pdit-link-url",
                                    span { dangerous_inner_html: kind_icon(f.kind), style: "display: contents" }
                                    " {label}"
                                }
                                button {
                                    class: "sa-control",
                                    r#type: "button",
                                    onclick: move |_| ui.open_settings(page, annotation),
                                    span { dangerous_inner_html: ICON_SETTINGS, style: "display: contents" }
                                    "Settings"
                                }
                                button {
                                    class: "sa-control",
                                    r#type: "button",
                                    onclick: move |_| ui.delete(page, annotation),
                                    span { dangerous_inner_html: ICON_TRASH, style: "display: contents" }
                                    "Delete"
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(s) = settings {
            SettingsBox { s, page_width_pt, page_height_pt, scale }
        }
    }
}

/// The Settings box: the link box's surface, with "Tabs sliding".
#[component]
fn SettingsBox(s: Settings, page_width_pt: f32, page_height_pt: f32, scale: f32) -> Element {
    let mut ui = use_context::<FormEdit>();
    let pill_key = (s.shown, s.required);
    use_effect(use_reactive!(|pill_key| {
        let _ = pill_key;
        crate::doc_ui::place_pills_soon(".pdit-fe-box .t-tabs");
    }));
    let Some(f) = field(s.page, s.annotation) else {
        return rsx! {};
    };
    let [l, b, _, _] = rect_of(&f);
    let left = (l * scale).min(page_width_pt * scale - 300.0).max(8.0);
    let top = (page_height_pt - b) * scale + 56.0;
    let class = if s.shown {
        "cm-menu t-dropdown pdit-link-box pdit-fe-box is-open"
    } else {
        "cm-menu t-dropdown pdit-link-box pdit-fe-box"
    };
    rsx! {
        div {
            class,
            "data-origin": "top-left",
            style: "left: {left}px; top: {top}px;",
            onpointerdown: move |event| event.stop_propagation(),
            onclick: move |event| event.stop_propagation(),
            div { class: "pdit-link-label", if s.kind == FieldKind::Radio { "Group name" } else { "Name" } }
            input {
                class: "pdit-link-field",
                r#type: "text",
                "aria-label": "Field name",
                value: "{s.name}",
                onmounted: move |event| {
                    spawn(async move {
                        let _ = event.data().set_focus(true).await;
                    });
                },
                oninput: move |event| ui.edit_settings(move |b| b.name = event.value()),
                onkeydown: move |event| {
                    if event.key() == Key::Enter {
                        ui.save_settings();
                    }
                },
            }
            if s.kind == FieldKind::Radio {
                div { class: "pdit-link-label", "This button’s choice" }
                input {
                    class: "pdit-link-field",
                    r#type: "text",
                    "aria-label": "Choice",
                    value: "{s.choice}",
                    oninput: move |event| ui.edit_settings(move |b| b.choice = event.value()),
                }
            }
            if matches!(s.kind, FieldKind::ComboBox | FieldKind::ListBox) {
                div { class: "pdit-link-label", "Options — one per line" }
                textarea {
                    class: "pdit-link-field pdit-fe-options",
                    "aria-label": "Options",
                    value: "{s.options}",
                    oninput: move |event| ui.edit_settings(move |b| b.options = event.value()),
                }
            }
            div { class: "row",
                crate::doc_ui::Tabs {
                    options: vec![("Optional".into(), !s.required), ("Required".into(), s.required)],
                    on_pick: move |i: usize| ui.edit_settings(move |b| b.required = i == 1),
                }
                if s.kind == FieldKind::Radio {
                    span { class: "pdit-fe-note", "for the whole group" }
                }
            }
            if let Some(message) = s.error.clone() {
                div { class: "pdit-link-error", role: "alert", "{message}" }
            }
            div { class: "row sa-root",
                span { class: "grow" }
                button { class: "sa-control", r#type: "button", onclick: move |_| ui.settings.set(None), "Cancel" }
                button { class: "sa-primary", r#type: "button", onclick: move |_| ui.save_settings(), "Save" }
            }
        }
    }
}
