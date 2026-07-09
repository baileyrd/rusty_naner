//! Port of `DiagnosticsCommand` + `DiagnosticsService` and its three
//! services (`EnvironmentReporter`, `DirectoryVerifier`,
//! `ConfigurationVerifier`), output preserved verbatim.

use std::path::Path;

use naner_core::{config, constants, logger, paths};

pub fn execute() -> i32 {
    logger::header("Naner Diagnostics");
    logger::newline();

    // 1. Executable info.
    report_executable_info();

    // 2. Find NANER_ROOT.
    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(root) => {
            logger::success(&format!("Naner Root: {}", root.display()));
            logger::newline();
            root
        }
        Err(err) => {
            logger::failure("Could not locate Naner root directory");
            logger::newline();
            println!("{}", err.message);
            logger::newline();
            logger::info("Please run this from within your Naner installation,");
            logger::info("or run 'naner init' first to set up Naner.");
            return 1;
        }
    };

    // 3. Directory structure.
    verify_directories(&naner_root);

    // 4. Configuration.
    verify_configuration(&naner_root);

    // 5. Environment variables.
    report_environment();

    logger::newline();
    logger::success("Diagnostics complete!");
    0
}

fn report_executable_info() {
    logger::status("Executable Information:");
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    logger::info(&format!("  Location: {exe_dir}"));
    let command_line: Vec<String> = std::env::args().collect();
    logger::info(&format!("  Command Line: {}", command_line.join(" ")));
    logger::newline();
}

/// `DirectoryVerifier.Verify`: the *essential* four (incl. `home`), each
/// with a colored ✓/✗ line.
fn verify_directories(naner_root: &Path) {
    logger::status("Verifying directory structure:");
    for dir in constants::directory_names::ESSENTIAL {
        let exists = naner_root.join(dir).is_dir();
        let (symbol, color) = if exists { ("✓", "92") } else { ("✗", "91") };
        println!("\x1b[{color}m  {symbol} {dir}/\x1b[0m");
    }
    logger::newline();
}

/// `ConfigurationVerifier.Verify`.
fn verify_configuration(naner_root: &Path) {
    match config::find_configuration_file(naner_root) {
        Some(config_path) => {
            logger::success("Configuration file found");
            logger::info(&format!("  Path: {}", config_path.display()));
            match config::load(naner_root, Some(&config_path)) {
                Ok(cfg) => {
                    logger::info(&format!("  Default Profile: {}", cfg.default_profile));
                    logger::info(&format!("  Vendor Paths: {}", cfg.vendor_paths.len()));
                    logger::info(&format!("  Profiles: {}", cfg.profiles.len()));
                    logger::newline();
                    verify_vendor_paths(&cfg);
                }
                Err(err) => {
                    logger::failure(&format!("Configuration error: {err}"));
                }
            }
        }
        None => {
            logger::failure(
                "Configuration file missing. Expected one of: naner.json, naner.yaml, or naner.yml in config/",
            );
        }
    }
    logger::newline();
}

fn verify_vendor_paths(cfg: &config::NanerConfig) {
    logger::status("Vendor Paths:");
    for vendor_key in ["WindowsTerminal", "PowerShell", "GitBash"] {
        if let Some(vendor_path) = cfg.vendor_paths.get(vendor_key) {
            let exists = Path::new(vendor_path).is_file();
            let (symbol, color) = if exists { ("✓", "92") } else { ("✗", "91") };
            println!("\x1b[{color}m  {symbol} {vendor_key}: {vendor_path}\x1b[0m");
        }
    }
}

/// `EnvironmentReporter.Report`.
fn report_environment() {
    logger::status("Environment Variables:");
    for env_var in ["NANER_ROOT", "NANER_ENVIRONMENT", "HOME", "PATH"] {
        match std::env::var(env_var) {
            Ok(mut value) => {
                if env_var == "PATH" {
                    let cut = value
                        .char_indices()
                        .nth(100)
                        .map(|(i, _)| i)
                        .unwrap_or(value.len());
                    value.truncate(cut);
                    value.push_str("...");
                }
                logger::info(&format!("  {env_var}={value}"));
            }
            Err(_) => logger::info(&format!("  {env_var}=(not set)")),
        }
    }
}
