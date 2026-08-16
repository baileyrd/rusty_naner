//! Port of `VersionCommand` — the long-form version screen shown when
//! `--version`/`-v` is the first argument.

use naner_core::constants;

pub fn execute() -> i32 {
    println!(
        "Naner Terminal Environment Manager - Version {}",
        constants::VERSION
    );
    println!("Phase: {}", constants::PHASE_NAME);
    println!();
    println!("A unified terminal environment for Windows development");
    println!("Copyright (c) 2026");
    0
}

/// Port of `Program.ShowVersion` — the short form reached when `-v` appears
/// in launch options (i.e. not as the first argument).
pub fn show_short() {
    println!("naner {}", constants::VERSION);
    println!("{}", constants::PHASE_NAME);
}
