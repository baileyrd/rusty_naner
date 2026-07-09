//! Port of `EnvironmentExporter` (Naner.Core): renders the configured
//! environment as an eval-able script for PowerShell, Bash, or CMD. Pure
//! stdout composability is the launcher's flagship Unix trait
//! (`naner --export-env | Invoke-Expression`) — the caller trims the result
//! and prints it prefix-free.

use indexmap::IndexMap;

/// C# builds the script with `AppendLine` (`Environment.NewLine`).
#[cfg(windows)]
const NEWLINE: &str = "\r\n";
#[cfg(not(windows))]
const NEWLINE: &str = "\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellFormat {
    PowerShell,
    Bash,
    Cmd,
}

#[derive(Debug, thiserror::Error)]
#[error("Unknown format: {0}. Supported formats: powershell, bash, cmd")]
pub struct UnknownFormat(pub String);

/// Parse a `-f/--format` value (`EnvironmentExporter.ParseFormat`).
pub fn parse_format(format: &str) -> Result<ShellFormat, UnknownFormat> {
    match format.to_lowercase().as_str() {
        "powershell" | "ps" | "ps1" => Ok(ShellFormat::PowerShell),
        "bash" | "sh" | "zsh" => Ok(ShellFormat::Bash),
        "cmd" | "bat" | "batch" => Ok(ShellFormat::Cmd),
        _ => Err(UnknownFormat(format.to_string())),
    }
}

/// Render the export script (`EnvironmentExporter.Export`). PATH is emitted
/// first; a variable literally named PATH (any case) in the map is skipped.
pub fn export(
    environment_variables: &IndexMap<String, String>,
    path: &str,
    format: ShellFormat,
    no_comments: bool,
) -> String {
    export_at(
        environment_variables,
        path,
        format,
        no_comments,
        &chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    )
}

/// [`export`] with an explicit timestamp, for deterministic tests.
pub fn export_at(
    environment_variables: &IndexMap<String, String>,
    path: &str,
    format: ShellFormat,
    no_comments: bool,
    timestamp: &str,
) -> String {
    let mut out = String::new();
    let mut line = |s: String| {
        out.push_str(&s);
        out.push_str(NEWLINE);
    };

    if !no_comments {
        line(comment(
            &format!("Naner Environment Setup - Generated {timestamp}"),
            format,
        ));
        line(comment(
            "Source this file or execute these commands to configure your shell",
            format,
        ));
        line(comment("PATH Configuration", format));
    }

    line(set_variable("PATH", path, format));

    if !no_comments {
        line(comment("Environment Variables", format));
    }

    for (key, value) in environment_variables {
        if key.eq_ignore_ascii_case("PATH") {
            continue;
        }
        line(set_variable(key, value, format));
    }

    out
}

fn comment(text: &str, format: ShellFormat) -> String {
    match format {
        ShellFormat::PowerShell | ShellFormat::Bash => format!("# {text}"),
        ShellFormat::Cmd => format!("REM {text}"),
    }
}

fn set_variable(name: &str, value: &str, format: ShellFormat) -> String {
    match format {
        ShellFormat::PowerShell => {
            // Single quotes doubled.
            format!("$env:{name} = '{}'", value.replace('\'', "''"))
        }
        ShellFormat::Bash => {
            // PATH gets per-entry Windows→Unix conversion; every other value
            // gets the single-path conversion (backslashes and drive letter).
            let converted = if name.eq_ignore_ascii_case("PATH") {
                convert_path_to_unix(value)
            } else {
                convert_single_path_to_unix(value)
            };
            let escaped = converted
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
                .replace('`', "\\`")
                .replace('!', "\\!");
            format!("export {name}=\"{escaped}\"")
        }
        ShellFormat::Cmd => {
            // `%` doubled.
            format!("SET \"{name}={}\"", value.replace('%', "%%"))
        }
    }
}

/// Semicolon-separated Windows PATH → colon-separated Unix PATH.
fn convert_path_to_unix(windows_path: &str) -> String {
    windows_path
        .split(';')
        .filter(|p| !p.is_empty())
        .map(convert_single_path_to_unix)
        .collect::<Vec<_>>()
        .join(":")
}

/// Backslashes → slashes; `C:` drive prefix → `/c`.
fn convert_single_path_to_unix(windows_path: &str) -> String {
    if windows_path.is_empty() {
        return windows_path.to_string();
    }
    let result = windows_path.replace('\\', "/");
    let mut chars = result.chars();
    match (chars.next(), chars.next()) {
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic() => {
            format!("/{}{}", drive.to_ascii_lowercase(), &result[2..])
        }
        _ => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn lines(s: &str) -> Vec<&str> {
        s.lines().collect()
    }

    #[test]
    fn format_parsing_accepts_all_aliases() {
        for f in ["powershell", "ps", "PS1"] {
            assert_eq!(parse_format(f).unwrap(), ShellFormat::PowerShell);
        }
        for f in ["bash", "SH", "zsh"] {
            assert_eq!(parse_format(f).unwrap(), ShellFormat::Bash);
        }
        for f in ["cmd", "bat", "Batch"] {
            assert_eq!(parse_format(f).unwrap(), ShellFormat::Cmd);
        }
        let err = parse_format("fish").unwrap_err();
        assert_eq!(
            err.to_string(),
            "Unknown format: fish. Supported formats: powershell, bash, cmd"
        );
    }

    #[test]
    fn powershell_output_quotes_and_orders() {
        let script = export_at(
            &vars(&[("GOROOT", "C:\\naner\\vendor\\go"), ("QUOTED", "it's")]),
            "C:\\naner\\bin;C:\\Windows",
            ShellFormat::PowerShell,
            false,
            "2026-01-01 00:00:00",
        );
        let l = lines(&script);
        assert_eq!(
            l[0],
            "# Naner Environment Setup - Generated 2026-01-01 00:00:00"
        );
        assert_eq!(l[3], "$env:PATH = 'C:\\naner\\bin;C:\\Windows'");
        assert_eq!(l[4], "# Environment Variables");
        assert_eq!(l[5], "$env:GOROOT = 'C:\\naner\\vendor\\go'");
        assert_eq!(l[6], "$env:QUOTED = 'it''s'");
    }

    #[test]
    fn no_comments_strips_all_comment_lines() {
        let script = export_at(
            &vars(&[("A", "1")]),
            "C:\\bin",
            ShellFormat::PowerShell,
            true,
            "t",
        );
        assert_eq!(
            lines(&script),
            vec!["$env:PATH = 'C:\\bin'", "$env:A = '1'"]
        );
    }

    #[test]
    fn bash_converts_paths_and_escapes() {
        let script = export_at(
            &vars(&[("HOME2", "C:\\naner\\home"), ("BANG", "hey!")]),
            "C:\\naner\\bin;D:\\tools",
            ShellFormat::Bash,
            true,
            "t",
        );
        let l = lines(&script);
        assert_eq!(l[0], "export PATH=\"/c/naner/bin:/d/tools\"");
        assert_eq!(l[1], "export HOME2=\"/c/naner/home\"");
        assert_eq!(l[2], "export BANG=\"hey\\!\"");
    }

    #[test]
    fn cmd_doubles_percents() {
        let script = export_at(
            &vars(&[("PCT", "100%")]),
            "C:\\bin",
            ShellFormat::Cmd,
            false,
            "t",
        );
        let l = lines(&script);
        assert_eq!(l[0], "REM Naner Environment Setup - Generated t");
        assert_eq!(l[3], "SET \"PATH=C:\\bin\"");
        assert_eq!(l[5], "SET \"PCT=100%%\"");
    }

    #[test]
    fn path_key_in_map_is_skipped() {
        let script = export_at(
            &vars(&[("Path", "shadowed"), ("A", "1")]),
            "real",
            ShellFormat::PowerShell,
            true,
            "t",
        );
        assert_eq!(lines(&script), vec!["$env:PATH = 'real'", "$env:A = '1'"]);
    }
}
