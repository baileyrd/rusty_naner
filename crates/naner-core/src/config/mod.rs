//! Port of Naner.Configuration: models, providers (JSON/YAML/env), loader,
//! and validator.
//!
//! Model contract (from `NanerJsonContext` options): PascalCase keys,
//! case-insensitive matching (approximated with camelCase/lowercase aliases —
//! the shipped configs use PascalCase), comments and trailing commas
//! tolerated, unknown fields ignored (so root `$schema`/`title`/`description`
//! keys pass through harmlessly). `UnifiedPath`, `PreservePath`,
//! `VerboseLogging` are parsed but never read — kept for schema
//! compatibility, inert on purpose (MIGRATION_ANALYSIS §3).

mod env_overrides;
mod json;
mod loader;
mod merge;
mod validator;

pub use env_overrides::{apply_env_overrides, apply_env_overrides_from};
pub use json::{load_json, strip_json_comments};
pub use loader::{ConfigError, find_configuration_file, load, load_verbatim};
pub use merge::{NanerConfigMergeOutcome, merge_shipped_naner_defaults};
pub use validator::{ValidationReport, validate};

// Re-exported so binaries don't need a second direct dependency.
pub use crate::collections::OrderedMap;
use serde::{Deserialize, Serialize};

/// Root configuration model (`NanerConfig`), mapping `config/naner.json`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct NanerConfig {
    #[serde(default, alias = "vendorPaths", alias = "vendorpaths")]
    pub vendor_paths: OrderedMap<String>,

    #[serde(default, alias = "environment")]
    pub environment: EnvironmentConfig,

    #[serde(
        default = "defaults::default_profile",
        alias = "defaultProfile",
        alias = "defaultprofile"
    )]
    pub default_profile: String,

    #[serde(default, alias = "profiles")]
    pub profiles: OrderedMap<ProfileConfig>,

    #[serde(default, alias = "windowsTerminal", alias = "windowsterminal")]
    pub windows_terminal: WindowsTerminalConfig,

    #[serde(default, alias = "advanced")]
    pub advanced: AdvancedConfig,

    #[serde(default, alias = "customProfiles", alias = "customprofiles")]
    pub custom_profiles: OrderedMap<ProfileConfig>,
}

/// `EnvironmentConfig`: PATH precedence and environment variables.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct EnvironmentConfig {
    /// Parsed but never read (inert; kept for schema compatibility).
    #[serde(
        default = "defaults::yes",
        alias = "unifiedPath",
        alias = "unifiedpath"
    )]
    pub unified_path: bool,

    #[serde(default, alias = "pathPrecedence", alias = "pathprecedence")]
    pub path_precedence: Vec<String>,

    #[serde(
        default,
        alias = "environmentVariables",
        alias = "environmentvariables"
    )]
    pub environment_variables: OrderedMap<String>,
}

impl Default for EnvironmentConfig {
    fn default() -> Self {
        Self {
            unified_path: true,
            path_precedence: Vec::new(),
            environment_variables: OrderedMap::new(),
        }
    }
}

/// `AdvancedConfig`: power-user switches. `preserve_path` and
/// `verbose_logging` are inert (parsed, never read) — as in C#.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct AdvancedConfig {
    #[serde(default, alias = "preservePath", alias = "preservepath")]
    pub preserve_path: bool,

    #[serde(
        default = "defaults::yes",
        alias = "inheritSystemPath",
        alias = "inheritsystempath"
    )]
    pub inherit_system_path: bool,

    #[serde(default, alias = "verboseLogging", alias = "verboselogging")]
    pub verbose_logging: bool,

    #[serde(default, alias = "debugMode", alias = "debugmode")]
    pub debug_mode: bool,

    /// Additive (no C# counterpart): testing/dev switch. When true, every
    /// process environment variable not on `env_isolation::KEEP_ON_ISOLATE`
    /// is cleared before NANER_ROOT/NANER_ENVIRONMENT/HOME/PATH/configured
    /// variables are set, so tools already installed system- or user-wide
    /// can't leak into naner's environment during a test run.
    #[serde(default, alias = "isolateEnvironment", alias = "isolateenvironment")]
    pub isolate_environment: bool,

    /// Additive (no C# counterpart): directory junctions created under
    /// `home\` on first launch after init, `{link name under home -> real
    /// target path}`. `%HOST_USERPROFILE%` in a target resolves to the
    /// real Windows profile directory as it was before naner redirected
    /// `USERPROFILE` to its own tree -- see `home_junctions` for the rest
    /// of the expansion rules. Bridges specific, well-known real
    /// directories (Documents/Downloads/Desktop, a personal dev folder)
    /// back out from underneath that redirect, without giving up
    /// USERPROFILE's containment for everything else.
    #[serde(default, alias = "homeJunctions", alias = "homejunctions")]
    pub home_junctions: OrderedMap<String>,
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            preserve_path: false,
            inherit_system_path: true,
            verbose_logging: false,
            debug_mode: false,
            isolate_environment: false,
            home_junctions: OrderedMap::new(),
        }
    }
}

/// `WindowsTerminalConfig`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct WindowsTerminalConfig {
    #[serde(
        default = "defaults::yes",
        alias = "defaultTerminal",
        alias = "defaultterminal"
    )]
    pub default_terminal: bool,

    /// `default`, `maximized`, `fullscreen`, or `focus`.
    #[serde(
        default = "defaults::launch_mode",
        alias = "launchMode",
        alias = "launchmode"
    )]
    pub launch_mode: String,

    #[serde(
        default = "defaults::tab_title",
        alias = "tabTitle",
        alias = "tabtitle"
    )]
    pub tab_title: String,

    #[serde(
        default = "defaults::yes",
        alias = "suppressApplicationTitle",
        alias = "suppressapplicationtitle"
    )]
    pub suppress_application_title: bool,
}

impl Default for WindowsTerminalConfig {
    fn default() -> Self {
        Self {
            default_terminal: true,
            launch_mode: defaults::launch_mode(),
            tab_title: defaults::tab_title(),
            suppress_application_title: true,
        }
    }
}

/// `ProfileConfig`: one terminal profile.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ProfileConfig {
    #[serde(default, alias = "name")]
    pub name: String,

    #[serde(default, alias = "description")]
    pub description: Option<String>,

    /// `PowerShell`, `Bash`, `CMD`, or `Custom`.
    #[serde(default = "defaults::shell", alias = "shell")]
    pub shell: String,

    #[serde(
        default = "defaults::starting_directory",
        alias = "startingDirectory",
        alias = "startingdirectory"
    )]
    pub starting_directory: String,

    #[serde(default, alias = "icon")]
    pub icon: Option<String>,

    #[serde(
        default = "defaults::color_scheme",
        alias = "colorScheme",
        alias = "colorscheme"
    )]
    pub color_scheme: String,

    #[serde(
        default = "defaults::yes",
        alias = "useVendorPath",
        alias = "usevendorpath"
    )]
    pub use_vendor_path: bool,

    #[serde(default, alias = "customShell", alias = "customshell")]
    pub custom_shell: Option<CustomShellConfig>,

    /// Additive: pre-launch and post-launch event hook script paths.
    #[serde(default, alias = "preLaunch", alias = "prelaunch")]
    pub pre_launch: Option<String>,

    #[serde(default, alias = "postLaunch", alias = "postlaunch")]
    pub post_launch: Option<String>,

    /// Additive (no C# counterpart): which terminal hosts the profile.
    /// `None` (the default) means Windows Terminal — existing configs are
    /// untouched. Recognized values: `WindowsTerminal`, `RustyTerm`.
    #[serde(default, alias = "terminal")]
    pub terminal: Option<String>,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: None,
            shell: defaults::shell(),
            starting_directory: defaults::starting_directory(),
            icon: None,
            color_scheme: defaults::color_scheme(),
            use_vendor_path: true,
            custom_shell: None,
            pre_launch: None,
            post_launch: None,
            terminal: None,
        }
    }
}

/// `CustomShellConfig`: explicit executable + arguments.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct CustomShellConfig {
    #[serde(default, alias = "executablePath", alias = "executablepath")]
    pub executable_path: String,

    #[serde(default, alias = "arguments")]
    pub arguments: Option<String>,
}

mod defaults {
    pub fn yes() -> bool {
        true
    }
    pub fn default_profile() -> String {
        "Unified".to_string()
    }
    pub fn launch_mode() -> String {
        "default".to_string()
    }
    pub fn tab_title() -> String {
        "Naner".to_string()
    }
    pub fn shell() -> String {
        "PowerShell".to_string()
    }
    pub fn starting_directory() -> String {
        "%USERPROFILE%".to_string()
    }
    pub fn color_scheme() -> String {
        "Campbell".to_string()
    }
}

impl NanerConfig {
    /// `ConfigurationManager.GetProfile`: standard profiles first, then
    /// custom, then (optionally) the default profile with a warning.
    pub fn get_profile(
        &self,
        profile_name: &str,
        use_default_on_not_found: bool,
    ) -> Result<&ProfileConfig, String> {
        if let Some(profile) = self.profiles.get(profile_name) {
            return Ok(profile);
        }
        if let Some(profile) = self.custom_profiles.get(profile_name) {
            return Ok(profile);
        }
        if use_default_on_not_found
            && !self.default_profile.is_empty()
            && let Some(profile) = self.profiles.get(&self.default_profile)
        {
            crate::logger::warning(&format!(
                "Profile '{profile_name}' not found, using default: {}",
                self.default_profile
            ));
            return Ok(profile);
        }
        Err(format!(
            "Profile '{profile_name}' not found in configuration"
        ))
    }
}
