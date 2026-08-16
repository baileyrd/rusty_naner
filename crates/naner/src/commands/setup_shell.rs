//! Command: `naner setup-shell [pwsh|bash|cmd] [--dry-run]`
//!
//! Adds the naner environment export to a shell's startup file. Writing to a
//! file the user owns, so it is idempotent, backed up, and `--dry-run` really
//! does mean "show me, do not touch it".

use std::fs;
use std::path::{Path, PathBuf};

use naner_core::{constants, logger, paths};

/// Marks the block so a re-run replaces it rather than appending a duplicate.
const BEGIN: &str = "# >>> naner initialize >>>";
const END: &str = "# <<< naner initialize <<<";

/// Where each shell reads its startup commands from.
fn profile_path(shell: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)?;
    match shell {
        "pwsh" | "powershell" => Some(
            home.join("Documents")
                .join("PowerShell")
                .join("Microsoft.PowerShell_profile.ps1"),
        ),
        "bash" => Some(home.join(".bashrc")),
        _ => None,
    }
}

/// Replace an existing naner block, or append one. Returns the new contents,
/// or `None` when the file already says exactly this.
fn upsert_block(existing: &str, block: &str) -> Option<String> {
    if let (Some(start), Some(end)) = (existing.find(BEGIN), existing.find(END)) {
        if end < start {
            // Markers out of order: treat as absent rather than cutting a
            // negative range out of the user's file.
            return Some(format!("{}\n{block}", existing.trim_end()));
        }
        let end = end + END.len();
        let replaced = format!("{}{block}{}", &existing[..start], &existing[end..]);
        let replaced = replaced.trim_end().to_string() + "\n";
        return (replaced != existing).then_some(replaced);
    }
    let mut out = existing.trim_end().to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(block);
    out.push('\n');
    Some(out)
}

/// Where `naner.exe` lives inside a naner root.
fn naner_exe_path(naner_root: &Path) -> PathBuf {
    naner_root
        .join(constants::directory_names::VENDOR)
        .join(constants::directory_names::BIN)
        .join(constants::executables::NANER)
}

pub fn execute(args: &[String]) -> i32 {
    let shell = args
        .first()
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "pwsh".to_string());
    let dry_run = args.iter().any(|a| a == "--dry-run");

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    // `vendor/bin`, not `bin`. That is where the release workflow stages
    // naner.exe, where `naner-init` installs and updates it, and what
    // `naner.bat` calls. `bin/` is the user's own directory and ships empty.
    // The generated block guards on `Test-Path`/`-f`, so pointing at the wrong
    // path does not fail loudly -- the integration just silently never runs.
    let naner_exe = naner_exe_path(&naner_root);

    let block = match shell.as_str() {
        "pwsh" | "powershell" => format!(
            "{BEGIN}\nif (Test-Path \"{exe}\") {{ & \"{exe}\" --export-env -f powershell | Invoke-Expression }}\n{END}",
            exe = naner_exe.display()
        ),
        "bash" => format!(
            "{BEGIN}\nif [ -f \"{exe}\" ]; then eval \"$(\"{exe}\" --export-env -f bash)\"; fi\n{END}",
            exe = naner_exe.display()
        ),
        "cmd" => {
            // cmd has no per-user startup file naner can safely edit; the
            // AutoRun registry key is not something to write behind a user's
            // back. Print the line and say why.
            logger::header("Naner Shell Integration: CMD");
            println!("@call \"{}\" --export-env -f cmd", naner_exe.display());
            logger::newline();
            logger::info(
                "cmd has no startup file to edit. Add the line above to your own \
                 batch launcher, or set it as a Command Processor AutoRun value.",
            );
            return 0;
        }
        other => {
            eprintln!("Unknown shell '{other}'. Supported: pwsh, bash, cmd");
            return 1;
        }
    };

    let Some(profile) = profile_path(&shell) else {
        logger::failure("Could not determine a home directory (HOME / USERPROFILE unset)");
        return 1;
    };

    logger::header(&format!("Naner Shell Integration: {shell}"));
    logger::info(&format!("Profile: {}", profile.display()));

    let existing = fs::read_to_string(&profile).unwrap_or_default();
    let Some(updated) = upsert_block(&existing, &block) else {
        logger::success("Already integrated - nothing to change.");
        return 0;
    };

    if dry_run {
        logger::newline();
        logger::info("Dry run - nothing written. The block would be:");
        logger::newline();
        println!("{block}");
        return 0;
    }

    if let Some(parent) = profile.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        logger::failure(&format!("Could not create {}: {err}", parent.display()));
        return 1;
    }
    if let Err(err) = crate::config_file::replace(&profile, &updated) {
        logger::failure(&format!("Failed to update shell profile: {err}"));
        return 1;
    }

    logger::success(&format!("Updated {}", profile.display()));
    logger::info("Restart the shell, or re-source the profile, to pick it up.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "# >>> naner initialize >>>\nnew line\n# <<< naner initialize <<<";

    /// The integration block is guarded by `Test-Path` / `-f`, so a wrong path
    /// produces no error at all -- the block is written, looks correct in
    /// `--dry-run`, and silently never runs. That is how `bin/naner.exe`
    /// survived: nothing fails when it is wrong.
    ///
    /// `vendor/bin` is the location the release workflow stages to, that
    /// `naner-init` installs and updates, and that `naner.bat` calls.
    #[test]
    fn the_integration_block_points_at_where_naner_exe_actually_is() {
        let exe = naner_exe_path(Path::new("C:/naner"));
        assert!(
            exe.ends_with("vendor/bin/naner.exe"),
            "expected <root>/vendor/bin/naner.exe, got {}",
            exe.display()
        );
        assert_ne!(
            exe,
            Path::new("C:/naner").join("bin").join("naner.exe"),
            "`bin/` is the user's own directory and ships empty"
        );
    }

    #[test]
    fn appends_to_a_file_that_has_no_block() {
        let out = upsert_block("export FOO=1\n", BLOCK).expect("changed");
        assert!(
            out.starts_with("export FOO=1"),
            "user content preserved: {out}"
        );
        assert!(out.contains(BLOCK));
    }

    #[test]
    fn replaces_an_existing_block_instead_of_appending_a_second() {
        let first = upsert_block("", BLOCK).unwrap();
        let updated = "# >>> naner initialize >>>\ndifferent\n# <<< naner initialize <<<";
        let second = upsert_block(&first, updated).expect("changed");
        assert_eq!(
            second.matches(BEGIN).count(),
            1,
            "duplicated block: {second}"
        );
        assert!(second.contains("different"));
    }

    #[test]
    fn an_identical_block_is_a_no_op() {
        let first = upsert_block("existing\n", BLOCK).unwrap();
        assert!(
            upsert_block(&first, BLOCK).is_none(),
            "re-running must not keep rewriting the file"
        );
    }

    #[test]
    fn surrounding_content_survives_a_replacement() {
        let original = format!("before\n{BLOCK}\nafter\n");
        let updated = "# >>> naner initialize >>>\nX\n# <<< naner initialize <<<";
        let out = upsert_block(&original, updated).expect("changed");
        assert!(out.contains("before"), "{out}");
        assert!(out.contains("after"), "{out}");
        assert!(out.contains('X'));
    }

    /// Markers in the wrong order must not cut a negative range out of a file
    /// the user owns.
    #[test]
    fn reversed_markers_do_not_corrupt_the_file() {
        let weird = format!("{END}\nstuff\n{BEGIN}\n");
        let out = upsert_block(&weird, BLOCK).expect("changed");
        assert!(out.contains("stuff"), "user content lost: {out}");
    }
}
