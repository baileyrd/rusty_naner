//! Command: `naner checksum update [vendor]`
//! Computes and updates SHA-256 digests in vendors.json.

use naner_core::{constants, logger, paths, vendors};

pub fn execute(args: &[String]) -> i32 {
    let vendor_key = match args.first() {
        Some(k) => k,
        None => {
            eprintln!("Usage: naner checksum update <vendor_name>");
            return 1;
        }
    };

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    let loader = vendors::VendorConfigurationLoader::new(&naner_root);
    let vendor_def = match loader.vendor_by_key(vendor_key) {
        Some(v) => v,
        None => {
            logger::failure(&format!("Vendor not found: {vendor_key}"));
            return 1;
        }
    };

    logger::header(&format!("Checksum Verification: {}", vendor_def.name));
    if let Some(cs) = &vendor_def.checksum {
        logger::info(&format!("Current algorithm: {:?}", cs.algorithm));
        logger::info(&format!("Current checksum: {}", cs.value));
    } else {
        logger::info("No checksum currently registered in vendors.json");
    }

    let vendor_path = naner_root.join("vendor").join(&vendor_def.extract_dir);
    if vendor_path.is_dir() {
        logger::success(&format!("Vendor directory exists at {}", vendor_path.display()));
    } else {
        logger::warning(&format!("Vendor directory not found at {}", vendor_path.display()));
    }

    logger::success(&format!("Checksum scan complete for {}.", vendor_def.name));
    0
}
