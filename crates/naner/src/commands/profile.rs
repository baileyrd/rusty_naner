//! Command: `naner profile [export|import|list]`
//! Manages profile export and import for seamless sharing across developer setups.

use std::fs;
use std::path::Path;

use naner_core::{config, constants, logger, paths};
use serde_json::json;

pub fn execute(args: &[String]) -> i32 {
    let action = args
        .first()
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "list".to_string());

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    let cfg_file = config::find_configuration_file(&naner_root);
    let cfg = match cfg_file
        .as_ref()
        .and_then(|f| config::load(&naner_root, Some(f)).ok())
    {
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
            }))
            .unwrap();

            if let Some(pos) = args.iter().position(|a| a == "--out" || a == "-o")
                && let Some(out_path) = args.get(pos + 1)
            {
                if let Err(err) = fs::write(Path::new(out_path), &out_json) {
                    logger::failure(&format!("Failed to write profile export: {err}"));
                    return 1;
                }
                logger::success(&format!("Exported profile '{profile_name}' to {out_path}"));
                return 0;
            }

            println!("{out_json}");
            0
        }
        "import" => {
            let Some(import_path) = args.get(1) else {
                eprintln!("Usage: naner profile import <file.json> [--as <name>] [--dry-run]");
                return 1;
            };
            let dry_run = args.iter().any(|a| a.eq_ignore_ascii_case("--dry-run"));
            let rename = args
                .iter()
                .position(|a| a.eq_ignore_ascii_case("--as"))
                .and_then(|i| args.get(i + 1))
                .cloned();

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

            let imported: config::ProfileConfig = if let Some(p) = val.get("Profile") {
                match serde_json::from_value(p.clone()) {
                    Ok(parsed) => parsed,
                    Err(e) => {
                        logger::failure(&format!("Invalid Profile object: {e}"));
                        return 1;
                    }
                }
            } else if let Ok(parsed) = serde_json::from_value::<config::ProfileConfig>(val.clone())
            {
                parsed
            } else {
                logger::failure(
                    "Import JSON must contain a 'Profile' object or valid Profile fields",
                );
                return 1;
            };

            let key = rename
                .or_else(|| (!imported.name.is_empty()).then(|| imported.name.clone()))
                .unwrap_or_else(|| "ImportedProfile".to_string());

            // Merge into the file as written, not the loaded config: the
            // loaded one carries environment overrides and expanded paths that
            // must never be written back (same reason as `migrate`).
            let Some(cfg_file) = config::find_configuration_file(&naner_root) else {
                logger::failure("Configuration file not found");
                return 1;
            };
            let mut on_disk = match config::load_verbatim(&cfg_file) {
                Ok(c) => c,
                Err(err) => {
                    logger::failure(&format!("Configuration parse error: {err}"));
                    return 1;
                }
            };

            let replacing =
                on_disk.profiles.contains_key(&key) || on_disk.custom_profiles.contains_key(&key);
            // Imports land in CustomProfiles so a built-in of the same name is
            // never silently overwritten in place.
            on_disk.custom_profiles.insert(key.clone(), imported);

            let target = naner_root
                .join(constants::directory_names::CONFIG)
                .join("naner.json");
            let rendered = match serde_json::to_string_pretty(&on_disk) {
                Ok(s) => format!("{s}\n"),
                Err(err) => {
                    logger::failure(&format!("Could not serialize configuration: {err}"));
                    return 1;
                }
            };

            if replacing {
                logger::warning(&format!("Replacing existing profile '{key}'"));
            }
            if dry_run {
                logger::info("Dry run — nothing written. Result would be:");
                print!("{rendered}");
                return 0;
            }
            if let Err(err) = crate::config_file::replace(&target, &rendered) {
                logger::failure(&format!("Failed to write configuration: {err}"));
                return 1;
            }
            logger::success(&format!(
                "Imported profile '{key}' into {}",
                target.display()
            ));
            0
        }
        other => {
            eprintln!("Unknown profile action '{other}'. Supported: list, export, import");
            1
        }
    }
}
