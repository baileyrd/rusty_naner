//! Command: `naner bench [profile]`
//! Startup latency profiler: measures execution timings for root discovery,
//! config loading, PATH assembly, and argument building in milliseconds.

use naner_core::{config, constants, logger, paths};
use std::time::Instant;

pub fn execute(args: &[String]) -> i32 {
    let profile_name = args.first().map(|s| s.as_str()).unwrap_or("Unified");

    logger::header("Naner Startup Latency Profiler");
    logger::newline();

    let start_total = Instant::now();

    let start_root = Instant::now();
    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };
    let root_elapsed = start_root.elapsed();

    let start_cfg = Instant::now();
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
    let cfg_elapsed = start_cfg.elapsed();

    let start_profile = Instant::now();
    let _profile = match cfg.get_profile(profile_name, true) {
        Ok(p) => p,
        Err(_) => {
            logger::failure(&format!("Profile not found: {profile_name}"));
            return 1;
        }
    };
    let profile_elapsed = start_profile.elapsed();

    let start_path = Instant::now();
    let _path_env = paths::build_unified_path(
        &cfg.environment.path_precedence,
        &naner_root.to_string_lossy(),
        cfg.advanced.inherit_system_path,
    );
    let path_elapsed = start_path.elapsed();

    let total_elapsed = start_total.elapsed();

    logger::status("Performance Benchmark Results:");
    println!(
        "  - Root Discovery:     {: >6.2?} ms",
        root_elapsed.as_secs_f64() * 1000.0
    );
    println!(
        "  - Config Load:         {: >6.2?} ms",
        cfg_elapsed.as_secs_f64() * 1000.0
    );
    println!(
        "  - Profile Resolution:  {: >6.2?} ms",
        profile_elapsed.as_secs_f64() * 1000.0
    );
    println!(
        "  - PATH Assembly:       {: >6.2?} ms",
        path_elapsed.as_secs_f64() * 1000.0
    );
    println!("  ----------------------------------------");
    println!(
        "  Total Setup Latency:   {: >6.2?} ms",
        total_elapsed.as_secs_f64() * 1000.0
    );

    logger::newline();
    logger::success("Benchmark complete.");
    0
}
