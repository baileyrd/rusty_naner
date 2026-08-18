//! Regression guard for `dist-assets/naner.bat`, the shim that ships at the
//! root of every bundle. Nothing else in the workspace reads it -- it is
//! consumed only by `cmd.exe` on a user's machine -- so the two bugs it
//! carried were invisible to the whole test suite:
//!
//! 1. `set "NANER_ROOT=%~dp0"` exported the root with a trailing backslash,
//!    which escapes the closing quote of any `"%NANER_ROOT%"` a child
//!    process builds into a command line.
//! 2. It still advertised a PowerShell fallback at `src\powershell\
//!    Invoke-Naner.ps1` and a `src\csharp` build tree, neither of which
//!    exists in this repo -- so the not-found path printed instructions that
//!    could not work. That branch now hands over to `naner-init.exe`, which
//!    is the third thing guarded here: the handoff has to go through
//!    `start /wait`, because `naner-init` is a GUI-subsystem binary that
//!    `cmd.exe` will not wait for (rusty_naner#81).

use std::path::{Path, PathBuf};

fn bat_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/naner-core -> crates -> repo root")
        .join("dist-assets")
        .join("naner.bat")
}

fn bat() -> String {
    std::fs::read_to_string(bat_path()).expect("dist-assets/naner.bat exists")
}

#[test]
fn naner_root_is_exported_without_a_trailing_separator() {
    let bat = bat();

    // `%~dp0` always ends with `\`, so assigning it straight across is the
    // bug. The shim strips the separator via a trailing-dot round-trip.
    assert!(
        !bat.contains("set \"NANER_ROOT=%~dp0\""),
        "NANER_ROOT is assigned raw %~dp0, which always ends in a backslash"
    );
    assert!(
        bat.contains("%%~fI"),
        "expected the `for %%I in (\"%~dp0.\")` round-trip that drops the separator"
    );

    // With the separator gone, every path built from it needs its own.
    assert!(
        bat.contains("%NANER_ROOT%\\vendor\\bin\\naner.exe"),
        "the exe path must join NANER_ROOT with an explicit separator"
    );
    assert!(
        !bat.contains("%NANER_ROOT%vendor"),
        "a path is still relying on NANER_ROOT carrying its own trailing backslash"
    );
}

#[test]
fn no_reference_to_the_retired_powershell_or_csharp_trees() {
    let bat = bat().to_lowercase();

    for stale in [
        "invoke-naner",
        "src\\powershell",
        "src\\csharp",
        "powershell fallback",
        "c# version",
        "powershell.exe",
    ] {
        assert!(
            !bat.contains(stale),
            "naner.bat still references `{stale}`, which does not exist in this repo"
        );
    }
}

#[test]
fn the_shim_keeps_crlf_line_endings() {
    // `.gitattributes` pins `*.bat` to CRLF because cmd.exe mis-parses a
    // LF-only batch file. Reading it back as bytes is the only way to catch
    // a checkout or an editor quietly normalizing it.
    let bytes = std::fs::read(bat_path()).expect("dist-assets/naner.bat exists");
    let lf = bytes.iter().filter(|b| **b == b'\n').count();
    let crlf = bytes.windows(2).filter(|w| w == b"\r\n").count();

    assert_eq!(lf, crlf, "naner.bat has {} bare LF line endings", lf - crlf);
    assert!(lf > 0, "naner.bat is empty");
}

#[test]
fn the_missing_exe_branch_hands_over_to_naner_init() {
    let bat = bat();

    // Both locations naner-init is ever found at: the root, where a
    // first-time user drops it, and vendor\bin, where an install that has
    // updated itself keeps it (`self_update::find_naner_init`).
    assert!(
        bat.contains("%NANER_ROOT%\\naner-init.exe"),
        "the handoff must look for naner-init.exe at the root"
    );
    assert!(
        bat.contains("%NANER_ROOT%\\vendor\\bin\\naner-init.exe"),
        "the handoff must also look in vendor\\bin"
    );

    // `naner-init` is GUI-subsystem, so cmd.exe does not wait for it: invoked
    // bare, cmd's own next prompt races naner-init's `(Y/n)` for the user's
    // keystrokes and initialization can silently fail (rusty_naner#81). This
    // is the one detail that makes the handoff work rather than misfire.
    assert!(
        bat.contains("start /wait \"\" \"%NANER_INIT%\""),
        "naner-init must be launched via `start /wait`, not called directly"
    );

    // Falling through both is still a hard error, not a silent success.
    assert!(
        bat.contains("exit /b 1"),
        "with neither binary present the shim must fail loudly"
    );
}

/// `cmd.exe` ends a parenthesized block at the first unescaped `)` -- including
/// one inside a `REM`. A comment mentioning `(Y/n)` inside an `if exist (...)`
/// block would close the block early and silently change control flow, which is
/// exactly the kind of thing that looks fine in a diff.
#[test]
fn no_parenthesized_block_contains_a_paren_in_a_comment() {
    let mut depth = 0i32;
    for (n, line) in bat().lines().enumerate() {
        let trimmed = line.trim_start();
        if depth > 0 && trimmed.to_lowercase().starts_with("rem") {
            assert!(
                !trimmed.contains(')') && !trimmed.contains('('),
                "line {}: a REM inside a block contains a paren, which cmd.exe \
                 treats as block structure: {trimmed}",
                n + 1
            );
        }
        depth += line.matches('(').count() as i32 - line.matches(')').count() as i32;
        depth = depth.max(0);
    }
}
