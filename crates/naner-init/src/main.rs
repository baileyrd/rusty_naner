//! `naner-init.exe` — the bootstrapper/updater. Port of `Naner.Init.Program`
//! (MIGRATION_ANALYSIS §6 Phase 4): route `init`/`update`/`check-update`/
//! `--version`/`--help`, pass everything else through to naner.exe, and on a
//! bare launch check initialization + version sync first.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod updater;

use std::io::{BufRead, Write};
use std::path::PathBuf;

use naner_core::console::{self, ConsoleState};
use naner_core::github::GitHubReleasesClient;
use naner_core::http::UreqHttp;
use naner_core::vendors::{UnifiedVendorInstaller, essential_vendor_definitions};
use naner_core::{constants, logger, paths};
use updater::NanerUpdater;

/// `InitCommandNames.ConsoleCommands`.
const CONSOLE_COMMANDS: [&str; 7] = [
    "--version",
    "-v",
    "--help",
    "-h",
    "init",
    "update",
    "check-update",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut state = console::setup(console::arg_needs_console(&args, &CONSOLE_COMMANDS));

    // Root discovery with the init-specific fallback: current directory
    // (a fresh double-click in an empty folder is the install-here case).
    let naner_root = paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH)
        .map(|r| r.to_path_buf())
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let github = GitHubReleasesClient::new(constants::github::OWNER, constants::github::REPO);

    let code = match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("--version") | Some("-v") => {
            println!("naner-init {}", constants::VERSION);
            0
        }
        Some("--help") | Some("-h") => {
            show_help();
            0
        }
        Some("init") => initialize_naner(&naner_root, &github, state),
        Some("update") => update_naner(&naner_root, &github),
        Some("check-update") => check_for_updates(&naner_root, &github),
        // Anything else (including nothing) is the default flow with
        // pass-through arguments.
        _ => run_default_flow(&naner_root, &github, &args, &mut state),
    };

    std::process::exit(code);
}

/// `RunDefaultFlowAsync`: first-run prompt+bootstrap, else silent update
/// check (notification only), then pass-through launch.
fn run_default_flow(
    naner_root: &std::path::Path,
    github: &GitHubReleasesClient,
    args: &[String],
    state: &mut ConsoleState,
) -> i32 {
    let updater = NanerUpdater::new(naner_root, github);

    if !updater.is_initialized() {
        ensure_console(state);

        logger::header("Naner Initializer");
        logger::newline();
        logger::info("Naner is not initialized yet.");
        logger::info(&format!(
            "This will download Naner v{} from GitHub.",
            updater.target_version()
        ));
        logger::newline();

        if !prompt_yes("Initialize Naner now? (Y/n): ") {
            logger::info("Initialization cancelled.");
            wait_for_key_before_exit(*state);
            return 0;
        }

        if !updater.initialize() {
            wait_for_key_before_exit(*state);
            return 1;
        }

        download_essentials(naner_root);

        logger::newline();
        logger::info("Additional development tools can be installed later.");
        logger::info("Run 'naner update-vendors' to update vendor tools.");
        logger::newline();
        logger::success("Naner is ready!");
        logger::newline();

        if prompt_yes("Launch Naner now? (Y/n): ") {
            return updater.launch_naner(args);
        }
        logger::info("Run 'naner' or 'naner-init' to launch Naner later.");
        return 0;
    }

    // Silent update check — a failure here must never block the launch.
    let (update_available, latest_version) = updater.check_for_update();
    if update_available && latest_version.is_some() {
        ensure_console(state);
        logger::warning(&format!(
            "A new version of Naner is available: {}",
            latest_version.as_deref().unwrap_or_default()
        ));
        logger::info("Run 'naner-init update' to update");
        logger::newline();
    }

    updater.launch_naner(args)
}

/// `InitializeNanerAsync` (`naner-init init`).
fn initialize_naner(
    naner_root: &std::path::Path,
    github: &GitHubReleasesClient,
    state: ConsoleState,
) -> i32 {
    let updater = NanerUpdater::new(naner_root, github);

    if updater.is_initialized() {
        logger::warning("Naner is already initialized.");
        logger::info(&format!(
            "Current version: {}",
            updater.installed_version().unwrap_or_default()
        ));
        logger::info("Use 'naner-init update' to update to the latest version.");
        return 0;
    }

    if !updater.initialize() {
        wait_for_key_before_exit(state);
        return 1;
    }

    download_essentials(naner_root);

    logger::newline();
    logger::info("Additional development tools can be installed later.");
    logger::info("Run 'naner update-vendors' to update vendor tools.");
    logger::newline();
    logger::success("Naner is ready!");
    logger::newline();

    if prompt_yes("Launch Naner now? (Y/n): ") {
        return updater.launch_naner(&[]);
    }
    logger::info("Run 'naner' or 'naner-init' to launch Naner later.");
    0
}

/// `UpdateNanerAsync` (`naner-init update`).
fn update_naner(naner_root: &std::path::Path, github: &GitHubReleasesClient) -> i32 {
    let updater = NanerUpdater::new(naner_root, github);

    if !updater.is_initialized() {
        logger::failure("Naner is not initialized yet.");
        logger::info("Run 'naner-init' to initialize first.");
        return 1;
    }

    logger::info(&format!(
        "Current version: {}",
        updater.installed_version().unwrap_or_default()
    ));

    let (update_available, latest_version) = updater.check_for_update();
    if !update_available {
        logger::success("Naner is already up to date!");
        return 0;
    }

    logger::info(&format!(
        "Latest version: {}",
        latest_version.as_deref().unwrap_or_default()
    ));
    logger::newline();

    if !prompt_yes("Update now? (Y/n): ") {
        logger::info("Update cancelled.");
        return 0;
    }

    if updater.update_naner_exe() { 0 } else { 1 }
}

/// `CheckForUpdatesAsync` (`naner-init check-update`).
fn check_for_updates(naner_root: &std::path::Path, github: &GitHubReleasesClient) -> i32 {
    let updater = NanerUpdater::new(naner_root, github);

    if !updater.is_initialized() {
        logger::failure("Naner is not initialized yet.");
        return 1;
    }

    logger::info(&format!(
        "Current version: {}",
        updater.installed_version().unwrap_or_default()
    ));

    let (update_available, latest_version) = updater.check_for_update();
    if update_available && latest_version.is_some() {
        logger::warning(&format!(
            "Update available: {}",
            latest_version.as_deref().unwrap_or_default()
        ));
        logger::info("Run 'naner-init update' to update");
        return 0;
    }

    logger::success("Naner is up to date!");
    0
}

/// `EssentialVendorDownloader.DownloadAllEssentialsAsync`: the fixed
/// bootstrap order — 7-Zip first (it unblocks the other extractions).
fn download_essentials(naner_root: &std::path::Path) {
    logger::newline();
    logger::info("Setting up essential dependencies...");
    logger::newline();

    logger::info("Downloading essential vendors...");
    logger::newline();

    let http = UreqHttp::new();
    let installer = UnifiedVendorInstaller::new(naner_root, essential_vendor_definitions(), &http);

    let mut success = true;
    for name in [
        constants::vendor_names::SEVEN_ZIP,
        constants::vendor_names::POWERSHELL,
        constants::vendor_names::WINDOWS_TERMINAL,
        constants::vendor_names::MSYS2,
    ] {
        success &= installer.install_vendor(name);
        logger::newline();
    }
    installer.cleanup_downloads();

    if success {
        logger::success("All essential vendors installed successfully!");
    } else {
        logger::warning("Some vendors failed to install, but Naner may still function");
    }
}

/// Interactive prompts accept empty/`y`/`yes` (trimmed, case-insensitive)
/// as yes.
fn prompt_yes(question: &str) -> bool {
    print!("{question}");
    let _ = std::io::stdout().flush();
    let mut response = String::new();
    let _ = std::io::stdin().lock().read_line(&mut response);
    let normalized = response.trim().to_lowercase();
    normalized.is_empty() || normalized == "y" || normalized == "yes"
}

/// `WaitForKeyBeforeExit`: only when we allocated a fresh console (the
/// double-click case) — never when attached to a parent shell.
fn wait_for_key_before_exit(state: ConsoleState) {
    if state.allocated() {
        logger::newline();
        println!("Press any key to exit...");
        let _ = std::io::stdin().lock().read_line(&mut String::new());
    }
}

/// `EnsureConsoleAttached` for the mid-flow cases (first-run prompts, update
/// notification): only attempts console work when none was done yet.
fn ensure_console(state: &mut ConsoleState) {
    if *state == ConsoleState::Inherited {
        *state = console::setup(true);
    }
}

/// `ShowHelp`, output verbatim.
fn show_help() {
    println!("Naner Initializer - Standalone launcher for Naner");
    println!();
    println!("USAGE:");
    println!("  naner-init                Launch Naner (auto-initialize if needed)");
    println!("  naner-init init           Initialize Naner (download from GitHub)");
    println!("  naner-init update         Update Naner to the latest version");
    println!("  naner-init check-update   Check if an update is available");
    println!("  naner-init [args]         Pass arguments to naner.exe");
    println!();
    println!("OPTIONS:");
    println!("  --version, -v             Show version information");
    println!("  --help, -h                Show this help message");
    println!();
    println!("EXAMPLES:");
    println!("  naner-init                # Launch Naner");
    println!("  naner-init Unified        # Launch Naner with Unified profile");
    println!("  naner-init --version      # Show version");
    println!("  naner-init update         # Update to latest version");
    println!();
    println!("VENDOR MANAGEMENT:");
    println!("  Use 'naner update-vendors' to update PowerShell, Terminal, etc.");
}
