//! Command: `naner diff [profile]`
//! Displays a side-by-side diff comparing the current process environment
//! against the environment constructed by naner for the target profile.

use naner_core::{config, constants, logger, paths};

pub fn execute(args: &[String]) -> i32 {
    let profile_name = args.first().map(|s| s.as_str()).unwrap_or("Unified");

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

    let profile = match cfg.get_profile(profile_name, true) {
        Ok(p) => p,
        Err(_) => {
            logger::failure(&format!("Profile not found: {profile_name}"));
            return 1;
        }
    };

    logger::header(&format!("Environment Diff: {}", profile.name));
    logger::newline();

    logger::status("Profile Environment Variables:");
    for (k, v) in &cfg.environment.environment_variables {
        let expanded = paths::expand_naner_path(v, &naner_root.to_string_lossy());
        if let Ok(current) = std::env::var(k) {
            if current != expanded {
                println!("\x1b[93m ~ {k} = \"{expanded}\" (current: \"{current}\")\x1b[0m");
            } else {
                println!("   {k} = \"{expanded}\"");
            }
        } else {
            println!("\x1b[92m + {k} = \"{expanded}\"\x1b[0m");
        }
    }

    logger::newline();
    logger::success("Environment diff complete.");
    0
}
