//! `naner-init.exe` — the bootstrapper/updater. Phase 0 stub; the real
//! init/update/check-update flows land in Phase 4 (MIGRATION_ANALYSIS §6).

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::io::BufRead;

use naner_core::console;

/// `InitCommandNames.ConsoleCommands`. In C# the actual attach decision also
/// happens on first output for pass-through launches; Phase 4 refines this
/// against the real Program.cs flow.
const CONSOLE_COMMANDS: [&str; 7] = [
    "--version",
    "-v",
    "--help",
    "-h",
    "init",
    "update",
    "check-update",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Console setup must precede any output (MIGRATION_ANALYSIS §4.1).
    let state = console::setup(console::arg_needs_console(&args, &CONSOLE_COMMANDS));

    let code = match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("--version") | Some("-v") => {
            println!("Naner Init v{}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--help") | Some("-h") => {
            println!(
                "Naner Init v{} (Rust port, Phase 0 stub)",
                env!("CARGO_PKG_VERSION")
            );
            println!();
            println!("Implemented: --version, --help");
            println!("init / update / check-update land in Phase 4 (see MIGRATION_ANALYSIS.md).");
            0
        }
        _ => {
            eprintln!(
                "[!] naner-init (Rust port) Phase 0 stub: only --version and --help are implemented"
            );
            1
        }
    };

    // The exit pause fires only when we allocated a fresh console — i.e. the
    // double-click launch, where the window would otherwise vanish instantly.
    if state.allocated() {
        println!();
        println!("Press Enter to exit...");
        let _ = std::io::stdin().lock().read_line(&mut String::new());
    }

    std::process::exit(code);
}
