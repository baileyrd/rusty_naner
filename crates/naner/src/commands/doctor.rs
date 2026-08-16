//! Additive diagnostic command: `naner doctor` (or `naner --doctor`).
//! Inspects NANER_ROOT, vendor availability, PATH conflicts, and profiles.
//! Supports `--porcelain` for machine-readable JSON output compatible with
//! terminal side-channels (such as rusty_term's l13 / MCP protocol).

use naner_core::{config, constants, logger, paths, vendors};
use serde_json::json;

pub fn execute(args: &[String]) -> i32 {
    let porcelain = args
        .iter()
        .any(|a| a.eq_ignore_ascii_case("--porcelain") || a.eq_ignore_ascii_case("-p"));

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(root) => root,
        Err(err) => {
            if porcelain {
                println!(
                    "{}",
                    json!({
                        "status": "error",
                        "error": err.message,
                        "naner_root": null
                    })
                );
            } else {
                logger::failure("Could not locate Naner root directory");
                println!("{}", err.message);
            }
            return 1;
        }
    };

    let vendors_loader = vendors::VendorConfigurationLoader::new(&naner_root);
    let vendor_defs = vendors_loader.load_vendors();

    let mut vendor_status = Vec::new();
    let vendor_dir = naner_root.join(constants::directory_names::VENDOR);

    for v in &vendor_defs {
        let path = vendor_dir.join(&v.extract_dir);
        let exists = path.is_dir();
        vendor_status.push((v.name.clone(), v.extract_dir.clone(), exists));
    }

    let config_path = config::find_configuration_file(&naner_root);
    let config_ok = config_path
        .as_ref()
        .is_some_and(|p| config::load(&naner_root, Some(p)).is_ok());

    if porcelain {
        let json_output = json!({
            "status": "ok",
            "naner_root": naner_root.display().to_string(),
            "config_valid": config_ok,
            "config_path": config_path.map(|p| p.display().to_string()),
            "vendors": vendor_status.iter().map(|(name, dir, installed)| {
                json!({
                    "name": name,
                    "directory": dir,
                    "installed": installed
                })
            }).collect::<Vec<_>>()
        });
        println!("{}", json_output);
        return 0;
    }

    logger::header("Naner Environment Doctor");
    logger::newline();
    logger::success(&format!("Naner Root: {}", naner_root.display()));

    logger::status("Vendor Installation Status:");
    for (name, dir, installed) in &vendor_status {
        let (symbol, color) = if *installed { ("+", "92") } else { ("x", "91") };
        println!("\x1b[{color}m  {symbol} {name} (vendor/{dir})\x1b[0m");
    }
    logger::newline();

    let check_conflicts = args
        .iter()
        .any(|a| a.eq_ignore_ascii_case("--conflicts") || a.eq_ignore_ascii_case("-c"));

    if check_conflicts {
        logger::status("Scanning PATH for binary collisions...");
        if let Ok(path_env) = std::env::var("PATH") {
            let mut seen = std::collections::HashMap::new();
            for dir in std::env::split_paths(&path_env) {
                if dir.is_dir()
                    && let Ok(entries) = std::fs::read_dir(&dir)
                {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_file() {
                            let name = p.file_name().unwrap().to_string_lossy().to_lowercase();
                            seen.entry(name)
                                .or_insert_with(Vec::new)
                                .push(p.display().to_string());
                        }
                    }
                }
            }
            // Sorted so a truncated view is the same list every run — a
            // HashMap's iteration order is arbitrary, so `.take(5)` used to
            // show an arbitrary five.
            let mut conflicts: Vec<_> = seen
                .into_iter()
                .filter(|(_, paths)| paths.len() > 1)
                .collect();
            conflicts.sort_by(|a, b| a.0.cmp(&b.0));

            const SHOWN: usize = 5;
            if conflicts.is_empty() {
                logger::success("No binary PATH collisions detected.");
            } else {
                let shown = conflicts.len().min(SHOWN);
                logger::warning(&format!(
                    "Found {} binary collisions (showing {shown}):",
                    conflicts.len()
                ));
                for (name, paths) in conflicts.iter().take(SHOWN) {
                    logger::info(&format!("  - {name}:"));
                    for p in paths {
                        logger::info(&format!("      -> {p}"));
                    }
                }
            }
        }
    }

    logger::newline();
    logger::success("Doctor check complete!");
    0
}
