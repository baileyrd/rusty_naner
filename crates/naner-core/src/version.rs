//! Port of `VersionComparer` (Naner.Init) — deliberately NOT the `semver`
//! crate. Its quirks are the update protocol (MIGRATION_ANALYSIS §2.3):
//! leading `v`/`V` chars stripped, everything after the first `-` dropped
//! (unless the dash is the first char), exactly major/minor/patch compared,
//! unparseable components read as 0.

use std::cmp::Ordering;

/// Compare two version strings with the C# semantics.
pub fn compare(version1: &str, version2: &str) -> Ordering {
    let v1 = parse(version1);
    let v2 = parse(version2);
    v1.cmp(&v2)
}

/// True when `version1` is strictly newer than `version2`.
pub fn is_newer(version1: &str, version2: &str) -> bool {
    compare(version1, version2) == Ordering::Greater
}

/// Normalize a version string: strip leading `v`/`V` chars and any `-suffix`
/// (C# `TrimStart('v', 'V')` removes *all* leading v/V characters, and the
/// dash strip only applies when the dash is not the first character).
pub fn normalize(version: &str) -> String {
    let trimmed = version.trim_start_matches(['v', 'V']);
    match trimmed.find('-') {
        Some(idx) if idx > 0 => trimmed[..idx].to_string(),
        _ => trimmed.to_string(),
    }
}

fn parse(version: &str) -> (i64, i64, i64) {
    let normalized = normalize(version);
    let mut parts = normalized.split('.');
    let mut next = || -> i64 {
        parts
            .next()
            .and_then(|p| p.parse::<i64>().ok())
            .unwrap_or(0)
    };
    (next(), next(), next())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_ordering() {
        assert_eq!(compare("1.0.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare("1.0.1", "1.0.0"), Ordering::Greater);
        assert_eq!(compare("1.0.0", "1.1.0"), Ordering::Less);
        assert_eq!(compare("2.0.0", "1.9.9"), Ordering::Greater);
    }

    #[test]
    fn v_prefix_is_stripped() {
        assert_eq!(compare("v1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(compare("V1.2.3", "v1.2.3"), Ordering::Equal);
        assert!(is_newer("v2.0.0", "1.0.0"));
    }

    #[test]
    fn prerelease_suffix_is_dropped() {
        assert_eq!(compare("1.2.3-beta", "1.2.3"), Ordering::Equal);
        assert_eq!(compare("1.2.3-rc1", "1.2.3-alpha"), Ordering::Equal);
    }

    #[test]
    fn missing_or_bad_components_read_as_zero() {
        // The B5 quirk: "1.2" parses as 1.2.0 — numerically equal, but the
        // updater's *string* sync check would still see them differ
        // (MIGRATION_ANALYSIS §3, B5).
        assert_eq!(compare("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(compare("garbage", "0.0.0"), Ordering::Equal);
        assert_eq!(compare("1.x.3", "1.0.3"), Ordering::Equal);
    }

    #[test]
    fn normalize_matches_csharp() {
        assert_eq!(normalize("v1.2.3"), "1.2.3");
        assert_eq!(normalize("vv1.2.3"), "1.2.3");
        assert_eq!(normalize("1.2.3-beta.1"), "1.2.3");
        // Dash at index 0 after trimming is NOT stripped (C# `dashIndex > 0`).
        assert_eq!(normalize("-weird"), "-weird");
        assert_eq!(normalize("0.4.6"), "0.4.6");
    }
}
