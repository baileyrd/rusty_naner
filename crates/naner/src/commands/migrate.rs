//! Command: `naner config migrate`
//! Upgrades legacy configuration files to the latest canonical JSON schema and formats them cleanly.

use naner_core::{config, constants, logger, paths};
use std::fs;

pub fn execute(_args: &[String]) -> i32 {
    logger::header("Naner Configuration Auto-Migration");
    logger::newline();

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    let cfg_file = match config::find_configuration_file(&naner_root) {
        Some(p) => p,
        None => {
            logger::failure("Configuration file not found");
            return 1;
        }
    };

    let cfg = match config::load(&naner_root, Some(&cfg_file)) {
        Ok(c) => c,
        Err(err) => {
            logger::failure(&format!("Configuration parse error: {err}"));
            return 1;
        }
    };

    logger::info(&format!(
        "Source configuration file: {}",
        cfg_file.display()
    ));

    let target_json_path = naner_root
        .join(constants::directory_names::CONFIG)
        .join("naner.json");
    let json_output = serde_json::to_string_pretty(&cfg).unwrap();

    if let Err(err) = fs::write(&target_json_path, json_output) {
        logger::failure(&format!("Failed to write migrated configuration: {err}"));
        return 1;
    }

    logger::success(&format!(
        "Configuration successfully migrated and canonicalized to {}",
        target_json_path.display()
    ));
    0
}
