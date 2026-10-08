//! The frame (D-062, approved mockup .claude/research/canva-ui/): no top bar;
//! a centre bar at the top with Undo · Edit | AI · Redo, and a bottom bar with
//! the file name, zoom, page count, full screen and Save — a React island
//! (web/gooey-island `mountFrame`, Devigner UI + Devigner Icons).
//!
//! **Edit mode** is the editor. **AI mode** hides the tools rail, the Pages
//! panel and the other pages: the current page sits on the left half and the
//! AI on the right half (for now the Analysis and Ask panels; the chat comes in
//! redesign step 6). The PDF is analysed when AI mode opens if it never was or
//! changed since, once the models allow it.

use crate::page_tools::PageTools;
use crate::pages::OpenDocument;
use crate::zoom_ui::Zoom;
use dioxus::prelude::*;
use js_sys::{Function, Object, Reflect};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Edit,
    Ai,
}

/// Shared frame state.
#[derive(Clone, Copy)]
pub struct Frame {
    pub mode: Signal<Mode>,
    /// The page AI mode shows (the current page when it opened).
    pub ai_page: Signal<u16>,
}

impl Frame {
    pub fn provide() -> Self {
        use_context_provider(|| Frame {
            mode: Signal::new(Mode::Edit),
            ai_page: Signal::new(0),
        })
    }

    pub fn is_ai(&self) -> bool {
        (self.mode)() == Mode::Ai
    }

    /// Back to Edit mode (the AI chat's "Not now").
    pub fn leave_ai(self) {
        self.set_mode(Mode::Edit);
    }

    fn set_mode(mut self, mode: Mode) {
        if *self.mode.peek() == mode {
            return;
        }
        let ask = consume_context::<crate::ask_ui::Ask>();
        match mode {
            Mode::Ai => {
                self.ai_page.set(crate::bookmarks_ui::current_page());
                consume_context::<crate::tools_ui::Tools>().escape();
                consume_context::<crate::search_ui::Find>().close();
                consume_context::<crate::editing::Editing>().cancel();
                consume_context::<crate::ai_ui::Ai>().entered();
                ask.open();
            }
            Mode::Edit => ask.close(),
        }
        self.mode.set(mode);
        if let Some(window) = web_sys::window() {
            window.scroll_to_with_x_and_y(0.0, 0.0);
        }
    }
}

#[component]
pub fn FrameBars() -> Element {
    let frame = use_context::<Frame>();
    let document = use_context::<Signal<Option<OpenDocument>>>();
    let zoom = use_context::<Zoom>();
    let tools = use_context::<PageTools>();
    let ai = use_context::<crate::ai_ui::Ai>();
    let ask = use_context::<crate::ask_ui::Ask>();
    let update = use_hook(|| Rc::new(RefCell::new(None::<Function>)));
    let mut ready = use_signal(|| false);
    // The island calls back from outside Dioxus: through callbacks, which run
    // inside the app (contexts and spawn work there).
    let on_mode =
        use_callback(move |ai: bool| frame.set_mode(if ai { Mode::Ai } else { Mode::Edit }));
    let on_zoom = use_callback(move |p: f64| zoom.set(p as f32, None));
    let on_undo = use_callback(move |_: ()| tools.undo_last());
    let on_save = use_callback(move |_: ()| crate::save_current());
    let on_pages =
        use_callback(move |_: ()| consume_context::<crate::tools_ui::Tools>().open_pages());

    // A new file opens in Edit mode.
    let open_seq = document.read().as_ref().map(|_| crate::ai_ui::open_seq());
    use_effect(use_reactive!(|open_seq| {
        let _ = open_seq;
        frame.set_mode(Mode::Edit);
    }));
    // AI mode: analyse once the models allow it (downloaded, or not needed here).
    use_effect(move || {
        if frame.is_ai() && ask.can_analyze() {
            ai.analyze_if_needed();
        }
    });
    // The page list makes room for the AI half (frame.css / pages.css).
    use_effect(move || {
        let ai_mode = frame.is_ai();
        if let Some(body) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.body())
        {
            let _ = body.class_list().toggle_with_force("pdit-ai-mode", ai_mode);
        }
    });

    let state = (
        document
            .read()
            .as_ref()
            .map(|d| (d.name.clone(), d.page_sizes.len())),
        (frame.mode)(),
        *zoom.percent.read(),
        tools.can_undo(),
    );
    let push = update.clone();
    use_effect(use_reactive!(|state| {
        let _ = ready();
        let Some(update) = push.borrow().clone() else {
            return;
        };
        let (doc, mode, percent, can_undo) = state;
        let s = Object::new();
        let _ = Reflect::set(&s, &"hasDoc".into(), &doc.is_some().into());
        if let Some((name, pages)) = doc {
            let _ = Reflect::set(&s, &"fileName".into(), &name.into());
            let _ = Reflect::set(&s, &"pages".into(), &(pages as f64).into());
        }
        let mode = if mode == Mode::Ai { "ai" } else { "edit" };
        let _ = Reflect::set(&s, &"mode".into(), &mode.into());
        let _ = Reflect::set(&s, &"zoom".into(), &f64::from(percent).into());
        let _ = Reflect::set(&s, &"canUndo".into(), &can_undo.into());
        let _ = update.call1(&JsValue::NULL, &s);
    }));

    rsx! {
        document::Stylesheet { href: crate::island::ISLAND_CSS }
        div {
            class: "pdit-frame",
            onmounted: move |event| {
                let update = update.clone();
                async move {
                    let Some(element) = event.data().downcast::<web_sys::Element>().cloned() else {
                        return;
                    };
                    let options = Object::new();
                    let on = |name: &str, f: Closure<dyn FnMut(JsValue)>| {
                        let _ = Reflect::set(&options, &name.into(), f.as_ref());
                        // The frame lives as long as the page.
                        f.forget();
                    };
                    on("onMode", Closure::new(move |m: JsValue| on_mode.call(m.as_string().as_deref() == Some("ai"))));
                    on("onZoom", Closure::new(move |p: JsValue| {
                        if let Some(p) = p.as_f64() {
                            on_zoom.call(p);
                        }
                    }));
                    on("onUndo", Closure::new(move |_| on_undo.call(())));
                    on("onSave", Closure::new(move |_| on_save.call(())));
                    on("onPages", Closure::new(move |_| on_pages.call(())));
                    match crate::island::mount_with(&element, "mountFrame", &options).await {
                        Ok(controller) => {
                            *update.borrow_mut() = Reflect::get(&controller, &"update".into())
                                .ok()
                                .and_then(|f| f.dyn_into::<Function>().ok());
                            ready.set(true);
                        }
                        Err(error) => crate::log(&format!("pdit: could not load the frame: {error:?}")),
                    }
                }
            },
        }
    }
}
