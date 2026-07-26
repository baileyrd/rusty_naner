//! Command: `naner profile [export|import|list]`
//! Manages profile export and import for seamless sharing across developer setups.

use std::fs;
use std::path::Path;

use naner_core::{config, constants, logger, paths};
use serde_json::json;

pub fn execute(args: &[String]) -> i32 {
    let action = args.first().map(|s| s.to_lowercase()).unwrap_or_else(|| "list".to_string());

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    let cfg_file = config::find_configuration_file(&naner_root);
    let cfg = match cfg_file.as_ref().and_then(|f| config::load(&naner_root, Some(f)).ok()) {
        Some(c) => c,
        None => {
            logger::failure("Could not load naner configuration file");
            return 1;
        }
    };

    match action.as_str() {
        "list" => {
            logger::header("Configured Profiles");
            for (key, p) in &cfg.profiles {
                println!("  - {key}: {}", p.name);
            }
            0
        }
        "export" => {
            let profile_name = match args.get(1) {
                Some(name) => name,
                None => {
                    eprintln!("Usage: naner profile export <profile_name> [--out <file.json>]");
                    return 1;
                }
            };

            let profile = match cfg.get_profile(profile_name, true) {
                Ok(p) => p,
                Err(_) => {
                    logger::failure(&format!("Profile not found: {profile_name}"));
                    return 1;
                }
            };

            let out_json = serde_json::to_string_pretty(&json!({
                "Profile": profile
            })).unwrap();

            if let Some(pos) = args.iter().position(|a| a == "--out" || a == "-o") {
                if let Some(out_path) = args.get(pos + 1) {
                    if let Err(err) = fs::write(Path::new(out_path), &out_json) {
                        logger::failure(&format!("Failed to write profile export: {err}"));
                        return 1;
                    }
                    logger::success(&format!("Exported profile '{profile_name}' to {out_path}"));
                    return 0;
                }
            }

            println!("{out_json}");
            0
        }
        "import" => {
            let import_path = match args.get(1) {
                Some(path) => path,
                None => {
                    eprintln!("Usage: naner profile import <file.json>");
                    return 1;
                }
            };

            let content = match fs::read_to_string(Path::new(import_path)) {
                Ok(c) => c,
                Err(err) => {
                    logger::failure(&format!("Failed to read import file: {err}"));
                    return 1;
                }
            };

            let val: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(err) => {
                    logger::failure(&format!("Invalid JSON in import file: {err}"));
                    return 1;
                }
            };

            let imported_profile: config::ProfileConfig = if let Some(p) = val.get("Profile") {
                match serde_json::from_value(p.clone()) {
                    Ok(parsed) => parsed,
                    Err(e) => {
                        logger::failure(&format!("Invalid Profile object: {e}"));
                        return 1;
                    }
                }
            } else if let Ok(parsed) = serde_json::from_value::<config::ProfileConfig>(val.clone()) {
                parsed
            } else {
                logger::failure("Import JSON must contain a 'Profile' object or valid Profile fields");
                return 1;
            };

            let key = if !imported_profile.name.is_empty() {
                imported_profile.name.clone()
            } else {
                "ImportedProfile".to_string()
            };

            let target_cfg_path = cfg_file.unwrap_or_else(|| naner_root.join("config").join("naner.json"));
            logger::success(&format!("Validated imported profile '{key}'. Target config: {}", target_cfg_path.display()));
            0
        }
        other => {
            eprintln!("Unknown profile action '{other}'. Supported: list, export, import");
            1
        }
    }
}
