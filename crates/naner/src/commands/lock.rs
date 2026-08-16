//! Command: `naner lock [--refresh [vendor...]] [--porcelain]`
//!
//! Inspects and maintains `naner.lock`, the pin of exactly which vendor
//! artifacts this environment installs. Entries are written automatically by a
//! successful `naner install`; this command exists to read them back and to
//! drop pins so the next install re-resolves.

use naner_core::lockfile::NanerLockfile;
use naner_core::{constants, logger, paths, vendors};

const REFRESH_FLAG: &str = "--refresh";
const PORCELAIN_FLAG: &str = "--porcelain";

pub fn execute(args: &[String]) -> i32 {
    let Some(naner_root) = find_root_or_explain() else {
        return 1;
    };

    let refresh = args.iter().any(|a| a.eq_ignore_ascii_case(REFRESH_FLAG));
    let porcelain = args.iter().any(|a| a.eq_ignore_ascii_case(PORCELAIN_FLAG));
    let names: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();

    if refresh {
        return refresh_pins(&naner_root, &names);
    }
    show(&naner_root, porcelain)
}

fn find_root_or_explain() -> Option<std::path::PathBuf> {
    match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(root) => Some(root),
        Err(err) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", err.message);
            None
        }
    }
}

fn show(naner_root: &std::path::Path, porcelain: bool) -> i32 {
    let Some(lock) = NanerLockfile::load(naner_root) else {
        if porcelain {
            return 0;
        }
        logger::info(&format!(
            "No {} yet — it is written as vendors are installed.",
            naner_core::lockfile::LOCKFILE_NAME
        ));
        return 0;
    };

    if porcelain {
        // name<TAB>version<TAB>sha256<TAB>url — sha256 empty when unpinned.
        for (key, entry) in &lock.vendors {
            println!(
                "{key}\t{}\t{}\t{}",
                entry.version,
                entry.sha256.as_deref().unwrap_or(""),
                entry.url
            );
        }
        return 0;
    }

    logger::header("Locked Vendors");
    logger::newline();
    if lock.is_empty() {
        logger::info("Lockfile is present but empty.");
        return 0;
    }

    let mut unverifiable = 0;
    for (key, entry) in &lock.vendors {
        let digest = match entry.sha256.as_deref() {
            Some(sha) if !sha.is_empty() => format!("sha256:{}…", &sha[..sha.len().min(12)]),
            _ => {
                unverifiable += 1;
                "no digest".to_string()
            }
        };
        println!("  {key:<16} {:<24} {digest}", entry.version);
    }
    logger::newline();

    if unverifiable > 0 {
        logger::warning(&format!(
            "{unverifiable} pin(s) fix a URL but carry no digest — those installs are not verified."
        ));
    }
    logger::info("Use 'naner lock --refresh [vendor...]' to re-resolve on next install.");
    0
}

fn refresh_pins(naner_root: &std::path::Path, names: &[&String]) -> i32 {
    let Some(mut lock) = NanerLockfile::load(naner_root) else {
        logger::info(&format!(
            "No {} to refresh.",
            naner_core::lockfile::LOCKFILE_NAME
        ));
        return 0;
    };

    // Resolve the given names the same way `install` does, so `naner lock
    // --refresh nodejs` works with the display name or the key.
    let loader = vendors::VendorConfigurationLoader::new(naner_root);
    let mut dropped = 0;
    let mut unknown = Vec::new();

    if names.is_empty() {
        dropped = lock.vendors.len();
        lock.vendors.clear();
    } else {
        for name in names {
            let key = loader
                .vendor_by_key(name)
                .map(|v| v.key)
                .unwrap_or_else(|| (*name).clone());
            if lock.remove(&key) {
                dropped += 1;
            } else {
                unknown.push((*name).clone());
            }
        }
    }

    for name in &unknown {
        logger::warning(&format!("Not pinned: {name}"));
    }

    if dropped == 0 {
        logger::info("Nothing to refresh.");
        return if unknown.is_empty() { 0 } else { 1 };
    }

    match lock.save(naner_root) {
        Ok(()) => {
            logger::success(&format!(
                "Dropped {dropped} pin(s). The next install re-resolves and re-pins them."
            ));
            0
        }
        Err(e) => {
            logger::failure(&format!("Could not write lockfile: {e}"));
            1
        }
    }
}
