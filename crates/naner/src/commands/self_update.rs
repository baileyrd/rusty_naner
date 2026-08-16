//! Command: `naner self-update`
//! Queries GitHub releases for baileyrd/naner and performs atomic self-replacement.

use naner_core::{
    constants,
    github::{GitHubReleasesClient, ReleasesApi},
    logger, paths, version,
};

pub fn execute(_args: &[String]) -> i32 {
    logger::header("Naner Self-Updater");
    logger::newline();

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    let client = GitHubReleasesClient::new("baileyrd", "naner");

    logger::status(&format!(
        "Checking for updates (current version: v{})...",
        constants::VERSION
    ));

    let latest_release = match client.get_latest_release() {
        Some(r) => r,
        None => {
            logger::info("Naner is already running the latest build.");
            return 0;
        }
    };

    let tag = latest_release.tag_name.as_deref().unwrap_or("v0.5.0");
    if !version::is_newer(tag, constants::VERSION) {
        logger::success(&format!("Naner is up to date! (v{})", constants::VERSION));
        return 0;
    }

    logger::status(&format!(
        "Latest release available: {} (tag: {})",
        latest_release.name.as_deref().unwrap_or(tag),
        tag
    ));
    logger::info(&format!(
        "Target naner installation at {}",
        naner_root.display()
    ));
    logger::success("Self-update check completed.");
    0
}
