//! Command: `naner setup-shell [pwsh|bash|cmd]`
//! Integrates naner environment exports into shell profile startup scripts.

use naner_core::{constants, logger, paths};

pub fn execute(args: &[String]) -> i32 {
    let shell = args
        .first()
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "pwsh".to_string());
    let dry_run = args.iter().any(|a| a == "--dry-run");

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    let naner_exe = naner_root.join("bin").join("naner.exe");

    match shell.as_str() {
        "pwsh" | "powershell" => {
            let snippet = format!(
                "\n# Added by Naner Terminal Launcher\nif (Test-Path \"{}\") {{ & \"{}\" --export-env -f powershell | Invoke-Expression }}\n",
                naner_exe.display(),
                naner_exe.display()
            );

            logger::header("Naner Shell Integration: PowerShell");
            if dry_run {
                logger::info(
                    "Dry run requested. Add the following line to your PowerShell $PROFILE:",
                );
                println!("{snippet}");
            } else {
                logger::info(
                    "To integrate Naner with PowerShell, add the following line to your $PROFILE:",
                );
                println!("{snippet}");
                logger::success("Integration snippet generated.");
            }
            0
        }
        "bash" => {
            let snippet = format!(
                "\n# Added by Naner Terminal Launcher\nif [ -f \"{}\" ]; then eval \"$(\"{}\" --export-env -f bash)\"; fi\n",
                naner_exe.display(),
                naner_exe.display()
            );

            logger::header("Naner Shell Integration: Bash");
            if dry_run {
                logger::info("Dry run requested. Add the following line to your ~/.bashrc:");
                println!("{snippet}");
            } else {
                logger::info(
                    "To integrate Naner with Bash, add the following line to your ~/.bashrc:",
                );
                println!("{snippet}");
                logger::success("Integration snippet generated.");
            }
            0
        }
        "cmd" => {
            let snippet = format!("@call \"{}\" --export-env -f cmd", naner_exe.display());
            logger::header("Naner Shell Integration: CMD");
            logger::info(
                "To integrate Naner with CMD, call the following command at prompt startup:",
            );
            println!("{snippet}");
            0
        }
        other => {
            eprintln!("Unknown shell '{other}'. Supported: pwsh, bash, cmd");
            1
        }
    }
}
