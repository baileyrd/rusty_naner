//! Port of `EnvironmentConfigurationProvider`: env-var overrides applied on
//! top of the file-based config, highest priority.
//!
//! - `NANER_DEFAULT_PROFILE` — replaces `DefaultProfile`
//! - `NANER_INHERIT_SYSTEM_PATH` — anything except (case-insensitive)
//!   `"false"` enables inheritance (exact C# comparison)
//! - `NANER_DEBUG` — exactly (case-insensitive) `"true"` enables debug;
//!   any other non-empty value disables it
//! - `NANER_ISOLATE_ENVIRONMENT` — exactly (case-insensitive) `"true"`
//!   enables environment isolation (see `env_isolation`); any other
//!   non-empty value disables it. Additive (no C# counterpart).
//! - `NANER_ENV_<NAME>` — adds/overrides an environment variable
//! - `NANER_PATH_<NAME>` — prepends a PATH entry

use super::NanerConfig;

const ENV_VAR_PREFIX: &str = "NANER_ENV_";
const PATH_PREFIX: &str = "NANER_PATH_";

/// Apply overrides from the real process environment.
pub fn apply_env_overrides(config: &mut NanerConfig) {
    apply_env_overrides_from(config, std::env::vars());
}

/// Apply overrides from an explicit variable set (testable core).
pub fn apply_env_overrides_from(
    config: &mut NanerConfig,
    vars: impl IntoIterator<Item = (String, String)>,
) {
    let vars: Vec<(String, String)> = vars.into_iter().collect();
    let get = |name: &str| -> Option<&str> {
        vars.iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };

    // Ensure privacy telemetry opt-out variables are set by default.
    //
    // `DOTNET_CLI_TELEMETRY_OPTOUT` is deliberately absent: it belongs to the
    // .NET SDK and now lives in `config/vendors/DotNetSDK.json` with the rest
    // of that vendor's environment. Setting it here as well made the code the
    // winner over the vendor file -- this runs first, and the merge only fills
    // in keys that are still missing -- so the vendor file's value was dead.
    // The two below have no vendor to belong to: PowerShell's opt-out covers a
    // shell naner may launch from the host, and the Azure CLI is not a vendor
    // naner installs at all.
    if !config
        .environment
        .environment_variables
        .contains_key("POWERSHELL_TELEMETRY_OPTOUT")
    {
        config
            .environment
            .environment_variables
            .insert("POWERSHELL_TELEMETRY_OPTOUT".to_string(), "1".to_string());
    }
    if !config
        .environment
        .environment_variables
        .contains_key("AZURE_CORE_COLLECT_TELEMETRY")
    {
        config
            .environment
            .environment_variables
            .insert("AZURE_CORE_COLLECT_TELEMETRY".to_string(), "0".to_string());
    }

    if let Some(profile) = get("NANER_DEFAULT_PROFILE")
        && !profile.is_empty()
    {
        config.default_profile = profile.to_string();
    }

    if let Some(inherit) = get("NANER_INHERIT_SYSTEM_PATH")
        && !inherit.is_empty()
    {
        config.advanced.inherit_system_path = !inherit.eq_ignore_ascii_case("false");
    }

    if let Some(debug) = get("NANER_DEBUG")
        && !debug.is_empty()
    {
        config.advanced.debug_mode = debug.eq_ignore_ascii_case("true");
    }

    if let Some(isolate) = get("NANER_ISOLATE_ENVIRONMENT")
        && !isolate.is_empty()
    {
        config.advanced.isolate_environment = isolate.eq_ignore_ascii_case("true");
    }

    // NANER_ENV_*: prefix matched case-insensitively (C# StartsWith
    // OrdinalIgnoreCase); the variable name keeps the suffix's casing.
    for (key, value) in &vars {
        if key.len() > ENV_VAR_PREFIX.len()
            && key[..ENV_VAR_PREFIX.len()].eq_ignore_ascii_case(ENV_VAR_PREFIX)
        {
            let name = &key[ENV_VAR_PREFIX.len()..];
            if !name.is_empty() {
                config
                    .environment
                    .environment_variables
                    .insert(name.to_string(), value.clone());
            }
        }
    }

    // NANER_PATH_*: values collected then prepended ahead of the configured
    // precedence list.
    let mut additional: Vec<String> = Vec::new();
    for (key, value) in &vars {
        if key.len() > PATH_PREFIX.len()
            && key[..PATH_PREFIX.len()].eq_ignore_ascii_case(PATH_PREFIX)
            && !value.is_empty()
        {
            additional.push(value.clone());
        }
    }
    if !additional.is_empty() {
        additional.append(&mut config.environment.path_precedence);
        config.environment.path_precedence = additional;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn default_profile_override() {
        let mut config = NanerConfig {
            default_profile: "Unified".into(),
            ..Default::default()
        };
        apply_env_overrides_from(&mut config, vars(&[("NANER_DEFAULT_PROFILE", "Bash")]));
        assert_eq!(config.default_profile, "Bash");
    }

    #[test]
    fn inherit_system_path_is_anything_but_false() {
        let mut config = NanerConfig::default();
        apply_env_overrides_from(&mut config, vars(&[("NANER_INHERIT_SYSTEM_PATH", "FALSE")]));
        assert!(!config.advanced.inherit_system_path);

        apply_env_overrides_from(
            &mut config,
            vars(&[("NANER_INHERIT_SYSTEM_PATH", "anything")]),
        );
        assert!(config.advanced.inherit_system_path);
    }

    #[test]
    fn debug_is_exactly_true() {
        let mut config = NanerConfig::default();
        apply_env_overrides_from(&mut config, vars(&[("NANER_DEBUG", "TRUE")]));
        assert!(config.advanced.debug_mode);
        apply_env_overrides_from(&mut config, vars(&[("NANER_DEBUG", "1")]));
        assert!(!config.advanced.debug_mode);
    }

    #[test]
    fn isolate_environment_is_exactly_true() {
        let mut config = NanerConfig::default();
        apply_env_overrides_from(&mut config, vars(&[("NANER_ISOLATE_ENVIRONMENT", "TRUE")]));
        assert!(config.advanced.isolate_environment);
        apply_env_overrides_from(&mut config, vars(&[("NANER_ISOLATE_ENVIRONMENT", "1")]));
        assert!(!config.advanced.isolate_environment);
    }

    #[test]
    fn naner_env_adds_variables() {
        let mut config = NanerConfig::default();
        config
            .environment
            .environment_variables
            .insert("EDITOR".into(), "notepad".into());
        apply_env_overrides_from(
            &mut config,
            vars(&[("NANER_ENV_EDITOR", "vim"), ("naner_env_Pager", "less")]),
        );
        assert_eq!(config.environment.environment_variables["EDITOR"], "vim");
        assert_eq!(config.environment.environment_variables["Pager"], "less");
    }

    #[test]
    fn naner_path_prepends() {
        let mut config = NanerConfig::default();
        config.environment.path_precedence = vec!["existing".into()];
        apply_env_overrides_from(&mut config, vars(&[("NANER_PATH_CUSTOM", "/custom/bin")]));
        assert_eq!(
            config.environment.path_precedence,
            vec!["/custom/bin".to_string(), "existing".to_string()]
        );
    }

    #[test]
    fn empty_values_are_ignored() {
        let mut config = NanerConfig {
            default_profile: "Unified".into(),
            ..Default::default()
        };
        apply_env_overrides_from(
            &mut config,
            vars(&[
                ("NANER_DEFAULT_PROFILE", ""),
                ("NANER_PATH_EMPTY", ""),
                ("NANER_DEBUG", ""),
            ]),
        );
        assert_eq!(config.default_profile, "Unified");
        assert!(config.environment.path_precedence.is_empty());
        assert!(!config.advanced.debug_mode);
    }
}
