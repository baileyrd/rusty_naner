//! Command: `naner suggest <name> [--porcelain]`
//!
//! Maps an executable name the shell failed to find to the vendor that would
//! provide it, entirely offline: each vendor's optional `provides` list first,
//! then names derived from `naner.json`'s `VendorPaths` entries. Built to be
//! called from shell command-not-found hooks (`setup-shell` writes them), so
//! the contract is strict: one hint on stdout when there is a match, nothing
//! at all when there is not - a noisy wrong guess is worse than no hint - and
//! no network or subprocess work, ever.
//!
//! Exit codes: 0 = match (hint printed), 1 = no match (silent), 2 = usage.

use naner_core::{config, constants, logger, paths, vendors};
use serde_json::json;

pub fn execute(args: &[String]) -> i32 {
    let porcelain = args
        .iter()
        .any(|a| a.eq_ignore_ascii_case("--porcelain") || a.eq_ignore_ascii_case("-p"));
    let Some(query) = args.iter().find(|a| !a.starts_with('-')) else {
        eprintln!("Usage: naner suggest <command-name> [--porcelain]");
        return 2;
    };

    // This command's stdout is the hint and nothing else: the loader's
    // status/info chatter would either drown it or teach the hooks to print
    // noise on every mistyped command. Warnings still reach stderr, which the
    // hooks redirect away.
    logger::set_quiet(true);

    // No naner tree means nothing to suggest - the shell's own error stands.
    let Ok(naner_root) = paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH)
    else {
        return 1;
    };

    let loader = vendors::VendorConfigurationLoader::new(&naner_root);
    // All vendors, disabled included: "installed but switched off" is one of
    // the states this command exists to explain.
    let all = loader.load_all_vendors();
    // VendorPaths is the fallback name source; a config that fails to load
    // just means `provides` is the only source, not that suggest errors out.
    let vendor_paths = config::find_configuration_file(&naner_root)
        .and_then(|p| config::load(&naner_root, Some(&p)).ok())
        .map(|c| c.vendor_paths)
        .unwrap_or_default();

    let Some(vendor) = find_provider(query, &all, &vendor_paths) else {
        return 1;
    };

    let installed = loader.is_vendor_installed(vendor);
    let hint = hint_for(query, vendor, installed);

    if porcelain {
        println!(
            "{}",
            json!({
                "query": query,
                "vendor": vendor.key,
                "name": vendor.name,
                "installed": installed,
                "enabled": vendor.enabled,
                "hint": hint,
            })
        );
    } else {
        println!("{hint}");
    }
    0
}

/// Case-insensitive executable-name normalization: `Node.EXE` and `node` ask
/// the same question. Only known Windows launcher extensions are stripped —
/// a dot elsewhere (`msys2_shell.cmd` → `msys2_shell`) must not eat the name.
fn normalize(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".com", ".ps1"] {
        if let Some(stripped) = lower.strip_suffix(ext) {
            return stripped.to_string();
        }
    }
    lower
}

/// Resolve `query` to a vendor: explicit `provides` entries win over names
/// derived from `VendorPaths`, so a vendor can correct a derived guess.
fn find_provider<'a>(
    query: &str,
    all: &'a [vendors::VendorDefinition],
    vendor_paths: &naner_core::collections::OrderedMap<String>,
) -> Option<&'a vendors::VendorDefinition> {
    let wanted = normalize(query);
    if wanted.is_empty() {
        return None;
    }

    if let Some(vendor) = all
        .iter()
        .find(|v| v.provides.iter().any(|p| normalize(p) == wanted))
    {
        return Some(vendor);
    }

    // A VendorPaths value is `%NANER_ROOT%\vendor\<extractDir>\...\<exe>`:
    // the last segment names the executable, the segment after `vendor` names
    // the directory a vendor definition owns. Entries that do not point into
    // a vendor directory (naner.exe itself lives in `vendor\bin`) resolve to
    // no vendor and stay silent.
    for (_key, path) in vendor_paths.iter() {
        let segments: Vec<&str> = path.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
        let Some(exe) = segments.last() else {
            continue;
        };
        if normalize(exe) != wanted {
            continue;
        }
        let extract_dir = segments
            .iter()
            .position(|s| s.eq_ignore_ascii_case(constants::directory_names::VENDOR))
            .and_then(|i| segments.get(i + 1));
        if let Some(dir) = extract_dir
            && let Some(vendor) = all.iter().find(|v| v.extract_dir.eq_ignore_ascii_case(dir))
        {
            return Some(vendor);
        }
    }
    None
}

/// The one line the shell hook prints above its own error. `naner install`
/// refuses a disabled vendor, so the hint never suggests a command that
/// would immediately bounce: disabled vendors get the enable step first.
fn hint_for(query: &str, vendor: &vendors::VendorDefinition, installed: bool) -> String {
    let enable_step = format!(
        "set \"enabled\": true in config{sep}vendors{sep}{key}.json",
        sep = std::path::MAIN_SEPARATOR,
        key = vendor.key
    );
    match (installed, vendor.enabled) {
        (false, true) => format!(
            "'{query}' is provided by {} - install it with: naner install {}",
            vendor.name, vendor.key
        ),
        (false, false) => format!(
            "'{query}' is provided by {} - {enable_step}, then: naner install {}",
            vendor.name, vendor.key
        ),
        (true, false) => format!(
            "'{query}' is installed ({}) but disabled - {enable_step}",
            vendor.name
        ),
        (true, true) => format!(
            "'{query}' is installed ({}) and on PATH inside naner shells - is this terminal naner-launched?",
            vendor.name
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use naner_core::collections::OrderedMap;
    use naner_core::vendors::VendorDefinition;

    fn vendor(key: &str, name: &str, extract_dir: &str, enabled: bool) -> VendorDefinition {
        VendorDefinition {
            key: key.into(),
            name: name.into(),
            extract_dir: extract_dir.into(),
            enabled,
            ..Default::default()
        }
    }

    fn catalog() -> Vec<VendorDefinition> {
        let mut node = vendor("NodeJS", "Node.js", "nodejs", false);
        node.provides = vec!["node".into(), "npm".into(), "npx".into()];
        let git = vendor("GitForWindows", "Git for Windows", "git", true);
        vec![node, git]
    }

    fn paths() -> OrderedMap<String> {
        let mut map = OrderedMap::new();
        map.insert(
            "Naner".into(),
            r"%NANER_ROOT%\vendor\bin\naner.exe".to_string(),
        );
        map.insert(
            "Git".into(),
            r"%NANER_ROOT%\vendor\git\bin\git.exe".to_string(),
        );
        map.insert(
            "NodeJS".into(),
            r"%NANER_ROOT%\vendor\nodejs\node.exe".to_string(),
        );
        map
    }

    #[test]
    fn normalization_strips_launcher_extensions_only() {
        assert_eq!(normalize("Node.EXE"), "node");
        assert_eq!(normalize("gem.cmd"), "gem");
        assert_eq!(normalize("msys2_shell.cmd"), "msys2_shell");
        assert_eq!(normalize("my.tool"), "my.tool");
        assert_eq!(normalize("  npm "), "npm");
    }

    #[test]
    fn provides_entries_resolve_case_insensitively() {
        let all = catalog();
        let hit = find_provider("NPX", &all, &paths()).expect("npx maps to NodeJS");
        assert_eq!(hit.key, "NodeJS");
        let hit = find_provider("node.exe", &all, &paths()).unwrap();
        assert_eq!(hit.key, "NodeJS");
    }

    #[test]
    fn vendor_paths_fall_back_via_the_extract_dir_segment() {
        let all = catalog();
        // `git` has no provides entry; its VendorPaths value points into
        // vendor\git, which is GitForWindows' extractDir.
        let hit = find_provider("git", &all, &paths()).expect("git maps via VendorPaths");
        assert_eq!(hit.key, "GitForWindows");
    }

    /// `vendor\bin\naner.exe` is a VendorPaths entry that no vendor
    /// definition owns; it must resolve to nothing, not to whichever vendor
    /// happens to sort first.
    #[test]
    fn a_vendor_paths_entry_outside_any_vendor_dir_is_no_match() {
        let all = catalog();
        assert!(find_provider("naner", &all, &paths()).is_none());
    }

    #[test]
    fn unknown_names_and_empty_queries_are_no_match() {
        let all = catalog();
        assert!(find_provider("definitely-not-a-tool", &all, &paths()).is_none());
        assert!(find_provider("", &all, &paths()).is_none());
        assert!(find_provider(".exe", &all, &paths()).is_none());
    }

    #[test]
    fn hints_match_the_vendor_state() {
        let all = catalog();
        let node = &all[0]; // disabled, not installed (the shipped default)
        let hint = hint_for("node", node, false);
        assert!(hint.contains("\"enabled\": true"), "{hint}");
        assert!(hint.contains("naner install NodeJS"), "{hint}");

        let hint = hint_for("node", node, true);
        assert!(hint.contains("disabled"), "{hint}");
        assert!(!hint.contains("naner install"), "{hint}");

        let git = &all[1]; // enabled
        let hint = hint_for("git", git, false);
        assert!(hint.contains("naner install GitForWindows"), "{hint}");
        assert!(!hint.contains("\"enabled\""), "{hint}");

        let hint = hint_for("git", git, true);
        assert!(hint.contains("naner-launched"), "{hint}");
    }
}
