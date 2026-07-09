//! Port of `LaunchOptions` (CommandLineParser) via clap. Router verbs are
//! checked before this parser runs, preserving the C# two-layer dispatch;
//! clap's auto help/version are disabled so the router's `--help`/`--version`
//! outputs stay canonical.

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "naner", disable_help_flag = true, disable_version_flag = true)]
pub struct LaunchOptions {
    /// Terminal profile to launch (Unified, PowerShell, Bash, CMD)
    #[arg(short = 'p', long = "profile")]
    pub profile: Option<String>,

    /// Environment name (default, work, personal, etc.)
    #[arg(short = 'e', long = "environment", default_value = "default")]
    pub environment: String,

    /// Starting directory for terminal session
    #[arg(short = 'd', long = "directory")]
    pub directory: Option<String>,

    /// Path to config file (supports .json, .yaml, .yml)
    #[arg(short = 'c', long = "config")]
    pub config_path: Option<String>,

    /// Enable debug/verbose output
    #[arg(long = "debug")]
    pub debug: bool,

    /// Display version information
    #[arg(short = 'v', long = "version")]
    pub version: bool,

    /// Setup environment without launching terminal
    #[arg(long = "setup-only")]
    pub setup_only: bool,

    /// Export environment setup commands for shell integration
    #[arg(long = "export-env")]
    pub export_env: bool,

    /// Output format for --export-env (powershell, bash, cmd)
    #[arg(short = 'f', long = "format", default_value = "powershell")]
    pub format: String,

    /// Omit comments from --export-env output
    #[arg(long = "no-comments")]
    pub no_comments: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<LaunchOptions, clap::Error> {
        LaunchOptions::try_parse_from(std::iter::once("naner").chain(args.iter().copied()))
    }

    #[test]
    fn defaults_match_csharp() {
        let opts = parse(&[]).unwrap();
        assert_eq!(opts.environment, "default");
        assert_eq!(opts.format, "powershell");
        assert!(opts.profile.is_none());
        assert!(!opts.debug && !opts.export_env && !opts.setup_only && !opts.no_comments);
    }

    #[test]
    fn all_flags_parse() {
        let opts = parse(&[
            "-p",
            "Bash",
            "-e",
            "work",
            "-d",
            "C:\\projects",
            "-c",
            "custom.yaml",
            "--debug",
            "--export-env",
            "-f",
            "bash",
            "--no-comments",
            "--setup-only",
        ])
        .unwrap();
        assert_eq!(opts.profile.as_deref(), Some("Bash"));
        assert_eq!(opts.environment, "work");
        assert_eq!(opts.directory.as_deref(), Some("C:\\projects"));
        assert_eq!(opts.config_path.as_deref(), Some("custom.yaml"));
        assert!(opts.debug && opts.export_env && opts.no_comments && opts.setup_only);
        assert_eq!(opts.format, "bash");
    }

    #[test]
    fn unknown_options_are_errors() {
        assert!(parse(&["--definitely-not-a-real-flag"]).is_err());
        assert!(parse(&["stray-positional"]).is_err());
    }
}
