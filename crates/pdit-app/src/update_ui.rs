//! Updates (D-058, desktop app): at start and every 6 hours the app asks the
//! desktop side (desktop/src/update.rs) whether GitHub has a newer release.
//! If so, the toast pill under the top bar offers it (Later / Update →
//! "Downloading… n %" → Later / Restart); after Later, a row at the bottom of
//! the tools rail brings the pill back. Nothing downloads before "Update".
//! Look: toast.css (+ assets/css/update.css).

use crate::ai_ui::desktop;
use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;
use js_sys::{Object, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const TOAST_CSS: Asset = asset!("/assets/css/toast.css");
const UPDATE_CSS: Asset = asset!("/assets/css/update.css");
/// How often to look again while the app stays open.
const CHECK_EVERY_MS: i32 = 6 * 60 * 60 * 1000;
/// The pill's close time (Transitions.dev "Toast open / close").
const CLOSE_MS: i32 = 260;

#[derive(Clone, PartialEq)]
enum Stage {
    None,
    Available { version: String, current: String },
    Downloading(u32),
    Ready,
    Failed(String),
}

/// Shared update state.
#[derive(Clone, Copy)]
pub struct Updates {
    stage: Signal<Stage>,
    /// The pill is on screen (otherwise the rail row stands in for it).
    shown: Signal<bool>,
    open: Signal<bool>,
}

impl Updates {
    pub fn provide() -> Self {
        use_context_provider(|| Updates {
            stage: Signal::new(Stage::None),
            shown: Signal::new(false),
            open: Signal::new(false),
        })
    }

    /// The rail's row: an update is waiting and the pill was put away.
    pub fn waiting(&self) -> Option<&'static str> {
        if (self.shown)() {
            return None;
        }
        match *self.stage.read() {
            Stage::Available { .. } | Stage::Failed(_) => Some("Update"),
            Stage::Ready => Some("Restart"),
            _ => None,
        }
    }

    pub fn show(mut self) {
        self.shown.set(true);
        self.open.set(false);
        let mut open = self.open;
        next_frame(move || next_frame(move || open.set(true)));
    }

    fn later(mut self) {
        self.open.set(false);
        let mut shown = self.shown;
        set_timeout(CLOSE_MS, move || shown.set(false));
    }

    async fn check(mut self) {
        if matches!(*self.stage.peek(), Stage::Downloading(_) | Stage::Ready) {
            return;
        }
        let found = match desktop::invoke("update_check", &Object::new()).await {
            Ok(found) => found,
            Err(error) => return crate::log(&format!("pdit: update check: {error:?}")),
        };
        if found.is_null() || found.is_undefined() {
            return;
        }
        let get = |k: &str| {
            Reflect::get(&found, &k.into())
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default()
        };
        let next = Stage::Available {
            version: get("version"),
            current: get("current"),
        };
        crate::log(&format!("pdit: update available: {}", get("version")));
        if *self.stage.peek() != next {
            self.stage.set(next);
            self.show();
        }
    }

    fn update(mut self) {
        self.stage.set(Stage::Downloading(0));
        // The download runs in the desktop app; its progress is read meanwhile.
        let mut stage = self.stage;
        spawn(async move {
            while matches!(*stage.peek(), Stage::Downloading(_)) {
                if let Ok(p) = desktop::invoke("update_progress", &Object::new()).await {
                    let n = |k: &str| {
                        Reflect::get(&p, &k.into())
                            .ok()
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0)
                    };
                    let pct = if n("total") > 0.0 {
                        (n("done") / n("total") * 100.0).min(100.0)
                    } else {
                        0.0
                    };
                    if matches!(*stage.peek(), Stage::Downloading(_)) {
                        stage.set(Stage::Downloading(pct as u32));
                    }
                }
                crate::print_ui::pause(300).await;
            }
        });
        spawn(async move {
            match desktop::invoke("update_install", &Object::new()).await {
                Ok(_) => {
                    crate::log("pdit: update installed");
                    self.stage.set(Stage::Ready)
                }
                Err(error) => {
                    crate::log(&format!("pdit: update: {error:?}"));
                    self.stage.set(Stage::Failed(
                        error
                            .as_string()
                            .unwrap_or_else(|| "the download stopped".into()),
                    ));
                }
            }
        });
    }

    fn restart(self) {
        spawn(async move {
            let _ = desktop::invoke("update_restart", &Object::new()).await;
        });
    }
}

/// The pill, and the timer that looks for updates (desktop app only).
#[component]
pub fn UpdateUi() -> Element {
    let ui = use_context::<Updates>();
    // Timers run outside Dioxus, where spawn doesn't work: they only count,
    // and this effect does the checking inside Dioxus.
    let mut ticks = use_signal(|| 0u32);
    use_effect(move || {
        if ticks() > 0 {
            spawn(async move { ui.check().await });
        }
    });
    use_hook(move || {
        if !desktop::available() {
            return;
        }
        // A little after start, then every 6 hours.
        set_timeout(3000, move || ticks += 1);
        let every = Closure::<dyn FnMut()>::new(move || ticks += 1);
        if let Some(window) = web_sys::window() {
            let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
                every.as_ref().unchecked_ref(),
                CHECK_EVERY_MS,
            );
        }
        // The app lives as long as the page.
        every.forget();
    });
    let stage = (ui.stage)();
    if !(ui.shown)() || stage == Stage::None {
        return rsx! {};
    }
    rsx! {
        document::Stylesheet { href: TOAST_CSS }
        document::Stylesheet { href: UPDATE_CSS }
        div { class: "pdit-toast-host pdit-update-host",
            div {
                class: if (ui.open)() { "pdit-toast t-toast sa-root pdit-update is-open" } else { "pdit-toast t-toast sa-root pdit-update" },
                role: "status",
                "aria-live": "polite",
                match stage {
                    Stage::Available { version, current } => rsx! {
                        span {
                            "pdit {version} is available "
                            span { class: "pdit-update-dim", "(you have {current})" }
                        }
                        button { class: "sa-control", r#type: "button", onclick: move |_| ui.later(), "Later" }
                        button { class: "sa-primary", r#type: "button", onclick: move |_| ui.update(), "Update" }
                    },
                    Stage::Downloading(pct) => rsx! {
                        span { "Downloading the update… " span { class: "pdit-update-pct", "{pct}" } " %" }
                    },
                    Stage::Ready => rsx! {
                        span { "Update installed — restart pdit to use it" }
                        button { class: "sa-control", r#type: "button", onclick: move |_| ui.later(), "Later" }
                        button { class: "sa-primary", r#type: "button", onclick: move |_| ui.restart(), "Restart" }
                    },
                    Stage::Failed(error) => rsx! {
                        span { "The update stopped: {error}" }
                        button { class: "sa-control", r#type: "button", onclick: move |_| ui.later(), "Later" }
                        button {
                            class: "sa-primary",
                            r#type: "button",
                            onclick: move |_| {
                                let mut ui = ui;
                                ui.stage.set(Stage::None);
                                spawn(async move { ui.check().await });
                            },
                            "Try again"
                        }
                    },
                    Stage::None => rsx! {},
                }
            }
        }
    }
}
