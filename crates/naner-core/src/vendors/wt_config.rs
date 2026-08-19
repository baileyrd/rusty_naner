//! Port of `WindowsTerminalConfigurator`: `.portable` marker + `settings/
//! settings.json` with a GUID-identified merge into an existing file (#52).
//!
//! Naner's four profile entries in `profiles.list` are generated fresh, on
//! every call, from `config/naner.json`'s own `Profiles` section — the
//! single source of truth (#83). Before #83 they were hand-duplicated a
//! second time in WT's own schema, in
//! `dist-assets/home/.config/windows-terminal/settings.json`; nothing kept
//! that copy in sync, and it had already drifted from `naner.json` in the
//! shipped repo by the time #83 was filed. That file is gone.
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
//! Every Naner profile carries a fixed GUID (`FIXED_GUIDS`), which is the
//! identity a profile is located by -- never its name, which a user is free
//! to rename without an update mistaking the result for a new profile, and
//! never derived from the `naner.json` key either, since a freshly-derived
//! GUID would make every already-installed `settings.json` look like the
//! user deleted all four profiles on their next update -- the same failure
//! class as #50, just narrower. A GUID present in the user's file is
//! refreshed to match naner.json; a GUID that has gone missing is checked
//! against `.naner-managed-profiles.json` (written next to `settings.json`)
//! before deciding whether that is a profile naner has never offered yet
//! (added) or one the user deliberately removed (left alone). A tree
//! upgrading from before this marker existed is the one case that cannot be
//! told apart from "never added"; it is treated as "assume nothing is owned
//! yet" so an old, hand-deleted profile is never silently resurrected -- the
//! safe direction, matching #50's own resolution.
//!
//! One deliberate behavioural gap between naner.json's two consumers of the
//! same `Profiles` data: `naner --profile X` (`launcher::build_terminal_arguments`)
//! sets up the naner environment on itself before it ever spawns `wt.exe`,
//! so `CustomShell.Arguments` for a PowerShell profile just sources
//! `profile.ps1` directly. A profile picked straight from Windows
//! Terminal's own profile list (double-click, pinned tile, WT's own "+"
//! menu) starts `pwsh.exe` cold, with none of that -- so the generated
//! `commandline` here splices in a self-bootstrap
//! (`with_export_env_bootstrap`) that `naner --profile X` does not need and
//! does not get.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::config::{self, strip_json_comments};
use crate::fs_atomic::{back_up, write_atomic};
use crate::logger;

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

/// Naner's four built-in profiles, keyed by their `naner.json` `Profiles`
/// entry, each paired with the GUID the original hand-maintained template
/// shipped. WT locates a profile by GUID, not name or position, so these
/// can never change without resurrecting every profile a user ever deleted
/// (#50, #83) -- a `naner.json` profile with no entry here is simply not a
/// profile the generator can place in Windows Terminal.
const FIXED_GUIDS: [(&str, &str); 4] = [
    ("Unified", "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}"),
    ("PowerShell", "{574e775e-4f2a-5b96-ac1e-a2962a402336}"),
    ("Bash", "{2c4de342-38b7-51cf-b940-2309a097f518}"),
    ("CMD", "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}"),
];

fn fixed_guid(profile_key: &str) -> Option<&'static str> {
    FIXED_GUIDS
        .iter()
        .find(|(key, _)| *key == profile_key)
        .map(|(_, guid)| *guid)
}

/// naner.exe's own `wt.exe --` launch already has the environment set on
/// itself before wt.exe starts (`launcher::setup_path_environment` runs
/// first), so naner.json's own `CustomShell.Arguments` for a PowerShell
/// profile just sources `profile.ps1` directly -- see the module doc. A
/// profile launched straight from Windows Terminal needs the same
/// self-bootstrap the old hand-maintained template always carried for these
/// two profiles: run `naner.exe --export-env --no-comments |
/// Invoke-Expression` before whatever `-Command` naner.json's Arguments
/// already specifies. Spliced in right after the opening quote of that
/// `-Command` body rather than rebuilt from scratch, so a hand-written,
/// unusual Arguments string is only ever extended, never reinterpreted.
fn with_export_env_bootstrap(args: &str, naner_exe: &str) -> String {
    let marker = "-Command \"";
    let Some(range) = crate::paths::match_ranges_ignore_case(args, marker)
        .into_iter()
        .next()
    else {
        // Doesn't shape up as `...-Command "<body>"` -- leave it alone
        // rather than guess where a bootstrap would even belong.
        return args.to_string();
    };
    let bootstrap = format!("& '{naner_exe}' --export-env --no-comments | Invoke-Expression; ");
    format!("{}{bootstrap}{}", &args[..range.end], &args[range.end..])
}

/// WT's `commandline` is tokenized like a shell command line: an unquoted
/// executable path breaks the moment `%NANER_ROOT%` expands to somewhere
/// containing a space.
fn quote_if_needed(path: &str) -> String {
    if path.chars().any(char::is_whitespace) {
        format!("\"{path}\"")
    } else {
        path.to_string()
    }
}

/// What `update_settings` actually did, so the caller can log something more
/// specific than "configured".
#[derive(Debug, PartialEq, Eq)]
pub enum SettingsOutcome {
    /// No `settings.json` existed yet; the full template was written.
    Created,
    /// An existing file was inspected and every Naner profile it should have
    /// already matches naner.json -- nothing written.
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

    /// `CreateSettings`: the generated skeleton, with Naner's profiles
    /// freshly built from `naner.json`. No existing file to reconcile
    /// against, so this always writes fresh.
    pub fn create_settings(&self, settings_path: &Path) -> std::io::Result<()> {
        std::fs::write(settings_path, self.rendered_template()?)?;
        let guids = self.template_naner_guids();
        self.write_managed_guids(settings_path, &guids)?;
        Ok(())
    }

    /// Reconcile Naner's profiles into an existing `settings.json`, or write
    /// the generated skeleton fresh if none exists yet.
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

        let template_profiles = self.template_naner_profiles();
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

    /// The full `settings.json` text: `DEFAULT_SETTINGS`'s skeleton
    /// ($schema, keybindings, colour schemes, ...) with `profiles.list` and
    /// `defaultProfile` replaced by what naner.json currently describes. A
    /// `naner.json` that fails to load leaves the skeleton's own single
    /// built-in fallback profile untouched, matching what a missing
    /// template used to do.
    fn rendered_template(&self) -> std::io::Result<String> {
        let mut skeleton: Value = serde_json::from_str(DEFAULT_SETTINGS)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let profiles = self.template_naner_profiles();
        if !profiles.is_empty()
            && let Value::Object(root) = &mut skeleton
        {
            let list: Vec<Value> = profiles.into_iter().map(|(_, v)| v).collect();
            if let Some(Value::Object(profiles_obj)) = root.get_mut("profiles") {
                profiles_obj.insert("list".to_string(), Value::Array(list));
            }
            if let Some(default_guid) = self.default_profile_guid() {
                root.insert("defaultProfile".to_string(), Value::String(default_guid));
            }
        }

        serde_json::to_string_pretty(&skeleton)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    fn default_profile_guid(&self) -> Option<String> {
        let config = config::load(self.naner_root, None).ok()?;
        fixed_guid(&config.default_profile).map(str::to_string)
    }

    /// Naner's own profile objects, generated fresh from `naner.json`'s own
    /// `Profiles` -- the single source of truth per #83 -- keyed by GUID. A
    /// `naner.json` that cannot be loaded yields no profiles rather than
    /// erroring the whole Windows Terminal install; the caller's fallback
    /// (an empty list changes nothing) is the safe default, matching what a
    /// missing template used to do.
    fn template_naner_profiles(&self) -> Vec<(String, Value)> {
        let Ok(config) = config::load(self.naner_root, None) else {
            return Vec::new();
        };
        let root = self.naner_root.to_string_lossy();
        let naner_exe = format!("{root}\\vendor\\bin\\naner.exe");

        FIXED_GUIDS
            .iter()
            .filter_map(|(key, guid)| {
                let profile = config.profiles.get(key)?;
                let custom = profile.custom_shell.as_ref()?;
                if custom.executable_path.trim().is_empty() {
                    return None;
                }

                let exe = custom.executable_path.replace("%NANER_ROOT%", &root);
                let mut args = custom
                    .arguments
                    .as_deref()
                    .unwrap_or_default()
                    .replace("%NANER_ROOT%", &root);
                if profile.shell.eq_ignore_ascii_case("PowerShell") {
                    args = with_export_env_bootstrap(&args, &naner_exe);
                }

                let commandline = if args.is_empty() {
                    quote_if_needed(&exe)
                } else {
                    format!("{} {args}", quote_if_needed(&exe))
                };

                let mut obj = Map::new();
                obj.insert("guid".to_string(), Value::String((*guid).to_string()));
                obj.insert("name".to_string(), Value::String(profile.name.clone()));
                obj.insert("hidden".to_string(), Value::Bool(false));
                obj.insert("commandline".to_string(), Value::String(commandline));
                obj.insert(
                    "startingDirectory".to_string(),
                    Value::String(profile.starting_directory.replace("%NANER_ROOT%", &root)),
                );
                obj.insert(
                    "colorScheme".to_string(),
                    Value::String(profile.color_scheme.clone()),
                );
                if let Some(icon) = &profile.icon {
                    obj.insert(
                        "icon".to_string(),
                        Value::String(icon.replace("%NANER_ROOT%", &root)),
                    );
                }

                Some(((*guid).to_string(), Value::Object(obj)))
            })
            .collect()
    }

    fn template_naner_guids(&self) -> Vec<String> {
        self.template_naner_profiles()
            .into_iter()
            .map(|(guid, _)| guid)
            .collect()
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

/// Last-resort skeleton: used whole when `naner.json` cannot be loaded at
/// all (its own single profile is the fallback), and as the base every
/// other field (`$schema`, keybindings, colour schemes, `newTabMenu`) comes
/// from even on a normal install, with `profiles.list`/`defaultProfile`
/// replaced by what naner.json currently describes.
const DEFAULT_SETTINGS: &str = r#"{
    "$schema": "https://aka.ms/terminal-profiles-schema",
    "defaultProfile": "{naner-unified}",
    "copyOnSelect": false,
    "copyFormatting": "none",
    "keybindings": [
        { "id": "Terminal.CopyToClipboard", "keys": "ctrl+c" },
        { "id": "Terminal.PasteFromClipboard", "keys": "ctrl+v" },
        { "id": "Terminal.FindText", "keys": "ctrl+shift+f" },
        { "id": "Terminal.DuplicatePaneAuto", "keys": "alt+shift+d" }
    ],
    "newTabMenu": [
        { "type": "remainingProfiles" }
    ],
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

    #[test]
    fn guids_are_fixed_and_never_derived() {
        // A regression guard, not a design decision to relitigate: swapping
        // any of these breaks every already-installed settings.json (#50).
        assert_eq!(
            fixed_guid("Unified"),
            Some("{61c54bbd-c2c6-5271-96e7-009a87ff44bf}")
        );
        assert_eq!(
            fixed_guid("PowerShell"),
            Some("{574e775e-4f2a-5b96-ac1e-a2962a402336}")
        );
        assert_eq!(
            fixed_guid("Bash"),
            Some("{2c4de342-38b7-51cf-b940-2309a097f518}")
        );
        assert_eq!(
            fixed_guid("CMD"),
            Some("{0caa0dad-35be-5f56-a8ff-afceeeaa6101}")
        );
        assert_eq!(fixed_guid("SomeNewProfile"), None);
    }

    #[test]
    fn export_env_bootstrap_is_spliced_after_the_command_flag() {
        let args = r#"-NoExit -NoLogo -NoProfile -Command ". 'profile.ps1'""#;
        let spliced = with_export_env_bootstrap(args, "naner.exe");
        assert!(
            spliced.starts_with(
                r#"-NoExit -NoLogo -NoProfile -Command "& 'naner.exe' --export-env --no-comments | Invoke-Expression; . 'profile.ps1'""#
            ),
            "{spliced}"
        );
    }

    #[test]
    fn export_env_bootstrap_leaves_non_command_arguments_alone() {
        // Bash/CMD-shaped Arguments never reach this function in practice
        // (only PowerShell-shell profiles call it), but the function itself
        // must not guess when there is nothing to splice into.
        assert_eq!(
            with_export_env_bootstrap("--login -i", "naner.exe"),
            "--login -i"
        );
        assert_eq!(with_export_env_bootstrap("", "naner.exe"), "");
    }

    #[test]
    fn quoting_only_happens_when_needed() {
        assert_eq!(quote_if_needed("cmd.exe"), "cmd.exe");
        assert_eq!(
            quote_if_needed(r"C:\tools\naner\pwsh.exe"),
            r"C:\tools\naner\pwsh.exe"
        );
        assert_eq!(
            quote_if_needed(r"C:\Users\Bailey RD\naner\pwsh.exe"),
            r#""C:\Users\Bailey RD\naner\pwsh.exe""#
        );
    }

    const MINIMAL_NANER_JSON: &str = r#"{
        "DefaultProfile": "Unified",
        "Profiles": {
            "Unified": {
                "Name": "Naner (Unified)",
                "Shell": "PowerShell",
                "StartingDirectory": "%USERPROFILE%",
                "ColorScheme": "Campbell",
                "CustomShell": {
                    "ExecutablePath": "%NANER_ROOT%\\vendor\\powershell\\pwsh.exe",
                    "Arguments": "-NoExit -NoLogo -NoProfile -Command \". '%NANER_ROOT%\\home\\.config\\powershell\\profile.ps1'\""
                }
            },
            "Bash": {
                "Name": "Naner Bash",
                "Shell": "Bash",
                "StartingDirectory": "~",
                "ColorScheme": "Campbell",
                "CustomShell": {
                    "ExecutablePath": "%NANER_ROOT%\\vendor\\git\\bin\\bash.exe",
                    "Arguments": "--login -i"
                }
            }
        }
    }"#;

    const SIMPLE_NANER_JSON: &str = r#"{
        "DefaultProfile": "Unified",
        "Profiles": {
            "Unified": {
                "Name": "Naner (Unified)",
                "Shell": "PowerShell",
                "CustomShell": { "ExecutablePath": "new-pwsh.exe" }
            }
        }
    }"#;

    fn write_naner_config(root: &Path, body: &str) {
        let config_dir = root.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("naner.json"), body).unwrap();
    }

    #[test]
    fn naner_root_is_substituted_in_generated_profiles() {
        let root = tempfile::tempdir().unwrap();
        write_naner_config(root.path(), MINIMAL_NANER_JSON);

        let target = root.path().join("vendor/terminal");
        std::fs::create_dir_all(&target).unwrap();
        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();

        assert!(target.join(".portable").is_file());
        let written = std::fs::read_to_string(target.join("settings/settings.json")).unwrap();
        assert!(!written.contains("%NANER_ROOT%"));
        assert!(written.contains("powershell"));
        assert!(written.contains("bash.exe"));
    }

    #[test]
    fn missing_naner_json_writes_the_default_fallback() {
        let root = tempfile::tempdir().unwrap();
        // No config/naner.json at all.
        let target = root.path().join("terminal");
        std::fs::create_dir_all(&target).unwrap();
        WindowsTerminalConfigurator::new(root.path())
            .configure_portable_mode(&target)
            .unwrap();
        let written = std::fs::read_to_string(target.join("settings/settings.json")).unwrap();
        assert!(written.contains("{naner-unified}"));
    }

    /// naner.json's two profiles are absent from a hand-written file with no
    /// prior marker -- "never added", the safe interpretation -- so both are
    /// added. Everything the user already had, including a field naner
    /// knows nothing about, survives untouched.
    #[test]
    fn missing_naner_profiles_are_added_and_the_rest_of_the_file_survives() {
        let root = tempfile::tempdir().unwrap();
        write_naner_config(root.path(), MINIMAL_NANER_JSON);

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
        assert_eq!(list.len(), 3);
        assert!(
            list.iter()
                .any(|p| p["guid"] == "{someone-elses}" && p["name"] == "Their Shell"),
            "a third-party profile must survive the merge: {list:?}"
        );
        assert!(
            list.iter()
                .any(|p| p["guid"] == "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}"),
            "the missing Unified profile must be added: {list:?}"
        );
        assert!(
            list.iter()
                .any(|p| p["guid"] == "{2c4de342-38b7-51cf-b940-2309a097f518}"),
            "the missing Bash profile must be added: {list:?}"
        );
    }

    /// A Naner profile the user has hand-customised is refreshed back to
    /// what naner.json currently describes on update -- "config changes
    /// reach an existing install" is the entire point of #52 and #83.
    #[test]
    fn a_present_naner_profile_is_refreshed_to_match_naner_json() {
        let root = tempfile::tempdir().unwrap();
        write_naner_config(root.path(), SIMPLE_NANER_JSON);

        let target = root.path().join("vendor/terminal");
        std::fs::create_dir_all(target.join("settings")).unwrap();
        let settings = target.join("settings/settings.json");
        std::fs::write(
            &settings,
            r#"{ "profiles": { "list": [
                { "guid": "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}", "name": "Naner (Unified)", "commandline": "old-pwsh.exe" }
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
        write_naner_config(root.path(), MINIMAL_NANER_JSON);

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
                .filter(|p| p["guid"] != "{2c4de342-38b7-51cf-b940-2309a097f518}")
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
        assert_eq!(list[0]["guid"], "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}");
    }

    /// A file naner cannot parse is never overwritten -- the file might be
    /// mid hand-edit, and a merge that cannot understand it must say so
    /// rather than replace it with naner's best guess.
    #[test]
    fn an_unparseable_settings_file_is_left_exactly_as_found() {
        let root = tempfile::tempdir().unwrap();
        write_naner_config(root.path(), SIMPLE_NANER_JSON);

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
        write_naner_config(root.path(), SIMPLE_NANER_JSON);

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
        write_naner_config(root.path(), SIMPLE_NANER_JSON);

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
