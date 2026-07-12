//! Port of `PathUtilities` and `PathBuilder` (Naner.Core): root discovery,
//! placeholder expansion, and unified-PATH assembly.

use std::path::{Path, PathBuf};

use crate::constants;

/// Error from [`find_naner_root`], carrying the verbose diagnostic message
/// the C# implementation throws (printed verbatim by the launcher).
#[derive(Debug)]
pub struct RootNotFound {
    pub message: String,
}

impl std::fmt::Display for RootNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RootNotFound {}

/// Find the Naner root directory. Port of `PathUtilities.FindNanerRoot`:
///
/// 1. `NANER_ROOT` env var wins if it points at an existing directory that
///    contains the three marker dirs (`bin/`, `vendor/`, `config/`).
/// 2. Otherwise walk up from `start_path` (defaults to the executable's
///    directory) up to `max_depth` levels looking for the markers.
/// 3. Otherwise fail with a verbose error.
pub fn find_naner_root(
    start_path: Option<&Path>,
    max_depth: usize,
) -> Result<PathBuf, RootNotFound> {
    let env_root = std::env::var("NANER_ROOT").ok();
    let exe_dir = exe_directory();
    let start = start_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| exe_dir.clone());
    find_naner_root_impl(env_root.as_deref(), &start, &exe_dir, max_depth)
}

/// The testable core of [`find_naner_root`] with all environment inputs
/// explicit.
pub fn find_naner_root_impl(
    env_root: Option<&str>,
    start_path: &Path,
    exe_dir: &Path,
    max_depth: usize,
) -> Result<PathBuf, RootNotFound> {
    // NANER_ROOT env var first (highest priority).
    if let Some(root) = env_root
        && !root.is_empty()
        && Path::new(root).is_dir()
    {
        let trimmed = trim_trailing_separators(root);
        let candidate = Path::new(&trimmed);
        if has_root_markers(candidate) {
            return Ok(candidate.to_path_buf());
        }
    }

    let full = normalize_path_buf(start_path);
    let mut current = PathBuf::from(trim_trailing_separators(&full.to_string_lossy()));
    let mut searched: Vec<String> = Vec::new();
    let mut depth = 0;

    while depth < max_depth {
        searched.push(current.to_string_lossy().into_owned());

        if has_root_markers(&current) {
            return Ok(current);
        }

        match current.parent() {
            Some(parent) if parent != current && !parent.as_os_str().is_empty() => {
                current = parent.to_path_buf();
            }
            _ => break,
        }
        depth += 1;
    }

    let paths_list = searched
        .iter()
        .map(|p| format!("    - {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    Err(RootNotFound {
        message: format!(
            "Could not locate Naner root directory.\n\n\
             Search Details:\n\
             \x20 Starting path: {}\n\
             \x20 Executable location: {}\n\
             \x20 Paths searched ({}):\n{}\n\n\
             Requirements:\n\
             \x20 Naner root must contain:\n\
             \x20   - bin/      (binaries directory)\n\
             \x20   - vendor/   (vendor dependencies)\n\
             \x20   - config/   (configuration files)\n\n\
             Solutions:\n\
             \x20 1. Copy naner.exe to vendor/bin/ in your Naner installation\n\
             \x20 2. Run from within the Naner directory structure\n\
             \x20 3. Set NANER_ROOT environment variable to your Naner directory",
            start_path.display(),
            exe_dir.display(),
            searched.len(),
            paths_list
        ),
    })
}

fn has_root_markers(dir: &Path) -> bool {
    dir.join(constants::directory_names::BIN).is_dir()
        && dir.join(constants::directory_names::VENDOR).is_dir()
        && dir.join(constants::directory_names::CONFIG).is_dir()
}

fn exe_directory() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn trim_trailing_separators(path: &str) -> String {
    path.trim_end_matches(['/', '\\']).to_string()
}

/// Expand a path containing `%NANER_ROOT%` and environment variables. Port
/// of `PathUtilities.ExpandNanerPath`, three passes in order:
///
/// 1. `%NANER_ROOT%` — case-insensitive literal replacement.
/// 2. Windows-style `%VAR%` — .NET `Environment.ExpandEnvironmentVariables`
///    semantics: unset variables stay literal.
/// 3. PowerShell-style `$env:VAR` (`\w+` name, prefix case-insensitive) —
///    unset variables stay literal.
pub fn expand_naner_path(path: &str, naner_root: &str) -> String {
    expand_naner_path_with(path, naner_root, |name| std::env::var(name).ok())
}

/// [`expand_naner_path`] with an explicit variable lookup, for tests.
pub fn expand_naner_path_with(
    path: &str,
    naner_root: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> String {
    if path.trim().is_empty() {
        return path.to_string();
    }

    let expanded = replace_case_insensitive(path, "%NANER_ROOT%", naner_root);
    let expanded = expand_windows_env(&expanded, &lookup);
    expand_psenv(&expanded, &lookup)
}

/// Case-insensitive literal replacement (C# `string.Replace(...,
/// OrdinalIgnoreCase)`).
fn replace_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(haystack.len());
    let lower_haystack = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    let mut pos = 0;
    while let Some(found) = lower_haystack[pos..].find(&lower_needle) {
        let start = pos + found;
        result.push_str(&haystack[pos..start]);
        result.push_str(replacement);
        pos = start + needle.len();
    }
    result.push_str(&haystack[pos..]);
    result
}

/// .NET Core's `ExpandEnvironmentVariablesCore` algorithm, ported exactly:
/// scan for a `%`, look for the next `%` after it; if the enclosed name is a
/// defined variable, substitute and continue after the closing `%`; else emit
/// one character and rescan (so a closing `%` can open the next pair).
/// Notably `%%` and `%UNDEFINED%` pass through unchanged.
fn expand_windows_env(input: &str, lookup: &impl Fn(&str) -> Option<String>) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut result = String::with_capacity(input.len());
    let mut last_pos = 0;

    while last_pos < chars.len() {
        let next_percent = chars[last_pos + 1..]
            .iter()
            .position(|&c| c == '%')
            .map(|i| i + last_pos + 1);
        let Some(pos) = next_percent else { break };

        if chars[last_pos] == '%' {
            let key: String = chars[last_pos + 1..pos].iter().collect();
            if let Some(value) = lookup(&key) {
                result.push_str(&value);
                last_pos = pos + 1;
                continue;
            }
        }
        result.push(chars[last_pos]);
        last_pos += 1;
    }
    result.extend(&chars[last_pos..]);
    result
}

/// PowerShell-style `$env:VAR` expansion: `\w+` variable names, the `$env:`
/// prefix matched case-insensitively (C# regex has IgnoreCase), unset
/// variables left as the original text.
fn expand_psenv(input: &str, lookup: &impl Fn(&str) -> Option<String>) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut result = String::with_capacity(input.len());
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '$' && i + 5 <= chars.len() {
            let prefix: String = chars[i + 1..(i + 4).min(chars.len())].iter().collect();
            if prefix.eq_ignore_ascii_case("env")
                && chars.get(i + 4) == Some(&':')
                && chars
                    .get(i + 5)
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            {
                let mut end = i + 5;
                while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                    end += 1;
                }
                let name: String = chars[i + 5..end].iter().collect();
                match lookup(&name) {
                    Some(value) => result.push_str(&value),
                    None => result.extend(&chars[i..end]),
                }
                i = end;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// True if the path exists as a file or directory
/// (`PathUtilities.PathExists`).
pub fn path_exists(path: &Path) -> bool {
    path.exists()
}

/// Create the directory (and parents) if missing
/// (`PathUtilities.EnsureDirectoryExists`).
pub fn ensure_directory_exists(path: &Path) -> std::io::Result<()> {
    if !path.is_dir() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

/// Resolve to an absolute, normalized path (`Path.GetFullPath` — no symlink
/// resolution, which is why this is `std::path::absolute` and not
/// `canonicalize`).
pub fn normalize_path(path: &str) -> String {
    if path.trim().is_empty() {
        return path.to_string();
    }
    normalize_path_buf(Path::new(path))
        .to_string_lossy()
        .into_owned()
}

fn normalize_path_buf(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Port of `PathBuilder.BuildUnifiedPath`: expand each configured entry,
/// silently drop entries whose directory doesn't exist, join with `;`, then
/// append the process PATH when `include_system_path`. The silent drop is
/// bug-for-bug preserved behavior — the loud-warning variant is a post-parity
/// change (MIGRATION_ANALYSIS §2.4 tier 3).
pub fn build_unified_path(
    path_precedence: &[String],
    naner_root: &str,
    include_system_path: bool,
) -> String {
    build_unified_path_with(
        path_precedence,
        naner_root,
        include_system_path,
        std::env::var("PATH").ok(),
        |name| std::env::var(name).ok(),
        |p| Path::new(p).is_dir(),
    )
}

/// [`build_unified_path`] with explicit environment/filesystem seams, for
/// tests.
pub fn build_unified_path_with(
    path_precedence: &[String],
    naner_root: &str,
    include_system_path: bool,
    system_path: Option<String>,
    lookup: impl Fn(&str) -> Option<String>,
    dir_exists: impl Fn(&str) -> bool,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    for entry in path_precedence {
        let expanded = expand_naner_path_with(entry, naner_root, &lookup);
        if dir_exists(&expanded) {
            parts.push(expanded);
        }
    }

    if include_system_path
        && let Some(sys) = system_path
        && !sys.is_empty()
    {
        parts.push(sys);
    }

    parts.join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn naner_root_replaced_case_insensitively() {
        let lookup = env(&[]);
        assert_eq!(
            expand_naner_path_with("%NANER_ROOT%\\bin", "C:\\naner", lookup),
            "C:\\naner\\bin"
        );
        let lookup = env(&[]);
        assert_eq!(
            expand_naner_path_with("%naner_root%/bin", "/opt/naner", lookup),
            "/opt/naner/bin"
        );
    }

    #[test]
    fn windows_vars_expand_and_unknown_stays_literal() {
        let lookup = env(&[("USERPROFILE", "C:\\Users\\me")]);
        assert_eq!(
            expand_naner_path_with("%USERPROFILE%\\dev", "root", &lookup),
            "C:\\Users\\me\\dev"
        );
        assert_eq!(
            expand_naner_path_with("%NOPE%\\dev", "root", &lookup),
            "%NOPE%\\dev"
        );
        // %% passes through (no variable named "").
        assert_eq!(expand_naner_path_with("100%%", "root", &lookup), "100%%");
    }

    #[test]
    fn dotnet_rescan_semantics() {
        // "a%FOO%" — leading non-% chars emit one at a time, then the pair
        // expands; and an unmatched closing % can open the next pair.
        let lookup = env(&[("FOO", "X"), ("B", "Y")]);
        assert_eq!(expand_naner_path_with("a%FOO%", "r", &lookup), "aX");
        assert_eq!(expand_naner_path_with("%NOPE%B%", "r", &lookup), "%NOPEY");
    }

    #[test]
    fn psenv_vars_expand_and_unknown_stays() {
        let lookup = env(&[("HOME", "/home/me")]);
        assert_eq!(
            expand_naner_path_with("$env:HOME/dev", "root", &lookup),
            "/home/me/dev"
        );
        assert_eq!(
            expand_naner_path_with("$ENV:HOME/dev", "root", &lookup),
            "/home/me/dev"
        );
        assert_eq!(
            expand_naner_path_with("$env:MISSING/dev", "root", &lookup),
            "$env:MISSING/dev"
        );
    }

    #[test]
    fn expansion_order_is_root_then_var_then_psenv() {
        // %NANER_ROOT% is replaced before %VAR% expansion, so a root value
        // containing %X% gets a second-pass expansion — order matters.
        let lookup = env(&[("X", "expanded")]);
        assert_eq!(
            expand_naner_path_with("%NANER_ROOT%", "%X%", &lookup),
            "expanded"
        );
    }

    #[test]
    fn empty_input_passes_through() {
        let lookup = env(&[]);
        assert_eq!(expand_naner_path_with("", "root", &lookup), "");
        let lookup = env(&[]);
        assert_eq!(expand_naner_path_with("   ", "root", lookup), "   ");
    }

    #[test]
    fn unified_path_drops_missing_dirs_silently() {
        let path = build_unified_path_with(
            &[
                "%NANER_ROOT%\\bin".to_string(),
                "%NANER_ROOT%\\missing".to_string(),
                "%NANER_ROOT%\\opt".to_string(),
            ],
            "C:\\naner",
            false,
            None,
            |_| None,
            |p| !p.contains("missing"),
        );
        assert_eq!(path, "C:\\naner\\bin;C:\\naner\\opt");
    }

    #[test]
    fn unified_path_appends_system_path_when_asked() {
        let path = build_unified_path_with(
            &["%NANER_ROOT%\\bin".to_string()],
            "C:\\naner",
            true,
            Some("C:\\Windows;C:\\Windows\\System32".to_string()),
            |_| None,
            |_| true,
        );
        assert_eq!(path, "C:\\naner\\bin;C:\\Windows;C:\\Windows\\System32");

        let without = build_unified_path_with(
            &["%NANER_ROOT%\\bin".to_string()],
            "C:\\naner",
            false,
            Some("C:\\Windows".to_string()),
            |_| None,
            |_| true,
        );
        assert_eq!(without, "C:\\naner\\bin");
    }

    #[test]
    fn root_discovery_walks_up_and_respects_env() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("tree");
        for d in ["bin", "vendor", "config"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let nested = root.join("vendor").join("bin");

        // Walk up from a nested dir finds the root.
        let found = find_naner_root_impl(None, &nested, &nested, 10).unwrap();
        assert_eq!(found, std::path::absolute(&root).unwrap());

        // A valid NANER_ROOT env var wins.
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let found =
            find_naner_root_impl(Some(root.to_str().unwrap()), &elsewhere, &elsewhere, 10).unwrap();
        assert_eq!(found, root);

        // An env var pointing at a non-root dir falls through to the walk,
        // which fails here (with the verbose message).
        let err =
            find_naner_root_impl(Some(elsewhere.to_str().unwrap()), &elsewhere, &elsewhere, 2)
                .unwrap_err();
        assert!(
            err.message
                .contains("Could not locate Naner root directory")
        );
        assert!(err.message.contains("Paths searched"));
    }

    #[test]
    fn root_discovery_respects_max_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("tree");
        for d in ["bin", "vendor", "config"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let deep = root.join("a/b/c/d/e");
        std::fs::create_dir_all(&deep).unwrap();

        // Depth 3 can't reach the root from 5 levels down...
        assert!(find_naner_root_impl(None, &deep, &deep, 3).is_err());
        // ...but depth 10 can.
        assert!(find_naner_root_impl(None, &deep, &deep, 10).is_ok());
    }
}
