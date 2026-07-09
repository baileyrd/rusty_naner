//! JSON provider. System.Text.Json is configured with
//! `ReadCommentHandling.Skip` and `AllowTrailingCommas`; serde_json supports
//! neither, so a small preprocessor strips `//`/`/* */` comments and trailing
//! commas (string-literal aware) before parsing.

use super::NanerConfig;

/// Parse a `naner.json` document with the C# tolerances.
pub fn load_json(content: &str) -> Result<NanerConfig, serde_json::Error> {
    serde_json::from_str(&strip_json_comments(content))
}

/// Remove `//` line comments, `/* */` block comments, and trailing commas
/// (a `,` whose next non-whitespace character is `}` or `]`) outside string
/// literals. Comment characters inside strings are untouched.
pub fn strip_json_comments(input: &str) -> String {
    // Pass 1: strip comments.
    let mut no_comments = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            no_comments.push(c);
            if c == '\\' && i + 1 < chars.len() {
                no_comments.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
        } else if c == '"' {
            in_string = true;
            no_comments.push(c);
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(chars.len());
        } else {
            no_comments.push(c);
            i += 1;
        }
    }

    // Pass 2: strip trailing commas.
    let chars: Vec<char> = no_comments.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    let mut in_string = false;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
        } else if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
        } else if c == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if matches!(chars.get(j), Some('}') | Some(']')) {
                i += 1; // drop the comma; whitespace and closer follow as-is
            } else {
                out.push(c);
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_trailing_commas_are_tolerated() {
        let json = r#"
        {
            // line comment
            "DefaultProfile": "Bash", /* block comment */
            "Profiles": {
                "Bash": { "Name": "Bash", "Shell": "Bash", },
            },
        }
        "#;
        let config = load_json(json).unwrap();
        assert_eq!(config.default_profile, "Bash");
        assert_eq!(config.profiles["Bash"].shell, "Bash");
    }

    #[test]
    fn comment_markers_inside_strings_survive() {
        let json = r#"{ "DefaultProfile": "http://example.com // not-a-comment" }"#;
        let config = load_json(json).unwrap();
        assert_eq!(
            config.default_profile,
            "http://example.com // not-a-comment"
        );
        let json = r#"{ "DefaultProfile": "a,]" }"#;
        assert_eq!(load_json(json).unwrap().default_profile, "a,]");
    }

    #[test]
    fn unknown_fields_and_schema_keys_are_ignored() {
        let json = r#"
        {
            "$schema": "https://json-schema.org/draft-07/schema#",
            "title": "Naner Configuration",
            "description": "whatever",
            "NotARealField": { "nested": [1, 2, 3] },
            "DefaultProfile": "Unified"
        }
        "#;
        let config = load_json(json).unwrap();
        assert_eq!(config.default_profile, "Unified");
    }

    #[test]
    fn defaults_match_csharp() {
        let config = load_json("{}").unwrap();
        assert_eq!(config.default_profile, "Unified");
        assert!(config.advanced.inherit_system_path);
        assert!(!config.advanced.debug_mode);
        assert!(config.environment.unified_path);
        assert_eq!(config.windows_terminal.launch_mode, "default");
        assert_eq!(config.windows_terminal.tab_title, "Naner");
        assert!(config.windows_terminal.suppress_application_title);
    }

    #[test]
    fn real_naner_json_shape_parses() {
        let json = r#"
        {
            "VendorPaths": { "PowerShell": "%NANER_ROOT%\\vendor\\powershell\\pwsh.exe" },
            "Environment": {
                "UnifiedPath": true,
                "PathPrecedence": ["%NANER_ROOT%\\bin", "%NANER_ROOT%\\opt"],
                "EnvironmentVariables": { "NANER_ROOT": "%NANER_ROOT%", "MSYSTEM": "MINGW64" }
            },
            "DefaultProfile": "Unified",
            "Profiles": {
                "Unified": {
                    "Name": "Naner (Unified)",
                    "Shell": "PowerShell",
                    "StartingDirectory": "%USERPROFILE%",
                    "ColorScheme": "Campbell",
                    "UseVendorPath": true,
                    "CustomShell": {
                        "ExecutablePath": "%NANER_ROOT%\\vendor\\powershell\\pwsh.exe",
                        "Arguments": "-NoExit -NoLogo"
                    }
                }
            },
            "WindowsTerminal": { "DefaultTerminal": true, "LaunchMode": "default" },
            "Advanced": { "InheritSystemPath": true, "DebugMode": false }
        }
        "#;
        let config = load_json(json).unwrap();
        assert_eq!(config.profiles.len(), 1);
        let p = &config.profiles["Unified"];
        assert_eq!(p.name, "Naner (Unified)");
        assert_eq!(
            p.custom_shell.as_ref().unwrap().arguments.as_deref(),
            Some("-NoExit -NoLogo")
        );
        // Insertion order preserved (IndexMap) — PATH assembly depends on it.
        assert_eq!(
            config.environment.path_precedence,
            vec!["%NANER_ROOT%\\bin", "%NANER_ROOT%\\opt"]
        );
        let keys: Vec<_> = config.environment.environment_variables.keys().collect();
        assert_eq!(keys, vec!["NANER_ROOT", "MSYSTEM"]);
    }
}
