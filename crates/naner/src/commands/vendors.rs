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

    // C# uses the hardcoded factory set for update-vendors (not vendors.json).
    let vendors = essential_vendor_definitions();
    let http = UreqHttp::new();
    let installer = UnifiedVendorInstaller::new(&naner_root, vendors, &http);
    installer.update_all_vendors();

    logger::newline();
    logger::success("Vendor updates completed!");
    0
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
    logger::info("Restart your terminal to use the newly installed tools.");
    if failed > 0 { 1 } else { 0 }
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
        return 0;
    }
    if !already.is_empty() {
        logger::newline();
    }

    logger::header(&format!("Installing {} Vendor(s)", needs.len()));
    logger::newline();

    let http = UreqHttp::new();
    let installer = UnifiedVendorInstaller::new(naner_root, all_vendors, &http);
    let mut failed = 0;
    for vendor in &needs {
        if !install_with_dependencies(&installer, loader, vendor) {
            failed += 1;
        }
        logger::newline();
    }
    installer.cleanup_downloads();

    logger::newline();
    if failed == 0 {
        logger::success("Installation completed successfully!");
    } else {
        logger::warning(&format!("Completed with {failed} failure(s)."));
    }
    logger::info("Restart your terminal to use the newly installed tools.");
    if failed > 0 { 1 } else { 0 }
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
