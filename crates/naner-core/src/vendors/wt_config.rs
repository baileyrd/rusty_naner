//! Port of `WindowsTerminalConfigurator`: `.portable` marker + `settings/
//! settings.json` from the home template with string-level `%NANER_ROOT%`
//! substitution on first install (backslashes doubled for JSON); a
//! GUID-identified merge into an existing file after that (#52).
//!
//! The merge is a `serde_json::Value` round-trip, not a byte-level JSONC
//! splice: comments in the user's file do not survive it. That is the same
//! accepted, warned, backed-up trade `naner migrate` already makes for
//! `naner.json` -- defensible there because naner owns the file outright,
//! less so here where Windows Terminal owns it, but the alternative is a
//! hand-rolled JSONC editor with its own correctness risk, in the same file
//! a prior overwrite bug (#50) already cost a user every colour scheme and
//! key binding they had. Key *order* does survive: `serde_json` here runs
//! with `preserve_order`, so only the profiles actually touched move.
//!
//! Every profile the shipped template defines carries a fixed GUID, which is
//! the identity a profile is located by -- never its name, which a user is
//! free to rename without an update mistaking the result for a new profile.
//! A GUID present in the user's file is refreshed to match the template; a
//! GUID that has gone missing is checked against `.naner-managed-profiles.json`
//! (written next to `settings.json`) before deciding whether that is a
//! profile naner has never offered yet (added) or one the user deliberately
//! removed (left alone -- re-adding it would be the same shape of bug as #50,
//! just smaller). A tree upgrading from before this marker existed is the one
//! case that cannot be told apart from "never added"; it is treated as
//! "assume nothing is owned yet" so an old, hand-deleted profile is never
//! silently resurrected -- the safe direction, matching #50's own resolution.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::config::strip_json_comments;
use crate::{logger, timestamp};

/// `IsWindowsTerminal`: substring match on the display name.
pub fn is_windows_terminal(vendor_name: &str) -> bool {
    vendor_name
        .to_lowercase()
        .contains(&"Windows Terminal".to_lowercase())
}

/// Sidecar recording which of Naner's profile GUIDs are currently under
/// management, so a profile the user removed on purpose is never mistaken
/// for one that simply hasn't been added yet.
const MANAGED_PROFILES_FILE: &str = ".naner-managed-profiles.json";

/// What `update_settings` actually did, so the caller can log something more
/// specific than "configured".
#[derive(Debug, PartialEq, Eq)]
pub enum SettingsOutcome {
    /// No `settings.json` existed yet; the full template was written.
    Created,
    /// An existing file was inspected and every Naner profile it should have
    /// already matches the template -- nothing written.
    UpToDate,
    /// An existing file was rewritten: `touched` Naner profiles were added
    /// or refreshed; `respected_deletions` were left gone because the user
    /// removed them on purpose.
    Merged {
        touched: usize,
        respected_deletions: usize,
    },
    /// The existing file's JSON could not be parsed. Left byte-for-byte
    /// untouched rather than risk overwriting something naner cannot read.
    LeftUnparsed,
}

pub struct WindowsTerminalConfigurator<'a> {
    naner_root: &'a Path,
}

impl<'a> WindowsTerminalConfigurator<'a> {
    pub fn new(naner_root: &'a Path) -> Self {
        Self { naner_root }
    }

    /// `ConfigurePortableMode`.
    pub fn configure_portable_mode(&self, target_dir: &Path) -> std::io::Result<()> {
        std::fs::write(target_dir.join(".portable"), "")?;
        logger::info("    Created .portable file for portable mode");

        let settings_dir = target_dir.join("settings");
        std::fs::create_dir_all(&settings_dir)?;
        let settings_path = settings_dir.join("settings.json");

        match self.update_settings(&settings_path)? {
            SettingsOutcome::Created => {
                logger::info("    Created settings/settings.json with Naner profiles");
            }
            SettingsOutcome::UpToDate => {
                logger::info("    settings/settings.json already up to date");
            }
            SettingsOutcome::Merged {
                touched,
                respected_deletions,
            } => {
                let mut line =
                    format!("    Updated {touched} Naner profile(s) in settings/settings.json");
                if respected_deletions > 0 {
                    line.push_str(&format!(
                        " ({respected_deletions} left removed, as you left them)"
                    ));
                }
                logger::info(&line);
            }
            SettingsOutcome::LeftUnparsed => {
                logger::warning("    settings/settings.json could not be parsed; left unchanged");
            }
        }
        Ok(())
    }

    /// `CreateSettings`: template from `home/.config/windows-terminal/
    /// settings.json` with `%NANER_ROOT%` replaced by the root path with
    /// doubled backslashes; inline default otherwise. No existing file to
    /// reconcile against, so this always writes fresh.
    pub fn create_settings(&self, settings_path: &Path) -> std::io::Result<()> {
        std::fs::write(settings_path, self.rendered_template()?)?;
        let guids = self.template_naner_guids().unwrap_or_default();
        self.write_managed_guids(settings_path, &guids)?;
        Ok(())
    }

    /// Reconcile Naner's profiles into an existing `settings.json`, or write
    /// the template fresh if none exists yet.
    fn update_settings(&self, settings_path: &Path) -> std::io::Result<SettingsOutcome> {
        if !settings_path.is_file() {
            self.create_settings(settings_path)?;
            return Ok(SettingsOutcome::Created);
        }

        let existing_text = std::fs::read_to_string(settings_path)?;
        let stripped = strip_json_comments(&existing_text);
        let Ok(mut existing) = serde_json::from_str::<Value>(&stripped) else {
            return Ok(SettingsOutcome::LeftUnparsed);
        };

        let template_profiles = self.template_naner_profiles()?;
        let marker_path = Self::managed_marker_path(settings_path);
        let had_marker = marker_path.is_file();
        let previously_managed = Self::read_managed_guids(&marker_path);

        let mut list: Vec<Value> = existing["profiles"]["list"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let mut touched = 0usize;
        let mut respected_deletions = 0usize;
        let mut changed = false;
        let mut now_managed: Vec<String> = Vec::new();

        for (guid, template_profile) in &template_profiles {
            let existing_pos = list
                .iter()
                .position(|p| p.get("guid").and_then(Value::as_str) == Some(guid.as_str()));

            match existing_pos {
                Some(pos) => {
                    if &list[pos] != template_profile {
                        list[pos] = template_profile.clone();
                        changed = true;
                    }
                    touched += 1;
                    now_managed.push(guid.clone());
                }
                None if had_marker && previously_managed.contains(guid) => {
                    // The user removed this on purpose; a merge that puts it
                    // back is the same class of bug #50 already was.
                    respected_deletions += 1;
                }
                None => {
                    // Never offered before -- a fresh Naner profile, or a
                    // marker-less pre-#52 tree, where "never added" and
                    // "already deleted" cannot be told apart. Add it; see
                    // the module doc for why that is the safe default.
                    list.push(template_profile.clone());
                    touched += 1;
                    changed = true;
                    now_managed.push(guid.clone());
                }
            }
        }

        if changed && let Value::Object(root) = &mut existing {
            let profiles = root
                .entry("profiles")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(profiles) = profiles {
                profiles.insert("list".to_string(), Value::Array(list));
            }

            if let Some(path) = back_up(settings_path)? {
                logger::info(&format!("    Backup: {}", path.display()));
            }
            let pretty = serde_json::to_string_pretty(&existing)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            write_atomic(settings_path, &format!("{pretty}\n"))?;
        }

        self.write_managed_guids(settings_path, &now_managed)?;

        Ok(if changed {
            SettingsOutcome::Merged {
                touched,
                respected_deletions,
            }
        } else {
            SettingsOutcome::UpToDate
        })
    }

    /// `%NANER_ROOT%`-substituted template text, or the inline default.
    fn rendered_template(&self) -> std::io::Result<String> {
        let template_path = self.template_path();
        if template_path.is_file() {
            let template = std::fs::read_to_string(&template_path)?;
            let root = self.naner_root.to_string_lossy().replace('\\', "\\\\");
            Ok(template.replace("%NANER_ROOT%", &root))
        } else {
            Ok(DEFAULT_SETTINGS.to_string())
        }
    }

    fn template_path(&self) -> PathBuf {
        self.naner_root
            .join("home")
            .join(".config")
            .join("windows-terminal")
            .join("settings.json")
    }

    /// Naner's own profile objects from the template, keyed by GUID. The
    /// template only ever contains Naner's profiles, so every entry that
    /// carries a `guid` is one -- nothing to keep in sync with a second,
    /// hardcoded GUID list.
    fn template_naner_profiles(&self) -> std::io::Result<Vec<(String, Value)>> {
        let rendered = self.rendered_template()?;
        let stripped = strip_json_comments(&rendered);
        let parsed: Value = serde_json::from_str(&stripped)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(parsed["profiles"]["list"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| {
                let guid = p.get("guid").and_then(Value::as_str)?.to_string();
                Some((guid, p))
            })
            .collect())
    }

    fn template_naner_guids(&self) -> std::io::Result<Vec<String>> {
        Ok(self
            .template_naner_profiles()?
            .into_iter()
            .map(|(guid, _)| guid)
            .collect())
    }

    fn managed_marker_path(settings_path: &Path) -> PathBuf {
        settings_path
            .parent()
            .map(|dir| dir.join(MANAGED_PROFILES_FILE))
            .unwrap_or_else(|| PathBuf::from(MANAGED_PROFILES_FILE))
    }

    fn read_managed_guids(marker_path: &Path) -> Vec<String> {
        std::fs::read_to_string(marker_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
            .unwrap_or_default()
    }

    fn write_managed_guids(&self, settings_path: &Path, guids: &[String]) -> std::io::Result<()> {
        let marker = Self::managed_marker_path(settings_path);
        let body = serde_json::to_string(guids)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(marker, body)
    }
}

/// Copy `target` aside before it is overwritten. Timestamped so a second run
/// cannot clobber the only copy of the original. Mirrors `naner`'s
/// `config_file::back_up` -- duplicated rather than shared because that one
/// lives in the binary crate and this merge runs from `naner-core`, used by
/// both `naner` and `naner-init`.
fn back_up(target: &Path) -> std::io::Result<Option<PathBuf>> {
    if !target.is_file() {
        return Ok(None);
    }
    let backup = target.with_extension(format!("{}.bak", timestamp::file_stamp()));
    std::fs::copy(target, &backup)?;
    Ok(Some(backup))
}

/// Write via a temp file and a rename, so an interrupted write leaves the
/// previous file intact rather than a truncated one Windows Terminal cannot
/// parse.
fn write_atomic(target: &Path, contents: &str) -> std::io::Result<()> {
    let temp = target.with_extension("tmp");
    std::fs::write(&temp, contents)?;
    if let Err(e) = std::fs::rename(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok(())
}

const DEFAULT_SETTINGS: &str = r#"{
    "$schema": "https://aka.ms/terminal-profiles-schema",
    "defaultProfile": "{naner-unified}",
    "copyOnSelect": false,
    "copyFormatting": "none",
    "profiles": {
        "defaults": {},
        "list": [
            {
                "guid": "{naner-unified}",
                "name": "Naner (Unified)",
                "commandline": "pwsh.exe",
                "startingDirectory": "%USERPROFILE%",
                "colorScheme": "Campbell"
            }
        ]
    },
    "schemes": [],
    "actions": []
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_matching() {
        assert!(is_windows_terminal("Windows Terminal"));
        assert!(is_windows_terminal("windows terminal preview"));
        assert!(!is_windows_terminal("PowerShell"));
        assert!(!is_windows_terminal(""));
    }

    fn write_template(root: &Path, body: &str) {
        let template_dir = root.join("home/.config/windows-terminal");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::write(template_dir.join("settings.json"), body).unwrap();
    }

    #[test]
    fn template_substitution_doubles_backslashes() {
        let root = tempfile::tempdir().unwrap();
        write_template(
            root.path(),
            r#"{ "commandline": "%NANER_ROOT%\\vendor\\powershell\\pwsh.exe" }"#,
        );

        let target = root.path().join("vendor/terminal");
        std::fs::create_dir_all(&target).unwrap();
        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();

        assert!(target.join(".portable").is_file());
        let written = std::fs::read_to_string(target.join("settings/settings.json")).unwrap();
        let expected_root = root.path().to_string_lossy().replace('\\', "\\\\");
        assert!(written.contains(&expected_root));
        assert!(!written.contains("%NANER_ROOT%"));
    }

    #[test]
    fn missing_template_writes_default() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("terminal");
        std::fs::create_dir_all(&target).unwrap();
        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();
        let written = std::fs::read_to_string(target.join("settings/settings.json")).unwrap();
        assert!(written.contains("{naner-unified}"));
    }

    /// The template's one profile is absent from a hand-written file with no
    /// prior marker -- "never added", the safe interpretation -- so it is
    /// added. Everything the user already had, including a field the
    /// template does not know about, survives untouched.
    #[test]
    fn a_missing_naner_profile_is_added_and_the_rest_of_the_file_survives() {
        let root = tempfile::tempdir().unwrap();
        write_template(
            root.path(),
            r#"{ "profiles": { "list": [
                { "guid": "{naner-unified}", "name": "Naner (Unified)", "commandline": "pwsh.exe" }
            ] } }"#,
        );

        let target = root.path().join("vendor/terminal");
        std::fs::create_dir_all(target.join("settings")).unwrap();
        let settings = target.join("settings/settings.json");
        std::fs::write(
            &settings,
            r#"{ "mine": "keep this", "profiles": { "list": [
                { "guid": "{someone-elses}", "name": "Their Shell", "commandline": "elsewhere.exe" }
            ] } }"#,
        )
        .unwrap();

        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(written["mine"], "keep this");
        let list = written["profiles"]["list"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert!(
            list.iter()
                .any(|p| p["guid"] == "{someone-elses}" && p["name"] == "Their Shell"),
            "a third-party profile must survive the merge: {list:?}"
        );
        assert!(
            list.iter().any(|p| p["guid"] == "{naner-unified}"),
            "the missing Naner profile must be added: {list:?}"
        );
    }

    /// A Naner profile the user has hand-customised is refreshed back to the
    /// current template on update -- "template changes reach an existing
    /// install" is the entire point of #52.
    #[test]
    fn a_present_naner_profile_is_refreshed_to_match_the_template() {
        let root = tempfile::tempdir().unwrap();
        write_template(
            root.path(),
            r#"{ "profiles": { "list": [
                { "guid": "{naner-unified}", "name": "Naner (Unified)", "commandline": "new-pwsh.exe" }
            ] } }"#,
        );

        let target = root.path().join("vendor/terminal");
        std::fs::create_dir_all(target.join("settings")).unwrap();
        let settings = target.join("settings/settings.json");
        std::fs::write(
            &settings,
            r#"{ "profiles": { "list": [
                { "guid": "{naner-unified}", "name": "Naner (Unified)", "commandline": "old-pwsh.exe" }
            ] } }"#,
        )
        .unwrap();

        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            written["profiles"]["list"][0]["commandline"],
            "new-pwsh.exe"
        );
    }

    /// A profile the user removed on purpose, once naner has recorded it as
    /// managed, must not come back on the next update -- the #50 failure
    /// mode, one profile wide instead of the whole file.
    #[test]
    fn a_deliberately_removed_naner_profile_is_not_resurrected() {
        let root = tempfile::tempdir().unwrap();
        write_template(
            root.path(),
            r#"{ "profiles": { "list": [
                { "guid": "{naner-unified}", "name": "Naner (Unified)", "commandline": "pwsh.exe" },
                { "guid": "{naner-bash}", "name": "Naner Bash", "commandline": "bash.exe" }
            ] } }"#,
        );

        let target = root.path().join("vendor/terminal");
        std::fs::create_dir_all(target.join("settings")).unwrap();
        let settings = target.join("settings/settings.json");

        // First install: both profiles land, and get recorded as managed.
        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();
        let after_install: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            after_install["profiles"]["list"].as_array().unwrap().len(),
            2
        );

        // The user deletes Bash by hand.
        let mut edited = after_install;
        edited["profiles"]["list"] = Value::Array(
            edited["profiles"]["list"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["guid"] != "{naner-bash}")
                .cloned()
                .collect(),
        );
        std::fs::write(&settings, serde_json::to_string_pretty(&edited).unwrap()).unwrap();

        // A later update must not bring it back.
        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();
        let after_update: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let list = after_update["profiles"]["list"].as_array().unwrap();
        assert_eq!(list.len(), 1, "deleted profile must stay deleted: {list:?}");
        assert_eq!(list[0]["guid"], "{naner-unified}");
    }

    /// A file naner cannot parse is never overwritten -- the file might be
    /// mid hand-edit, and a merge that cannot understand it must say so
    /// rather than replace it with naner's best guess.
    #[test]
    fn an_unparseable_settings_file_is_left_exactly_as_found() {
        let root = tempfile::tempdir().unwrap();
        write_template(
            root.path(),
            r#"{ "profiles": { "list": [
                { "guid": "{naner-unified}", "name": "Naner (Unified)", "commandline": "pwsh.exe" }
            ] } }"#,
        );

        let target = root.path().join("vendor/terminal");
        std::fs::create_dir_all(target.join("settings")).unwrap();
        let settings = target.join("settings/settings.json");
        let broken = r#"{ "profiles": { "list": [ oops not json"#;
        std::fs::write(&settings, broken).unwrap();

        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();

        assert_eq!(std::fs::read_to_string(&settings).unwrap(), broken);
    }

    /// Nothing to add, nothing removed by hand: a no-op update must not
    /// write the file (and so must not spend a backup) at all.
    #[test]
    fn an_already_reconciled_file_is_not_rewritten() {
        let root = tempfile::tempdir().unwrap();
        write_template(
            root.path(),
            r#"{ "profiles": { "list": [
                { "guid": "{naner-unified}", "name": "Naner (Unified)", "commandline": "pwsh.exe" }
            ] } }"#,
        );

        let target = root.path().join("vendor/terminal");
        std::fs::create_dir_all(target.join("settings")).unwrap();
        let settings = target.join("settings/settings.json");

        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();
        let first_write = std::fs::read_to_string(&settings).unwrap();

        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();

        assert_eq!(std::fs::read_to_string(&settings).unwrap(), first_write);
        let backups: Vec<_> = std::fs::read_dir(settings.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
            .collect();
        assert!(
            backups.is_empty(),
            "a no-op merge must not spend a backup: {backups:?}"
        );
    }

    /// A merge that does rewrite the file backs up what was there first.
    #[test]
    fn a_merge_that_rewrites_the_file_backs_it_up_first() {
        let root = tempfile::tempdir().unwrap();
        write_template(
            root.path(),
            r#"{ "profiles": { "list": [
                { "guid": "{naner-unified}", "name": "Naner (Unified)", "commandline": "pwsh.exe" }
            ] } }"#,
        );

        let target = root.path().join("vendor/terminal");
        std::fs::create_dir_all(target.join("settings")).unwrap();
        let settings = target.join("settings/settings.json");
        std::fs::write(&settings, r#"{ "mine": "keep this" }"#).unwrap();

        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();

        let backups: Vec<_> = std::fs::read_dir(settings.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std::fs::read_to_string(backups[0].path()).unwrap(),
            r#"{ "mine": "keep this" }"#
        );
    }
}
