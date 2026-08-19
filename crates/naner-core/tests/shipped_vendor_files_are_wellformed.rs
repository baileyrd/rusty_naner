//! Regression guard for `dist-assets/config/vendors/`, the per-vendor
//! definition files that ship in every bundle and get compiled into the
//! binary by `build.rs`.
//!
//! `build.rs` already enforces the authoring contract -- one vendor per file,
//! file name matching the declared key -- but it enforces it by panicking the
//! build, which is a blunt way to learn you typed a file name wrong. These
//! tests assert the same invariants where a failure reads as a test failure,
//! and add the ones a build script has no business checking: that the shipped
//! set is intact and that `dependencies` resolve. The `%TARGETDIR%` rule the
//! HiFile/Obsidian/Zed/Zen wave was fixed for is checked in `loader.rs`, which
//! reads the same generated catalog.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn vendors_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/naner-core -> crates -> repo root")
        .join("dist-assets")
        .join("config")
        .join("vendors")
}

/// Every shipped file, as (file stem, parsed document).
fn shipped() -> Vec<(String, serde_json::Value)> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(vendors_dir())
        .expect("dist-assets/config/vendors exists")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();

    files
        .into_iter()
        .map(|path| {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()));
            let parsed = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()));
            (stem, parsed)
        })
        .collect()
}

#[test]
fn every_file_declares_exactly_one_vendor_named_after_it() {
    for (stem, document) in shipped() {
        let object = document
            .as_object()
            .unwrap_or_else(|| panic!("{stem}.json must hold a JSON object"));
        assert_eq!(
            object.len(),
            1,
            "{stem}.json must declare exactly one vendor, found {}",
            object.len()
        );
        let key = object.keys().next().unwrap();
        assert_eq!(
            key, &stem,
            "{stem}.json declares {key:?}; the file name must match the key, or \
             `naner.lock` pins and `dependencies` references point at nothing"
        );
    }
}

/// Splitting a 499-line file into 22 is exactly the kind of change that
/// silently drops one. The essentials are named explicitly because losing one
/// of those breaks bootstrap rather than merely hiding an optional tool.
#[test]
fn the_shipped_set_is_intact() {
    let keys: Vec<String> = shipped().into_iter().map(|(stem, _)| stem).collect();

    assert_eq!(keys.len(), 23, "expected 23 vendor files, found {keys:?}");
    for essential in ["SevenZip", "PowerShell", "WindowsTerminal", "GitForWindows"] {
        assert!(
            keys.iter().any(|k| k == essential),
            "{essential} is missing from the shipped vendor set"
        );
    }
}

/// Every `dependencies` entry has to name a vendor that actually ships, and
/// no vendor may depend on itself. In one big file a bad reference was at
/// least visible next to its target; across 22 files nothing puts them side
/// by side, so the check has to be automated.
#[test]
fn dependencies_resolve_to_vendors_that_exist() {
    let all = shipped();
    let keys: Vec<&String> = all.iter().map(|(stem, _)| stem).collect();

    for (stem, document) in &all {
        let definition = &document[stem];
        let Some(dependencies) = definition.get("dependencies").and_then(|d| d.as_array()) else {
            continue;
        };
        for dependency in dependencies {
            let name = dependency
                .as_str()
                .unwrap_or_else(|| panic!("{stem}.json has a non-string dependency: {dependency}"));
            assert_ne!(name, stem, "{stem}.json depends on itself");
            assert!(
                keys.iter().any(|k| *k == name),
                "{stem}.json depends on {name:?}, which no vendor file declares"
            );
        }
    }
}

/// `build.rs` merges the files into one catalog keyed by vendor key. Two files
/// declaring the same key would silently collapse into one entry.
#[test]
fn no_vendor_key_is_declared_twice() {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for (stem, document) in shipped() {
        let key = document.as_object().unwrap().keys().next().unwrap().clone();
        if let Some(previous) = seen.insert(key.clone(), stem.clone()) {
            panic!("{key:?} is declared by both {previous}.json and {stem}.json");
        }
    }
}

/// `vendors-schema.json` shipped with a `$ref` pointing at a `definitions`
/// block that a previous edit had removed, so the schema resolved to nothing
/// and validated everything. Nothing in the workspace reads it -- it exists
/// for editors -- so only a reader would ever have noticed.
#[test]
fn the_shipped_schema_has_no_dangling_refs() {
    let schema_path = vendors_dir()
        .parent()
        .expect("config/vendors -> config")
        .join("vendors-schema.json");
    let text = std::fs::read_to_string(&schema_path).expect("vendors-schema.json exists");
    let schema: serde_json::Value = serde_json::from_str(&text).expect("schema is valid JSON");

    let definitions = schema
        .get("definitions")
        .and_then(|d| d.as_object())
        .expect("schema has a definitions block");

    let mut refs = Vec::new();
    collect_refs(&schema, &mut refs);
    assert!(!refs.is_empty(), "expected the schema to use $ref at all");
    for name in refs {
        assert!(
            definitions.contains_key(&name),
            "$ref points at #/definitions/{name}, which the schema does not define"
        );
    }
}

fn collect_refs(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == "$ref"
                    && let Some(name) = child
                        .as_str()
                        .and_then(|r| r.strip_prefix("#/definitions/"))
                {
                    out.push(name.to_string());
                }
                collect_refs(child, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|i| collect_refs(i, out)),
        _ => {}
    }
}

/// The three fields that moved out of `naner.json` have to be describable, or
/// an editor flags every vendor file that uses them.
#[test]
fn the_schema_describes_the_fields_the_vendor_files_actually_use() {
    let schema_path = vendors_dir().parent().unwrap().join("vendors-schema.json");
    let schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&schema_path).unwrap()).unwrap();
    let described = &schema["definitions"]["vendor"]["properties"];

    let mut used = BTreeMap::new();
    for (stem, document) in shipped() {
        for field in document[&stem].as_object().unwrap().keys() {
            used.insert(field.clone(), stem.clone());
        }
    }

    for (field, vendor) in used {
        assert!(
            !described[&field].is_null(),
            "{vendor}.json uses {field:?}, which vendors-schema.json does not describe"
        );
    }
}
