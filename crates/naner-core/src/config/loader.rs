//! Port of `ConfigurationProviderService` + `ConfigurationManager.Load`:
//! find the config file (`naner.json`), parse it, apply env-var overrides,
//! expand `%NANER_ROOT%`/env placeholders through the whole config, then
//! validate (warnings logged, errors fail).
//!
//! JSON is the only supported format. The YAML alternative was dropped along
//! with the shipped `naner.yaml` twin, which had silently drifted out of sync
//! with `naner.json` -- two files describing the same thing, only ever one of
//! them loaded.

use std::path::{Path, PathBuf};

use crate::collections::OrderedMap;

use super::{NanerConfig, apply_env_overrides, validate};
use crate::{constants, logger, paths};

#[derive(Debug)]
pub enum ConfigError {
    /// No file in the search order exists.
    NotFound,

    /// No supported file exists, but a pre-v0.7.0 YAML config does. Its own
    /// variant rather than a NotFound footnote because the failure is quiet
    /// by nature -- the tree looks configured to its owner -- and the fix is
    /// specific: convert the named file, not create one from scratch.
    LegacyYaml(PathBuf),

    /// An explicit path was given but its extension is not `.json`.
    UnsupportedFormat(String),

    FileNotFound(String),

    Parse(String),

    /// Validation errors (the `ThrowIfInvalid` message).
    Invalid(String),

    Io(std::io::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NotFound => f.write_str(
                "No configuration file found. Please create naner.json in the config directory.",
            ),
            ConfigError::LegacyYaml(path) => write!(
                f,
                "{} is a YAML configuration, which naner no longer reads (since \
                 v0.7.0). Convert it to config/naner.json -- same structure, JSON \
                 syntax -- and remove the YAML file.",
                path.display()
            ),
            ConfigError::UnsupportedFormat(path) => write!(
                f,
                "No configuration provider found for: {path}. Supported formats: JSON"
            ),
            ConfigError::FileNotFound(path) => write!(f, "Configuration file not found: {path}"),
            ConfigError::Parse(msg) | ConfigError::Invalid(msg) => f.write_str(msg),
            ConfigError::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        ConfigError::Io(err)
    }
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
        None => match find_configuration_file(naner_root) {
            Some(found) => found,
            None => return Err(not_found_error(naner_root)),
        },
    };

    let mut config = load_file(&path)?;

    // Env-var overrides are the highest-priority provider.
    apply_env_overrides(&mut config);

    // Fold in what each vendor contributes to the environment. Done here,
    // inside `load`, rather than at each call site: six places read
    // `config.environment` and every one of them wants the merged view, so
    // making it the only view is what keeps them from drifting apart.
    // `load_verbatim` deliberately skips this -- tooling that writes the
    // user's file back must not bake vendor entries into it.
    merge_vendor_environment(&mut config, naner_root);

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

/// The error for a tree with no loadable configuration: [`ConfigError::LegacyYaml`]
/// naming the file when a pre-v0.7.0 YAML config is sitting where the JSON
/// should be, plain [`ConfigError::NotFound`] otherwise.
fn not_found_error(naner_root: &Path) -> ConfigError {
    let config_dir = naner_root.join(constants::directory_names::CONFIG);
    for name in constants::LEGACY_YAML_CONFIG_FILE_NAMES {
        let candidate = config_dir.join(name);
        if candidate.is_file() {
            return ConfigError::LegacyYaml(candidate);
        }
    }
    ConfigError::NotFound
}

/// Parse a config file exactly as written, with no environment overrides and
/// no placeholder expansion.
///
/// For tooling that rewrites the user's file. `load` folds in `NANER_ENV_*`,
/// `NANER_DEFAULT_PROFILE` and the telemetry opt-out defaults, and expands
/// `%NANER_ROOT%` to a concrete path — all correct for running naner, all
/// wrong to write back to disk, where they would silently become permanent.
pub fn load_verbatim(path: &Path) -> Result<NanerConfig, ConfigError> {
    load_file(path)
}

fn load_file(path: &Path) -> Result<NanerConfig, ConfigError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if ext != "json" {
        return Err(ConfigError::UnsupportedFormat(path.display().to_string()));
    }

    if !path.is_file() {
        return Err(ConfigError::FileNotFound(path.display().to_string()));
    }
    let content = std::fs::read_to_string(path)?;
    super::load_json(&content).map_err(|e| ConfigError::Parse(e.to_string()))
}

/// The marker in `PathPrecedence` that the vendors' own entries replace.
/// A literal element rather than an append, because the entries around it
/// matter: `%NANER_ROOT%\opt` sits *after* the vendors and is meant to stay
/// there, lowest precedence of all.
pub const VENDOR_PATHS_MARKER: &str = "%VENDOR_PATHS%";

/// Merge each enabled vendor's `pathPrecedence` and `environmentVariables`
/// into the config.
///
/// Vendors are ordered by `pathPriority` (lower first, so it wins conflicts --
/// Git for Windows and MSYS2 both ship `bash.exe`), and vendors without one
/// sort after those with one, by key, so the order is total and stable rather
/// than dependent on directory listing order.
///
/// A variable the user set in `naner.json` always wins over a vendor's: the
/// vendor value is a default, the config file is an instruction.
fn merge_vendor_environment(config: &mut NanerConfig, naner_root: &Path) {
    // `load_vendors`, so a vendor switched off contributes nothing: no PATH
    // entry, no variable. `enabled` means "I want this vendor", and installing
    // one requires it, so on a real tree this mostly agrees with what
    // `build_unified_path` already did by dropping directories that do not
    // exist. Where it differs is the case that motivated the change -- a vendor
    // installed and later switched off used to keep its directory on PATH and
    // its variables set, which is precisely what switching it off should stop.
    let mut vendors = crate::vendors::VendorConfigurationLoader::new(naner_root).load_vendors();
    vendors.sort_by(|a, b| match (a.path_priority, b.path_priority) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.key.cmp(&b.key)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.key.cmp(&b.key),
    });

    let vendor_paths: Vec<String> = vendors
        .iter()
        .flat_map(|v| v.path_precedence.iter().cloned())
        .collect();

    match config
        .environment
        .path_precedence
        .iter()
        .position(|e| e.eq_ignore_ascii_case(VENDOR_PATHS_MARKER))
    {
        Some(at) => {
            config
                .environment
                .path_precedence
                .splice(at..=at, vendor_paths);
        }
        None if !vendor_paths.is_empty() => {
            // An older config file predating the marker. Appending is the only
            // defensible guess, and it is a guess -- say so rather than
            // silently giving vendor directories the lowest precedence.
            logger::warning(&format!(
                "Environment.PathPrecedence has no {VENDOR_PATHS_MARKER} entry; \
                 appending vendor paths at the end"
            ));
            config.environment.path_precedence.extend(vendor_paths);
        }
        None => {}
    }

    for vendor in &vendors {
        for (key, value) in &vendor.environment_variables {
            if !config.environment.environment_variables.contains_key(key) {
                config
                    .environment
                    .environment_variables
                    .insert(key.clone(), value.clone());
            }
        }
    }
}

/// `ConfigurationManager.ExpandConfigPaths`: vendor paths, PATH precedence,
/// and environment-variable values all get the three-pass expansion.
fn expand_config_paths(config: &mut NanerConfig, naner_root: &str) {
    let expanded: OrderedMap<String> = config
        .vendor_paths
        .iter()
        .map(|(k, v)| (k.clone(), paths::expand_naner_path(v, naner_root)))
        .collect();
    config.vendor_paths = expanded;

    for entry in &mut config.environment.path_precedence {
        *entry = paths::expand_naner_path(entry, naner_root);
    }

    let expanded: OrderedMap<String> = config
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
    fn json_is_the_only_configuration_file() {
        let tmp = fixture_root(&[
            ("naner.json", MINIMAL_JSON),
            ("naner.yaml", "DefaultProfile: FromYaml\n"),
        ]);
        let found = find_configuration_file(tmp.path()).unwrap();
        assert!(found.ends_with("config/naner.json"));

        let config = load(tmp.path(), None).unwrap();
        assert_eq!(config.default_profile, "P");
    }

    /// YAML support went away with the shipped `naner.yaml` twin. A tree whose
    /// only config is YAML gets told exactly that, by file name, with the fix
    /// -- not a generic "not found" while a good-looking file sits right there.
    #[test]
    fn a_yaml_only_tree_is_told_to_convert_by_name() {
        for legacy in ["naner.yaml", "naner.yml"] {
            let yaml = "DefaultProfile: P\nProfiles:\n  P:\n    Name: P\n";
            let tmp = fixture_root(&[(legacy, yaml)]);
            assert!(find_configuration_file(tmp.path()).is_none());

            let err = load(tmp.path(), None).unwrap_err();
            let ConfigError::LegacyYaml(path) = &err else {
                panic!("expected LegacyYaml, got {err:?}");
            };
            assert!(path.ends_with(format!("config/{legacy}")));
            let message = err.to_string();
            assert!(message.contains(legacy), "message must name the file");
            assert!(
                message.contains("naner.json"),
                "message must say what to convert to"
            );
        }
    }

    /// The YAML hint must not shadow the plain missing-config case.
    #[test]
    fn an_empty_config_dir_still_reports_not_found() {
        let tmp = fixture_root(&[]);
        assert!(matches!(load(tmp.path(), None), Err(ConfigError::NotFound)));
    }

    #[test]
    fn missing_config_is_the_exact_error() {
        let tmp = fixture_root(&[]);
        let err = load(tmp.path(), None).unwrap_err();
        assert_eq!(
            err.to_string(),
            "No configuration file found. Please create naner.json in the config directory."
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

#[cfg(test)]
mod vendor_merge_tests {
    use super::*;

    /// The whole safety claim of moving PATH entries into the vendor files:
    /// the assembled list must be byte-identical to the single ordered list
    /// that `naner.json` used to carry. Reads the real shipped config and the
    /// real shipped vendor directory, so a priority typo fails here.
    #[test]
    fn the_shipped_config_reproduces_the_original_path_order() {
        const EXPECTED: [&str; 27] = [
            "bin",
            "home/.npm-global",
            "home/go/bin",
            "home/.cargo/bin",
            "home/.gem/bin",
            "home/.local/bin",
            "home/.local/Scripts",
            "vendor/bin",
            "vendor/go/bin",
            "vendor/rust/cargo/bin",
            "vendor/rust/rustc/bin",
            "vendor/ruby/bin",
            "vendor/anaconda",
            "vendor/anaconda/Scripts",
            "vendor/anaconda/Library/bin",
            "vendor/nodejs",
            "vendor/uv",
            "vendor/bun",
            "vendor/git/cmd",
            "vendor/git/mingw64/bin",
            "vendor/git/usr/bin",
            "vendor/msys64/mingw64/bin",
            "vendor/msys64/usr/bin",
            "vendor/powershell",
            "vendor/7zip",
            "vendor/dotnet-sdk",
            "opt",
        ];

        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        std::fs::create_dir_all(config.join("vendors")).unwrap();
        std::fs::copy(
            repo.join("dist-assets/config/naner.json"),
            config.join("naner.json"),
        )
        .unwrap();
        for entry in std::fs::read_dir(repo.join("dist-assets/config/vendors")).unwrap() {
            let path = entry.unwrap().path();
            std::fs::copy(
                &path,
                config.join("vendors").join(path.file_name().unwrap()),
            )
            .unwrap();
        }

        // Every shipped vendor is switched on for this test. The assertion is
        // about `pathPriority` being right, and eight of the eleven vendors
        // carrying PATH entries ship `enabled: false` -- reading them as
        // shipped would leave most of the ordering data unexercised.
        for entry in std::fs::read_dir(config.join("vendors")).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(
                &path,
                text.replace("\"enabled\": false", "\"enabled\": true"),
            )
            .unwrap();
        }

        let mut cfg = load_file(&config.join("naner.json")).unwrap();
        merge_vendor_environment(&mut cfg, tmp.path());

        let actual: Vec<String> = cfg
            .environment
            .path_precedence
            .iter()
            .map(|e| e.replace("%NANER_ROOT%\\", "").replace('\\', "/"))
            .collect();
        assert_eq!(actual, EXPECTED, "assembled PATH order drifted");
    }

    /// A vendor switched off contributes nothing at all. Before the split
    /// every entry sat unconditionally in `naner.json`, so a vendor installed
    /// and then disabled kept its directory on PATH and its variables set --
    /// exactly what switching it off is supposed to stop.
    #[test]
    fn a_disabled_vendor_contributes_neither_path_nor_variables() {
        let tmp = tempfile::tempdir().unwrap();
        let vendors = tmp.path().join("config/vendors");
        std::fs::create_dir_all(&vendors).unwrap();
        std::fs::write(
            vendors.join("Off.json"),
            r#"{"Off":{"name":"Off","extractDir":"off","enabled":false,
                 "pathPriority":1,"pathPrecedence":["%NANER_ROOT%\\vendor\\off"],
                 "environmentVariables":{"OFF_HOME":"x"}}}"#,
        )
        .unwrap();

        let mut cfg = NanerConfig {
            environment: crate::config::EnvironmentConfig {
                path_precedence: vec![VENDOR_PATHS_MARKER.to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        merge_vendor_environment(&mut cfg, tmp.path());

        assert!(
            cfg.environment.path_precedence.is_empty(),
            "a disabled vendor must not put a directory on PATH"
        );
        assert!(
            !cfg.environment
                .environment_variables
                .contains_key("OFF_HOME"),
            "a disabled vendor must not set its variables"
        );
    }

    /// A value in `naner.json` is an instruction; a vendor's is a default.
    #[test]
    fn a_user_set_variable_wins_over_a_vendors() {
        let tmp = tempfile::tempdir().unwrap();
        let vendors = tmp.path().join("config/vendors");
        std::fs::create_dir_all(&vendors).unwrap();
        std::fs::write(
            vendors.join("Go.json"),
            r#"{"Go":{"name":"Go","extractDir":"go",
                 "environmentVariables":{"GOROOT":"vendor-default","GOPATH":"vendor-only"}}}"#,
        )
        .unwrap();

        let mut cfg = NanerConfig::default();
        cfg.environment
            .environment_variables
            .insert("GOROOT".into(), "user-set".into());
        merge_vendor_environment(&mut cfg, tmp.path());

        assert_eq!(cfg.environment.environment_variables["GOROOT"], "user-set");
        assert_eq!(
            cfg.environment.environment_variables["GOPATH"],
            "vendor-only"
        );
    }

    /// Lower `pathPriority` runs earlier, and that is what settles a real
    /// conflict: Git for Windows and MSYS2 both ship `bash.exe`.
    #[test]
    fn priority_orders_vendors_and_missing_priority_sorts_last() {
        let tmp = tempfile::tempdir().unwrap();
        let vendors = tmp.path().join("config/vendors");
        std::fs::create_dir_all(&vendors).unwrap();
        for (key, body) in [
            ("Late", r#""pathPriority":90,"pathPrecedence":["late"]"#),
            ("Early", r#""pathPriority":10,"pathPrecedence":["early"]"#),
            ("Unranked", r#""pathPrecedence":["unranked"]"#),
        ] {
            std::fs::write(
                vendors.join(format!("{key}.json")),
                format!(r#"{{"{key}":{{"name":"{key}","extractDir":"{key}",{body}}}}}"#),
            )
            .unwrap();
        }

        let mut cfg = NanerConfig {
            environment: crate::config::EnvironmentConfig {
                path_precedence: vec![VENDOR_PATHS_MARKER.to_string(), "last".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        merge_vendor_environment(&mut cfg, tmp.path());

        assert_eq!(
            cfg.environment.path_precedence,
            vec!["early", "late", "unranked", "last"],
            "priority order, then unranked by key, and the marker's neighbours stay put"
        );
    }

    /// `DOTNET_CLI_TELEMETRY_OPTOUT` used to be force-set in
    /// `apply_env_overrides` *and* declared in `DotNetSDK.json`. Because the
    /// overrides run first and the merge only fills in missing keys, the code
    /// won and the vendor file's copy was dead. The vendor file is now the only
    /// source -- which also means the variable follows the vendor: no .NET SDK
    /// enabled, nothing to opt out of.
    #[test]
    fn the_dotnet_opt_out_comes_from_the_vendor_file_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let vendors = tmp.path().join("config/vendors");
        std::fs::create_dir_all(&vendors).unwrap();
        std::fs::write(
            vendors.join("DotNetSDK.json"),
            r#"{"DotNetSDK":{"name":".NET SDK","extractDir":"dotnet-sdk","enabled":true,
                 "environmentVariables":{"DOTNET_CLI_TELEMETRY_OPTOUT":"1"}}}"#,
        )
        .unwrap();

        let mut cfg = NanerConfig::default();
        crate::config::apply_env_overrides_from(&mut cfg, Vec::new());
        assert!(
            !cfg.environment
                .environment_variables
                .contains_key("DOTNET_CLI_TELEMETRY_OPTOUT"),
            "the env-override layer must no longer set this itself"
        );
        // The two with no vendor to belong to are still guaranteed there.
        assert_eq!(
            cfg.environment.environment_variables["POWERSHELL_TELEMETRY_OPTOUT"],
            "1"
        );
        assert_eq!(
            cfg.environment.environment_variables["AZURE_CORE_COLLECT_TELEMETRY"],
            "0"
        );

        merge_vendor_environment(&mut cfg, tmp.path());
        assert_eq!(
            cfg.environment.environment_variables["DOTNET_CLI_TELEMETRY_OPTOUT"], "1",
            "an enabled .NET SDK supplies it"
        );
    }
}
