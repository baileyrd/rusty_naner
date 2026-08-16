//! Command: `naner self-update`
//!
//! Delegates to `naner-init`, which owns the update protocol. `naner.exe`
//! cannot replace itself while running on Windows — the file is locked — which
//! is why `naner-init` exists as a separate executable in the first place.

use std::path::PathBuf;

use naner_core::{constants, logger, paths};

/// `naner-init` as shipped: alongside naner.exe in `vendor/bin`, or on PATH.
fn find_naner_init(naner_root: &std::path::Path) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "naner-init.exe"
    } else {
        "naner-init"
    };
    let bundled = naner_root.join("vendor").join("bin").join(name);
    if bundled.is_file() {
        return Some(bundled);
    }
    let beside_us = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .filter(|p| p.is_file());
    if beside_us.is_some() {
        return beside_us;
    }
    std::env::var("PATH").ok().and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    })
}

pub fn execute(args: &[String]) -> i32 {
    logger::header("Naner Self-Update");
    logger::newline();

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    let Some(init) = find_naner_init(&naner_root) else {
        logger::failure("naner-init not found");
        logger::info(
            "naner-init performs the update - it is a separate executable because \
             naner.exe cannot replace itself while running.",
        );
        logger::info("Expected at vendor/bin/, beside naner.exe, or on PATH.");
        return 1;
    };

    logger::info(&format!("Current version: v{}", constants::VERSION));
    logger::status(&format!("Handing over to {}...", init.display()));
    logger::newline();

    // Inherit stdio so naner-init's prompts and progress reach the user, and
    // pass its exit code straight through rather than inventing one.
    match std::process::Command::new(&init).args(args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            logger::failure(&format!("Could not run {}: {err}", init.display()));
            1
        }
    }
}
