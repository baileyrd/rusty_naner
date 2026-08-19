//! Command: `naner outdated [--porcelain]`
//!
//! Compares each *installed* vendor's recorded version (`.vendor-version`)
//! against what its release source currently calls latest, and says which
//! installs have fallen behind — loudest for a major-version jump (Rust going
//! 1.x → 2.x is a different conversation than a patch bump). Network is the
//! point here; the offline nudge lives in `naner doctor`, which compares
//! against the shipped fallback pins instead.
//!
//! Exit codes: 0 = everything current, 1 = at least one update available (or
//! the environment couldn't be checked at all).

use naner_core::http::UreqHttp;
use naner_core::{constants, logger, paths, vendors, version};
use serde_json::json;

pub fn execute(args: &[String]) -> i32 {
    let porcelain = args
        .iter()
        .any(|a| a.eq_ignore_ascii_case("--porcelain") || a.eq_ignore_ascii_case("-p"));

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(root) => root,
        Err(err) => {
            if porcelain {
                println!("{}", json!({ "status": "error", "error": err.message }));
            } else {
                logger::failure("Could not locate Naner root directory");
                println!("{}", err.message);
            }
            return 1;
        }
    };

    let loader = vendors::VendorConfigurationLoader::new(&naner_root);
    let all = loader.load_all_vendors();
    let http = UreqHttp::new();
    let resolver = vendors::UnifiedVendorInstaller::new(&naner_root, Vec::new(), &http);

    let mut rows = Vec::new();
    let mut outdated = 0usize;
    for vendor in all.iter().filter(|v| loader.is_vendor_installed(v)) {
        let row = check_one(vendor, loader.vendor_version(vendor), &resolver);
        if row.state.starts_with("outdated") {
            outdated += 1;
        }
        rows.push(row);
    }

    if porcelain {
        println!(
            "{}",
            json!({
                "status": "ok",
                "outdated": outdated,
                "vendors": rows.iter().map(|r| json!({
                    "vendor": r.vendor,
                    "name": r.name,
                    "installed": r.installed,
                    "latest": r.latest,
                    "state": r.state,
                })).collect::<Vec<_>>(),
            })
        );
        return i32::from(outdated > 0);
    }

    logger::header("Installed Vendor Versions");
    if rows.is_empty() {
        logger::info("No vendors are installed.");
        return 0;
    }
    for row in &rows {
        let line = format!(
            "{} - installed {}, latest {} [{}]",
            row.name,
            row.installed.as_deref().unwrap_or("?"),
            row.latest.as_deref().unwrap_or("?"),
            row.state
        );
        match row.state.as_str() {
            "outdated (major)" => logger::warning(&line),
            "outdated" => logger::warning(&line),
            "current" => logger::success(&line),
            _ => logger::info(&line),
        }
    }
    logger::newline();
    if outdated > 0 {
        logger::warning(&format!(
            "{outdated} vendor(s) have updates available. Run 'naner update-vendors' \
             to update them all, or 'naner install <vendor>' after removing one."
        ));
    } else {
        logger::success("Every checked vendor is current.");
    }
    i32::from(outdated > 0)
}

struct Row {
    vendor: String,
    name: String,
    installed: Option<String>,
    latest: Option<String>,
    state: String,
}

fn check_one(
    vendor: &vendors::VendorDefinition,
    installed: Option<String>,
    resolver: &vendors::UnifiedVendorInstaller,
) -> Row {
    let mut row = Row {
        vendor: vendor.key.clone(),
        name: vendor.name.clone(),
        installed,
        latest: None,
        state: String::new(),
    };

    // A static vendor's "latest" is whatever its own config pins — resolving
    // it would compare the pin to itself and always answer "current."
    if vendor.source_type == vendors::VendorSourceType::StaticUrl {
        row.latest = vendor.fallback_version.clone();
        row.state = "unchecked (static)".into();
        return row;
    }
    let latest = match resolver.resolve_upstream(vendor) {
        Ok(Some(info)) => info.version,
        Ok(None) | Err(_) => None,
    };
    let Some(latest) = latest else {
        row.state = "unknown (resolution failed)".into();
        return row;
    };
    row.latest = Some(latest.clone());
    let Some(installed) = row.installed.clone() else {
        row.state = "unknown (no .vendor-version)".into();
        return row;
    };

    row.state = match version::vendor_compare(&installed, &latest) {
        std::cmp::Ordering::Less if version::vendor_major_differs(&installed, &latest) => {
            "outdated (major)".into()
        }
        std::cmp::Ordering::Less => "outdated".into(),
        _ => "current".into(),
    };
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use naner_core::vendors::VendorDefinition;

    fn resolverless_row(installed: Option<&str>, latest: &str) -> String {
        // `check_one` needs a resolver only for the network half; the state
        // derivation below is the part worth pinning down, so exercise it
        // through the same comparison the command uses.
        let installed = installed.map(str::to_string);
        match installed {
            None => "unknown (no .vendor-version)".into(),
            Some(i) => match version::vendor_compare(&i, latest) {
                std::cmp::Ordering::Less if version::vendor_major_differs(&i, latest) => {
                    "outdated (major)".into()
                }
                std::cmp::Ordering::Less => "outdated".into(),
                _ => "current".into(),
            },
        }
    }

    #[test]
    fn state_derivation_covers_the_three_answers() {
        assert_eq!(resolverless_row(Some("v20.11.0"), "v20.19.0"), "outdated");
        assert_eq!(
            resolverless_row(Some("1.9.9"), "2.0.0"),
            "outdated (major)",
            "the Rust v1 -> v2 case must be flagged distinctly"
        );
        assert_eq!(resolverless_row(Some("go1.22.0"), "go1.22.0"), "current");
        // Installed newer than upstream (a prerelease, a yanked release):
        // not "outdated."
        assert_eq!(resolverless_row(Some("2.0.0"), "1.9.0"), "current");
        assert_eq!(
            resolverless_row(None, "1.0.0"),
            "unknown (no .vendor-version)"
        );
    }

    #[test]
    fn a_static_vendor_is_reported_unchecked_not_current() {
        let vendor = VendorDefinition {
            key: "Static".into(),
            name: "Static".into(),
            source_type: vendors::VendorSourceType::StaticUrl,
            fallback_version: Some("1.0.0".into()),
            ..Default::default()
        };
        // No network happens on this path, so a throwaway resolver is safe.
        let http = UreqHttp::new();
        let resolver =
            vendors::UnifiedVendorInstaller::new(std::path::Path::new("."), Vec::new(), &http);
        let row = check_one(&vendor, Some("1.0.0".into()), &resolver);
        assert_eq!(row.state, "unchecked (static)");
    }
}
