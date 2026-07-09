//! Port of the C# `Logger`/`ConsoleLogger` (Naner.Core). The output contract
//! (MIGRATION_ANALYSIS §1.3): `[*]` cyan status, `[OK]` green, `[✗]` red,
//! gray 4-space-indented info, `[DEBUG]` yellow (only in debug mode), header
//! with `=` underline — all on stdout; `[!]` yellow warnings are the ONLY
//! thing on stderr.
//!
//! Colors are ANSI (VT processing is enabled by `console::setup` on Windows).
//! `ConsoleColor` mapping: Cyan→96, Green→92, Red→91, Yellow→93, Gray→37 —
//! .NET's non-Dark `ConsoleColor`s are the bright variants, except `Gray`,
//! which is the dim "white".

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static COLOR_ENABLED: AtomicBool = AtomicBool::new(true);

/// Disable colors (e.g. when stdout is not a terminal). The C# code always
/// emitted color calls; making this switchable costs nothing and the default
/// (on) matches the C# behavior.
pub fn set_color_enabled(enabled: bool) {
    COLOR_ENABLED.store(enabled, Ordering::Relaxed);
}

fn paint(code: &str, line: &str) -> String {
    if COLOR_ENABLED.load(Ordering::Relaxed) {
        format!("\x1b[{code}m{line}\x1b[0m")
    } else {
        line.to_string()
    }
}

/// `[*]` cyan status line (stdout).
pub fn status(message: &str) {
    println!("{}", paint("96", &format!("[*] {message}")));
}

/// `[OK]` green success line (stdout).
pub fn success(message: &str) {
    println!("{}", paint("92", &format!("[OK] {message}")));
}

/// `[✗]` red failure line (stdout — not stderr; only warnings go to stderr).
pub fn failure(message: &str) {
    println!("{}", paint("91", &format!("[✗] {message}")));
}

/// Gray, 4-space-indented info line (stdout).
pub fn info(message: &str) {
    println!("{}", paint("37", &format!("    {message}")));
}

/// `[DEBUG]` yellow line (stdout), emitted only when `debug_mode`.
pub fn debug(message: &str, debug_mode: bool) {
    if debug_mode {
        println!("{}", paint("93", &format!("[DEBUG] {message}")));
    }
}

/// `[!]` yellow warning — the only output on stderr.
pub fn warning(message: &str) {
    let line = paint("93", &format!("[!] {message}"));
    let _ = writeln!(std::io::stderr(), "{line}");
}

/// Blank line (stdout).
pub fn newline() {
    println!();
}

/// Cyan header with a full-width `=` underline, then a blank line (stdout).
pub fn header(text: &str) {
    println!("{}", paint("96", text));
    println!("{}", paint("96", &"=".repeat(text.chars().count())));
    println!();
}
