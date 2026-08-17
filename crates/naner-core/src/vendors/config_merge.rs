//! Bring new vendor definitions this binary ships into an existing
//! `config/vendors.json` -- the `vendors.json` counterpart to
//! `config::merge::merge_shipped_naner_defaults`. See that module's doc
//! comment for why the shipped content has to be compiled in
//! (`include_str!`) rather than read from disk or a bundle: a bare
//! `naner.exe` swap has neither.
//!
//! Unlike `naner.json`'s `VendorPaths`/`Profiles`, a vendor definition that
//! already exists in the user's file is never refreshed here -- only added
//! if entirely missing. A vendor's install-time behavior (its resolved
//! version, checksum, URL) is re-resolved fresh on every `naner install`
//! regardless of what is recorded in `vendors.json`, so there is no
//! "silently stale forever" failure mode for an existing entry the way
//! `naner.json`'s `Profiles.Bash` had -- the concrete, observed problem was
//! specifically a new vendor (Git for Windows, Anaconda, Bun) never
//! reaching an existing tree at all, not an existing one going stale.

use std::path::Path;

use serde_json::{Map, Value};

use crate::config::strip_json_comments;
use crate::fs_atomic::{back_up, write_atomic};

/// `vendors.json` as shipped with this binary.
const SHIPPED_VENDORS_JSON: &str = include_str!("../../../../dist-assets/config/vendors.json");

/// What `merge_shipped_vendor_defaults` actually did.
#[derive(Debug, PartialEq, Eq)]
pub enum VendorsMergeOutcome {
    /// No `vendors.json` exists yet -- nothing to merge into.
    NoConfig,
    /// The existing file's JSON could not be parsed. Left byte-for-byte
    /// untouched.
    LeftUnparsed,
    /// Every vendor key this binary ships already exists in the user's file.
    UpToDate,
    /// These vendor keys were missing from the user's file and have been
    /// added, verbatim, from the shipped defaults.
    Added(Vec<String>),
}

/// Reconcile `config_path` against this binary's shipped `vendors.json`,
/// adding any vendor key the user's file does not have yet.
pub fn merge_shipped_vendor_defaults(config_path: &Path) -> std::io::Result<VendorsMergeOutcome> {
    if !config_path.is_file() {
        return Ok(VendorsMergeOutcome::NoConfig);
    }

    let existing_text = std::fs::read_to_string(config_path)?;
    let stripped = strip_json_comments(&existing_text);
    let Ok(mut existing) = serde_json::from_str::<Value>(&stripped) else {
        return Ok(VendorsMergeOutcome::LeftUnparsed);
    };

    let shipped_stripped = strip_json_comments(SHIPPED_VENDORS_JSON);
    let shipped: Value = serde_json::from_str(&shipped_stripped)
        .expect("the vendors.json this binary ships is always valid JSON");
    let shipped_vendors = shipped
        .get("vendors")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let Value::Object(root) = &mut existing else {
        return Ok(VendorsMergeOutcome::LeftUnparsed);
    };
    let vendors_entry = root
        .entry("vendors")
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(vendors) = vendors_entry else {
        return Ok(VendorsMergeOutcome::LeftUnparsed);
    };

    let mut added = Vec::new();
    for (key, def) in shipped_vendors {
        if !vendors.contains_key(&key) {
            vendors.insert(key.clone(), def);
            added.push(key);
        }
    }

    if added.is_empty() {
        return Ok(VendorsMergeOutcome::UpToDate);
    }

    if let Some(path) = back_up(config_path)? {
        crate::logger::info(&format!("    Backup: {}", path.display()));
    }
    let pretty = serde_json::to_string_pretty(&existing)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_atomic(config_path, &format!("{pretty}\n"))?;

    Ok(VendorsMergeOutcome::Added(added))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, contents: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn no_config_file_is_reported_and_nothing_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vendors.json");
        assert_eq!(
            merge_shipped_vendor_defaults(&path).unwrap(),
            VendorsMergeOutcome::NoConfig
        );
        assert!(!path.is_file());
    }

    #[test]
    fn unparseable_json_is_left_byte_for_byte_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "vendors.json", "{ not json");
        assert_eq!(
            merge_shipped_vendor_defaults(&path).unwrap(),
            VendorsMergeOutcome::LeftUnparsed
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    /// The concrete bug: a pre-#64 tree's `vendors.json` has never heard of
    /// Git for Windows, Anaconda, or Bun. All three must be added.
    #[test]
    fn a_pre_64_tree_gains_the_new_vendors() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "vendors.json",
            r#"{
                "vendors": {
                    "SevenZip": { "name": "7-Zip", "description": "test", "extractDir": "7zip" }
                }
            }"#,
        );

        let outcome = merge_shipped_vendor_defaults(&path).unwrap();
        let VendorsMergeOutcome::Added(added) = outcome else {
            panic!("expected vendors to be added, got {outcome:?}");
        };
        assert!(added.contains(&"GitForWindows".to_string()));
        assert!(added.contains(&"Anaconda".to_string()));
        assert!(added.contains(&"Bun".to_string()));

        let updated: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            updated["vendors"]["GitForWindows"]["name"],
            "Git for Windows"
        );
        // The user's pre-existing entry must survive untouched.
        assert_eq!(updated["vendors"]["SevenZip"]["description"], "test");
    }

    /// A vendor entry the user has already customized (a different name, a
    /// tweaked description, `enabled` flipped) must never be overwritten --
    /// only genuinely missing keys are ever touched.
    #[test]
    fn an_existing_customized_vendor_entry_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "vendors.json",
            r#"{
                "vendors": {
                    "GitForWindows": { "name": "My Custom Git", "description": "custom", "extractDir": "mygit", "enabled": false }
                }
            }"#,
        );

        merge_shipped_vendor_defaults(&path).unwrap();

        let updated: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(updated["vendors"]["GitForWindows"]["name"], "My Custom Git");
        assert_eq!(updated["vendors"]["GitForWindows"]["enabled"], false);
    }

    #[test]
    fn an_already_current_tree_reports_up_to_date_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "vendors.json", SHIPPED_VENDORS_JSON);
        let before = std::fs::read_to_string(&path).unwrap();

        assert_eq!(
            merge_shipped_vendor_defaults(&path).unwrap(),
            VendorsMergeOutcome::UpToDate
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }
}
