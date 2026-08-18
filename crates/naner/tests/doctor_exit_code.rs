//! Regression test: `naner doctor` used to always return exit code 0,
//! regardless of what it found -- a missing required vendor, or a
//! configuration file that failed to load, both still reported success at
//! the process level. That made `doctor` useless as a CI/script health gate.
//!
//! Spawning the real binary, same rationale as `explicit_profile_fails_loudly.rs`:
//! the bug is only observable in the process's actual exit code.

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
}

/// Write a vendor definition to `config/vendors/<Key>.json`, the per-vendor
/// layout the loader reads. One file per vendor, key inside the file.
fn write_vendor(root: &Path, key: &str, definition: &str) {
    let dir = root.join("config").join("vendors");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{key}.json")),
        format!("{{ \"{key}\": {definition} }}"),
    )
    .unwrap();
}

fn run_doctor(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_naner"))
        .arg("doctor")
        .args(args)
        .env("NANER_ROOT", root)
        .output()
        .expect("naner doctor should run")
}

#[test]
fn a_missing_required_vendor_fails_the_exit_code() {
    let root = tempfile::tempdir().expect("temp dir");
    init_tree(root.path());
    write_vendor(
        root.path(),
        "SevenZip",
        r#"{
            "name": "7-Zip",
            "description": "test",
            "extractDir": "7zip",
            "enabled": true,
            "required": true
        }"#,
    );
    // vendor/7zip is deliberately never created: SevenZip stays "missing".

    let out = run_doctor(root.path(), &[]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a required vendor missing must not report exit 0"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Missing required vendor(s): 7-Zip"),
        "must name the missing required vendor; got:\n{stdout}"
    );
}

#[test]
fn only_optional_vendors_missing_still_succeeds() {
    let root = tempfile::tempdir().expect("temp dir");
    init_tree(root.path());
    write_vendor(
        root.path(),
        "NodeJS",
        r#"{
            "name": "Node.js",
            "description": "test",
            "extractDir": "nodejs",
            "enabled": false,
            "required": false
        }"#,
    );

    let out = run_doctor(root.path(), &[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "an optional vendor being absent is normal and must not fail doctor"
    );
}

#[test]
fn porcelain_output_reflects_the_same_exit_code() {
    let root = tempfile::tempdir().expect("temp dir");
    init_tree(root.path());
    write_vendor(
        root.path(),
        "SevenZip",
        r#"{
            "name": "7-Zip",
            "description": "test",
            "extractDir": "7zip",
            "enabled": true,
            "required": true
        }"#,
    );

    let out = run_doctor(root.path(), &["--porcelain"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"status\":\"unhealthy\"") || stdout.contains("\"status\": \"unhealthy\""),
        "porcelain output must not claim \"ok\" when the exit code says otherwise; got:\n{stdout}"
    );
}
