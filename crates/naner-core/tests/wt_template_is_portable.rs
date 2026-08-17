//! Regression guard for the WT settings template shipping a hardcoded
//! dev-machine path instead of `%NANER_ROOT%` (rusty_naner#58). The bug
//! was invisible to every existing test because `wt_config.rs`'s unit
//! tests only exercise `%NANER_ROOT%` substitution against a synthetic
//! template built in-memory -- never the real file that actually ships
//! in `dist-assets/` and gets copied into every release bundle. This
//! test reads that real file.

use std::path::{Path, PathBuf};

use naner_core::vendors::WindowsTerminalConfigurator;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/naner-core -> crates -> repo root")
        .to_path_buf()
}

fn template_path() -> PathBuf {
    repo_root()
        .join("dist-assets")
        .join("home")
        .join(".config")
        .join("windows-terminal")
        .join("settings.json")
}

#[test]
fn shipped_template_has_no_hardcoded_dev_machine_path() {
    let template = std::fs::read_to_string(template_path()).expect("dist-assets template exists");

    // The exact regression: someone captured already-substituted output
    // from their own machine (once at `C:\tools\cmd_line\naner`) instead
    // of authoring the template with the placeholder token
    // `create_settings` actually substitutes.
    assert!(
        !template.to_lowercase().contains("cmd_line"),
        "template contains a literal dev-machine path instead of %NANER_ROOT%"
    );
    assert!(
        template.contains("%NANER_ROOT%"),
        "template has no %NANER_ROOT% placeholder for create_settings to substitute"
    );
}

#[test]
fn shipped_template_substitutes_cleanly_for_a_real_root() {
    let root = tempfile::tempdir().unwrap();
    let template_dir = root.path().join("home/.config/windows-terminal");
    std::fs::create_dir_all(&template_dir).unwrap();
    std::fs::copy(template_path(), template_dir.join("settings.json")).unwrap();

    let settings_path = root.path().join("out-settings.json");
    WindowsTerminalConfigurator::new(root.path())
        .create_settings(&settings_path)
        .unwrap();

    let written = std::fs::read_to_string(&settings_path).unwrap();
    assert!(
        !written.contains("%NANER_ROOT%"),
        "a leftover %NANER_ROOT% means create_settings and the template disagree on the token"
    );
    assert!(
        !written.to_lowercase().contains("cmd_line"),
        "the substituted output must not retain the old dev-machine path"
    );

    // Every profile's commandline/icon must actually resolve to this
    // install's root, not silently fall back to something unrelated.
    let expected_root = root.path().to_string_lossy().replace('\\', "\\\\");
    assert!(
        written.contains(&expected_root),
        "substituted settings must reference this install's own root"
    );
}
