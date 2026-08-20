//! Optional environment isolation for testing naner against a clean slate,
//! without interference from tools already installed system- or user-wide
//! on the host. Additive (no C# counterpart) -- gated by
//! `Advanced.IsolateEnvironment` in `naner.json` / `NANER_ISOLATE_ENVIRONMENT`,
//! off by default.
//!
//! `Advanced.InheritSystemPath` already controls whether the inherited PATH
//! is appended to naner's own vendor paths (`paths::build_unified_path`).
//! This module covers everything else: HOME-equivalents, and any
//! `GIT_*`/`CARGO_HOME`/`RUSTUP_HOME`/`PYTHONHOME`/npm-config-style variable
//! a prior system install may have left set, which would otherwise leak into
//! naner's environment and mask what its own vendored tools would actually
//! see on a clean machine.

/// Host variables kept even when isolation is enabled: things a spawned
/// console or shell needs to function at all, none of which reveal which
/// dev tools are installed. `PATH` is handled separately by
/// `Advanced.InheritSystemPath` and is always kept regardless of this list.
pub const KEEP_ON_ISOLATE: &[&str] = &[
    "SystemRoot",
    "windir",
    "SystemDrive",
    "ComSpec",
    "PATHEXT",
    "TEMP",
    "TMP",
    "OS",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "PROCESSOR_ARCHITEW6432",
    "PROCESSOR_IDENTIFIER",
    "PROCESSOR_LEVEL",
    "PROCESSOR_REVISION",
    "PROGRAMDATA",
    "ALLUSERSPROFILE",
    "PUBLIC",
    // Standard OS directory locations, not tool-install indicators -- same
    // category as PROGRAMDATA/ALLUSERSPROFILE above. Reported live: missing
    // `ProgramFiles(x86)` broke a script that reads it (PowerShell needs
    // `${env:ProgramFiles(x86)}` to reference it at all; an unset read
    // apparently surfaced as a bare `x86` command).
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "CommonProgramW6432",
];

fn is_kept(name: &str) -> bool {
    name.eq_ignore_ascii_case("PATH")
        || KEEP_ON_ISOLATE.iter().any(|k| k.eq_ignore_ascii_case(name))
}

/// Names isolation would remove, out of the given set (testable core).
pub fn host_vars_to_clear_from(names: impl IntoIterator<Item = String>) -> Vec<String> {
    names.into_iter().filter(|n| !is_kept(n)).collect()
}

/// Names isolation would remove, out of the real process environment.
pub fn host_vars_to_clear() -> Vec<String> {
    host_vars_to_clear_from(std::env::vars().map(|(k, _)| k))
}

/// Remove every process env var isolation doesn't keep, returning the names
/// removed. Callers re-apply NANER_ROOT/NANER_ENVIRONMENT/HOME/PATH/
/// configured variables afterward, same as an uninitialized process would
/// see them applied for the first time.
pub fn clear_host_environment() -> Vec<String> {
    let cleared = host_vars_to_clear();
    for name in &cleared {
        // SAFETY: called before any other thread could be reading/writing
        // the environment, same as the launcher's other env setup calls.
        unsafe { std::env::remove_var(name) };
    }
    cleared
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_os_survival_vars_and_path() {
        assert!(is_kept("PATH"));
        assert!(is_kept("path"));
        assert!(is_kept("ComSpec"));
        assert!(is_kept("TEMP"));
        assert!(!is_kept("CARGO_HOME"));
        assert!(!is_kept("GIT_CONFIG_GLOBAL"));
        assert!(!is_kept("APPDATA"));
    }

    #[test]
    fn keeps_the_program_files_family() {
        assert!(is_kept("ProgramFiles"));
        assert!(is_kept("ProgramFiles(x86)"));
        assert!(is_kept("programfiles(x86)"));
        assert!(is_kept("ProgramW6432"));
        assert!(is_kept("CommonProgramFiles"));
        assert!(is_kept("CommonProgramFiles(x86)"));
        assert!(is_kept("CommonProgramW6432"));
    }

    #[test]
    fn clear_list_drops_kept_names_case_insensitively() {
        let names = [
            "PATH",
            "Path",
            "ComSpec",
            "CARGO_HOME",
            "RUSTUP_HOME",
            "TEMP",
        ]
        .into_iter()
        .map(str::to_string);
        let cleared = host_vars_to_clear_from(names);
        assert_eq!(
            cleared,
            vec!["CARGO_HOME".to_string(), "RUSTUP_HOME".to_string()]
        );
    }
}
