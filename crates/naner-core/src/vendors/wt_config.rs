//! Port of `WindowsTerminalConfigurator`: `.portable` marker +
//! `settings/settings.json` from the home template with string-level
//! `%NANER_ROOT%` substitution (backslashes doubled for JSON). Deliberately
//! NOT a JSON round-trip — WT settings are JSONC and a round-trip would lose
//! comments (MIGRATION_ANALYSIS §4.4).

use std::path::Path;

use crate::logger;

/// `IsWindowsTerminal`: substring match on the display name.
pub fn is_windows_terminal(vendor_name: &str) -> bool {
    vendor_name
        .to_lowercase()
        .contains(&"Windows Terminal".to_lowercase())
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
        self.create_settings(&settings_dir.join("settings.json"))?;
        logger::info("    Created settings/settings.json with Naner profiles");
        Ok(())
    }

    /// `CreateSettings`: template from `home/.config/windows-terminal/
    /// settings.json` with `%NANER_ROOT%` replaced by the root path with
    /// doubled backslashes; inline default otherwise.
    pub fn create_settings(&self, settings_path: &Path) -> std::io::Result<()> {
        let template_path = self
            .naner_root
            .join("home")
            .join(".config")
            .join("windows-terminal")
            .join("settings.json");

        if template_path.is_file() {
            let template = std::fs::read_to_string(&template_path)?;
            let root = self.naner_root.to_string_lossy().replace('\\', "\\\\");
            std::fs::write(settings_path, template.replace("%NANER_ROOT%", &root))?;
        } else {
            std::fs::write(settings_path, DEFAULT_SETTINGS)?;
        }
        Ok(())
    }
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

    #[test]
    fn template_substitution_doubles_backslashes() {
        let root = tempfile::tempdir().unwrap();
        let template_dir = root.path().join("home/.config/windows-terminal");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::write(
            template_dir.join("settings.json"),
            r#"{ "commandline": "%NANER_ROOT%\\vendor\\powershell\\pwsh.exe" }"#,
        )
        .unwrap();

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
}
