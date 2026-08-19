//! Regression guard for `naner.json` shipping a hardcoded dev-machine path
//! in one of its `Profiles` instead of `%NANER_ROOT%` (rusty_naner#58,
//! carried forward when #83 made `naner.json` the only source Windows
//! Terminal profiles are generated from). The bug was invisible to every
//! `wt_config.rs` unit test because those only exercise `%NANER_ROOT%`
//! substitution against synthetic config built in-memory -- never the real
//! file that ships in `dist-assets/` and gets copied into every release
//! bundle. This test reads that real file.

use std::path::{Path, PathBuf};

use naner_core::vendors::WindowsTerminalConfigurator;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/naner-core -> crates -> repo root")
        .to_path_buf()
}

fn naner_json_path() -> PathBuf {
    repo_root()
        .join("dist-assets")
        .join("config")
        .join("naner.json")
}

#[test]
fn shipped_naner_json_has_no_hardcoded_dev_machine_path() {
    let config =
        std::fs::read_to_string(naner_json_path()).expect("dist-assets/config/naner.json exists");

    // The exact regression: someone captured already-substituted output
    // from their own machine (once at `C:\tools\cmd_line\naner`) instead
    // of authoring a profile with the placeholder token the generator
    // actually substitutes.
    assert!(
        !config.to_lowercase().contains("cmd_line"),
        "naner.json contains a literal dev-machine path instead of %NANER_ROOT%"
    );
    assert!(
        config.contains("%NANER_ROOT%"),
        "naner.json's Profiles have no %NANER_ROOT% placeholder to substitute"
    );
}

#[test]
fn shipped_naner_json_generates_wt_profiles_that_substitute_cleanly() {
    let root = tempfile::tempdir().unwrap();
    let config_dir = root.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::copy(naner_json_path(), config_dir.join("naner.json")).unwrap();

    let settings_path = root.path().join("out-settings.json");
    WindowsTerminalConfigurator::new(root.path())
        .create_settings(&settings_path)
        .unwrap();

    let written = std::fs::read_to_string(&settings_path).unwrap();
    assert!(
        !written.contains("%NANER_ROOT%"),
        "a leftover %NANER_ROOT% means the generator and naner.json disagree on the token"
    );
    assert!(
        !written.to_lowercase().contains("cmd_line"),
        "the substituted output must not retain the old dev-machine path"
    );

    // Every generated profile's commandline/icon must actually resolve to
    // this install's root, not silently fall back to something unrelated.
    let expected_root = root.path().to_string_lossy().replace('\\', "\\\\");
    assert!(
        written.contains(&expected_root),
        "substituted settings must reference this install's own root"
    );
}
