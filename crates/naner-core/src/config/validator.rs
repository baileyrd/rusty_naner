//! Port of `ConfigurationValidator`: errors block, warnings are logged and
//! tolerated. Messages preserved verbatim — they are user-visible output.

use std::path::Path;

use super::{NanerConfig, ProfileConfig};
use crate::paths;

/// Validation outcome: errors fail the load; warnings go to the logger.
#[derive(Debug, Default)]
pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// `ThrowIfInvalid` message shape.
    pub fn error_message(&self) -> Option<String> {
        if self.errors.is_empty() {
            return None;
        }
        Some(format!(
            "Configuration validation failed with {} error(s):\n{}",
            self.errors.len(),
            self.errors
                .iter()
                .map(|e| format!("  - {e}"))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

/// Validate a configuration against a root directory.
pub fn validate(config: &NanerConfig, naner_root: &str) -> ValidationReport {
    let mut report = ValidationReport::default();
    validate_default_profile(config, &mut report);
    validate_profiles(config, naner_root, &mut report);
    validate_vendor_paths(config, naner_root, &mut report);
    validate_environment(config, naner_root, &mut report);
    report
}

fn validate_default_profile(config: &NanerConfig, report: &mut ValidationReport) {
    if config.default_profile.trim().is_empty() {
        report.errors.push("DefaultProfile cannot be empty".into());
        return;
    }
    if !config.profiles.contains_key(&config.default_profile)
        && !config.custom_profiles.contains_key(&config.default_profile)
    {
        report.errors.push(format!(
            "DefaultProfile '{}' does not exist in Profiles or CustomProfiles",
            config.default_profile
        ));
    }
}

fn validate_profiles(config: &NanerConfig, naner_root: &str, report: &mut ValidationReport) {
    if config.profiles.is_empty() && config.custom_profiles.is_empty() {
        report
            .errors
            .push("At least one profile must be defined in Profiles or CustomProfiles".into());
        return;
    }

    for (name, profile) in &config.profiles {
        validate_profile(name, profile, "Profiles", naner_root, report);
    }
    for (name, profile) in &config.custom_profiles {
        validate_profile(name, profile, "CustomProfiles", naner_root, report);
    }

    // Duplicates across the two maps, compared case-insensitively.
    let duplicates: Vec<&String> = config
        .profiles
        .keys()
        .filter(|k| {
            config
                .custom_profiles
                .keys()
                .any(|c| c.eq_ignore_ascii_case(k))
        })
        .collect();
    if !duplicates.is_empty() {
        report.warnings.push(format!(
            "Profile names appear in both Profiles and CustomProfiles: {}",
            duplicates
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

fn validate_profile(
    profile_name: &str,
    profile: &ProfileConfig,
    source: &str,
    naner_root: &str,
    report: &mut ValidationReport,
) {
    let prefix = format!("{source}[{profile_name}]");

    if profile.name.trim().is_empty() {
        report.errors.push(format!("{prefix}.Name cannot be empty"));
    }

    if profile.shell.trim().is_empty() {
        report
            .errors
            .push(format!("{prefix}.Shell cannot be empty"));
    } else {
        const VALID_SHELLS: [&str; 4] = ["PowerShell", "Bash", "CMD", "Custom"];
        if !VALID_SHELLS
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&profile.shell))
        {
            report.warnings.push(format!(
                "{prefix}.Shell '{}' is not a standard shell type (PowerShell, Bash, CMD, Custom)",
                profile.shell
            ));
        }

        if profile.shell.eq_ignore_ascii_case("Custom") {
            match &profile.custom_shell {
                None => report.errors.push(format!(
                    "{prefix}.CustomShell must be specified when Shell is 'Custom'"
                )),
                Some(custom) if custom.executable_path.trim().is_empty() => report.errors.push(
                    format!("{prefix}.CustomShell.ExecutablePath cannot be empty"),
                ),
                Some(_) => {}
            }
        }
    }

    // Additive check: `Terminal` has no C# counterpart; absent/empty means
    // Windows Terminal, unknown values warn (like nonstandard Shell values).
    if let Some(terminal) = &profile.terminal
        && !terminal.trim().is_empty()
    {
        const VALID_TERMINALS: [&str; 2] = ["WindowsTerminal", "RustyTerm"];
        if !VALID_TERMINALS
            .iter()
            .any(|t| t.eq_ignore_ascii_case(terminal))
        {
            report.warnings.push(format!(
                "{prefix}.Terminal '{terminal}' is not a recognized terminal type (WindowsTerminal, RustyTerm)"
            ));
        }
    }

    if profile.starting_directory.trim().is_empty() {
        report
            .errors
            .push(format!("{prefix}.StartingDirectory cannot be empty"));
    }

    if let Some(icon) = &profile.icon
        && !icon.trim().is_empty()
    {
        let icon_path = paths::expand_naner_path(icon, naner_root);
        if !Path::new(&icon_path).is_file() && !icon_path.contains('%') {
            report
                .warnings
                .push(format!("{prefix}.Icon file does not exist: {icon_path}"));
        }
    }
}

fn validate_vendor_paths(config: &NanerConfig, naner_root: &str, report: &mut ValidationReport) {
    if config.vendor_paths.is_empty() {
        report
            .warnings
            .push("VendorPaths is empty - no vendor tools are configured".into());
        return;
    }

    for (vendor, path) in &config.vendor_paths {
        if path.trim().is_empty() {
            report
                .errors
                .push(format!("VendorPaths[{vendor}] cannot be empty"));
            continue;
        }
        // C# checks Directory.Exists on what are typically FILE paths, so
        // this warns for existing exes too. Bug-for-bug preserved
        // (MIGRATION_ANALYSIS §3 stance: port observable behavior first).
        let expanded = paths::expand_naner_path(path, naner_root);
        if !Path::new(&expanded).is_dir() && !expanded.contains('%') {
            report.warnings.push(format!(
                "VendorPaths[{vendor}] directory does not exist: {expanded}"
            ));
        }
    }
}

fn validate_environment(config: &NanerConfig, naner_root: &str, report: &mut ValidationReport) {
    if config.environment.path_precedence.is_empty() {
        report.warnings.push(
            "Environment.PathPrecedence is empty - no custom paths will be added to PATH".into(),
        );
    } else {
        for (i, path) in config.environment.path_precedence.iter().enumerate() {
            if path.trim().is_empty() {
                report
                    .errors
                    .push(format!("Environment.PathPrecedence[{i}] cannot be empty"));
                continue;
            }
            let expanded = paths::expand_naner_path(path, naner_root);
            if !Path::new(&expanded).is_dir() && !expanded.contains('%') {
                report.warnings.push(format!(
                    "Environment.PathPrecedence[{i}] directory does not exist: {expanded}"
                ));
            }
        }
    }

    for key in config.environment.environment_variables.keys() {
        if key.trim().is_empty() {
            report
                .errors
                .push("Environment variable name cannot be empty".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_json;

    fn valid_config() -> NanerConfig {
        load_json(
            r#"{
                "DefaultProfile": "P",
                "Profiles": { "P": { "Name": "P", "Shell": "PowerShell" } },
                "VendorPaths": { "X": "%SOMEVAR%\\x" },
                "Environment": { "PathPrecedence": ["%SOMEVAR%\\bin"] }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn valid_config_passes() {
        let report = validate(&valid_config(), "/nonexistent-root");
        assert!(report.is_valid(), "errors: {:?}", report.errors);
        // Paths containing % (unexpanded) don't warn.
        assert!(
            report.warnings.is_empty(),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn missing_default_profile_is_an_error() {
        let mut config = valid_config();
        config.default_profile = "Nope".into();
        let report = validate(&config, "/root");
        assert!(report.errors.contains(
            &"DefaultProfile 'Nope' does not exist in Profiles or CustomProfiles".to_string()
        ));
    }

    #[test]
    fn no_profiles_is_an_error() {
        let config = load_json(r#"{ "DefaultProfile": "X" }"#).unwrap();
        let report = validate(&config, "/root");
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.contains("At least one profile"))
        );
    }

    #[test]
    fn custom_shell_required_for_custom() {
        let config = load_json(
            r#"{
                "DefaultProfile": "C",
                "Profiles": { "C": { "Name": "C", "Shell": "Custom" } }
            }"#,
        )
        .unwrap();
        let report = validate(&config, "/root");
        assert!(report.errors.contains(
            &"Profiles[C].CustomShell must be specified when Shell is 'Custom'".to_string()
        ));
    }

    #[test]
    fn nonstandard_shell_is_a_warning_not_error() {
        let config = load_json(
            r#"{
                "DefaultProfile": "R",
                "Profiles": { "R": { "Name": "R", "Shell": "rush" } }
            }"#,
        )
        .unwrap();
        let report = validate(&config, "/root");
        assert!(report.is_valid());
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("'rush' is not a standard shell type"))
        );
    }

    #[test]
    fn unknown_terminal_is_a_warning_not_error() {
        let config = load_json(
            r#"{
                "DefaultProfile": "T",
                "Profiles": { "T": { "Name": "T", "Shell": "PowerShell", "Terminal": "Kitty" } }
            }"#,
        )
        .unwrap();
        let report = validate(&config, "/root");
        assert!(report.is_valid());
        assert!(report.warnings.contains(
            &"Profiles[T].Terminal 'Kitty' is not a recognized terminal type (WindowsTerminal, RustyTerm)"
                .to_string()
        ));
    }

    #[test]
    fn recognized_terminals_do_not_warn() {
        for terminal in ["WindowsTerminal", "RustyTerm", "rustyterm", ""] {
            let config = load_json(&format!(
                r#"{{
                    "DefaultProfile": "T",
                    "Profiles": {{ "T": {{ "Name": "T", "Shell": "PowerShell", "Terminal": "{terminal}" }} }}
                }}"#,
            ))
            .unwrap();
            let report = validate(&config, "/root");
            assert!(report.is_valid());
            assert!(
                !report.warnings.iter().any(|w| w.contains(".Terminal")),
                "unexpected warning for {terminal:?}: {:?}",
                report.warnings
            );
        }
    }

    #[test]
    fn missing_dirs_warn_after_expansion() {
        let config = load_json(
            r#"{
                "DefaultProfile": "P",
                "Profiles": { "P": { "Name": "P" } },
                "Environment": { "PathPrecedence": ["%NANER_ROOT%\\definitely-missing"] }
            }"#,
        )
        .unwrap();
        let report = validate(&config, "/tmp/no-such-root");
        assert!(report.is_valid());
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("Environment.PathPrecedence[0] directory does not exist"))
        );
    }

    #[test]
    fn error_message_shape() {
        let config = load_json(r#"{ "DefaultProfile": "" }"#).unwrap();
        let report = validate(&config, "/root");
        let msg = report.error_message().unwrap();
        assert!(msg.starts_with("Configuration validation failed with"));
        assert!(msg.contains("  - DefaultProfile cannot be empty"));
    }
}
