//! Port of the *live* parts of `FirstRunDetector` (`IsFirstRun` /
//! `GetFirstRunInfo`). The interactive setup wizard and `EstablishNanerRoot`
//! are dead code and deliberately not ported (MIGRATION_ANALYSIS §3).

use std::path::{Path, PathBuf};

use naner_core::{constants, paths};

#[derive(Debug, Default)]
pub struct FirstRunInfo {
    pub is_first_run: bool,
    pub naner_root: Option<PathBuf>,
    pub missing_directories: Vec<String>,
    pub messages: Vec<String>,
}

/// `FirstRunDetector.IsFirstRun`: true when the root can't be found, the
/// init marker is missing, an *essential* directory (incl. `home` — stricter
/// than the root markers) is missing, or no config file exists.
pub fn is_first_run() -> bool {
    let Ok(root) = paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) else {
        return true;
    };
    is_first_run_at(&root)
}

/// Testable core against an explicit root.
pub fn is_first_run_at(naner_root: &Path) -> bool {
    if !naner_root
        .join(constants::INITIALIZATION_MARKER_FILE)
        .is_file()
    {
        return true;
    }
    for dir in constants::directory_names::ESSENTIAL {
        if !naner_root.join(dir).is_dir() {
            return true;
        }
    }
    let config_dir = naner_root.join(constants::directory_names::CONFIG);
    if !constants::CONFIG_FILE_NAMES
        .iter()
        .any(|name| config_dir.join(name).is_file())
    {
        return true;
    }
    false
}

/// `FirstRunDetector.GetFirstRunInfo`, messages preserved verbatim.
pub fn get_first_run_info() -> FirstRunInfo {
    match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(root) => get_first_run_info_at(&root),
        Err(_) => FirstRunInfo {
            is_first_run: true,
            naner_root: None,
            missing_directories: Vec::new(),
            messages: vec!["Could not locate Naner installation directory (NANER_ROOT)".into()],
        },
    }
}

/// Testable core against an explicit root.
pub fn get_first_run_info_at(naner_root: &Path) -> FirstRunInfo {
    let mut info = FirstRunInfo {
        naner_root: Some(naner_root.to_path_buf()),
        ..Default::default()
    };

    if !naner_root
        .join(constants::INITIALIZATION_MARKER_FILE)
        .is_file()
    {
        info.messages.push(format!(
            "Initialization marker file not found: {}",
            constants::INITIALIZATION_MARKER_FILE
        ));
    }

    for dir in constants::directory_names::ESSENTIAL {
        if !naner_root.join(dir).is_dir() {
            info.missing_directories.push(dir.to_string());
        }
    }
    if !info.missing_directories.is_empty() {
        info.messages.push(format!(
            "Missing essential directories: {}",
            info.missing_directories.join(", ")
        ));
    }

    let config_dir = naner_root.join(constants::directory_names::CONFIG);
    let has_config = config_dir.is_dir()
        && constants::CONFIG_FILE_NAMES
            .iter()
            .any(|name| config_dir.join(name).is_file());
    if !has_config {
        info.messages.push(format!(
            "No configuration file found (supported: {})",
            constants::CONFIG_FILE_NAMES.join(", ")
        ));
    }

    info.is_first_run = !info.messages.is_empty();
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_tree() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for dir in ["bin", "vendor", "config", "home"] {
            std::fs::create_dir_all(tmp.path().join(dir)).unwrap();
        }
        std::fs::write(tmp.path().join(".naner-initialized"), "ok").unwrap();
        std::fs::write(tmp.path().join("config/naner.json"), "{}").unwrap();
        tmp
    }

    #[test]
    fn complete_tree_is_not_first_run() {
        let tmp = full_tree();
        assert!(!is_first_run_at(tmp.path()));
        assert!(!get_first_run_info_at(tmp.path()).is_first_run);
    }

    #[test]
    fn missing_marker_triggers_first_run() {
        let tmp = full_tree();
        std::fs::remove_file(tmp.path().join(".naner-initialized")).unwrap();
        assert!(is_first_run_at(tmp.path()));
        let info = get_first_run_info_at(tmp.path());
        assert!(
            info.messages
                .contains(&"Initialization marker file not found: .naner-initialized".to_string())
        );
    }

    #[test]
    fn missing_home_triggers_first_run_but_not_root_discovery() {
        // The root markers are bin+vendor+config; first-run additionally
        // requires home — the intentional asymmetry (MIGRATION_ANALYSIS §1.5).
        let tmp = full_tree();
        std::fs::remove_dir(tmp.path().join("home")).unwrap();
        assert!(is_first_run_at(tmp.path()));
        let info = get_first_run_info_at(tmp.path());
        assert_eq!(info.missing_directories, vec!["home"]);
        assert!(
            info.messages
                .contains(&"Missing essential directories: home".to_string())
        );
    }

    #[test]
    fn missing_config_file_triggers_first_run() {
        let tmp = full_tree();
        std::fs::remove_file(tmp.path().join("config/naner.json")).unwrap();
        let info = get_first_run_info_at(tmp.path());
        assert!(info.is_first_run);
        assert!(
            info.messages.contains(
                &"No configuration file found (supported: naner.json, naner.yaml, naner.yml)"
                    .to_string()
            )
        );
    }
}
