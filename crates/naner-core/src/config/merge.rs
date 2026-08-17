//! Bring the vendor/profile defaults this binary ships into an existing,
//! already-initialized `config/naner.json` -- the missing counterpart to
//! `WindowsTerminalConfigurator`'s `settings.json` merge (`vendors/wt_config.rs`).
//!
//! An upgrade that only replaces `vendor/bin/naner.exe` (the documented,
//! supported update path -- see `docs/VALIDATION.md` Step 5) never touches
//! `config/naner.json`. Before this, that meant a vendor-set change like #64
//! (Git for Windows replacing MSYS2 as the default Bash provider) reached a
//! brand-new install but never an existing one: `Environment.VendorPaths`
//! and `Profiles.Bash` kept pointing at MSYS2 forever, because nothing ever
//! looked at them again after the tree's first init.
//!
//! The shipped defaults are embedded at compile time (`include_str!`), not
//! read from `<naner_root>/config/naner.json` on disk or the sibling
//! `naner-bundle.zip` -- a bare binary swap has neither. This is what makes
//! the merge possible at all on that upgrade path: the new `naner.exe`
//! always knows its own current defaults, regardless of what is on disk.
//!
//! Three kinds of change:
//! - A `VendorPaths`/`Profiles` key entirely missing from the user's file is
//!   always added. A missing key cannot be a customization; there is nothing
//!   to protect by leaving it out.
//! - A handful of specific fields changed by a config-shape migration (right
//!   now, just #64) are refreshed *only* when the user's current value still
//!   matches exactly what naner itself last shipped there. Any other value
//!   means the user (or a prior hand-edit) set it deliberately, and it is
//!   left alone -- the same "never resurrect a deliberate change" rule
//!   `wt_config.rs` already applies to profile deletions.
//! - `Environment.PathPrecedence` entries the shipped config has that the
//!   user's list does not get appended, with the same GUID-marker technique
//!   `wt_config.rs` uses for profiles -- a `.naner-managed-path-precedence.json`
//!   sidecar (next to the config file) records which shipped entries are
//!   currently under naner's management, so an entry the user removed on
//!   purpose is never silently added back. A tree upgrading from before this
//!   marker existed cannot tell "never added" apart from "deliberately
//!   removed"; it is treated as "never added" (added), the same accepted
//!   one-time trade-off `wt_config.rs` already makes for a marker-less tree.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::strip_json_comments;
use crate::fs_atomic::{back_up, write_atomic};

/// `naner.json`/`naner.yaml` as shipped with this binary. Field-for-field
/// identical content in both formats (verified by
/// `an_already_current_tree_reports_up_to_date_and_writes_nothing` running
/// against each), so `merge_shipped_naner_defaults` only has to pick the
/// matching one for whichever format the user's file is in.
const SHIPPED_NANER_JSON: &str = include_str!("../../../../dist-assets/config/naner.json");
const SHIPPED_NANER_YAML: &str = include_str!("../../../../dist-assets/config/naner.yaml");

/// A field whose value naner itself sets, and what it used to be before a
/// specific, named migration -- not a general "diff against last shipped"
/// mechanism, just an explicit, auditable list of exact past-and-current
/// values. Appending a rule here is how the *next* such change (a Bash
/// provider swap, say) reaches existing trees the same way.
struct FieldMigration {
    /// RFC 6901 JSON pointer into the config document.
    pointer: &'static str,
    /// Exact prior value(s) that mean "still what naner wrote, never
    /// customized". Any other current value is left alone.
    old_values: &'static [&'static str],
    new_value: &'static str,
}

/// #64: Git for Windows replaces MSYS2 as the default/required Bash
/// provider.
const FIELD_MIGRATIONS: &[FieldMigration] = &[
    FieldMigration {
        pointer: "/VendorPaths/GitBash",
        old_values: &["%NANER_ROOT%\\vendor\\msys64\\usr\\bin\\bash.exe"],
        new_value: "%NANER_ROOT%\\vendor\\git\\bin\\bash.exe",
    },
    FieldMigration {
        pointer: "/Profiles/Bash/Description",
        old_values: &["MSYS2 Bash environment"],
        new_value: "Git Bash environment",
    },
    FieldMigration {
        pointer: "/Profiles/Bash/CustomShell/ExecutablePath",
        old_values: &["%NANER_ROOT%\\vendor\\msys64\\usr\\bin\\bash.exe"],
        new_value: "%NANER_ROOT%\\vendor\\git\\bin\\bash.exe",
    },
];

/// What `merge_shipped_naner_defaults` actually did.
#[derive(Debug, PartialEq, Eq)]
pub enum NanerConfigMergeOutcome {
    /// No `naner.json` exists yet -- nothing to merge into (a fresh init
    /// writes the current defaults directly).
    NoConfig,
    /// The existing file's JSON could not be parsed. Left byte-for-byte
    /// untouched, same as `wt_config`'s `LeftUnparsed`.
    LeftUnparsed,
    /// Every `VendorPaths`/`Profiles` key already matches or exceeds the
    /// shipped defaults, and no known field migration applies.
    UpToDate,
    /// `added` names newly-introduced `VendorPaths`/`Profiles` keys and any
    /// `Environment.PathPrecedence` entries appended; `refreshed` names
    /// field pointers updated because they still matched what naner had
    /// last shipped there. `respected_deletions` counts shipped
    /// `PathPrecedence` entries left out because the user removed them on
    /// purpose.
    Merged {
        added: Vec<String>,
        refreshed: Vec<String>,
        respected_deletions: usize,
    },
}

/// Sidecar recording which shipped `Environment.PathPrecedence` entries are
/// currently under naner's management -- the `PathPrecedence` counterpart to
/// `wt_config.rs`'s `.naner-managed-profiles.json`, and for the same reason:
/// telling "the user never had this" apart from "the user removed this on
/// purpose" needs a record of what naner itself last added.
const MANAGED_PATH_PRECEDENCE_FILE: &str = ".naner-managed-path-precedence.json";

pub fn merge_shipped_naner_defaults(
    config_path: &Path,
) -> std::io::Result<NanerConfigMergeOutcome> {
    if !config_path.is_file() {
        return Ok(NanerConfigMergeOutcome::NoConfig);
    }

    let is_yaml = config_path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"));

    let existing_text = std::fs::read_to_string(config_path)?;
    let Ok(mut existing) = parse_document(&existing_text, is_yaml) else {
        return Ok(NanerConfigMergeOutcome::LeftUnparsed);
    };

    // `naner.yaml` mirrors `naner.json` field-for-field (same keys, same
    // shape), so the merge logic below operates on `serde_json::Value`
    // either way -- only parsing/serializing differ by format.
    let shipped_source = if is_yaml {
        SHIPPED_NANER_YAML
    } else {
        SHIPPED_NANER_JSON
    };
    let shipped = parse_document(shipped_source, is_yaml)
        .expect("the config this binary ships is always valid");

    let mut added = Vec::new();
    let mut refreshed = Vec::new();

    add_missing_object_keys(
        &mut existing,
        &shipped,
        "/VendorPaths",
        "VendorPaths.",
        &mut added,
    );
    add_missing_object_keys(
        &mut existing,
        &shipped,
        "/Profiles",
        "Profiles.",
        &mut added,
    );

    for migration in FIELD_MIGRATIONS {
        let current = existing.pointer(migration.pointer).and_then(Value::as_str);
        let Some(current) = current else { continue };
        if migration.old_values.contains(&current)
            && current != migration.new_value
            && let Some(slot) = existing.pointer_mut(migration.pointer)
        {
            *slot = Value::String(migration.new_value.to_string());
            refreshed.push(migration.pointer.to_string());
        }
    }

    let (path_precedence_added, respected_deletions) =
        merge_path_precedence(&mut existing, &shipped, config_path)?;
    added.extend(path_precedence_added);

    if added.is_empty() && refreshed.is_empty() {
        return Ok(NanerConfigMergeOutcome::UpToDate);
    }

    if let Some(path) = back_up(config_path)? {
        crate::logger::info(&format!("    Backup: {}", path.display()));
    }
    let rendered = render_document(&existing, is_yaml)?;
    write_atomic(config_path, &rendered)?;

    Ok(NanerConfigMergeOutcome::Merged {
        added,
        refreshed,
        respected_deletions,
    })
}

fn managed_path_precedence_marker(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|dir| dir.join(MANAGED_PATH_PRECEDENCE_FILE))
        .unwrap_or_else(|| PathBuf::from(MANAGED_PATH_PRECEDENCE_FILE))
}

fn read_managed_path_precedence(marker_path: &Path) -> Vec<String> {
    std::fs::read_to_string(marker_path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .unwrap_or_default()
}

fn write_managed_path_precedence(config_path: &Path, entries: &[String]) -> std::io::Result<()> {
    let marker = managed_path_precedence_marker(config_path);
    let body = serde_json::to_string(entries)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(marker, body)
}

/// Reconcile `Environment.PathPrecedence`: a shipped entry missing from the
/// user's list is appended, unless it was previously under naner's
/// management and has since been removed -- the same rule `wt_config.rs`
/// applies to a deliberately deleted profile. Entries the user added
/// themselves (never shipped by naner) are never touched or removed.
///
/// Writes the managed-entries marker unconditionally, same as
/// `wt_config.rs`'s equivalent -- a marker refresh with no other change is
/// not itself reported as a merge.
fn merge_path_precedence(
    existing: &mut Value,
    shipped: &Value,
    config_path: &Path,
) -> std::io::Result<(Vec<String>, usize)> {
    let shipped_entries: Vec<String> = shipped
        .pointer("/Environment/PathPrecedence")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if shipped_entries.is_empty() {
        return Ok((Vec::new(), 0));
    }

    let Some(list) = existing
        .pointer_mut("/Environment/PathPrecedence")
        .and_then(Value::as_array_mut)
    else {
        return Ok((Vec::new(), 0));
    };

    let marker_path = managed_path_precedence_marker(config_path);
    let had_marker = marker_path.is_file();
    let previously_managed = read_managed_path_precedence(&marker_path);

    let mut added = Vec::new();
    let mut respected_deletions = 0usize;
    let mut now_managed = Vec::new();

    for entry in &shipped_entries {
        let present = list.iter().any(|v| v.as_str() == Some(entry.as_str()));
        if present {
            now_managed.push(entry.clone());
            continue;
        }
        if had_marker && previously_managed.contains(entry) {
            // The user removed this on purpose; adding it back would be the
            // same class of bug #50 already was for Windows Terminal
            // profiles.
            respected_deletions += 1;
            continue;
        }
        // Never offered before -- a fresh PathPrecedence entry, or a
        // marker-less pre-existing tree, where "never added" and "already
        // deleted" cannot be told apart. Add it; see the module doc for why
        // that is the safe default.
        list.push(Value::String(entry.clone()));
        added.push(format!("Environment.PathPrecedence: {entry}"));
        now_managed.push(entry.clone());
    }

    write_managed_path_precedence(config_path, &now_managed)?;
    Ok((added, respected_deletions))
}

/// Parse a config document as JSON (comments/trailing commas tolerated, same
/// as every other JSON reader in this crate) or YAML, into a
/// `serde_json::Value` either way so the merge logic never needs to care
/// which format it started as.
fn parse_document(text: &str, is_yaml: bool) -> Result<Value, ()> {
    if is_yaml {
        serde_yaml_ng::from_str(text).map_err(|_| ())
    } else {
        let stripped = strip_json_comments(text);
        serde_json::from_str(&stripped).map_err(|_| ())
    }
}

fn render_document(value: &Value, is_yaml: bool) -> std::io::Result<String> {
    if is_yaml {
        serde_yaml_ng::to_string(value)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    } else {
        let pretty = serde_json::to_string_pretty(value)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(format!("{pretty}\n"))
    }
}

/// Insert any key present in `shipped.pointer(object_pointer)` but absent
/// from the same location in `existing`, recording `"{prefix}{key}"` for
/// each. Existing keys are never touched here -- only `FIELD_MIGRATIONS`
/// refreshes a key that already exists.
fn add_missing_object_keys(
    existing: &mut Value,
    shipped: &Value,
    object_pointer: &str,
    label_prefix: &str,
    added: &mut Vec<String>,
) {
    let Some(shipped_obj) = shipped.pointer(object_pointer).and_then(Value::as_object) else {
        return;
    };
    let shipped_obj = shipped_obj.clone();

    let Some(existing_obj) = existing
        .pointer_mut(object_pointer)
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for (key, value) in shipped_obj {
        if !existing_obj.contains_key(&key) {
            existing_obj.insert(key.clone(), value);
            added.push(format!("{label_prefix}{key}"));
        }
    }
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
        let path = dir.path().join("naner.json");
        assert_eq!(
            merge_shipped_naner_defaults(&path).unwrap(),
            NanerConfigMergeOutcome::NoConfig
        );
        assert!(!path.is_file());
    }

    #[test]
    fn unparseable_json_is_left_byte_for_byte_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "naner.json", "{ not json");
        assert_eq!(
            merge_shipped_naner_defaults(&path).unwrap(),
            NanerConfigMergeOutcome::LeftUnparsed
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    /// The concrete bug: a tree from before #64 has `GitBash` pointing at
    /// MSYS2 and a Bash profile description/shell path that still say so.
    /// Never touched by hand -- both must refresh to Git for Windows.
    #[test]
    fn a_pre_64_tree_gets_its_bash_provider_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "naner.json",
            r#"{
                "VendorPaths": {
                    "GitBash": "%NANER_ROOT%\\vendor\\msys64\\usr\\bin\\bash.exe"
                },
                "Profiles": {
                    "Bash": {
                        "Name": "Bash",
                        "Description": "MSYS2 Bash environment",
                        "CustomShell": {
                            "ExecutablePath": "%NANER_ROOT%\\vendor\\msys64\\usr\\bin\\bash.exe",
                            "Arguments": "--login -i"
                        }
                    }
                }
            }"#,
        );

        let outcome = merge_shipped_naner_defaults(&path).unwrap();
        let NanerConfigMergeOutcome::Merged { refreshed, .. } = outcome else {
            panic!("expected a merge, got {outcome:?}");
        };
        assert!(refreshed.contains(&"/VendorPaths/GitBash".to_string()));
        assert!(refreshed.contains(&"/Profiles/Bash/Description".to_string()));
        assert!(refreshed.contains(&"/Profiles/Bash/CustomShell/ExecutablePath".to_string()));

        let updated: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            updated["VendorPaths"]["GitBash"],
            "%NANER_ROOT%\\vendor\\git\\bin\\bash.exe"
        );
        assert_eq!(
            updated["Profiles"]["Bash"]["Description"],
            "Git Bash environment"
        );
        assert_eq!(
            updated["Profiles"]["Bash"]["CustomShell"]["ExecutablePath"],
            "%NANER_ROOT%\\vendor\\git\\bin\\bash.exe"
        );
        // A field this migration does not touch must survive verbatim.
        assert_eq!(
            updated["Profiles"]["Bash"]["CustomShell"]["Arguments"],
            "--login -i"
        );
    }

    /// A user who pointed `GitBash` at their own install must never have it
    /// silently swapped back to naner's default -- that is the #50 failure
    /// mode this whole mechanism exists to avoid repeating.
    #[test]
    fn a_hand_customized_vendor_path_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "naner.json",
            r#"{
                "VendorPaths": {
                    "GitBash": "C:\\tools\\my-own-git\\bin\\bash.exe"
                },
                "Profiles": {}
            }"#,
        );

        let outcome = merge_shipped_naner_defaults(&path).unwrap();
        // Profiles gains new keys from the shipped template, but the
        // customized VendorPath must not be among the refreshed fields.
        if let NanerConfigMergeOutcome::Merged { refreshed, .. } = &outcome {
            assert!(!refreshed.contains(&"/VendorPaths/GitBash".to_string()));
        }
        let updated: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            updated["VendorPaths"]["GitBash"],
            "C:\\tools\\my-own-git\\bin\\bash.exe"
        );
    }

    /// A vendor path the shipped config introduces that the user's file has
    /// never seen (new capability, e.g. a future vendor) must be added.
    #[test]
    fn a_newly_shipped_vendor_path_is_added() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "naner.json",
            r#"{ "VendorPaths": {}, "Profiles": {} }"#,
        );

        let outcome = merge_shipped_naner_defaults(&path).unwrap();
        let NanerConfigMergeOutcome::Merged { added, .. } = outcome else {
            panic!("expected new keys to be added, got {outcome:?}");
        };
        assert!(added.iter().any(|k| k == "VendorPaths.Bun"));

        let updated: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(updated["VendorPaths"]["Bun"].is_string());
    }

    /// The concrete regression this was built for: a tree from before
    /// `home\.local\bin`/`\Scripts` existed in the shipped config gets them
    /// appended, and nothing else in the user's list is disturbed.
    #[test]
    fn a_pre_local_bin_tree_gets_path_precedence_entries_added() {
        let dir = tempfile::tempdir().unwrap();
        let mut without_local: Value =
            serde_json::from_str(&strip_json_comments(SHIPPED_NANER_JSON)).unwrap();
        {
            let list = without_local["Environment"]["PathPrecedence"]
                .as_array_mut()
                .unwrap();
            list.retain(|v| {
                v.as_str() != Some("%NANER_ROOT%\\home\\.local\\bin")
                    && v.as_str() != Some("%NANER_ROOT%\\home\\.local\\Scripts")
            });
        }
        let path = write(
            dir.path(),
            "naner.json",
            &serde_json::to_string_pretty(&without_local).unwrap(),
        );

        let outcome = merge_shipped_naner_defaults(&path).unwrap();
        let NanerConfigMergeOutcome::Merged {
            added,
            respected_deletions,
            ..
        } = outcome
        else {
            panic!("expected a merge, got {outcome:?}");
        };
        assert!(added.iter().any(|a| a.contains("home\\.local\\bin")));
        assert!(added.iter().any(|a| a.contains("home\\.local\\Scripts")));
        assert_eq!(respected_deletions, 0);

        let updated: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let list = updated["Environment"]["PathPrecedence"].as_array().unwrap();
        assert!(
            list.iter()
                .any(|v| v.as_str() == Some("%NANER_ROOT%\\home\\.local\\bin"))
        );
        assert!(
            list.iter()
                .any(|v| v.as_str() == Some("%NANER_ROOT%\\home\\.local\\Scripts"))
        );
        // Every entry the tree already had survives, in place.
        assert_eq!(list[0], "%NANER_ROOT%\\bin");
    }

    /// A `PathPrecedence` entry the user added themselves -- never shipped
    /// by naner at all -- must never be touched, same as a hand-set
    /// `VendorPaths` value.
    #[test]
    fn a_users_own_path_precedence_entry_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "naner.json",
            r#"{
                "VendorPaths": {},
                "Profiles": {},
                "Environment": {
                    "PathPrecedence": ["C:\\my-own-tools", "%NANER_ROOT%\\bin"]
                }
            }"#,
        );

        merge_shipped_naner_defaults(&path).unwrap();

        let updated: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let list = updated["Environment"]["PathPrecedence"].as_array().unwrap();
        assert_eq!(list[0], "C:\\my-own-tools");
        assert_eq!(list[1], "%NANER_ROOT%\\bin");
    }

    /// Unit-level test of the deletion-respecting rule directly, independent
    /// of the JSON-file round trip: a `PathPrecedence` entry naner itself
    /// added and the user later removed must not be silently added back on
    /// the next merge, the same guarantee `wt_config.rs` makes for profiles.
    #[test]
    fn path_precedence_respects_a_deliberate_removal() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("naner.json");
        std::fs::write(&config_path, "{}").unwrap();

        let shipped = serde_json::json!({
            "Environment": {
                "PathPrecedence": ["%NANER_ROOT%\\bin", "%NANER_ROOT%\\home\\.local\\bin"]
            }
        });

        // First pass: nothing present yet, no marker -- both entries are
        // added and recorded as managed.
        let mut existing = serde_json::json!({ "Environment": { "PathPrecedence": [] } });
        let (added, respected) =
            merge_path_precedence(&mut existing, &shipped, &config_path).unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(respected, 0);

        // The user removes one of the two naner just added.
        existing["Environment"]["PathPrecedence"] = serde_json::json!(["%NANER_ROOT%\\bin"]);

        // Second pass: the marker says both were managed; the missing one
        // must be respected as a deliberate removal, not re-added.
        let (added2, respected2) =
            merge_path_precedence(&mut existing, &shipped, &config_path).unwrap();
        assert!(added2.is_empty());
        assert_eq!(respected2, 1);
        let list = existing["Environment"]["PathPrecedence"]
            .as_array()
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], "%NANER_ROOT%\\bin");
    }

    #[test]
    fn an_already_current_tree_reports_up_to_date_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "naner.json", SHIPPED_NANER_JSON);
        let before = std::fs::read_to_string(&path).unwrap();

        assert_eq!(
            merge_shipped_naner_defaults(&path).unwrap(),
            NanerConfigMergeOutcome::UpToDate
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn a_naner_yaml_tree_gets_the_same_bash_provider_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "naner.yaml",
            "VendorPaths:\n  GitBash: \"%NANER_ROOT%\\\\vendor\\\\msys64\\\\usr\\\\bin\\\\bash.exe\"\n\
             Profiles:\n  Bash:\n    Name: Bash\n    Description: MSYS2 Bash environment\n",
        );

        let outcome = merge_shipped_naner_defaults(&path).unwrap();
        let NanerConfigMergeOutcome::Merged { refreshed, .. } = outcome else {
            panic!("expected a merge, got {outcome:?}");
        };
        assert!(refreshed.contains(&"/VendorPaths/GitBash".to_string()));
        assert!(refreshed.contains(&"/Profiles/Bash/Description".to_string()));

        let updated: Value =
            serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            updated["VendorPaths"]["GitBash"],
            "%NANER_ROOT%\\vendor\\git\\bin\\bash.exe"
        );
        assert_eq!(
            updated["Profiles"]["Bash"]["Description"],
            "Git Bash environment"
        );
    }

    #[test]
    fn an_already_current_yaml_tree_reports_up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "naner.yaml", SHIPPED_NANER_YAML);

        assert_eq!(
            merge_shipped_naner_defaults(&path).unwrap(),
            NanerConfigMergeOutcome::UpToDate
        );
    }
}
