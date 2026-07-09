//! `naner.exe` — the launcher. Phase 0 stub: exists so the workspace, the
//! console spike, CI, and the parity harness all have something real to run.
//! Phase 2 replaces the routing below with the full CLI
//! (MIGRATION_ANALYSIS §1.4).

#![cfg_attr(windows, windows_subsystem = "windows")]

use naner_core::console::{self, Exe};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Console setup must precede any output (MIGRATION_ANALYSIS §4.1).
    let _console = console::setup(console::needs_console(Exe::Naner, &args));

    let code = match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("--version") | Some("-v") => {
            println!("Naner v{}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--help") | Some("-h") | Some("/?") => {
            print_help();
            0
        }
        _ => {
            eprintln!(
                "[!] naner (Rust port) Phase 0 stub: only --version and --help are implemented"
            );
            1
        }
    };
    std::process::exit(code);
}

fn print_help() {
    println!(
        "Naner v{} (Rust port, Phase 0 stub)",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("Implemented: --version, --help");
    println!("Everything else lands in Phase 2 (see MIGRATION_ANALYSIS.md).");
}
