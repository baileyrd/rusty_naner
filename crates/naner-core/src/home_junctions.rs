//! Directory junctions under `home\`, bridging specific real Windows
//! locations back out from underneath `Advanced.HomeJunctions`'
//! `USERPROFILE` redirect -- additive, no C# counterpart.
//!
//! A junction (`mklink /J`) rather than a symlink: no admin privilege or
//! Developer Mode required, unlike `std::os::windows::fs::symlink_dir`,
//! which needs `SeCreateSymbolicLinkPrivilege`. Created once, at first
//! launch after init -- a junction is a real filesystem entry, so every
//! later launch's `link_path.exists()` check is already true and skips it.

use std::path::{Path, PathBuf};

use crate::collections::OrderedMap;
use crate::{constants, logger, paths};

/// The one non-environment-variable lookup key `%...%` targets may use:
/// the real Windows profile directory as it was before naner redirected
/// `USERPROFILE` to its own tree. Resolved from a value the caller captured
/// at process start, before that redirect happened -- there is no live env
/// var to read it back from afterward.
const HOST_USERPROFILE_TOKEN: &str = "HOST_USERPROFILE";

/// What a configured target resolves to before any filesystem check: the
/// pure "would we even attempt this" decision, split out so it's testable
/// without a real Windows console or filesystem.
fn resolve_target(target: &str, naner_root: &str, host_userprofile: Option<&str>) -> String {
    let lookup = |name: &str| {
        if name.eq_ignore_ascii_case(HOST_USERPROFILE_TOKEN) {
            host_userprofile.map(str::to_string)
        } else {
            std::env::var(name).ok()
        }
    };
    paths::expand_naner_path_with(target, naner_root, lookup)
}

/// Create every configured junction under `home\` that doesn't already
/// exist, skipping (not failing) anything whose target isn't a real
/// directory yet -- a personal `dev` folder the user hasn't created, say.
/// `host_userprofile` is the real `USERPROFILE` value as captured before
/// naner's own redirect overwrote it for this process.
pub fn ensure_home_junctions(
    naner_root: &Path,
    home_junctions: &OrderedMap<String>,
    host_userprofile: Option<&str>,
) {
    let home = naner_root.join(constants::directory_names::HOME);
    if !home.is_dir() {
        return;
    }
    let root = naner_root.to_string_lossy();
    for (name, target) in home_junctions {
        let expanded = resolve_target(target, &root, host_userprofile);
        let target_path = PathBuf::from(&expanded);
        let link_path = home.join(name);

        if link_path.exists() {
            continue;
        }
        if !target_path.is_dir() {
            logger::debug(
                &format!("Skipping home junction {name} -> {expanded} (target doesn't exist yet)"),
                false,
            );
            continue;
        }
        create_junction(&link_path, &target_path);
    }
}

#[cfg(windows)]
fn create_junction(link: &Path, target: &Path) {
    // mklink is a cmd.exe built-in, not a standalone executable -- no
    // vendored shell (PowerShell may not be installed yet) or elevation
    // needed either way, since /J junctions don't require
    // SeCreateSymbolicLinkPrivilege the way /D symlinks do.
    match std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
    {
        Ok(out) if out.status.success() => {
            logger::success(&format!(
                "Linked {} -> {}",
                link.display(),
                target.display()
            ));
        }
        Ok(out) => {
            logger::warning(&format!(
                "Could not link {}: {}",
                link.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Err(e) => {
            logger::warning(&format!("Could not run mklink for {}: {e}", link.display()));
        }
    }
}

#[cfg(not(windows))]
fn create_junction(_link: &Path, _target: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_userprofile_token_resolves_from_the_captured_value() {
        assert_eq!(
            resolve_target(
                "%HOST_USERPROFILE%\\Documents",
                "C:\\naner",
                Some("C:\\Users\\me")
            ),
            "C:\\Users\\me\\Documents"
        );
    }

    /// No captured value (e.g. the host process never had USERPROFILE set
    /// at all) means nothing to expand into -- stays literal, same as any
    /// other unset variable, so `ensure_home_junctions`'s `is_dir()` check
    /// naturally skips it rather than mangling a path.
    #[test]
    fn missing_host_userprofile_stays_literal() {
        assert_eq!(
            resolve_target("%HOST_USERPROFILE%\\Documents", "C:\\naner", None),
            "%HOST_USERPROFILE%\\Documents"
        );
    }

    #[test]
    fn plain_absolute_targets_pass_through_untouched() {
        assert_eq!(
            resolve_target("C:\\dev", "C:\\naner", Some("C:\\Users\\me")),
            "C:\\dev"
        );
    }

    #[test]
    fn naner_root_still_expands_for_targets_that_use_it() {
        assert_eq!(
            resolve_target("%NANER_ROOT%\\shared", "C:\\naner", None),
            "C:\\naner\\shared"
        );
    }

    fn map(entries: &[(&str, &str)]) -> OrderedMap<String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// Without `home\` itself, there's nowhere to put a link -- the whole
    /// call is a no-op, not a directory-creation side effect of its own.
    #[test]
    fn does_nothing_without_a_home_directory() {
        let tmp = tempfile::tempdir().unwrap();
        ensure_home_junctions(tmp.path(), &map(&[("dev", "C:\\dev")]), None);
        assert!(!tmp.path().join("home").exists());
    }

    /// A missing target (the user hasn't created `C:\dev` yet, say) is
    /// skipped, not an error -- there is nothing to link to.
    #[test]
    fn skips_a_link_whose_target_does_not_exist() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("home")).unwrap();
        let missing_target = tmp.path().join("does-not-exist");
        ensure_home_junctions(
            tmp.path(),
            &map(&[("dev", missing_target.to_str().unwrap())]),
            None,
        );
        assert!(!tmp.path().join("home/dev").exists());
    }

    /// Something already at the link path -- a prior junction, or a real
    /// directory the user put there -- is left alone, not overwritten.
    #[test]
    fn leaves_an_existing_link_path_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("home/dev")).unwrap();
        std::fs::write(tmp.path().join("home/dev/marker.txt"), b"mine").unwrap();
        let real_target = tmp.path().join("real-dev");
        std::fs::create_dir_all(&real_target).unwrap();

        ensure_home_junctions(
            tmp.path(),
            &map(&[("dev", real_target.to_str().unwrap())]),
            None,
        );

        assert!(tmp.path().join("home/dev/marker.txt").is_file());
    }
}
