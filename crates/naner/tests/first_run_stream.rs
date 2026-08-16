//! Regression test for the first-run notice leaking into `--export-env`.
//!
//! `--export-env` writes a shell program to stdout, documented for use as
//! `naner --export-env | Invoke-Expression` and `eval "$(naner --export-env)"`.
//! The first-run gate fires before the launcher arguments are parsed, so on a
//! tree that is not initialized the notice was printed to stdout — and the
//! calling shell tried to execute it, line by line, while naner exited 0.
//!
//! Spawning the real binary is the point. The bug was in which stream the
//! output went to and what code the process returned, and neither is visible
//! from inside the process.

use std::process::Command;

/// Run naner in a directory that is definitely not a naner tree.
///
/// `NANER_ROOT` is cleared so the search cannot escape to a real installation
/// on the developer's machine, which would make this test pass for the wrong
/// reason.
fn run_outside_a_naner_tree(args: &[&str]) -> std::process::Output {
    let empty = tempfile::tempdir().expect("temp dir");
    Command::new(env!("CARGO_BIN_EXE_naner"))
        .args(args)
        .current_dir(empty.path())
        .env_remove("NANER_ROOT")
        .output()
        .expect("naner should run")
}

#[test]
fn the_first_run_notice_stays_out_of_the_export_env_stream() {
    let out = run_outside_a_naner_tree(&["--export-env", "--no-comments"]);

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim().is_empty(),
        "stdout is piped into Invoke-Expression/eval; it must carry a shell \
         program or nothing at all, got:\n{stdout}"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("First Run Detected"),
        "the notice must still be shown, on stderr; got:\n{stderr}"
    );
}

#[test]
fn export_env_reports_failure_when_nothing_was_exported() {
    let out = run_outside_a_naner_tree(&["--export-env"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "exiting 0 tells a wrapper the environment was exported when it was not"
    );
}

#[test]
fn an_interactive_first_run_still_exits_zero() {
    // Deliberate C# parity (`Program.HandleFirstRun`), and the double-click
    // case — a non-zero code there would be the regression.
    let out = run_outside_a_naner_tree(&[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("First Run Detected"),
        "the notice belongs on stderr in every invocation, not just the piped one"
    );
}
