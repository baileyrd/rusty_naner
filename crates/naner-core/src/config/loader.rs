//! Port of `ConfigurationProviderService` + `ConfigurationManager.Load`:
//! find exactly one config file (search order `naner.json` → `naner.yaml` →
//! `naner.yml`, no cross-file merging), parse it by extension, apply env-var
//! overrides, expand `%NANER_ROOT%`/env placeholders through the whole
//! config, then validate (warnings logged, errors fail).

use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use super::{NanerConfig, apply_env_overrides, validate};
use crate::{constants, logger, paths};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// No file in the search order exists.
    #[error(
        "No configuration file found. Please create naner.json or naner.yaml in the config directory."
    )]
    NotFound,

    /// An explicit path was given but no provider handles its extension.
    #[error("No configuration provider found for: {0}. Supported formats: JSON, YAML")]
    UnsupportedFormat(String),

    #[error("Configuration file not found: {0}")]
    FileNotFound(String),

    #[error("{0}")]
    Parse(String),

    /// Validation errors (the `ThrowIfInvalid` message).
    #[error("{0}")]
    Invalid(String),

    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Find the first configuration file in `<root>/config` per the search
/// order (`FindConfigurationFile`).
pub fn find_configuration_file(naner_root: &Path) -> Option<PathBuf> {
    let config_dir = naner_root.join(constants::directory_names::CONFIG);
    constants::CONFIG_FILE_NAMES
        .iter()
        .map(|name| config_dir.join(name))
        .find(|p| p.is_file())
}

/// Load, override, expand, and validate the configuration
/// (`ConfigurationManager.Load`). `config_path` overrides the search.
pub fn load(naner_root: &Path, config_path: Option<&Path>) -> Result<NanerConfig, ConfigError> {
    let path = match config_path {
        Some(p) => p.to_path_buf(),
        None => find_configuration_file(naner_root).ok_or(ConfigError::NotFound)?,
    };

    let mut config = load_file(&path)?;

    // Env-var overrides are the highest-priority provider.
    apply_env_overrides(&mut config);

    // Expand placeholders everywhere the C# ExpandConfigPaths does.
    let root = naner_root.to_string_lossy();
    expand_config_paths(&mut config, &root);

    // Validate: warnings are logged (stderr), errors abort.
    let report = validate(&config, &root);
    for warning in &report.warnings {
        logger::warning(&format!("Configuration validation warning: {warning}"));
    }
    if let Some(message) = report.error_message() {
        return Err(ConfigError::Invalid(message));
    }

    Ok(config)
}

fn load_file(path: &Path) -> Result<NanerConfig, ConfigError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "json" | "yaml" | "yml" => {}
        _ => return Err(ConfigError::UnsupportedFormat(path.display().to_string())),
    }

    if !path.is_file() {
        return Err(ConfigError::FileNotFound(path.display().to_string()));
    }
    let content = std::fs::read_to_string(path)?;

    match ext.as_str() {
        "json" => super::load_json(&content).map_err(|e| ConfigError::Parse(e.to_string())),
        _ => super::load_yaml(&content)
            .map_err(|e| ConfigError::Parse(format!("Invalid YAML format: {e}"))),
    }
}

/// `ConfigurationManager.ExpandConfigPaths`: vendor paths, PATH precedence,
/// and environment-variable values all get the three-pass expansion.
fn expand_config_paths(config: &mut NanerConfig, naner_root: &str) {
    let expanded: IndexMap<String, String> = config
        .vendor_paths
        .iter()
        .map(|(k, v)| (k.clone(), paths::expand_naner_path(v, naner_root)))
        .collect();
    config.vendor_paths = expanded;

    for entry in &mut config.environment.path_precedence {
        *entry = paths::expand_naner_path(entry, naner_root);
    }

    let expanded: IndexMap<String, String> = config
        .environment
        .environment_variables
        .iter()
        .map(|(k, v)| (k.clone(), paths::expand_naner_path(v, naner_root)))
        .collect();
    config.environment.environment_variables = expanded;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root(config_files: &[(&str, &str)]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("config")).unwrap();
        for (name, content) in config_files {
            std::fs::write(tmp.path().join("config").join(name), content).unwrap();
        }
        tmp
    }

    const MINIMAL_JSON: &str = r#"{
        "DefaultProfile": "P",
        "Profiles": { "P": { "Name": "P" } },
        "VendorPaths": { "Tool": "%NANER_ROOT%\\vendor\\tool" },
        "Environment": {
            "PathPrecedence": ["%NANER_ROOT%\\bin"],
            "EnvironmentVariables": { "NANER_HOME": "%NANER_ROOT%\\home" }
        }
    }"#;

    #[test]
    fn json_wins_the_search_order() {
        let tmp = fixture_root(&[
            ("naner.json", MINIMAL_JSON),
            ("naner.yaml", "DefaultProfile: FromYaml\n"),
        ]);
        let found = find_configuration_file(tmp.path()).unwrap();
        assert!(found.ends_with("config/naner.json"));

        let config = load(tmp.path(), None).unwrap();
        assert_eq!(config.default_profile, "P");
    }

    #[test]
    fn yaml_is_the_fallback() {
        let yaml = "DefaultProfile: P\nProfiles:\n  P:\n    Name: P\n";
        let tmp = fixture_root(&[("naner.yaml", yaml)]);
        let config = load(tmp.path(), None).unwrap();
        assert_eq!(config.default_profile, "P");
    }

    #[test]
    fn missing_config_is_the_exact_error() {
        let tmp = fixture_root(&[]);
        let err = load(tmp.path(), None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "No configuration file found. Please create naner.json or naner.yaml in the config directory."
        );
    }

    #[test]
    fn explicit_path_with_unknown_extension_fails() {
        let tmp = fixture_root(&[]);
        let bogus = tmp.path().join("config/naner.toml");
        std::fs::write(&bogus, "x").unwrap();
        let err = load(tmp.path(), Some(&bogus)).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("No configuration provider found for:")
        );
    }

    #[test]
    fn placeholders_are_expanded_throughout() {
        let tmp = fixture_root(&[("naner.json", MINIMAL_JSON)]);
        let config = load(tmp.path(), None).unwrap();
        let root = tmp.path().to_string_lossy();

        assert_eq!(config.vendor_paths["Tool"], format!("{root}\\vendor\\tool"));
        assert_eq!(
            config.environment.path_precedence[0],
            format!("{root}\\bin")
        );
        assert_eq!(
            config.environment.environment_variables["NANER_HOME"],
            format!("{root}\\home")
        );
    }

    #[test]
    fn validation_errors_abort_the_load() {
        let bad = r#"{ "DefaultProfile": "Ghost", "Profiles": { "P": { "Name": "P" } } }"#;
        let tmp = fixture_root(&[("naner.json", bad)]);
        let err = load(tmp.path(), None).unwrap_err();
        assert!(err.to_string().contains("Configuration validation failed"));
        assert!(err.to_string().contains("Ghost"));
    }
}
