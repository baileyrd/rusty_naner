//! Bring new vendor definitions this binary ships into an existing
//! `config/vendors/` directory -- the vendor counterpart to
//! `config::merge::merge_shipped_naner_defaults`. See that module's doc
//! comment for why the shipped content has to be compiled in
//! (`include_str!`) rather than read from disk or a bundle: a bare
//! `naner.exe` swap has neither.
//!
//! Unlike `naner.json`'s `VendorPaths`/`Profiles`, a vendor definition that
//! already exists in the user's tree is never refreshed here -- only added
//! if entirely missing. A vendor's install-time behavior (its resolved
//! version, checksum, URL) is re-resolved fresh on every `naner install`
//! regardless of what is recorded on disk, so there is no "silently stale
//! forever" failure mode for an existing entry the way `naner.json`'s
//! `Profiles.Bash` had -- the concrete, observed problem was specifically a
//! new vendor (Git for Windows, Anaconda, Bun) never reaching an existing
//! tree at all, not an existing one going stale.
//!
//! With one file per vendor, "add the missing ones" is literally writing the
//! files that are not there. Nothing rewrites a file the user already has, so
//! a customized definition cannot be clobbered by construction rather than by
//! a key-by-key check -- and a malformed file belonging to one vendor no
//! longer blocks every other vendor from being added.

use std::path::Path;

use serde_json::{Map, Value};

/// The vendor catalog as shipped with this binary: every file in
/// `dist-assets/config/vendors/` assembled into one document by `build.rs`.
const SHIPPED_VENDORS_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/vendors_catalog.json"));

/// What `merge_shipped_vendor_defaults` actually did.
#[derive(Debug, PartialEq, Eq)]
pub enum VendorsMergeOutcome {
    /// No `config/vendors/` directory exists yet -- nothing to merge into.
    NoConfig,
    /// Every vendor key this binary ships already has a file.
    UpToDate,
    /// These vendor keys had no file and have been written, verbatim, from
    /// the shipped defaults.
    Added(Vec<String>),
}

/// Reconcile `vendors_dir` against this binary's shipped catalog, writing a
/// file for any vendor key the directory does not have yet.
pub fn merge_shipped_vendor_defaults(vendors_dir: &Path) -> std::io::Result<VendorsMergeOutcome> {
    if !vendors_dir.is_dir() {
        return Ok(VendorsMergeOutcome::NoConfig);
    }

    let shipped: Value = serde_json::from_str(SHIPPED_VENDORS_JSON)
        .expect("the vendor catalog this binary ships is always valid JSON");
    let shipped_vendors = shipped
        .get("vendors")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let mut added = Vec::new();
    for (key, definition) in shipped_vendors {
        let path = vendors_dir.join(format!("{key}.json"));
        if path.exists() {
            continue;
        }

        let mut document = Map::new();
        document.insert(key.clone(), definition);
        let pretty = serde_json::to_string_pretty(&Value::Object(document))
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        crate::fs_atomic::write_atomic(&path, &format!("{pretty}\n"))?;
        added.push(key);
    }

    if added.is_empty() {
        return Ok(VendorsMergeOutcome::UpToDate);
    }
    added.sort();
    Ok(VendorsMergeOutcome::Added(added))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vendors_dir_with(entries: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let vendors = dir.path().join("vendors");
        std::fs::create_dir_all(&vendors).unwrap();
        for (key, contents) in entries {
            std::fs::write(vendors.join(format!("{key}.json")), contents).unwrap();
        }
        dir
    }

    #[test]
    fn no_vendors_directory_is_reported_and_nothing_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let vendors = dir.path().join("vendors");
        assert_eq!(
            merge_shipped_vendor_defaults(&vendors).unwrap(),
            VendorsMergeOutcome::NoConfig
        );
        assert!(!vendors.exists());
    }

    /// The concrete bug this exists for: a pre-#64 tree has never heard of
    /// Git for Windows, Anaconda, or Bun. All three must arrive.
    #[test]
    fn a_pre_64_tree_gains_the_new_vendors() {
        let dir = vendors_dir_with(&[(
            "SevenZip",
            r#"{ "SevenZip": { "name": "7-Zip", "description": "test", "extractDir": "7zip" } }"#,
        )]);
        let vendors = dir.path().join("vendors");

        let outcome = merge_shipped_vendor_defaults(&vendors).unwrap();
        let VendorsMergeOutcome::Added(added) = outcome else {
            panic!("expected vendors to be added, got {outcome:?}");
        };
        assert!(added.contains(&"GitForWindows".to_string()));
        assert!(added.contains(&"Anaconda".to_string()));
        assert!(added.contains(&"Bun".to_string()));

        let git: Value = serde_json::from_str(
            &std::fs::read_to_string(vendors.join("GitForWindows.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(git["GitForWindows"]["name"], "Git for Windows");
    }

    /// A vendor the user has already customized must never be rewritten. With
    /// one file per vendor this holds structurally -- an existing path is
    /// skipped outright -- but the guarantee is the whole point, so it is
    /// asserted rather than assumed.
    #[test]
    fn an_existing_customized_vendor_file_is_never_overwritten() {
        let custom = r#"{ "GitForWindows": { "name": "My Custom Git", "description": "custom", "extractDir": "mygit", "enabled": false } }"#;
        let dir = vendors_dir_with(&[("GitForWindows", custom)]);
        let vendors = dir.path().join("vendors");

        merge_shipped_vendor_defaults(&vendors).unwrap();

        assert_eq!(
            std::fs::read_to_string(vendors.join("GitForWindows.json")).unwrap(),
            custom,
            "an existing vendor file must be left byte-for-byte alone"
        );
    }

    /// A malformed file used to cost the user every other vendor: the whole
    /// document failed to parse, so nothing could be added. Now it only costs
    /// the vendor it belongs to.
    #[test]
    fn one_malformed_file_does_not_block_the_others() {
        let dir = vendors_dir_with(&[("SevenZip", "{ not json")]);
        let vendors = dir.path().join("vendors");

        let outcome = merge_shipped_vendor_defaults(&vendors).unwrap();
        let VendorsMergeOutcome::Added(added) = outcome else {
            panic!("expected vendors to be added, got {outcome:?}");
        };
        assert!(added.contains(&"PowerShell".to_string()));
        assert!(
            !added.contains(&"SevenZip".to_string()),
            "the malformed file exists, so its vendor is left alone"
        );
        assert_eq!(
            std::fs::read_to_string(vendors.join("SevenZip.json")).unwrap(),
            "{ not json",
            "and it is not repaired behind the user's back"
        );
    }

    #[test]
    fn an_already_current_tree_reports_up_to_date_and_writes_nothing() {
        let shipped: Value = serde_json::from_str(SHIPPED_VENDORS_JSON).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let vendors = dir.path().join("vendors");
        std::fs::create_dir_all(&vendors).unwrap();
        for key in shipped["vendors"].as_object().unwrap().keys() {
            std::fs::write(vendors.join(format!("{key}.json")), "placeholder").unwrap();
        }

        assert_eq!(
            merge_shipped_vendor_defaults(&vendors).unwrap(),
            VendorsMergeOutcome::UpToDate
        );
    }
}
