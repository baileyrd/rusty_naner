//! Port of `HelpCommand` + `HelpTextProvider`, output preserved verbatim —
//! including the placeholder docs URL, which is a known C# drift item
//! (MIGRATION_ANALYSIS §3) to fix post-parity.

use naner_core::{constants, logger};

pub fn execute() -> i32 {
    logger::header("Naner Terminal Launcher");
    println!("Version {} - {}", constants::VERSION, constants::PHASE_NAME);
    println!();

    println!("USAGE:");
    println!("  naner.exe [OPTIONS]");
    println!();

    println!("COMMANDS:");
    println!("  install                    Install optional vendor packages");
    println!("    install --list           List available vendors and status");
    println!("    install --all            Install all optional vendors");
    println!("    install <vendor> [...]   Install specific vendor(s)");
    println!("  init                       Initialize Naner in this folder (download from GitHub)");
    println!("  update                     Update Naner itself to the latest release");
    println!("  check-update               Check whether a newer release exists");
    println!("  update-vendors             Update all vendor dependencies to latest versions");
    println!("  root                       Print the Naner root directory and exit");
    println!("  add-to-path                Put naner on the user PATH (undo: --remove)");
    println!("  suggest <name>             Map a missing command to the vendor providing it");
    println!("  outdated                   Compare installed vendors against latest releases");
    println!("  refresh-pins [dir]         Re-resolve and rewrite vendor fallback pins");
    println!();

    println!("OPTIONS:");
    println!("  -p, --profile <NAME>       Terminal profile to launch");
    println!("                             (Unified, PowerShell, Bash, CMD)");
    println!("  -e, --environment <NAME>   Environment name (default, work, etc.)");
    println!("  -d, --directory <PATH>     Starting directory for terminal");
    println!("  -c, --config <PATH>        Path to config file (.json, .yaml, .yml)");
    println!("  --debug                    Enable debug/verbose output");
    println!("  -v, --version              Display version information");
    println!("  -h, --help                 Display this help message");
    println!("  --diagnose                 Run diagnostic checks");
    println!();
    println!("SHELL INTEGRATION:");
    println!("  --setup-only               Setup environment without launching terminal");
    println!("  --export-env               Export environment as shell commands");
    println!("  -f, --format <FORMAT>      Output format for --export-env:");
    println!("                             powershell (default), bash, cmd");
    println!();

    println!("EXAMPLES:");
    println!("  naner.exe                          # Launch default profile");
    println!("  naner.exe --profile PowerShell     # Launch PowerShell profile");
    println!("  naner.exe -p Bash -d C:\\projects   # Launch Bash in specific dir");
    println!("  naner.exe --debug                  # Show detailed diagnostics");
    println!("  naner.exe --diagnose               # Check installation health");
    println!("  naner.exe update-vendors           # Update vendor dependencies");
    println!("  naner.exe install --list           # List available vendors");
    println!("  naner.exe install ruby nodejs      # Install Ruby and Node.js");
    println!();
    println!("SHELL INTEGRATION EXAMPLES:");
    println!("  naner.exe --export-env             # Output PowerShell env commands");
    println!("  naner.exe --export-env -f bash     # Output Bash export commands");
    println!("  naner.exe --export-env -f cmd      # Output CMD SET commands");
    println!();
    println!("  # PowerShell: Source Naner environment");
    println!("  Invoke-Expression (naner.exe --export-env)");
    println!();
    println!("  # Bash: Source Naner environment");
    println!("  eval \"$(naner.exe --export-env -f bash)\"");
    println!();
    println!("INITIALIZATION:");
    println!("  naner is a single binary: run it in an empty folder to install,");
    println!("  'naner update' to update itself, 'naner update-vendors' for tools.");
    println!();

    println!("REQUIREMENTS:");
    println!("  naner.exe must be run from within a Naner installation that");
    println!("  contains bin/, vendor/, and config/ subdirectories.");
    println!();

    println!("DOCUMENTATION:");
    println!("  https://github.com/yourusername/naner");
    println!();

    0
}
