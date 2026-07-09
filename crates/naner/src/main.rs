//! `naner.exe` — the launcher. Port of `Naner.Launcher.Program`: console
//! decision → command router → first-run check → launch options → launcher.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod cli;
mod commands;
mod first_run;
mod launcher;

use std::path::Path;

use clap::Parser;
// naner-core re-exports its IndexMap so binaries don't need a second direct
// dependency on the crate.
use naner_core::config::IndexMap;
use naner_core::{config, console, constants, env_export, logger, paths};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Console decision, exactly as Program.Main: router console commands OR
    // first run OR --debug OR --export-env anywhere in the args.
    let needs_console = console::arg_needs_console(&args, &commands::names::CONSOLE_COMMANDS)
        || first_run::is_first_run()
        || args.iter().any(|a| a.to_lowercase() == "--debug")
        || args.iter().any(|a| a.to_lowercase() == "--export-env");
    let _console = console::setup(needs_console);

    // Layer 1: router verbs.
    if let Some(code) = commands::route(&args) {
        std::process::exit(code);
    }

    // First-run gate.
    if first_run::is_first_run() {
        std::process::exit(handle_first_run());
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

    std::process::exit(run_launcher(&opts));
}

fn run_launcher(opts: &cli::LaunchOptions) -> i32 {
    if opts.version {
        commands::version::show_short();
        return 0;
    }

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
        return handle_export_env(&cfg, &unified_path, &opts.format, opts.no_comments);
    }

    // --setup-only: environment configured, done.
    if opts.setup_only {
        logger::debug(
            "Setup-only mode: environment configured, exiting without launching terminal",
            opts.debug,
        );
        return 0;
    }

    // 4–5. Pick the profile and launch.
    let profile_name = opts
        .profile
        .clone()
        .unwrap_or_else(|| cfg.default_profile.clone());
    logger::debug(&format!("Selected profile: {profile_name}"), opts.debug);

    if !quiet {
        logger::newline();
    }
    let terminal = launcher::TerminalLauncher::new(&naner_root, &cfg, opts.debug);
    terminal.launch_profile(&profile_name, opts.directory.as_deref())
}

/// `PathResolver.SetupEnvironment`: NANER_ROOT, NANER_ENVIRONMENT, and (iff
/// `home/` exists) HOME + NANER_HOME on the process environment.
fn setup_environment(naner_root: &Path, environment: &str) {
    // SAFETY: single-threaded launcher flow.
    unsafe {
        std::env::set_var("NANER_ROOT", naner_root);
        std::env::set_var("NANER_ENVIRONMENT", environment);
        let home_path = naner_root.join(constants::directory_names::HOME);
        if home_path.is_dir() {
            std::env::set_var("HOME", &home_path);
            std::env::set_var("NANER_HOME", &home_path);
        }
    }
}

/// `Program.HandleExportEnv`: NANER_ROOT/NANER_ENVIRONMENT/NANER_HOME/HOME
/// first, then every configured variable read back from the process env;
/// output trimmed with no trailing newline (pipeline safety).
fn handle_export_env(
    cfg: &config::NanerConfig,
    unified_path: &str,
    format: &str,
    no_comments: bool,
) -> i32 {
    let shell_format = match env_export::parse_format(format) {
        Ok(f) => f,
        Err(err) => {
            logger::failure(&err.to_string());
            return 1;
        }
    };

    let mut env_vars: IndexMap<String, String> = IndexMap::new();
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

    let output = env_export::export(&env_vars, unified_path, shell_format, no_comments);
    print!("{}", output.trim_end());
    0
}

/// `Program.HandleFirstRun`, output verbatim; exits 0 like the C#.
fn handle_first_run() -> i32 {
    let info = first_run::get_first_run_info();

    logger::header("First Run Detected");
    println!();

    println!("The following issues were detected:");
    println!();
    for message in &info.messages {
        println!("  • {message}");
    }
    if let Some(root) = &info.naner_root {
        println!();
        println!("  Checked location: {}", root.display());
    }
    println!();

    println!("Please run 'naner-init' to initialize Naner.");
    println!();
    println!("naner-init provides:");
    println!("  • Automatic download of latest Naner from GitHub");
    println!("  • Automatic updates when new versions are available");
    println!("  • Simpler setup process");
    println!();

    0
}
