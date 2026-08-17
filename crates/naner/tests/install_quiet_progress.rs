//! Regression test: a redirected/piped `naner install` already suppressed
//! every `[*]`/`[OK]`/info status line (deliberate "Tier-3 auto-quiet in
//! pipelines" behavior, `commands::vendors::strip_quiet`), but the raw HTTP
//! download progress bar (`\r    Progress: N%`) was unconditional and kept
//! printing anyway -- the one thing piped output should never have to see
//! survived, while everything actually useful in a log got cut. Hits the
//! real network (a real vendor download), same convention as the other
//! network-dependent tests in this workspace.

use std::path::Path;
use std::process::Command;

fn init_tree(root: &Path) {
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
    // Real releaseSource so a real download happens. Git for Windows is a
    // self-extracting archive (no installer elevation prompt), unlike an
    // MSI-based vendor, which would fail outside an elevated shell for a
    // reason unrelated to what this test checks.
    std::fs::write(
        root.join("config").join("vendors.json"),
        r#"{
            "vendors": {
                "GitForWindows": {
                    "name": "Git for Windows",
                    "description": "test",
                    "extractDir": "git",
                    "enabled": true,
                    "required": true,
                    "releaseSource": {
                        "type": "github",
                        "repo": "git-for-windows/git",
                        "assetPattern": "PortableGit-*-64-bit.7z.exe"
                    },
                    "installerArgs": ["-y", "-o%TARGETDIR%"]
                }
            }
        }"#,
    )
    .unwrap();
}

#[test]
#[ignore = "hits the network"]
fn a_piped_install_never_prints_the_raw_progress_bar() {
    let root = tempfile::tempdir().expect("temp dir");
    init_tree(root.path());

    // `Command::output()` captures stdout via a pipe, not a tty -- exactly
    // the condition `strip_quiet` auto-enables quiet mode for.
    let out = Command::new(env!("CARGO_BIN_EXE_naner"))
        .args(["install", "GitForWindows"])
        .env("NANER_ROOT", root.path())
        .output()
        .expect("naner install should run");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("Progress:"),
        "a piped install must not print raw progress noise when every \
         other status line is already suppressed; got:\n{stdout}"
    );
    assert!(
        root.path().join("vendor/git").is_dir(),
        "the vendor must still actually install; got:\n{stdout}"
    );
}
