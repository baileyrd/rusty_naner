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

    let shell = match shell_str.as_str() {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "pwsh" | "powershell" => Shell::PowerShell,
        "fish" => Shell::Fish,
        other => {
            eprintln!("Unknown shell '{other}'. Supported: bash, zsh, pwsh, fish");
            return 1;
        }
    };

    let mut cmd = build_cli_command();
    generate(shell, &mut cmd, "naner", &mut io::stdout());
    0
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
}
