//! Commands: `naner init`, `naner update`, `naner check-update` — and the
//! interactive first-run flow the bare launcher enters on an uninitialized
//! tree. Absorbed from the retired `naner-init.exe`; the flows are its
//! `InitializeNanerAsync` / `UpdateNanerAsync` / `CheckForUpdatesAsync` and
//! `RunDefaultFlowAsync`'s first-run half, with "run naner-init" messaging
//! replaced by the binary they now live in.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use naner_core::console::{self, ConsoleState};
use naner_core::github::GitHubReleasesClient;
use naner_core::http::UreqHttp;
use naner_core::updater::NanerUpdater;
use naner_core::vendors::{UnifiedVendorInstaller, essential_vendor_definitions};
use naner_core::{constants, logger, paths, version};

/// Root discovery with the bootstrap-specific fallback: the current
/// directory. A fresh binary double-clicked in an empty folder is the
/// install-here case — the launcher's loud root-discovery failure is the
/// wrong answer for the one flow whose whole point is that no root exists
/// yet.
pub fn root_or_cwd() -> PathBuf {
    paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// `naner init`.
pub fn execute_init(state: ConsoleState) -> i32 {
    if let Some(code) = reexec_in_own_console_if_racy(state) {
        return code;
    }
    let naner_root = root_or_cwd();
    let github = GitHubReleasesClient::new(constants::github::OWNER, constants::github::REPO);
    let updater = NanerUpdater::new(&naner_root, &github);

    if updater.is_initialized() {
        logger::warning("Naner is already initialized.");
        logger::info(&format!(
            "Current version: {}",
            updater.installed_version().unwrap_or_default()
        ));
        logger::info("Use 'naner update' to update to the latest version.");
        return 0;
    }

    run_bootstrap(&updater, &naner_root, state)
}

/// The interactive first-run flow: prompt, bundle download by embedded tag,
/// essentials bootstrap, optional launch. Shared by `naner init` and the
/// bare launcher's uninitialized path.
pub fn run_bootstrap(updater: &NanerUpdater, naner_root: &Path, state: ConsoleState) -> i32 {
    logger::header("Naner Initializer");
    logger::newline();
    logger::info("Naner is not initialized yet.");
    logger::info(&format!(
        "This will download Naner v{} from GitHub into {}.",
        updater.target_version(),
        naner_root.display()
    ));
    logger::newline();

    if !prompt_yes("Initialize Naner now? (Y/n): ") {
        logger::info("Initialization cancelled.");
        wait_for_key_before_exit(state);
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

    offer_add_to_path(naner_root);

    if prompt_yes("Launch Naner now? (Y/n): ") {
        return updater.launch_naner(&[]);
    }
    logger::info("Run 'naner' to launch Naner later.");
    wait_for_key_before_exit(state);
    0
}

/// `naner update` (and `naner self-update`, its alias): update every copy of
/// the binary to the latest published release.
pub fn execute_update(state: ConsoleState) -> i32 {
    if let Some(code) = reexec_in_own_console_if_racy(state) {
        return code;
    }
    let naner_root = root_or_cwd();
    let github = GitHubReleasesClient::new(constants::github::OWNER, constants::github::REPO);
    let updater = NanerUpdater::new(&naner_root, &github);

    if !updater.is_initialized() {
        logger::failure("Naner is not initialized yet.");
        logger::info("Run 'naner init' to initialize first.");
        return 1;
    }

    let installed = updater.installed_version().unwrap_or_default();
    logger::info(&format!("Current version: {installed}"));

    logger::status("Checking the latest release...");
    let Some(release) = updater.fetch_latest() else {
        return 1;
    };
    let latest = release.tag_name.clone().unwrap_or_default();

    // Up to date means the tree AND this binary both match the latest
    // release: a stale binary next to a current version file still needs
    // the update.
    let current = version::canonical(&latest) == version::canonical(&installed)
        && version::canonical(&latest) == version::canonical(updater.target_version());
    if current {
        logger::success("Naner is already up to date!");
        return 0;
    }

    logger::info(&format!("Latest version: {latest}"));
    logger::newline();

    if !prompt_yes("Update now? (Y/n): ") {
        logger::info("Update cancelled.");
        return 0;
    }

    let Ok(self_path) = std::env::current_exe() else {
        logger::failure("Could not determine this executable's own path");
        return 1;
    };
    let code = if updater.update_from_release(&release, &self_path) {
        0
    } else {
        1
    };
    // In a console of our own the window closes with the process; hold it
    // open so the outcome is readable.
    wait_for_key_before_exit(state);
    code
}

/// `naner check-update`.
pub fn execute_check_update() -> i32 {
    let naner_root = root_or_cwd();
    let github = GitHubReleasesClient::new(constants::github::OWNER, constants::github::REPO);
    let updater = NanerUpdater::new(&naner_root, &github);

    if !updater.is_initialized() {
        logger::failure("Naner is not initialized yet.");
        return 1;
    }

    let installed = updater.installed_version().unwrap_or_default();
    logger::info(&format!("Current version: {installed}"));

    let Some(release) = updater.fetch_latest() else {
        return 1;
    };
    let latest = release.tag_name.unwrap_or_default();

    if version::canonical(&latest) != version::canonical(&installed)
        || version::canonical(&latest) != version::canonical(updater.target_version())
    {
        logger::warning(&format!("Update available: {latest}"));
        logger::info("Run 'naner update' to update");
        return 0;
    }

    logger::success("Naner is up to date!");
    0
}

/// The fixed bootstrap order — 7-Zip first (it unblocks the other
/// extractions).
fn download_essentials(naner_root: &Path) {
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
        constants::vendor_names::GIT_FOR_WINDOWS,
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

/// Offer PATH setup right after a successful install, so `naner` resolves
/// from any new shell without a second command. Declining is not a dead end
/// — the standalone `naner add-to-path` covers it later — and a registry
/// failure does not fail the bootstrap: the install itself is already done
/// and working. Windows-only; elsewhere there is no user PATH to edit.
#[cfg(windows)]
fn offer_add_to_path(naner_root: &Path) {
    if prompt_yes("Add naner to your PATH so 'naner' works in any new shell? (Y/n): ") {
        super::add_to_path::add(naner_root);
    } else {
        logger::info("Run 'naner add-to-path' to add it later.");
    }
    logger::newline();
}

#[cfg(not(windows))]
fn offer_add_to_path(_naner_root: &Path) {}

/// Interactive prompts accept a bare Enter/`y`/`yes` (trimmed,
/// case-insensitive) as yes.
///
/// EOF is NO. A bare Enter arrives as `"\n"` — one byte — where a closed
/// stdin reads zero bytes. Treating those the same made any non-interactive
/// spawn of the bare binary in an empty directory silently *consent* to
/// downloading and installing a full naner tree, which is how a CI test
/// first caught this. That silent EOF-is-no path is by design for scripted
/// use (`Command::output()` closes stdin, and this must stay quiet there),
/// so the diagnostics below only fire inside naner's own relaunched console
/// (`OWN_CONSOLE_ENV`) -- the one case where an instant EOF is *not*
/// expected, because that console exists specifically to be interactive.
fn prompt_yes(question: &str) -> bool {
    print!("{question}");
    let _ = std::io::stdout().flush();
    let own_console = std::env::var_os(OWN_CONSOLE_ENV).is_some();
    if !console::refresh_std_handles() && own_console {
        logger::warning("  Could not refresh this console's input/output before the prompt above");
    }
    let mut response = String::new();
    match std::io::stdin().lock().read_line(&mut response) {
        Ok(0) => {
            if own_console {
                logger::warning(
                    "  stdin read EOF immediately at the prompt above, inside naner's own \
                     console -- input from the keyboard isn't reaching this process; \
                     treating as \"no\"",
                );
            }
            return false;
        }
        Err(e) => {
            if own_console {
                logger::warning(&format!(
                    "  stdin read failed at the prompt above: {e} -- treating as \"no\""
                ));
            }
            return false;
        }
        Ok(_) => {}
    }
    let normalized = response.trim().to_lowercase();
    normalized.is_empty() || normalized == "y" || normalized == "yes"
}

/// Marker the re-exec sets so the child knows its console window is its own
/// -- it should pause before exit exactly as if it had allocated one, and it
/// must never re-exec again.
const OWN_CONSOLE_ENV: &str = "NANER_OWN_CONSOLE";

/// The #81 keystroke race, closed in code instead of documentation: neither
/// `cmd.exe` nor PowerShell waits for a GUI-subsystem process, so a prompt
/// read from the parent shell's console competes with the shell's own next
/// prompt for keystrokes — the flow looks hung, or worse, half the input
/// lands in each reader. `ConsoleState::Attached` is exactly that situation.
/// Interactive flows call this first: when attached, relaunch the same
/// command in a console of its own — where nothing competes — wait, and
/// mirror the exit code. Piped/redirected stdio (`Redirected`) never
/// re-execs, so scripted and CI use is unaffected.
#[cfg(windows)]
pub fn reexec_in_own_console_if_racy(state: ConsoleState) -> Option<i32> {
    if state != ConsoleState::Attached || std::env::var_os(OWN_CONSOLE_ENV).is_some() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();

    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    logger::info("Opening a console of naner's own for the prompts...");
    match std::process::Command::new(exe)
        .args(&args)
        .env(OWN_CONSOLE_ENV, "1")
        .creation_flags(CREATE_NEW_CONSOLE)
        .status()
    {
        Ok(status) => Some(status.code().unwrap_or(1)),
        Err(e) => {
            // Could not spawn: fall through and run inline -- racy beats
            // broken. Silent about *why* would be its own bug: without
            // this, the prompt below looks identical to the pre-#81 race
            // (types with no effect) and gives no clue what happened or how
            // to work around it.
            let joined = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            logger::warning(&format!("  Could not open a console of naner's own: {e}"));
            logger::warning(&format!(
                "  Continuing here -- if the next prompt doesn't respond to Y/n, run: \
                 Start-Process -Wait naner.exe -ArgumentList \"{joined}\""
            ));
            None
        }
    }
}

#[cfg(not(windows))]
pub fn reexec_in_own_console_if_racy(_state: ConsoleState) -> Option<i32> {
    None
}

/// Only when the console window is naner's own — freshly allocated
/// (double-click) or opened by the re-exec above — never when attached to a
/// parent shell that survives us.
pub fn wait_for_key_before_exit(state: ConsoleState) {
    if state.allocated() || std::env::var_os(OWN_CONSOLE_ENV).is_some() {
        logger::newline();
        println!("Press any key to exit...");
        console::wait_for_keypress();
    }
}

/// Attach or allocate a console mid-flow (the update notification on a
/// double-clicked launch) — only attempts console work when none was done
/// yet.
pub fn ensure_console(state: &mut ConsoleState) {
    if *state == ConsoleState::Inherited {
        *state = console::setup(true);
    }
}
