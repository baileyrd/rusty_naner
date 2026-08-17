//! Port of `InstallVendorsCommand` + `UpdateVendorsCommand` — the vendor
//! pipeline commands (MIGRATION_ANALYSIS §6 Phase 3), plus the additive
//! Unix-philosophy surface from §2.4 tier 2: `install --list --porcelain`
//! (one `name<TAB>installed<TAB>version` line per vendor, pure stdout) and
//! `--quiet` on both commands.

use naner_core::http::UreqHttp;
use naner_core::vendors::{
    UnifiedVendorInstaller, VendorConfigurationLoader, VendorDefinition,
    essential_vendor_definitions,
};
use naner_core::{constants, logger, paths};

const LIST_FLAG: &str = "--list";
const ALL_FLAG: &str = "--all";
const PORCELAIN_FLAG: &str = "--porcelain";
const QUIET_FLAG: &str = "--quiet";

/// `naner install ...`
pub fn execute_install(args: &[String]) -> i32 {
    let (args, quiet) = strip_quiet(args);
    logger::set_quiet(quiet);

    let Some(naner_root) = find_root_or_explain() else {
        return 1;
    };

    let loader = VendorConfigurationLoader::new(&naner_root);
    let all_vendors = loader.load_vendors();

    let first = args.first().map(|s| s.to_lowercase());
    match first.as_deref() {
        Some(LIST_FLAG) => {
            let porcelain = args.iter().any(|a| a.eq_ignore_ascii_case(PORCELAIN_FLAG));
            // Listing deliberately includes disabled vendors: hiding them
            // would make them undiscoverable, so a user could never find the
            // name to switch on. They are marked, and install refuses them.
            show_vendor_list(&loader, &loader.load_all_vendors(), porcelain)
        }
        Some(ALL_FLAG) => install_all_optional(&naner_root, &loader, all_vendors),
        None => {
            let every: Vec<VendorDefinition> = loader.load_all_vendors();
            let optional: Vec<&VendorDefinition> = every.iter().filter(|v| !v.required).collect();
            show_install_help(&optional);
            0
        }
        _ => install_specific(&naner_root, &loader, all_vendors, &args),
    }
}

/// `naner update-vendors`
pub fn execute_update(args: &[String]) -> i32 {
    let (_args, quiet) = strip_quiet(args);
    logger::set_quiet(quiet);

    logger::header("Updating Essential Vendors");
    logger::newline();

    let Some(naner_root) = find_root_or_explain() else {
        return 1;
    };

    logger::info(&format!("Naner Root: {}", naner_root.display()));
    logger::newline();

    // C# uses the hardcoded factory set for update-vendors (not vendors.json),
    // but honours the manifest's `enabled` -- see below.
    let vendors = enabled_essential_vendors(&VendorConfigurationLoader::new(&naner_root));
    if vendors.is_empty() {
        logger::warning("Every essential vendor is disabled in vendors.json; nothing to update.");
        return 0;
    }
    let http = UreqHttp::new();
    let installer = UnifiedVendorInstaller::new(&naner_root, vendors, &http);
    installer.update_all_vendors();

    logger::newline();
    merge_config_defaults(&naner_root);

    logger::newline();
    logger::success("Vendor updates completed!");
    0
}

/// Bring `config/naner.json` (or `.yaml`/`.yml`) and `config/vendors.json`
/// up to date with what this binary ships (see `config::merge` and
/// `vendors::config_merge`) -- the counterpart to `WindowsTerminalConfigurator`'s
/// `settings.json` merge, which already runs whenever Windows Terminal
/// itself gets installed or updated. This is the fix for a bare
/// `naner.exe`-swap upgrade never otherwise touching either file (#72).
fn merge_config_defaults(naner_root: &std::path::Path) {
    use naner_core::config::{
        NanerConfigMergeOutcome, find_configuration_file, merge_shipped_naner_defaults,
    };
    use naner_core::vendors::{VendorsMergeOutcome, merge_shipped_vendor_defaults};

    if let Some(config_path) = find_configuration_file(naner_root) {
        match merge_shipped_naner_defaults(&config_path) {
            Ok(NanerConfigMergeOutcome::Merged {
                added,
                refreshed,
                respected_deletions,
            }) => {
                if !added.is_empty() {
                    let mut line = format!(
                        "Added {} new naner.json default(s): {}",
                        added.len(),
                        added.join(", ")
                    );
                    if respected_deletions > 0 {
                        line.push_str(&format!(
                            " ({respected_deletions} left removed, as you left them)"
                        ));
                    }
                    logger::info(&line);
                }
                if !refreshed.is_empty() {
                    logger::info(&format!(
                        "Refreshed {} naner.json field(s) that still matched a prior shipped default: {}",
                        refreshed.len(),
                        refreshed.join(", ")
                    ));
                }
            }
            Ok(NanerConfigMergeOutcome::LeftUnparsed) => {
                logger::warning("    config/naner.json could not be parsed; left unchanged");
            }
            Ok(NanerConfigMergeOutcome::UpToDate | NanerConfigMergeOutcome::NoConfig) => {}
            Err(e) => {
                logger::warning(&format!("    Could not update config/naner.json: {e}"));
            }
        }
    }

    let vendors_path = naner_root
        .join(constants::directory_names::CONFIG)
        .join(constants::VENDORS_CONFIG_FILE_NAME);
    match merge_shipped_vendor_defaults(&vendors_path) {
        Ok(VendorsMergeOutcome::Added(keys)) => {
            logger::info(&format!(
                "Added {} new vendor definition(s) to vendors.json: {}",
                keys.len(),
                keys.join(", ")
            ));
        }
        Ok(VendorsMergeOutcome::LeftUnparsed) => {
            logger::warning("    config/vendors.json could not be parsed; left unchanged");
        }
        Ok(VendorsMergeOutcome::UpToDate | VendorsMergeOutcome::NoConfig) => {}
        Err(e) => {
            logger::warning(&format!("    Could not update config/vendors.json: {e}"));
        }
    }
}

/// The built-in essential set, minus anything `vendors.json` switches off.
///
/// `update-vendors` deliberately maintains a fixed set of *definitions* rather
/// than reading them from the manifest: those carry sources, asset globs and
/// fallback URLs that a user's `vendors.json` may be older than. But `enabled`
/// is the user's decision about what belongs on their machine, and a flag
/// honoured by `install` and ignored by `update-vendors` means nothing -- a
/// vendor switched off comes straight back on the next update, silently.
///
/// A manifest that cannot be read disables nothing. `load_all_vendors` falls
/// back to this same set when the file is missing, empty or unparseable, so
/// there is nothing to filter against; failing closed there would quietly stop
/// maintaining vendors the user actually has.
fn enabled_essential_vendors(loader: &VendorConfigurationLoader) -> Vec<VendorDefinition> {
    let disabled: Vec<String> = loader
        .load_all_vendors()
        .into_iter()
        .filter(|v| !v.enabled)
        .map(|v| v.key.to_lowercase())
        .collect();

    let (keep, skip): (Vec<_>, Vec<_>) = essential_vendor_definitions()
        .into_iter()
        .partition(|v| !disabled.contains(&v.key.to_lowercase()));

    // Say what was skipped. Silently doing less than asked is the same class of
    // problem as silently doing more.
    if !skip.is_empty() {
        let names: Vec<&str> = skip.iter().map(|v| v.name.as_str()).collect();
        logger::info(&format!(
            "Skipping (disabled in vendors.json): {}",
            names.join(", ")
        ));
    }
    keep
}

fn strip_quiet(args: &[String]) -> (Vec<String>, bool) {
    // Tier-3: auto-quiet in pipelines. Explicit --quiet still works in a
    // terminal; a redirected stdout suppresses the [*]/[OK]/info chatter on
    // its own. Failures and stderr warnings are unaffected, and porcelain
    // output prints directly (not via the logger).
    let quiet = args.iter().any(|a| a.eq_ignore_ascii_case(QUIET_FLAG))
        || !std::io::IsTerminal::is_terminal(&std::io::stdout());
    let rest = args
        .iter()
        .filter(|a| !a.eq_ignore_ascii_case(QUIET_FLAG))
        .cloned()
        .collect();
    (rest, quiet)
}

fn find_root_or_explain() -> Option<std::path::PathBuf> {
    match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(root) => Some(root),
        Err(err) => {
            logger::failure("Could not locate Naner root directory");
            logger::newline();
            println!("{}", err.message);
            logger::newline();
            logger::info("Please run this command from within your Naner installation,");
            logger::info("or run 'naner-init' first to set up Naner.");
            None
        }
    }
}

/// `ShowVendorList` — plus the machine-readable `--porcelain` variant.
fn show_vendor_list(
    loader: &VendorConfigurationLoader,
    all_vendors: &[VendorDefinition],
    porcelain: bool,
) -> i32 {
    if porcelain {
        for vendor in all_vendors {
            let state = if !vendor.enabled {
                "disabled"
            } else if loader.is_vendor_installed(vendor) {
                "installed"
            } else {
                "missing"
            };
            let version = loader.vendor_version(vendor).unwrap_or_default();
            println!("{}\t{}\t{}", vendor.name, state, version);
        }
        return 0;
    }

    logger::header("Available Vendors");
    logger::newline();

    let print_section = |label: &str, vendors: Vec<&VendorDefinition>| {
        if vendors.is_empty() {
            return;
        }
        println!("{label}");
        for vendor in vendors {
            let (status, color) = if !vendor.enabled {
                ("[--]", "90")
            } else if loader.is_vendor_installed(vendor) {
                ("[OK]", "92")
            } else {
                ("[ ]", "37")
            };
            println!(
                "\x1b[{color}m  {status} \x1b[0m{:<20} {}",
                vendor.name, vendor.description
            );
        }
        logger::newline();
    };
    print_section(
        "Essential (always installed):",
        all_vendors.iter().filter(|v| v.required).collect(),
    );
    print_section(
        "Optional:",
        all_vendors.iter().filter(|v| !v.required).collect(),
    );

    if all_vendors.iter().any(|v| !v.enabled) {
        println!("[--] is disabled in vendors.json - set \"enabled\": true to install it.");
    }
    println!("Use 'naner install <name>' to install a vendor.");
    println!("Use 'naner install --all' to install all optional vendors.");
    logger::newline();
    0
}

/// `InstallAllOptionalVendors`.
fn install_all_optional(
    naner_root: &std::path::Path,
    loader: &VendorConfigurationLoader,
    all_vendors: Vec<VendorDefinition>,
) -> i32 {
    logger::header("Installing All Optional Vendors");
    logger::newline();

    let to_install: Vec<VendorDefinition> = all_vendors
        .iter()
        .filter(|v| !v.required && !loader.is_vendor_installed(v))
        .cloned()
        .collect();

    if to_install.is_empty() {
        logger::success("All optional vendors are already installed!");
        return 0;
    }

    logger::status(&format!("Installing {} vendor(s)...", to_install.len()));
    logger::newline();

    let http = UreqHttp::new();
    let installer = UnifiedVendorInstaller::new(naner_root, all_vendors, &http);
    let mut failed = 0;
    for vendor in &to_install {
        if !install_with_dependencies(&installer, loader, vendor) {
            failed += 1;
        }
        logger::newline();
    }
    installer.cleanup_downloads();

    logger::newline();
    if failed == 0 {
        logger::success("All optional vendors installed successfully!");
    } else {
        logger::warning(&format!("Completed with {failed} failure(s)."));
    }
    if restart_hint_applies(to_install.len(), failed) {
        logger::info("Restart your terminal to use the newly installed tools.");
    }
    if failed > 0 { 1 } else { 0 }
}

/// Whether "restart your terminal" is true yet.
///
/// It is advice about a PATH that changed, so it only applies if something was
/// actually placed. When every vendor failed -- a checksum mismatch, say --
/// nothing changed, and telling someone to restart implies an install they did
/// not get.
///
/// Deliberately conservative on the mixed case where a named vendor failed but
/// one of its dependencies installed: this reports nothing rather than
/// guessing, since over-claiming is the failure mode being fixed.
fn restart_hint_applies(attempted: usize, failed: usize) -> bool {
    failed < attempted
}

/// `InstallSpecificVendors`.
fn install_specific(
    naner_root: &std::path::Path,
    loader: &VendorConfigurationLoader,
    all_vendors: Vec<VendorDefinition>,
    vendor_names: &[String],
) -> i32 {
    let mut to_install: Vec<VendorDefinition> = Vec::new();
    let mut not_found: Vec<&String> = Vec::new();
    let mut disabled = 0usize;

    for name in vendor_names {
        match loader.vendor_by_key(name) {
            Some(vendor) if vendor.enabled => to_install.push(vendor),
            // Present but switched off: say so, rather than "unknown vendor",
            // which would send the user looking for a typo.
            Some(vendor) => {
                logger::failure(&format!(
                    "{} is disabled in vendors.json (set \"enabled\": true to install it)",
                    vendor.name
                ));
                disabled += 1;
            }
            None => not_found.push(name),
        }
    }

    if !not_found.is_empty() {
        for name in &not_found {
            logger::failure(&format!("Unknown vendor: {name}"));
        }
    }
    if !not_found.is_empty() || disabled > 0 {
        logger::newline();
        logger::info("Use 'naner install --list' to see available vendors.");
        if to_install.is_empty() {
            return 1;
        }
        logger::newline();
    }

    let (already, needs): (Vec<_>, Vec<_>) = to_install
        .into_iter()
        .partition(|v| loader.is_vendor_installed(v));

    for vendor in &already {
        logger::info(&format!("{} is already installed", vendor.name));
    }
    if needs.is_empty() {
        if !already.is_empty() {
            logger::newline();
            logger::success("Nothing to install.");
        }
        return if disabled + not_found.len() > 0 { 1 } else { 0 };
    }
    if !already.is_empty() {
        logger::newline();
    }

    logger::header(&format!("Installing {} Vendor(s)", needs.len()));
    logger::newline();

    let http = UreqHttp::new();
    let installer = UnifiedVendorInstaller::new(naner_root, all_vendors, &http);
    let mut install_failed = 0;
    for vendor in &needs {
        if !install_with_dependencies(&installer, loader, vendor) {
            install_failed += 1;
        }
        logger::newline();
    }
    installer.cleanup_downloads();

    // Names that were unknown or disabled never reached the install loop, so
    // `install_failed` alone would under-report: `naner install Foo Bar` with
    // `Bar` unknown and `Foo` installed successfully must not print
    // "Installation completed successfully!" and exit 0 -- the user asked for
    // two vendors and got one.
    let total_failed = disabled + not_found.len() + install_failed;

    logger::newline();
    if total_failed == 0 {
        logger::success("Installation completed successfully!");
    } else {
        logger::warning(&format!("Completed with {total_failed} failure(s)."));
    }
    if restart_hint_applies(needs.len(), install_failed) {
        logger::info("Restart your terminal to use the newly installed tools.");
    }
    if total_failed > 0 { 1 } else { 0 }
}

/// `InstallVendorWithDependencies`: dependencies first (by key), then the
/// vendor itself.
fn install_with_dependencies(
    installer: &UnifiedVendorInstaller,
    loader: &VendorConfigurationLoader,
    vendor: &VendorDefinition,
) -> bool {
    // Dependencies install regardless of `enabled`: they were not chosen from
    // a menu, they are needed by something the user did choose, and failing the
    // install instead would be the larger surprise.
    for dep_key in &vendor.dependencies {
        if let Some(dep) = loader.vendor_by_key(dep_key)
            && !loader.is_vendor_installed(&dep)
        {
            logger::info(&format!("Installing dependency: {}", dep.name));
            if !installer.install_vendor(&dep.name) {
                logger::failure(&format!("Failed to install dependency: {}", dep.name));
                return false;
            }
        }
    }
    installer.install_vendor(&vendor.name)
}

/// `ShowInstallHelp`.
fn show_install_help(optional: &[&VendorDefinition]) {
    logger::header("Install Vendor Packages");
    logger::newline();

    println!("USAGE:");
    println!("  naner install [OPTIONS] [VENDOR...]");
    logger::newline();

    println!("OPTIONS:");
    println!("  --list                     List available vendors and status");
    println!("  --list --porcelain         Machine-readable list (name<TAB>status<TAB>version)");
    println!("  --all                      Install all optional vendors");
    println!("  --quiet                    Suppress progress chatter");
    logger::newline();

    println!("EXAMPLES:");
    println!("  naner install --list       # Show available vendors");
    println!("  naner install ruby         # Install Ruby");
    println!("  naner install nodejs go    # Install Node.js and Go");
    println!("  naner install --all        # Install all optional vendors");
    logger::newline();

    // Rendered from the loaded definitions rather than a literal. The literal
    // this replaced had drifted: it never gained rustyterm or rush, so the two
    // newest vendors were undiscoverable from `naner install` with no args.
    let enabled: Vec<String> = optional
        .iter()
        .filter(|v| v.enabled)
        .map(|v| v.key.to_lowercase())
        .collect();
    let disabled = optional.len() - enabled.len();

    println!("AVAILABLE VENDORS:");
    if enabled.is_empty() {
        println!("  (none enabled)");
    } else {
        println!("  {}", enabled.join(", "));
    }
    if disabled > 0 {
        println!(
            "  ({disabled} more disabled in vendors.json - 'naner install --list' shows them)"
        );
    }
    logger::newline();
}

#[cfg(test)]
mod tests {
    use super::{enabled_essential_vendors, restart_hint_applies};
    use naner_core::vendors::{VendorConfigurationLoader, essential_vendor_definitions};

    /// The bug: a checksum mismatch aborted the only install, and naner still
    /// said "Restart your terminal to use the newly installed tools." Nothing
    /// had been installed. Telling someone their PATH changed when it did not
    /// is the same over-claim as reporting success on a failure.
    #[test]
    fn nothing_installed_means_no_restart_advice() {
        assert!(!restart_hint_applies(1, 1), "the only vendor failed");
        assert!(!restart_hint_applies(3, 3), "every vendor failed");
    }

    #[test]
    fn a_partial_install_still_needs_a_restart() {
        assert!(restart_hint_applies(3, 1), "two of three landed");
    }

    /// The bug: `naner install SevenZip DisabledVendor` with `SevenZip`
    /// already installed took the "nothing to install" early return and
    /// reported success, even though `DisabledVendor` was never honoured.
    /// Two vendors were requested; only the exit code for the one that
    /// mattered got checked.
    #[test]
    fn already_installed_plus_a_disabled_name_still_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/vendors.json"),
            r#"{
                "vendors": {
                    "TestVendor": {
                        "name": "Test Vendor",
                        "description": "test",
                        "extractDir": "testvendor",
                        "enabled": true,
                        "required": false
                    },
                    "TestDisabled": {
                        "name": "Test Disabled",
                        "description": "test",
                        "extractDir": "testdisabled",
                        "enabled": false,
                        "required": false
                    }
                }
            }"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("vendor/testvendor")).unwrap();
        std::fs::write(dir.path().join("vendor/testvendor/marker"), "x").unwrap();

        let loader = VendorConfigurationLoader::new(dir.path());
        let all_vendors = loader.load_vendors();
        let code = super::install_specific(
            dir.path(),
            &loader,
            all_vendors,
            &["TestVendor".to_string(), "TestDisabled".to_string()],
        );
        assert_eq!(code, 1, "one of the two requested vendors was refused");
    }

    #[test]
    fn a_clean_run_advises_a_restart() {
        assert!(restart_hint_applies(2, 0));
    }

    /// `install --list` and an up-to-date tree both reach the summary with
    /// nothing attempted. No install, no advice.
    #[test]
    fn attempting_nothing_advises_nothing() {
        assert!(!restart_hint_applies(0, 0));
    }

    /// `update-vendors` used to install Rusty Term and Rush on every run,
    /// though `vendors.json` ships both `"enabled": false`. `install --list`
    /// showed them disabled and `install <name>` refused them, so the flag was
    /// honoured on one of the two paths that install things and ignored on the
    /// other.
    #[test]
    fn a_disabled_vendor_is_not_updated() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("vendors.json"),
            r#"{"vendors":{
                 "SevenZip":{"name":"7-Zip","extractDir":"7zip","enabled":true},
                 "RustyTerm":{"name":"Rusty Term","extractDir":"rusty_term","enabled":false},
                 "Rush":{"name":"Rush","extractDir":"rush","enabled":false}
               }}"#,
        )
        .unwrap();

        let kept = enabled_essential_vendors(&VendorConfigurationLoader::new(dir.path()));
        let keys: Vec<&str> = kept.iter().map(|v| v.key.as_str()).collect();

        assert!(keys.contains(&"SevenZip"));
        assert!(!keys.contains(&"RustyTerm"), "disabled vendor was updated");
        assert!(!keys.contains(&"Rush"), "disabled vendor was updated");
        // A vendor the manifest does not mention at all is still maintained --
        // absence is not a decision to switch something off.
        assert!(keys.contains(&"PowerShell"));
    }

    /// An unreadable manifest disables nothing. `load_all_vendors` falls back
    /// to this same built-in set, so there is nothing to filter against, and
    /// failing closed would silently stop maintaining vendors the user has.
    #[test]
    fn an_unreadable_manifest_disables_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let kept = enabled_essential_vendors(&VendorConfigurationLoader::new(dir.path()));
        assert_eq!(kept.len(), essential_vendor_definitions().len());
    }
}
