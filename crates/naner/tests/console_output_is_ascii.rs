//! Guard against typographic characters reaching the console.
//!
//! Rust emits UTF-8. A Windows console on the default code page (cp1252, which
//! is what a user gets unless they have changed it) decodes it as cp1252, so
//! `—` arrives as `ΓÇö` and `✗` as garbage. Setting the console code page would
//! fix the attached case and not the redirected one — a pipe's encoding belongs
//! to whatever reads it — so the characters themselves have to stay ASCII.
//!
//! This exists because grepping for the offenders by hand missed five sites on
//! the first pass: the search keyed on the print macro, and in a multi-line
//! `format!` the macro is on a different line from the string.

use std::path::{Path, PathBuf};

/// Characters that render as mojibake and have an ASCII spelling that costs
/// nothing. Deliberately not "all non-ASCII": `paths.rs` tests non-ASCII path
/// handling with real accented and CJK input, which must keep working.
const FORBIDDEN: [(char, &str); 8] = [
    ('\u{2014}', "em dash - use '-'"),
    ('\u{2013}', "en dash - use '-'"),
    ('\u{2026}', "ellipsis - use '...'"),
    ('\u{2022}', "bullet - use '-'"),
    ('\u{2713}', "check mark - use '+'"),
    ('\u{2717}', "ballot X - use 'x'"),
    ('\u{00A9}', "copyright sign - use '(c)'"),
    ('\u{2192}', "rightwards arrow - use '->'"),
];

fn crate_sources() -> Vec<PathBuf> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf();
    ["naner", "naner-core", "naner-init"]
        .iter()
        .map(|c| workspace.join(c).join("src"))
        .collect()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Everything before `//`. Crude — it truncates at a `//` inside a string, such
/// as a URL — but that direction is safe: it can only hide a violation, never
/// invent one.
fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

#[test]
fn no_typographic_characters_outside_comments() {
    let mut files = Vec::new();
    for dir in crate_sources() {
        rust_files(&dir, &mut files);
    }
    assert!(!files.is_empty(), "found no sources to scan");

    let mut offences = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let code = code_part(line);
            for (ch, advice) in FORBIDDEN {
                if code.contains(ch) {
                    offences.push(format!("{}:{}: {ch:?} ({advice})", file.display(), n + 1));
                }
            }
        }
    }

    assert!(
        offences.is_empty(),
        "typographic characters render as mojibake on a Windows console:\n{}",
        offences.join("\n")
    );
}
