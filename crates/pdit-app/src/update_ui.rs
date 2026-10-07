//! Updates (D-058, desktop app): at start and every 6 hours the app asks the
//! desktop side (desktop/src/update.rs) whether GitHub has a newer release.
//! If so, the toast pill under the top bar offers it (Later / Update →
//! "Downloading… n %" → Later / Restart); after Later, a row at the bottom of
//! the tools rail brings the pill back. Nothing downloads before "Update".
//! The About panel (the rail's last section) shows the version and checks on
//! demand, with the same Update → Restart steps.
//! Look: toast.css (+ assets/css/update.css).

use crate::ai_ui::desktop;
use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;
use js_sys::{Object, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const TOAST_CSS: Asset = asset!("/assets/css/toast.css");
pub(crate) const UPDATE_CSS: Asset = asset!("/assets/css/update.css");
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
    /// A check is running.
    checking: Signal<bool>,
    /// When the last check answered (local time, "HH:MM").
    checked: Signal<Option<String>>,
    /// The last check could not reach GitHub.
    check_failed: Signal<bool>,
}

impl Updates {
    pub fn provide() -> Self {
        use_context_provider(|| Updates {
            stage: Signal::new(Stage::None),
            shown: Signal::new(false),
            open: Signal::new(false),
            checking: Signal::new(false),
            checked: Signal::new(None),
            check_failed: Signal::new(false),
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

    /// Asks GitHub for a newer version. The timer's checks bring up the pill;
    /// About's (`manual`) show the answer in the panel.
    async fn check(mut self, manual: bool) {
        if *self.checking.peek()
            || matches!(*self.stage.peek(), Stage::Downloading(_) | Stage::Ready)
        {
            return;
        }
        self.checking.set(true);
        let answer = desktop::invoke("update_check", &Object::new()).await;
        self.checking.set(false);
        let found = match answer {
            Ok(found) => found,
            Err(error) => {
                self.check_failed.set(true);
                return crate::log(&format!("pdit: update check: {error:?}"));
            }
        };
        self.check_failed.set(false);
        let now = js_sys::Date::new_0();
        self.checked.set(Some(format!(
            "{:02}:{:02}",
            now.get_hours(),
            now.get_minutes()
        )));
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
            if !manual {
                self.show();
            }
        }
    }

    /// About's "Check for updates".
    pub fn check_now(self) {
        spawn(async move { self.check(true).await });
    }

    pub(crate) fn update(mut self) {
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

    pub(crate) fn restart(self) {
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
            spawn(async move { ui.check(false).await });
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
                                spawn(async move { ui.check(false).await });
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

pub const PROJECT: &str = "https://github.com/litoosh13/pdit";
// A copy of the app icon (desktop/icons/app-icon.svg, D-048).
const APP_ICON: Asset = asset!("/assets/app-icon.svg");

/// Opens one of pdit's pages on GitHub: through the desktop app (the system
/// browser), or a new tab online.
fn open_page(page: &'static str) {
    if desktop::available() {
        spawn(async move {
            let args = Object::new();
            let _ = Reflect::set(&args, &"page".into(), &page.into());
            if let Err(error) = desktop::invoke("open_project_page", &args).await {
                crate::log(&format!("pdit: could not open the page: {error:?}"));
            }
        });
        return;
    }
    let url = match page {
        "licence" => format!("{PROJECT}/blob/main/LICENSE"),
        "notices" => format!("{PROJECT}/blob/main/THIRD_PARTY_NOTICES.md"),
        _ => PROJECT.to_owned(),
    };
    if let Some(window) = web_sys::window() {
        let _ = window.open_with_url_and_target_and_features(&url, "_blank", "noopener,noreferrer");
    }
}

/// The About panel's content (in the tools rail's flyout): the app, its
/// version, updating by hand (desktop app), and pdit's pages on GitHub.
/// Approved mockup: .claude/research/about-ui/.
#[component]
pub fn AboutPanel() -> Element {
    let ui = use_context::<Updates>();
    let on_desktop = desktop::available();
    let mut version = use_signal(|| env!("CARGO_PKG_VERSION").to_owned());
    use_hook(move || {
        if on_desktop {
            spawn(async move {
                if let Some(v) = desktop::invoke("app_version", &Object::new())
                    .await
                    .ok()
                    .and_then(|v| v.as_string())
                {
                    version.set(v);
                }
            });
        }
    });
    let stage = (ui.stage)();
    let checked = (ui.checked)();
    let update_row = if !on_desktop {
        rsx! {
            span { class: "msg",
                "The online version is always the newest."
                small { "Reload the page to get it." }
            }
        }
    } else if (ui.checking)() {
        rsx! { span { class: "msg", "Checking…" } }
    } else {
        match stage {
            Stage::Available { version, .. } => rsx! {
                span { class: "msg", "pdit {version} is available" }
                button { class: "sa-primary", r#type: "button", onclick: move |_| ui.update(), "Update" }
            },
            Stage::Downloading(pct) => rsx! {
                span { class: "msg", "Downloading… " span { class: "pct", "{pct}" } " %" }
            },
            Stage::Ready => rsx! {
                span { class: "msg", "Installed. Restart pdit to use it." }
                button { class: "sa-primary", r#type: "button", onclick: move |_| ui.restart(), "Restart" }
            },
            Stage::Failed(error) => rsx! {
                span { class: "msg", "The update stopped" small { "{error}" } }
                button {
                    class: "sa-control",
                    r#type: "button",
                    onclick: move |_| {
                        let mut ui = ui;
                        ui.stage.set(Stage::None);
                        ui.check_now();
                    },
                    "Try again"
                }
            },
            Stage::None if (ui.check_failed)() => rsx! {
                span { class: "msg", "Couldn't reach GitHub" small { "Check the internet connection" } }
                button { class: "sa-control", r#type: "button", onclick: move |_| ui.check_now(), "Try again" }
            },
            Stage::None => match checked {
                Some(at) => rsx! {
                    span { class: "msg",
                        span { class: "ok", "✓ " }
                        "You have the newest version"
                        small { "Checked at {at}" }
                    }
                    button { class: "sa-control", r#type: "button", onclick: move |_| ui.check_now(), "Check again" }
                },
                None => rsx! {
                    span { class: "msg", "Updates" }
                    button { class: "sa-primary", r#type: "button", onclick: move |_| ui.check_now(), "Check for updates" }
                },
            },
        }
    };
    let link = |page: &'static str, label: &'static str, ext: &'static str| {
        rsx! {
            button {
                class: "sb-item",
                r#type: "button",
                onclick: move |_| open_page(page),
                span { "{label}" }
                span { class: "ext", "{ext}" }
            }
        }
    };
    rsx! {
        document::Stylesheet { href: UPDATE_CSS }
        div { class: "about-id",
            img { src: APP_ICON, alt: "" }
            div {
                b { "pdit" }
                span { if on_desktop { "Version {version}" } else { "Version {version} · online" } }
            }
        }
        div { class: "about-note", "A private PDF editor. Your PDFs stay on this computer." }
        div { class: "about-update sa-root", {update_row} }
        div { class: "sb-nav about-links",
            {link("source", "Source code", "GitHub ↗")}
            {link("licence", "Licence", "AGPL-3.0")}
            {link("notices", "Third-party notices", "")}
        }
    }
}
