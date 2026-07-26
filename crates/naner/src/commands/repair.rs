//! Command: `naner repair`
//! Diagnostic and self-healing engine: cleans broken download staging,
//! purges corrupt vendor folders, and re-executes essential vendor bootstrap.

use std::fs;
use naner_core::{constants, logger, paths, vendors};

pub fn execute(_args: &[String]) -> i32 {
    logger::header("Naner Environment Repair & Self-Healing");
    logger::newline();

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    let downloads_dir = naner_root.join(constants::directory_names::VENDOR).join(constants::directory_names::DOWNLOADS);
    if downloads_dir.is_dir() {
        logger::status("Cleaning leftover download staging...");
        if let Err(err) = fs::remove_dir_all(&downloads_dir) {
            logger::warning(&format!("Could not remove transient downloads dir: {err}"));
        } else {
            logger::success("Download staging cleaned.");
        }
    }

    let staging_dir = naner_root.join(constants::directory_names::VENDOR).join(".staging");
    if staging_dir.is_dir() {
        logger::status("Cleaning uncommitted staging directories...");
        let _ = fs::remove_dir_all(&staging_dir);
        logger::success("Staging directories cleaned.");
    }

    logger::status("Verifying essential directory structure...");
    for dir in constants::directory_names::ESSENTIAL {
        let p = naner_root.join(dir);
        if !p.is_dir() {
            logger::info(&format!("Recreating missing essential directory: {dir}/"));
            let _ = fs::create_dir_all(&p);
        }
    }

    logger::status("Inspecting essential vendor installations...");
    let loader = vendors::VendorConfigurationLoader::new(&naner_root);
    let essential_defs = vendors::essential_vendor_definitions();
    let http = naner_core::http::UreqHttp::new();
    let installer = vendors::UnifiedVendorInstaller::new(&naner_root, essential_defs.clone(), &http);

    let mut repaired_count = 0;
    for v in &essential_defs {
        if !loader.is_vendor_installed(v) && v.required {
            logger::info(&format!("Re-bootstrapping essential vendor: {}", v.name));
            if installer.install_vendor(&v.name) {
                repaired_count += 1;
            } else {
                logger::warning(&format!("Failed to repair vendor: {}", v.name));
            }
        }
    }

    logger::newline();
    logger::success(&format!("Repair scan complete. {repaired_count} vendor(s) restored."));
    0
}
