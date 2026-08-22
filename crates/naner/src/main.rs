//! `naner.exe` — the launcher. Port of `Naner.Launcher.Program`: console
//! decision → command router → first-run check → launch options → launcher.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod cli;
mod commands;
mod config_file;
mod first_run;
mod launcher;

use std::path::Path;

use clap::Parser;
// naner-core re-exports its OrderedMap so binaries don't need a second
// direct dependency on the collections module path.
use naner_core::config::OrderedMap;
use naner_core::{config, console, constants, env_export, logger, paths};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // A self-update renames the running binary aside rather than deleting it
    // (Windows will rename a running exe but not remove it). Sweep that
    // leftover now, when this process is the new binary and the old file is
    // no longer running. Best effort: a locked or absent file is fine.
    if let Ok(me) = std::env::current_exe() {
        let mut old = me.file_name().unwrap_or_default().to_os_string();
        old.push(".old");
        let _ = std::fs::remove_file(me.with_file_name(old));
    }

    let machine_readable = is_machine_readable(&args);

    // Console decision, exactly as Program.Main: router console commands OR
    // first run OR --debug OR --export-env anywhere in the args.
    let needs_console = console::arg_needs_console(&args, &commands::names::CONSOLE_COMMANDS)
        || first_run::is_first_run()
        || args.iter().any(|a| a.to_lowercase() == "--debug")
        || machine_readable;
    let mut console_state = console::setup(needs_console);

    // Layer 1: router verbs.
    if let Some(code) = commands::route(&args, console_state) {
        std::process::exit(code);
    }

    // First-run gate. Machine-readable invocations keep the terse notice --
    // an eval'd `--export-env` must never block on a prompt -- and a tree
    // that is damaged rather than absent (marker present, essentials
    // missing) keeps the diagnostic notice too. A genuinely uninitialized
    // tree gets the interactive bootstrap the retired naner-init.exe used
    // to own: this binary in an empty folder IS the installer.
    if first_run::is_first_run() {
        if machine_readable {
            std::process::exit(handle_first_run(machine_readable));
        }
        let naner_root = commands::bootstrap::root_or_cwd();
        let github = naner_core::github::GitHubReleasesClient::new(
            constants::github::OWNER,
            constants::github::REPO,
        );
        let updater = naner_core::updater::NanerUpdater::new(&naner_root, &github);
        if !updater.is_initialized() {
            commands::bootstrap::ensure_console(&mut console_state);
            if let Some(code) = commands::bootstrap::reexec_in_own_console_if_racy(console_state) {
                std::process::exit(code);
            }
            std::process::exit(commands::bootstrap::run_bootstrap(
                &updater,
                &naner_root,
                console_state,
            ));
        }
        std::process::exit(handle_first_run(machine_readable));
    }

    // Layer 2: launch options.
    let opts = match cli::LaunchOptions::try_parse_from(
        std::iter::once("naner".to_string()).chain(args.iter().cloned()),
    ) {
        Ok(opts) => opts,
        Err(err) => {
            // CommandLineParser printed its errors and returned 1; clap's
            // message stands in for that text.
            let _ = err.print();
            std::process::exit(1);
        }
    };

    std::process::exit(run_launcher(&opts, &mut console_state));
}

fn run_launcher(opts: &cli::LaunchOptions, console_state: &mut console::ConsoleState) -> i32 {
    if opts.version {
        commands::version::show_short();
        return 0;
    }

    // Captured before anything below can redirect USERPROFILE: the real
    // Windows profile directory, for Advanced.HomeJunctions' %HOST_USERPROFILE%
    // targets to bridge back out to. Once naner's own EnvironmentVariables
    // loop runs, this process's own $env:USERPROFILE no longer says it.
    let host_userprofile = std::env::var("USERPROFILE").ok();

    // Quiet unless debugging — the launcher's normal path prints nothing.
    let quiet = !opts.debug;

    if !quiet {
        logger::header("Naner Terminal Launcher");
        logger::debug(&format!("Version: {}", constants::VERSION), opts.debug);
        logger::debug(&format!("Phase: {}", constants::PHASE_NAME), opts.debug);
    }

    // 1. Find NANER_ROOT.
    logger::debug("Finding Naner root directory...", opts.debug);
    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(root) => root,
        Err(err) => {
            logger::failure("Could not locate Naner root directory");
            logger::newline();
            println!("{}", err.message);
            return 1;
        }
    };
    if !quiet {
        logger::success(&format!("Naner Root: {}", naner_root.display()));
    }

    // Update notice, absorbed from the retired naner-init.exe: a purely
    // local comparison (version file vs this binary's embedded version), so
    // launches stay offline. Skipped for --export-env: its stdout is an
    // eval-able script and this is the one launcher path a pipeline reads.
    if !opts.export_env {
        let github = naner_core::github::GitHubReleasesClient::new(
            constants::github::OWNER,
            constants::github::REPO,
        );
        let updater = naner_core::updater::NanerUpdater::new(&naner_root, &github);
        let (update_available, latest) = updater.check_for_update();
        if update_available && latest.is_some() {
            commands::bootstrap::ensure_console(console_state);
            logger::warning(&format!(
                "A new version of Naner is available: {}",
                latest.as_deref().unwrap_or_default()
            ));
            logger::info("Run 'naner update' to update");
            logger::newline();
        }
    }

    // 2. Load configuration.
    let config_path = opts.config_path.as_deref().map(Path::new);
    logger::debug(
        &format!(
            "Loading configuration from: {}",
            opts.config_path.as_deref().unwrap_or("auto-discover")
        ),
        opts.debug,
    );
    let cfg = match config::load(&naner_root, config_path) {
        Ok(cfg) => cfg,
        Err(config::ConfigError::NotFound) => {
            logger::failure(&format!(
                "File not found: {}",
                config::ConfigError::NotFound
            ));
            return 1;
        }
        Err(err @ config::ConfigError::LegacyYaml(_)) => {
            logger::failure(&err.to_string());
            return 1;
        }
        Err(err) => {
            logger::failure(&format!("Fatal error: {err}"));
            return 1;
        }
    };
    if !quiet {
        logger::success("Configuration loaded");
    }
    logger::debug(
        &format!("Default profile: {}", cfg.default_profile),
        opts.debug,
    );

    // 3. Environment setup (process env, inherited by the terminal).
    //
    // Additive (no C# counterpart): Advanced.IsolateEnvironment clears
    // everything not on env_isolation::KEEP_ON_ISOLATE from *this* process
    // before anything below sets NANER_ROOT/configured vars/PATH, so a
    // spawned terminal (which inherits this process's env, not the host's)
    // can't see tools a prior system install left on it.
    let isolated_vars = if cfg.advanced.isolate_environment {
        if !quiet {
            logger::status("Isolating environment (Advanced.IsolateEnvironment)...");
        }
        naner_core::env_isolation::clear_host_environment()
    } else {
        Vec::new()
    };
    if !quiet {
        logger::status("Setting up environment...");
    }
    setup_environment(&naner_root, &opts.environment);
    // SAFETY: single-threaded at this point; no concurrent env access.
    unsafe {
        for (key, value) in &cfg.environment.environment_variables {
            std::env::set_var(key, value);
        }
    }
    let unified_path = paths::build_unified_path(
        &cfg.environment.path_precedence,
        &naner_root.to_string_lossy(),
        cfg.advanced.inherit_system_path,
    );
    unsafe { std::env::set_var("PATH", &unified_path) };
    if !quiet {
        logger::success("Environment configured");
    }

    if opts.debug {
        logger::debug(
            &format!(
                "NANER_ROOT={}",
                std::env::var("NANER_ROOT").unwrap_or_default()
            ),
            true,
        );
        logger::debug(
            &format!(
                "NANER_ENVIRONMENT={}",
                std::env::var("NANER_ENVIRONMENT").unwrap_or_default()
            ),
            true,
        );
        let cut = unified_path
            .char_indices()
            .nth(150)
            .map(|(i, _)| i)
            .unwrap_or(unified_path.len());
        logger::debug(
            &format!("PATH (first 150 chars)={}...", &unified_path[..cut]),
            true,
        );
    }

    // --export-env: print the eval-able script and exit.
    if opts.export_env {
        return handle_export_env(
            &cfg,
            &unified_path,
            &opts.format,
            opts.no_comments,
            &isolated_vars,
        );
    }

    // Advanced.HomeJunctions: bridge specific real Windows locations back
    // out from underneath the USERPROFILE redirect above. A one-time
    // filesystem side effect (the junction persists after this), skipped
    // for --export-env since that path must stay a pure script generator.
    naner_core::home_junctions::ensure_home_junctions(
        &naner_root,
        &cfg.advanced.home_junctions,
        host_userprofile.as_deref(),
    );

    // --setup-only: environment configured, done.
    if opts.setup_only {
        logger::debug(
            "Setup-only mode: environment configured, exiting without launching terminal",
            opts.debug,
        );
        return 0;
    }

    // 4–5. Pick the profile and launch.
    let explicit = opts.profile.is_some();
    let profile_name = opts
        .profile
        .clone()
        .unwrap_or_else(|| cfg.default_profile.clone());
    logger::debug(&format!("Selected profile: {profile_name}"), opts.debug);

    if !quiet {
        logger::newline();
    }
    let terminal = launcher::TerminalLauncher::new(&naner_root, &cfg, opts.debug);
    terminal.launch_profile(&profile_name, explicit, opts.directory.as_deref())
}

/// `PathResolver.SetupEnvironment`: NANER_ROOT, NANER_ENVIRONMENT, and (iff
/// `home/` exists) HOME + NANER_HOME on the process environment.
///
/// Also ensures `home/.tmp` and `home/AppData/{Roaming,Local}` exist, unlike
/// the XDG cache/data dirs naner.json also points into `home/` -- those are
/// created by whichever tool first writes to them (the XDG spec requires
/// it), but there's no equivalent contract for TEMP/TMP or APPDATA/
/// LOCALAPPDATA. A real Windows profile always has all four; naner.json now
/// redirects them into naner's own tree, so naner itself has to uphold that
/// same guarantee.
fn setup_environment(naner_root: &Path, environment: &str) {
    // SAFETY: single-threaded launcher flow.
    unsafe {
        std::env::set_var("NANER_ROOT", naner_root);
        std::env::set_var("NANER_ENVIRONMENT", environment);
        let home_path = naner_root.join(constants::directory_names::HOME);
        if home_path.is_dir() {
            std::env::set_var("HOME", &home_path);
            std::env::set_var("NANER_HOME", &home_path);
            let _ = std::fs::create_dir_all(naner_root.join(constants::directory_names::TEMP));
            let _ = std::fs::create_dir_all(
                naner_root.join(constants::directory_names::APPDATA_ROAMING),
            );
            let _ =
                std::fs::create_dir_all(naner_root.join(constants::directory_names::APPDATA_LOCAL));
        }
    }
}

/// `Program.HandleExportEnv`: NANER_ROOT/NANER_ENVIRONMENT/NANER_HOME/HOME
/// first, then every configured variable read back from the process env;
/// output trimmed with no trailing newline (pipeline safety). `isolated_vars`
/// (additive, no C# counterpart) asks the emitted script to also unset those
/// names in the *calling* shell, for `Advanced.IsolateEnvironment` -- needed
/// because a profile launched straight from Windows Terminal's own list runs
/// this through `--export-env | Invoke-Expression` in an already-environed
/// pwsh, never through naner's own isolated process.
fn handle_export_env(
    cfg: &config::NanerConfig,
    unified_path: &str,
    format: &str,
    no_comments: bool,
    isolated_vars: &[String],
) -> i32 {
    let shell_format = match env_export::parse_format(format) {
        Ok(f) => f,
        Err(err) => {
            logger::failure(&err.to_string());
            return 1;
        }
    };

    let mut env_vars: OrderedMap<String> = OrderedMap::new();
    for key in ["NANER_ROOT", "NANER_ENVIRONMENT", "NANER_HOME", "HOME"] {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            env_vars.insert(key.to_string(), value);
        }
    }
    for key in cfg.environment.environment_variables.keys() {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            env_vars.insert(key.clone(), value);
        }
    }

    let output = env_export::export_at(
        &env_vars,
        unified_path,
        shell_format,
        no_comments,
        isolated_vars,
        &naner_core::timestamp::now_local(),
    );
    print!("{}", output.trim_end());
    0
}

/// True when this invocation's stdout is a program rather than a message.
///
/// Only `--export-env` qualifies on the launcher path: the console verbs that
/// also produce machine-readable output (`root`, anything `--porcelain`) are
/// dispatched by `commands::route` before the first-run gate is reached.
fn is_machine_readable(args: &[String]) -> bool {
    args.iter().any(|a| a.eq_ignore_ascii_case("--export-env"))
}

/// `Program.HandleFirstRun`. Text preserved verbatim; stream and exit code are
/// not.
///
/// The notice goes to **stderr**. `--export-env` writes a shell program to
/// stdout, meant for `Invoke-Expression` or `eval`, and the first-run gate
/// fires before the launcher arguments are parsed — so on an uninitialized
/// tree this prose was being handed to the calling shell to execute.
///
/// The exit code stays `0` for an interactive first run, which is deliberate
/// C# parity and the double-click case. For a machine-readable invocation it
/// is `1`: nothing was exported, and reporting success there tells a wrapper
/// the environment is set up when it is not.
fn handle_first_run(machine_readable: bool) -> i32 {
    eprint!("{}", first_run_notice(&first_run::get_first_run_info()));
    i32::from(machine_readable)
}

/// The notice as text, so its content is testable without capturing a stream.
fn first_run_notice(info: &first_run::FirstRunInfo) -> String {
    let mut out = logger::header_text("First Run Detected");
    out.push('\n');

    out.push_str("The following issues were detected:\n\n");
    for message in &info.messages {
        out.push_str(&format!("  - {message}\n"));
    }
    if let Some(root) = &info.naner_root {
        out.push_str(&format!("\n  Checked location: {}\n", root.display()));
    }
    out.push('\n');

    out.push_str("Please run 'naner-init' to initialize Naner.\n\n");
    out.push_str("naner-init provides:\n");
    out.push_str("  - Automatic download of latest Naner from GitHub\n");
    out.push_str("  - Automatic updates when new versions are available\n");
    out.push_str("  - Simpler setup process\n\n");

    out
}
