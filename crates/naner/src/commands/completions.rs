//! Command: `naner completions <shell>`
//! Generates tab completion scripts for bash, zsh, powershell, fish using clap_complete.

use clap::Command;
use clap_complete::{Shell, generate};
use std::io;

pub fn execute(args: &[String]) -> i32 {
    let shell_str = match args.first() {
        Some(s) => s.to_lowercase(),
        None => {
            eprintln!("Usage: naner completions <bash|zsh|pwsh|fish>");
            return 1;
        }
    };

    let Some(shell) = parse_shell(&shell_str) else {
        eprintln!("Unknown shell '{shell_str}'. Supported: bash, zsh, pwsh, fish");
        return 1;
    };

    let mut cmd = build_cli_command();
    generate(shell, &mut cmd, "naner", &mut io::stdout());
    0
}

/// Shell name → clap_complete target. Separate from `execute` so the accepted
/// spellings can be tested without generating a script to stdout.
fn parse_shell(name: &str) -> Option<Shell> {
    Some(match name {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "pwsh" | "powershell" => Shell::PowerShell,
        "fish" => Shell::Fish,
        _ => return None,
    })
}

fn build_cli_command() -> Command {
    Command::new("naner")
        .about("Portable terminal environment launcher for Windows")
        .subcommand(Command::new("--version").about("Print version information"))
        .subcommand(Command::new("--help").about("Print help information"))
        .subcommand(Command::new("--diagnose").about("Run detailed system diagnostics"))
        .subcommand(Command::new("doctor").about("Run environment health checks"))
        .subcommand(Command::new("schema").about("Print JSON schema for config or vendors"))
        .subcommand(Command::new("completions").about("Generate shell completions"))
        .subcommand(Command::new("shell-integration").about("Generate OSC 133 prompt hooks"))
        .subcommand(Command::new("setup-shell").about("Configure shell profile integration"))
        .subcommand(Command::new("repair").about("Repair corrupt vendor installations"))
        .subcommand(Command::new("profile").about("Import or export profile definitions"))
        .subcommand(Command::new("checksum").about("Update vendor checksums in vendors.json"))
        .subcommand(Command::new("install").about("Install vendor tools"))
        .subcommand(Command::new("update-vendors").about("Update installed vendors"))
        .subcommand(Command::new("root").about("Print NANER_ROOT path"))
        .subcommand(Command::new("add-to-path").about("Add naner to the user PATH"))
        .subcommand(
            Command::new("suggest").about("Map a missing command to the vendor providing it"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_documented_shell_is_accepted() {
        // These are the names the usage string promises.
        for name in ["bash", "zsh", "pwsh", "fish"] {
            assert!(parse_shell(name).is_some(), "usage advertises {name}");
        }
        // Accepted alias, not advertised.
        assert_eq!(parse_shell("powershell"), Some(Shell::PowerShell));
    }

    #[test]
    fn an_unknown_shell_is_rejected() {
        assert_eq!(parse_shell("nushell"), None);
        assert_eq!(parse_shell(""), None);
        // Case folding happens before this is called.
        assert_eq!(parse_shell("BASH"), None);
    }

    /// The generated script is what users pipe into their shell; a panic here
    /// would surface as a truncated completion file.
    #[test]
    fn completions_generate_for_every_supported_shell() {
        for name in ["bash", "zsh", "pwsh", "fish"] {
            let shell = parse_shell(name).unwrap();
            let mut buf = Vec::new();
            generate(shell, &mut build_cli_command(), "naner", &mut buf);
            assert!(!buf.is_empty(), "{name} produced an empty script");
        }
    }
}
