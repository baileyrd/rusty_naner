//! Command: `naner add-to-path [--remove] [--dry-run]`
//!
//! Puts `<NANER_ROOT>\vendor\bin` on the *user* PATH so `naner` resolves
//! from any shell, without importing the whole naner environment the way
//! `setup-shell` does. `--remove` undoes it; `--dry-run` shows the value
//! that would be written without touching the registry. Only `HKCU` is
//! edited, so no elevation is needed.

use std::path::Path;

use naner_core::{constants, logger, paths};

pub fn execute(args: &[String]) -> i32 {
    let remove = args.iter().any(|a| a == "--remove");
    let dry_run = args.iter().any(|a| a == "--dry-run");
    if let Some(unknown) = args
        .iter()
        .find(|a| *a != "--remove" && *a != "--dry-run")
    {
        eprintln!("Unknown argument '{unknown}'. Usage: naner add-to-path [--remove] [--dry-run]");
        return 1;
    }

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };
    run(&naner_root, remove, dry_run)
}

/// Same location `setup-shell` points at: `vendor/bin` is where the release
/// workflow stages `naner.exe` and where updates install it. `bin/` is the
/// user's own directory.
fn path_entry(naner_root: &Path) -> String {
    naner_root
        .join(constants::directory_names::VENDOR)
        .join(constants::directory_names::BIN)
        .display()
        .to_string()
}

#[cfg(windows)]
fn run(naner_root: &Path, remove: bool, dry_run: bool) -> i32 {
    use naner_core::user_path;

    let entry = path_entry(naner_root);
    logger::header("Naner PATH Setup");
    logger::info(&format!("Entry: {entry}"));

    let (current, kind) = match user_path::registry::read_user_path() {
        Ok(v) => v,
        Err(err) => {
            logger::failure(&format!("Could not read the user PATH: {err}"));
            return 1;
        }
    };

    let updated = if remove {
        match user_path::removed(&current, &entry) {
            Some(v) => v,
            None => {
                logger::success("Not on the user PATH - nothing to remove.");
                return 0;
            }
        }
    } else {
        match user_path::appended(&current, &entry) {
            Some(v) => v,
            None => {
                logger::success("Already on the user PATH - nothing to change.");
                return 0;
            }
        }
    };

    if dry_run {
        logger::newline();
        logger::info("Dry run - nothing written. The user PATH would become:");
        logger::newline();
        println!("{updated}");
        return 0;
    }

    if let Err(err) = user_path::registry::write_user_path(&updated, kind) {
        logger::failure(&format!("Could not write the user PATH: {err}"));
        return 1;
    }
    user_path::registry::broadcast_environment_change();

    if remove {
        logger::success("Removed from the user PATH.");
    } else {
        logger::success("Added to the user PATH.");
    }
    logger::info("New shells pick this up; shells already open keep their current PATH.");
    0
}

#[cfg(not(windows))]
fn run(naner_root: &Path, _remove: bool, _dry_run: bool) -> i32 {
    logger::failure("add-to-path manages the per-user Windows PATH and is only available on Windows.");
    logger::info(&format!(
        "On this platform, add {} to PATH in your shell profile instead.",
        path_entry(naner_root)
    ));
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_is_vendor_bin_not_the_users_bin() {
        let entry = path_entry(Path::new("C:/naner"));
        assert!(
            entry.ends_with("bin") && entry.contains("vendor"),
            "expected <root>/vendor/bin, got {entry}"
        );
        assert_ne!(
            entry,
            Path::new("C:/naner").join("bin").display().to_string(),
            "`bin/` is the user's own directory and ships empty"
        );
    }
}
