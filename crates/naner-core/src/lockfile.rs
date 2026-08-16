//! Deterministic environment lockfile (`naner.lock`).
//!
//! Pins each vendor to the exact artifact that was installed — version, URL and
//! SHA-256 — so a second machine, or the same machine later, reproduces the
//! environment instead of picking up whatever upstream currently calls latest.
//!
//! This is the other half of the artifact-verification story from
//! [ADR-0002](../../../docs/adr/0002-upstream-digests-over-a-lockfile.md).
//! Upstream digests cover the five vendors whose distributor publishes one, and
//! they protect the *first* install — but MSYS2 and the GitHub-sourced vendors
//! publish nothing, so they had no verification at all. Once a vendor is locked,
//! every subsequent install of it is verified, whatever its source publishes.
//!
//! What this deliberately does not claim: the first install of an unpublished-
//! digest vendor is still trust-on-first-use. The lock records what arrived; it
//! cannot know whether that was the right thing. Pinning makes the *second* and
//! later installs trustworthy and reproducible, which is a different and weaker
//! guarantee than an upstream digest — stated plainly here so the file is not
//! mistaken for something stronger.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const LOCKFILE_NAME: &str = "naner.lock";

/// Schema version of the file itself. Bump only for a breaking layout change;
/// a lock written by a newer naner is refused rather than misread.
pub const LOCKFILE_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NanerLockfile {
    pub version: String,
    /// Keyed by vendor key. `BTreeMap` so the serialized file is byte-stable
    /// across runs — a lockfile that reorders itself is useless in a diff.
    pub vendors: BTreeMap<String, LockedVendor>,
}

impl Default for NanerLockfile {
    fn default() -> Self {
        Self {
            version: LOCKFILE_VERSION.to_string(),
            vendors: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LockedVendor {
    pub version: String,
    pub url: String,
    /// Absent only for an entry written before the digest could be computed;
    /// an entry without one pins the URL but cannot verify the bytes.
    pub sha256: Option<String>,
}

impl NanerLockfile {
    pub fn path(naner_root: &Path) -> PathBuf {
        naner_root.join(LOCKFILE_NAME)
    }

    /// Read the lock, or `None` when there isn't one.
    ///
    /// A malformed or future-versioned file is `None` *with* a reported reason
    /// rather than a silent fallback to "unlocked": treating an unreadable lock
    /// as absent would quietly drop the pinning the user asked for.
    pub fn load(naner_root: &Path) -> Option<Self> {
        let path = Self::path(naner_root);
        if !path.is_file() {
            return None;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                crate::logger::warning(&format!("Could not read {LOCKFILE_NAME}: {e}"));
                return None;
            }
        };
        let parsed: Self = match serde_json::from_str(&content) {
            Ok(p) => p,
            Err(e) => {
                crate::logger::warning(&format!("Ignoring malformed {LOCKFILE_NAME}: {e}"));
                return None;
            }
        };
        if parsed.version != LOCKFILE_VERSION {
            crate::logger::warning(&format!(
                "Ignoring {LOCKFILE_NAME}: schema version {}, this naner understands {LOCKFILE_VERSION}",
                parsed.version
            ));
            return None;
        }
        Some(parsed)
    }

    /// [`load`](Self::load), falling back to an empty lock.
    pub fn load_or_default(naner_root: &Path) -> Self {
        Self::load(naner_root).unwrap_or_default()
    }

    pub fn get(&self, vendor_key: &str) -> Option<&LockedVendor> {
        self.vendors.get(vendor_key)
    }

    pub fn record(&mut self, vendor_key: &str, entry: LockedVendor) {
        self.vendors.insert(vendor_key.to_string(), entry);
    }

    /// Drop a pin so the next install re-resolves. Returns whether it existed.
    pub fn remove(&mut self, vendor_key: &str) -> bool {
        self.vendors.remove(vendor_key).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.vendors.is_empty()
    }

    /// Write the lock, replacing any existing one.
    ///
    /// Returns `Err` rather than a bool: a lock that silently failed to save is
    /// worse than no lock, because the next run reproduces something different
    /// while believing it is pinned.
    pub fn save(&self, naner_root: &Path) -> Result<(), String> {
        let path = Self::path(naner_root);
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Trailing newline: this is a file humans read and diff.
        fs::write(&path, format!("{json}\n")).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(version: &str, url: &str, sha: Option<&str>) -> LockedVendor {
        LockedVendor {
            version: version.into(),
            url: url.into(),
            sha256: sha.map(str::to_string),
        }
    }

    #[test]
    fn round_trips_through_disk() {
        let root = tempfile::tempdir().unwrap();
        let mut lock = NanerLockfile::default();
        lock.record(
            "NodeJS",
            entry("v26.7.0", "https://x/node.zip", Some("ab12")),
        );
        lock.record("MSYS2", entry("20240727", "https://y/msys2.tar.xz", None));
        lock.save(root.path()).unwrap();

        let read = NanerLockfile::load(root.path()).expect("loads");
        assert_eq!(read.version, LOCKFILE_VERSION);
        assert_eq!(read.get("NodeJS"), lock.get("NodeJS"));
        assert_eq!(read.get("MSYS2").unwrap().sha256, None);
        assert!(read.get("Missing").is_none());
    }

    #[test]
    fn serialization_is_byte_stable_regardless_of_insertion_order() {
        let root_a = tempfile::tempdir().unwrap();
        let root_b = tempfile::tempdir().unwrap();

        let mut a = NanerLockfile::default();
        a.record("Zebra", entry("1", "https://z", None));
        a.record("Alpha", entry("2", "https://a", None));
        a.save(root_a.path()).unwrap();

        let mut b = NanerLockfile::default();
        b.record("Alpha", entry("2", "https://a", None));
        b.record("Zebra", entry("1", "https://z", None));
        b.save(root_b.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(NanerLockfile::path(root_a.path())).unwrap(),
            std::fs::read_to_string(NanerLockfile::path(root_b.path())).unwrap()
        );
    }

    #[test]
    fn absent_malformed_and_future_versions_all_read_as_unlocked() {
        let root = tempfile::tempdir().unwrap();
        assert!(NanerLockfile::load(root.path()).is_none());
        assert!(NanerLockfile::load_or_default(root.path()).is_empty());

        std::fs::write(NanerLockfile::path(root.path()), "{ not json").unwrap();
        assert!(NanerLockfile::load(root.path()).is_none());

        std::fs::write(
            NanerLockfile::path(root.path()),
            r#"{"version":"99","vendors":{}}"#,
        )
        .unwrap();
        assert!(
            NanerLockfile::load(root.path()).is_none(),
            "a lock from a newer naner must be refused, not misread"
        );
    }

    #[test]
    fn remove_reports_whether_a_pin_existed() {
        let mut lock = NanerLockfile::default();
        lock.record("Go", entry("go1.26.6", "https://go", None));
        assert!(lock.remove("Go"));
        assert!(!lock.remove("Go"));
        assert!(lock.is_empty());
    }
}
