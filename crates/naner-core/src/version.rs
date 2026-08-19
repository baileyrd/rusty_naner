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

/// Canonical form for equality checks (fix for B5): leading `v`/`V` stripped,
/// major/minor/patch parsed and re-emitted as exactly three components, the
/// prerelease suffix preserved verbatim. `"1.2"` and `"1.2.0"` are canonically
/// equal, while `"0.5.0-alpha.0"` and `"0.5.0"` are not — the updater's
/// sync-to-embedded check must still fire across prerelease boundaries.
pub fn canonical(version: &str) -> String {
    let trimmed = version.trim_start_matches(['v', 'V']);
    let (base, suffix) = match trimmed.find('-') {
        Some(idx) if idx > 0 => (&trimmed[..idx], &trimmed[idx..]),
        _ => (trimmed, ""),
    };
    let mut parts = base.split('.');
    let mut next = || -> i64 {
        parts
            .next()
            .and_then(|p| p.parse::<i64>().ok())
            .unwrap_or(0)
    };
    let (major, minor, patch) = (next(), next(), next());
    format!("{major}.{minor}.{patch}{suffix}")
}

/// Numeric segments of a *vendor* version string, leniently extracted: every
/// run of ASCII digits, in order. Vendor versions come in whatever shape the
/// upstream picked — `go1.21.6`, `bun-v1.3.14`, `v20.11.0`, `2026.07-1`,
/// `1.21.14b` — and the C#-quirk [`compare`] above mangles most of them (a
/// dash truncates, a letter prefix parses as 0). Kept separate on purpose:
/// [`compare`]'s quirks are naner's own update protocol and must not drift.
pub fn vendor_segments(version: &str) -> Vec<u64> {
    let mut segments = Vec::new();
    let mut current: Option<u64> = None;
    for c in version.chars() {
        match (c.to_digit(10), current) {
            (Some(d), Some(n)) => current = Some(n.saturating_mul(10).saturating_add(u64::from(d))),
            (Some(d), None) => current = Some(u64::from(d)),
            (None, Some(n)) => {
                segments.push(n);
                current = None;
            }
            (None, None) => {}
        }
    }
    if let Some(n) = current {
        segments.push(n);
    }
    segments
}

/// Compare two vendor version strings by their numeric segments (missing
/// trailing segments read as 0, so `1.2` == `1.2.0`).
pub fn vendor_compare(version1: &str, version2: &str) -> Ordering {
    let a = vendor_segments(version1);
    let b = vendor_segments(version2);
    let len = a.len().max(b.len());
    for i in 0..len {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        match x.cmp(&y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

/// True when the two versions differ in their *first* numeric segment — the
/// "Rust went 1.x → 2.x" case an installed environment should hear about
/// louder than a patch bump.
pub fn vendor_major_differs(version1: &str, version2: &str) -> bool {
    let a = vendor_segments(version1);
    let b = vendor_segments(version2);
    a.first().copied().unwrap_or(0) != b.first().copied().unwrap_or(0)
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
    fn canonical_fixes_b5_but_keeps_prerelease_distinct() {
        // B5: "1.2" vs "1.2.0" used to string-mismatch into a spurious update.
        assert_eq!(canonical("1.2"), canonical("1.2.0"));
        assert_eq!(canonical("v0.4.6"), canonical("0.4.6"));
        // Prerelease suffixes still count — alpha -> final must sync.
        assert_ne!(canonical("0.5.0-alpha.0"), canonical("0.5.0"));
        assert_eq!(canonical("v0.5.0-alpha.0"), "0.5.0-alpha.0");
        assert_eq!(canonical("garbage"), "0.0.0");
    }

    #[test]
    fn vendor_versions_compare_across_upstream_formats() {
        // The shapes actually shipped in config/vendors/ today.
        assert_eq!(vendor_segments("go1.21.6"), vec![1, 21, 6]);
        assert_eq!(vendor_segments("bun-v1.3.14"), vec![1, 3, 14]);
        assert_eq!(vendor_segments("v20.11.0"), vec![20, 11, 0]);
        assert_eq!(vendor_segments("2026.07-1"), vec![2026, 7, 1]);
        assert_eq!(vendor_segments("1.21.14b"), vec![1, 21, 14]);
        assert_eq!(vendor_segments("v2.55.0.windows.4"), vec![2, 55, 0, 4]);

        assert_eq!(vendor_compare("go1.21.6", "go1.22.0"), Ordering::Less);
        assert_eq!(
            vendor_compare("bun-v1.3.14", "bun-v1.3.14"),
            Ordering::Equal
        );
        assert_eq!(vendor_compare("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(vendor_compare("v21.0.0", "v20.11.0"), Ordering::Greater);
        // The C#-quirk comparator gets exactly this wrong: a dash truncates.
        assert_eq!(vendor_compare("bun-v1.3.14", "bun-v1.4.0"), Ordering::Less);

        assert!(vendor_major_differs("1.9.9", "2.0.0"));
        assert!(vendor_major_differs("go1.21.6", "go2.0.0"));
        assert!(!vendor_major_differs("v20.11.0", "v20.19.0"));
        // No digits at all reads as 0 on both sides: not a major jump.
        assert!(!vendor_major_differs("latest", "latest"));
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
