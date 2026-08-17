//! Regression test for #57: a mistyped `-p <profile>` used to silently
//! launch the default profile instead of failing.
//!
//! Spawning the real binary is the point, same as `first_run_stream.rs` --
//! the bug was observable only in the process's actual exit code and output
//! text, not from inside a unit test that can't capture either without
//! reaching all the way through a real terminal spawn. `failure`/`info`
//! write to stdout, not stderr -- only `warning` goes to stderr (see
//! `naner_core::logger`'s module doc) -- so the assertions below read
//! stdout, not the more obvious-looking stderr.

use std::path::Path;
use std::process::Command;

/// A minimal but genuinely initialized naner tree: the marker file, the
/// four essential directories, and a config file naming exactly one
/// profile. No vendors are installed -- irrelevant here, since profile
/// resolution runs and fails before any terminal/shell is ever touched.
fn init_fixture_tree(root: &Path) {
    for dir in ["bin", "vendor", "config", "home"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join(".naner-initialized"), "").unwrap();
    std::fs::write(
        root.join("config").join("naner.json"),
        r#"{
            "DefaultProfile": "Unified",
            "Profiles": {
                "Unified": { "Name": "Naner (Unified)", "Shell": "PowerShell" }
            }
        }"#,
    )
    .unwrap();
}

fn run_in_fixture(args: &[&str]) -> std::process::Output {
    let root = tempfile::tempdir().expect("temp dir");
    init_fixture_tree(root.path());
    Command::new(env!("CARGO_BIN_EXE_naner"))
        .args(args)
        .env("NANER_ROOT", root.path())
        .output()
        .expect("naner should run")
}

#[test]
fn a_mistyped_explicit_profile_fails_with_exit_1_and_names_it() {
    let out = run_in_fixture(&["-p", "NoSuchProfile"]);

    assert_eq!(
        out.status.code(),
        Some(1),
        "exit 0 tells a caller the requested profile launched when it did not"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Profile not found: NoSuchProfile"),
        "must name the profile that failed to resolve; got:\n{stdout}"
    );
    assert!(
        stdout.contains("Available profiles:") && stdout.contains("Unified"),
        "must list what does exist so the caller can correct itself; got:\n{stdout}"
    );
}

#[test]
fn no_profile_flag_still_falls_back_to_the_default() {
    // No `-p`: implicit resolution keeps today's warn-and-fall-back
    // behavior. This fixture has no Windows Terminal installed, so the run
    // still fails overall -- but it must fail *past* profile resolution,
    // not be turned into the same hard failure the explicit case gets.
    let out = run_in_fixture(&[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("Profile not found"),
        "implicit resolution must not start hard-failing on the default \
         profile; got:\n{stdout}"
    );
}
