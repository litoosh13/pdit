//! Remembering a PDF's analysis (D-060, option C): a record per file, keyed by
//! the SHA-256 of the file's bytes, kept on this device only — the desktop
//! app's data folder (desktop/src/cache.rs) or, online, this browser's
//! storage. It holds what the analysis found: page kinds, language, field
//! suggestions, and the OCR words (so unsaved OCR comes back without
//! Tesseract). What belongs to the document itself (the OCR text layer, added
//! fields) goes into the PDF when it's saved.

use crate::ai_ui::desktop;
use js_sys::{Object, Reflect};
use pdit_core::fields::{FoundField, FoundKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const VERSION: u32 = 1;
const STORAGE_PREFIX: &str = "pdit-analysis:";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub page: u16,
    pub kind: String,
    pub rect: [f32; 4],
    pub name: String,
    pub filled: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OcrPage {
    pub page: u16,
    pub words: Vec<(String, [f32; 4])>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Record {
    pub v: u32,
    /// Per page: "text", "scan" or "blank".
    pub kinds: Vec<String>,
    pub language: Option<String>,
    pub suggestions: Vec<Suggestion>,
    pub ocr: Vec<OcrPage>,
}

impl Record {
    pub fn new() -> Self {
        Record {
            v: VERSION,
            ..Default::default()
        }
    }

    pub fn suggestions_of(found: &[(u16, FoundField)]) -> Vec<Suggestion> {
        found
            .iter()
            .map(|(page, f)| Suggestion {
                page: *page,
                kind: match f.kind {
                    FoundKind::Text => "text",
                    FoundKind::Checkbox => "checkbox",
                    FoundKind::Signature => "signature",
                }
                .into(),
                rect: f.rect,
                name: f.name.clone(),
                filled: f.filled,
            })
            .collect()
    }

    pub fn found(&self) -> Vec<(u16, FoundField)> {
        self.suggestions
            .iter()
            .map(|s| {
                let kind = match s.kind.as_str() {
                    "checkbox" => FoundKind::Checkbox,
                    "signature" => FoundKind::Signature,
                    _ => FoundKind::Text,
                };
                (
                    s.page,
                    FoundField {
                        kind,
                        rect: s.rect,
                        filled: s.filled,
                        name: s.name.clone(),
                    },
                )
            })
            .collect()
    }
}

/// The key of a file: SHA-256 of its bytes, as hex.
pub fn file_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The record for `hash`, if this device has one of the right version.
pub async fn load(hash: &str) -> Option<Record> {
    let json = if desktop::available() {
        let args = Object::new();
        let _ = Reflect::set(&args, &"hash".into(), &hash.into());
        desktop::invoke("analysis_get", &args)
            .await
            .ok()?
            .as_string()?
    } else {
        let storage = web_sys::window()?.local_storage().ok()??;
        storage
            .get_item(&format!("{STORAGE_PREFIX}{hash}"))
            .ok()??
    };
    serde_json::from_str::<Record>(&json)
        .ok()
        .filter(|r| r.v == VERSION)
}

/// Keeps `record` under `hash` (best effort: a full disk or storage only
/// means the next opening analyses again).
pub async fn store(hash: &str, record: &Record) {
    let Ok(json) = serde_json::to_string(record) else {
        return;
    };
    if desktop::available() {
        let args = Object::new();
        let _ = Reflect::set(&args, &"hash".into(), &hash.into());
        let _ = Reflect::set(&args, &"json".into(), &json.as_str().into());
        if let Err(error) = desktop::invoke("analysis_put", &args).await {
            crate::log(&format!("pdit: could not keep the analysis: {error:?}"));
        }
        return;
    }
    let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else {
        return;
    };
    let key = format!("{STORAGE_PREFIX}{hash}");
    if storage.set_item(&key, &json).is_err() {
        // ponytail: browser storage full → forget all kept analyses and try
        // once more; an oldest-first order needs IndexedDB.
        let len = storage.length().unwrap_or(0);
        let keys: Vec<String> = (0..len)
            .filter_map(|i| storage.key(i).ok().flatten())
            .filter(|k| k.starts_with(STORAGE_PREFIX))
            .collect();
        for k in keys {
            let _ = storage.remove_item(&k);
        }
        let _ = storage.set_item(&key, &json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_round_trip() {
        assert_eq!(
            file_hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut r = Record::new();
        r.kinds = vec!["text".into(), "scan".into()];
        r.ocr = vec![OcrPage {
            page: 1,
            words: vec![("Rent".into(), [1.0, 2.0, 3.0, 4.0])],
        }];
        let back: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
