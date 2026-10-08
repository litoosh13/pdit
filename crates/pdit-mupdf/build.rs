//! Compiles `src/story.c` (MuPDF's Story inside fz_try) against the headers of the MuPDF that mupdf-sys builds.
//! mupdf-sys doesn't export its include path, so its sources are found in Cargo's registry (or, failing that,
//! with `cargo metadata`).
use std::path::{Path, PathBuf};
use std::process::Command;

/// The mupdf-sys version pdit-mupdf depends on (Cargo.toml).
const MUPDF_SYS: &str = "mupdf-sys-0.8.";

fn main() {
    println!("cargo:rerun-if-changed=src/story.c");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        return;
    }
    let include = from_registry()
        .or_else(from_metadata)
        .expect("mupdf-sys sources (its mupdf/include)");
    cc::Build::new()
        .file("src/story.c")
        .include(include)
        .warnings(false)
        .compile("pdit_story");
}

/// `$CARGO_HOME/registry/src/<index>/mupdf-sys-0.8.x/mupdf/include`.
fn from_registry() -> Option<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|h| Path::new(&h).join(".cargo"))
        })?;
    for index in std::fs::read_dir(home.join("registry/src")).ok()?.flatten() {
        for package in std::fs::read_dir(index.path()).ok()?.flatten() {
            let include = package.path().join("mupdf/include");
            let name = package.file_name();
            if name.to_string_lossy().starts_with(MUPDF_SYS) && include.is_dir() {
                return Some(include);
            }
        }
    }
    None
}

fn from_metadata() -> Option<PathBuf> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--offline"])
        .current_dir(std::env::var("CARGO_MANIFEST_DIR").ok()?)
        .output()
        .ok()?;
    let json = String::from_utf8(out.stdout).ok()?;
    // The manifest path of the mupdf-sys package, e.g. ".../mupdf-sys-0.8.0/Cargo.toml".
    let manifest = json
        .split("\"manifest_path\":\"")
        .skip(1)
        .map(|s| &s[..s.find('"').unwrap_or(0)])
        .find(|p| p.contains(MUPDF_SYS))?;
    Some(PathBuf::from(manifest).parent()?.join("mupdf/include"))
}
