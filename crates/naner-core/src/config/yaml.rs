//! YAML provider (fallback format behind JSON in the search order).
//! YamlDotNet was configured with PascalCase naming + ignore-unmatched;
//! the serde renames on the models give the same key mapping, and serde's
//! default is to ignore unknown fields.

use super::NanerConfig;

/// Parse a `naner.yaml`/`naner.yml` document.
pub fn load_yaml(content: &str) -> Result<NanerConfig, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case_yaml_parses() {
        let yaml = r#"
DefaultProfile: Bash
Profiles:
  Bash:
    Name: Bash
    Shell: Bash
    StartingDirectory: "~"
Advanced:
  InheritSystemPath: false
"#;
        let config = load_yaml(yaml).unwrap();
        assert_eq!(config.default_profile, "Bash");
        assert_eq!(config.profiles["Bash"].starting_directory, "~");
        assert!(!config.advanced.inherit_system_path);
        // Unspecified sections take C# defaults.
        assert_eq!(config.windows_terminal.tab_title, "Naner");
    }

    #[test]
    fn unknown_yaml_fields_are_ignored() {
        let yaml = "DefaultProfile: X\nNotAField: 42\n";
        assert_eq!(load_yaml(yaml).unwrap().default_profile, "X");
    }
}
