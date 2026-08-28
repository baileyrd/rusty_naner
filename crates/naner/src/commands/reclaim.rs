//! Command: `naner reclaim [--dry-run]`
//!
//! Sweeps dotfolders/files known to leak into the real Windows profile
//! despite naner's `Environment.EnvironmentVariables` redirects -- Claude
//! Code (`.claude/`, `.claude.json`), Codex CLI (`.codex/`), Gemini CLI /
//! Antigravity (`.gemini/`) -- into `%NANER_ROOT%\home`, then bridges the
//! original location back so future writes land there too. See
//! `naner_core::leak_reclaim` for the mechanism and
//! `docs/VALIDATION.md`'s "Known limitations" for why these specific tools
//! need it despite the existing env-var redirects.

use naner_core::{constants, leak_reclaim, logger, paths};

pub fn execute(args: &[String]) -> i32 {
    let dry_run = args.iter().any(|a| a.eq_ignore_ascii_case("--dry-run"));

    logger::header("Naner Leak Reclaim");
    logger::newline();

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };
    let naner_home = naner_root.join(constants::directory_names::HOME);
    if !naner_home.is_dir() {
        logger::failure(&format!(
            "{} does not exist -- run naner once to initialize it first",
            naner_home.display()
        ));
        return 1;
    }

    let Some(real_profile) = leak_reclaim::real_user_profile() else {
        logger::failure("Could not determine the real Windows user profile directory");
        return 1;
    };

    logger::info(&format!("Real profile: {}", real_profile.display()));
    logger::info(&format!("Naner home:   {}", naner_home.display()));
    if dry_run {
        logger::info("Dry run - nothing will be moved or linked.");
    }
    logger::newline();

    let summary = leak_reclaim::reclaim(&real_profile, &naner_home, dry_run);

    logger::newline();
    if dry_run {
        logger::info(&format!(
            "Dry run complete: {} item(s) would migrate. Re-run without --dry-run to apply.",
            summary.migrated
        ));
        return 0;
    }

    logger::success(&format!(
        "Reclaim complete: {} item(s) migrated, {} linked, {} not linked.",
        summary.migrated, summary.linked, summary.link_failed
    ));
    if summary.swept_backups > 0 {
        logger::info(&format!(
            "Also swept {} loose backup file(s) into naner's home.",
            summary.swept_backups
        ));
    }
    if summary.link_failed > 0 {
        logger::info(
            "To make an unlinked item stay redirected too, enable Windows Developer Mode \
             (Settings > Privacy & Security > For Developers) or run as Administrator, \
             then run 'naner reclaim' again.",
        );
    }
    0
}
