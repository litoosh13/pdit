//! From images (D-071; approved mockup .claude/research/convert-ui/): the rail's From images (after New) opens the
//! system's file window for pictures, then a window in the Print look listing them in order — each card with
//! Selection Actions buttons to move it left or right or remove it — and the page size (Picture's own size | A4 |
//! Letter). Make PDF opens them as a new PDF (pdit-core `images_pdf`: JPEG photos as they are). Look:
//! assets/css/images.css.

use crate::page_tools::{next_frame, set_timeout};
use dioxus::prelude::*;
use pdit_core::images_pdf::{PageSize, pdf_from_images};
use std::rc::Rc;

const SIGNATURE_CSS: Asset = asset!("/assets/css/signature.css");
const DOC_CSS: Asset = asset!("/assets/css/doc.css");
const IMAGE_SELECT_CSS: Asset = asset!("/assets/css/image-select.css");
const IMAGES_CSS: Asset = asset!("/assets/css/images.css");
/// Devigner GalleryAdd2, picked by the user.
pub const ICON_FROM_IMAGES: &str = include_str!("../assets/icons/devigner/GalleryAdd2.svg");
const ICON_LEFT: &str = include_str!("../assets/icons/devigner/ArrowLeft.svg");
const ICON_REMOVE: &str = include_str!("../assets/icons/devigner/TrashBinMinimalistic.svg");
const INPUT_ID: &str = "pdit-from-images";
/// Transitions.dev "Modal open/close" close time (ms).
const CLOSE_MS: i32 = 200;
const SIZES: [(PageSize, &str); 3] = [
    (PageSize::Picture, "Picture’s own size"),
    (PageSize::A4, "A4"),
    (PageSize::Letter, "Letter"),
];

/// A picked picture: its name, bytes, and an object URL for its thumbnail.
#[derive(Clone, PartialEq)]
struct Picture {
    name: String,
    bytes: Rc<Vec<u8>>,
    url: String,
}

#[derive(Clone, PartialEq)]
struct Dialog {
    pictures: Vec<Picture>,
    size: usize,
    status: Option<String>,
    shown: bool,
    closing: bool,
}

/// Shared From images state.
#[derive(Clone, Copy)]
pub struct FromImages {
    dialog: Signal<Option<Dialog>>,
}

impl FromImages {
    pub fn provide() -> Self {
        use_context_provider(|| FromImages {
            dialog: Signal::new(None),
        })
    }

    /// The system's file window for pictures.
    pub fn pick(self) {
        if let Some(input) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(INPUT_ID))
            .and_then(|e| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(e).ok())
        {
            input.click();
        }
    }

    fn open(mut self, pictures: Vec<Picture>) {
        if pictures.is_empty() {
            return;
        }
        self.dialog.set(Some(Dialog {
            pictures,
            size: 0,
            status: None,
            shown: false,
            closing: false,
        }));
        let mut dialog = self.dialog;
        next_frame(move || {
            next_frame(move || {
                dialog.with_mut(|d| {
                    if let Some(d) = d.as_mut() {
                        d.shown = true;
                    }
                })
            })
        });
        crate::doc_ui::place_pills_soon(".pdit-from-images .t-tabs");
    }

    fn edit(mut self, f: impl FnOnce(&mut Dialog)) {
        self.dialog.with_mut(|d| {
            if let Some(d) = d.as_mut() {
                f(d);
            }
        });
    }

    fn close(self) {
        let Some(d) = self.dialog.peek().clone() else {
            return;
        };
        if d.closing {
            return;
        }
        for p in &d.pictures {
            let _ = web_sys::Url::revoke_object_url(&p.url);
        }
        self.edit(|d| d.closing = true);
        let mut dialog = self.dialog;
        set_timeout(CLOSE_MS, move || {
            if dialog.peek().as_ref().is_some_and(|d| d.closing) {
                dialog.set(None);
            }
        });
    }

    /// Make PDF: the pictures as a new document on screen.
    fn make(self) {
        let Some(d) = self.dialog.peek().clone() else {
            return;
        };
        let images: Vec<Vec<u8>> = d
            .pictures
            .iter()
            .map(|p| p.bytes.as_ref().clone())
            .collect();
        match pdf_from_images(&images, SIZES[d.size].0) {
            Ok(pdf) => {
                let stem = d.pictures[0]
                    .name
                    .rsplit_once('.')
                    .map_or(d.pictures[0].name.as_str(), |(s, _)| s);
                crate::open_bytes(pdf, format!("{stem}.pdf"));
                self.close();
            }
            Err(error) => {
                crate::log(&format!("pdit: from images: {error}"));
                self.edit(|d| {
                    d.status = Some("These pictures couldn't be made into a PDF.".into())
                });
            }
        }
    }
}

#[component]
pub fn FromImagesUi() -> Element {
    let images = use_context::<FromImages>();
    let pill_key = images.dialog.read().as_ref().map(|d| (d.shown, d.size));
    use_effect(use_reactive!(|pill_key| {
        if pill_key.is_some() {
            crate::doc_ui::place_pills_soon(".pdit-from-images .t-tabs");
        }
    }));
    let picker = rsx! {
        input {
            id: INPUT_ID,
            r#type: "file",
            accept: "image/png,image/jpeg,.png,.jpg,.jpeg",
            multiple: true,
            hidden: true,
            onchange: move |event| async move {
                let mut pictures = Vec::new();
                for file in event.files() {
                    match file.read_bytes().await {
                        Ok(bytes) => {
                            let bytes = bytes.to_vec();
                            let url = object_url(&bytes).unwrap_or_default();
                            pictures.push(Picture { name: file.name(), bytes: Rc::new(bytes), url });
                        }
                        Err(error) => crate::log(&format!("pdit: could not read a picture: {error}")),
                    }
                }
                images.open(pictures);
                // The same pictures can be picked again.
                if let Some(input) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id(INPUT_ID))
                    .and_then(|e| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlInputElement>(e).ok())
                {
                    input.set_value("");
                }
            },
        }
    };
    let Some(d) = images.dialog.read().clone() else {
        return picker;
    };
    let class = match (d.shown, d.closing) {
        (_, true) => "doc-box t-modal is-closing",
        (true, false) => "doc-box t-modal is-open",
        (false, false) => "doc-box t-modal",
    };
    let last = d.pictures.len().saturating_sub(1);
    rsx! {
        {picker}
        document::Stylesheet { href: SIGNATURE_CSS }
        document::Stylesheet { href: DOC_CSS }
        document::Stylesheet { href: IMAGE_SELECT_CSS }
        document::Stylesheet { href: IMAGES_CSS }
        div { class: "doc-layer sa-root pdit-from-images",
            div { class, role: "dialog", "aria-label": "From images",
                div { class: "doc-head", span { class: "title", "From images" } }
                div { class: "fi-note", "Each picture becomes one page, in this order." }
                div { class: "fi-pics",
                    for (i, p) in d.pictures.iter().cloned().enumerate() {
                        div { key: "{i}-{p.name}", class: "fi-card",
                            img { class: "fi-thumb", src: "{p.url}", alt: "{p.name}" }
                            span { class: "fi-name", title: "{p.name}", "{p.name}" }
                            div { class: "fi-moves",
                                button {
                                    class: "sa-control", r#type: "button", "aria-label": "Move left", disabled: i == 0,
                                    onclick: move |_| images.edit(|d| d.pictures.swap(i, i - 1)),
                                    span { dangerous_inner_html: ICON_LEFT, style: "display: contents" }
                                }
                                button {
                                    class: "sa-control", r#type: "button", "aria-label": "Remove",
                                    onclick: move |_| images.edit(|d| {
                                        let p = d.pictures.remove(i);
                                        let _ = web_sys::Url::revoke_object_url(&p.url);
                                    }),
                                    span { dangerous_inner_html: ICON_REMOVE, style: "display: contents" }
                                }
                                button {
                                    class: "sa-control sa-mirror", r#type: "button", "aria-label": "Move right", disabled: i == last,
                                    onclick: move |_| images.edit(|d| d.pictures.swap(i, i + 1)),
                                    span { dangerous_inner_html: ICON_LEFT, style: "display: contents" }
                                }
                            }
                        }
                    }
                }
                div {
                    div { class: "doc-lbl", "Page size" }
                    crate::doc_ui::Tabs {
                        options: SIZES.iter().enumerate().map(|(i, (_, t))| ((*t).into(), i == d.size)).collect(),
                        on_pick: move |i| images.edit(|d| d.size = i),
                    }
                }
                div { class: "doc-foot",
                    span { class: "fi-note",
                        {d.status.clone().unwrap_or_else(|| "Opens as a new PDF in pdit (save it with Save).".into())}
                    }
                    span { class: "grow" }
                    button { class: "sa-control", r#type: "button", onclick: move |_| images.close(), "Cancel" }
                    button {
                        class: "sa-primary", r#type: "button", disabled: d.pictures.is_empty(),
                        onclick: move |_| images.make(),
                        "Make PDF"
                    }
                }
            }
        }
    }
}

/// An object URL showing `bytes` (for the thumbnails).
fn object_url(bytes: &[u8]) -> Option<String> {
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).ok()?;
    web_sys::Url::create_object_url_with_blob(&blob).ok()
}
