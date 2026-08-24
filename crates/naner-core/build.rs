//! Assembles the per-vendor files in `dist-assets/config/vendors/` into the
//! single catalog `config_merge.rs` embeds with `include_str!`.
//!
//! The catalog has to be compiled in: a bare `naner.exe` swap has no bundle
//! to read a vendors file from, and merging new vendor definitions into an
//! existing tree is exactly what that path is for. `include_str!` takes one
//! file, so the 33 authored files are concatenated here into one generated
//! file in `OUT_DIR` rather than being embedded individually.
//!
//! Authoring contract, enforced below: one file per vendor, holding exactly
//! one top-level key, and that key matches the file stem. A file whose name
//! and key disagree fails the build rather than shipping a vendor under a
//! name nothing looks it up by.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendors_dir = manifest
        .join("..")
        .join("..")
        .join("dist-assets")
        .join("config")
        .join("vendors");

    println!("cargo:rerun-if-changed={}", vendors_dir.display());
    // The directory watch above only catches files being added or removed
    // -- Cargo doesn't guarantee a directory's mtime changes when a file
    // already inside it is merely edited, which is the overwhelmingly more
    // common case once a vendor exists. Watch every vendor file
    // individually too, or an in-place edit here can silently build against
    // a stale `vendors_catalog.json` (confirmed live: it did).
    if let Ok(entries) = std::fs::read_dir(&vendors_dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    let catalog = build_catalog(&vendors_dir);
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("vendors_catalog.json");
    std::fs::write(&out, catalog).expect("write generated vendor catalog");
}

fn build_catalog(vendors_dir: &Path) -> String {
    let entries = std::fs::read_dir(vendors_dir).unwrap_or_else(|e| {
        panic!(
            "vendor definitions directory {} is unreadable: {e}",
            vendors_dir.display()
        )
    });

    // Sorted by file name so the generated catalog is byte-stable across
    // platforms -- `read_dir` order is whatever the filesystem gives back.
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();

    assert!(
        !files.is_empty(),
        "no vendor definitions found in {}",
        vendors_dir.display()
    );

    let mut vendors = serde_json::Map::new();
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());

        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("vendor file name is valid UTF-8");
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("{} is unreadable: {e}", file.display()));
        let parsed: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", file.display()));

        let object = parsed
            .as_object()
            .unwrap_or_else(|| panic!("{} must hold a JSON object", file.display()));
        assert_eq!(
            object.len(),
            1,
            "{} must hold exactly one vendor, found {}",
            file.display(),
            object.len()
        );

        let (key, definition) = object.iter().next().expect("length checked above");
        assert_eq!(
            key,
            stem,
            "{} declares vendor {key:?}; the file name must match the key it declares",
            file.display()
        );
        assert!(
            vendors.insert(key.clone(), definition.clone()).is_none(),
            "vendor {key:?} is declared more than once"
        );
    }

    // A plain object, not the wire model: the loader and `config_merge` both
    // read `{"vendors": {...}}` and ignore anything alongside it.
    let mut root = serde_json::Map::new();
    root.insert("vendors".into(), serde_json::Value::Object(vendors));
    serde_json::to_string_pretty(&serde_json::Value::Object(root))
        .expect("a catalog assembled from parsed JSON always re-serializes")
}
