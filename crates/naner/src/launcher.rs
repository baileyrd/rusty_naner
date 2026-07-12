//! Port of `TerminalLauncher`: find wt.exe, build its argument string,
//! spawn fire-and-forget (exit 0 as soon as the spawn succeeds). Argument
//! building is pure and unit-tested everywhere; the spawn is Windows-only.

use std::path::{Path, PathBuf};

use naner_core::config::{NanerConfig, ProfileConfig};
use naner_core::{constants, logger, paths};

/// Additive terminal-abstraction seam (no C# counterpart): which terminal
/// binary hosts a profile. A profile without a `Terminal` field maps to
/// `WindowsTerminal`, keeping the original flow byte-identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalKind {
    WindowsTerminal,
    RustyTerm,
}

impl TerminalKind {
    /// Map a profile's optional `Terminal` value; absent/empty = Windows
    /// Terminal, unknown values are a launch-time error.
    fn from_profile(profile: &ProfileConfig) -> Result<Self, String> {
        match profile.terminal.as_deref().map(str::trim) {
            None | Some("") => Ok(Self::WindowsTerminal),
            Some(t) if t.eq_ignore_ascii_case("WindowsTerminal") => Ok(Self::WindowsTerminal),
            Some(t) if t.eq_ignore_ascii_case("RustyTerm") => Ok(Self::RustyTerm),
            Some(other) => Err(format!(
                "Unknown terminal '{other}' (supported: WindowsTerminal, RustyTerm)"
            )),
        }
    }
}

pub struct TerminalLauncher<'a> {
    naner_root: &'a Path,
    config: &'a NanerConfig,
    debug_mode: bool,
}

impl<'a> TerminalLauncher<'a> {
    pub fn new(naner_root: &'a Path, config: &'a NanerConfig, debug_mode: bool) -> Self {
        Self {
            naner_root,
            config,
            debug_mode,
        }
    }

    /// `LaunchProfile`: resolve profile → find wt.exe → build args → set
    /// PATH → spawn without waiting.
    pub fn launch_profile(&self, profile_name: &str, starting_directory: Option<&str>) -> i32 {
        let profile = match self.config.get_profile(profile_name, true) {
            Ok(p) => p,
            Err(_) => {
                logger::failure(&format!("Profile not found: {profile_name}"));
                logger::info(&format!(
                    "Available profiles: {}",
                    self.config
                        .profiles
                        .keys()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                return 1;
            }
        };

        let terminal_kind = match TerminalKind::from_profile(profile) {
            Ok(kind) => kind,
            Err(err) => {
                logger::failure(&err);
                return 1;
            }
        };

        let terminal_path = match terminal_kind {
            TerminalKind::WindowsTerminal => {
                let Some(path) = self.windows_terminal_path() else {
                    logger::failure("Windows Terminal not found");
                    logger::info("Please install Windows Terminal or configure vendor path");
                    return 1;
                };
                path
            }
            TerminalKind::RustyTerm => {
                let Some(path) = self.rusty_term_path() else {
                    logger::failure("RustyTerm not found");
                    logger::info("Please install RustyTerm or configure vendor path");
                    return 1;
                };
                path
            }
        };

        let arguments = match terminal_kind {
            TerminalKind::WindowsTerminal => {
                self.build_terminal_arguments(profile, starting_directory)
            }
            TerminalKind::RustyTerm => self.build_rusty_term_arguments(profile, starting_directory),
        };

        if self.debug_mode {
            logger::debug(&format!("Terminal Path: {}", terminal_path.display()), true);
            logger::debug(&format!("Arguments: {arguments}"), true);
            logger::debug(&format!("Profile: {}", profile.name), true);
            logger::debug(&format!("Shell: {}", profile.shell), true);
        }

        self.setup_path_environment();

        if self.debug_mode {
            logger::status(&format!("Launching {}...", profile.name));
        }

        match spawn_terminal(&terminal_path, &arguments, self.naner_root) {
            Ok(()) => {
                if self.debug_mode {
                    logger::success(&format!("Launched: {}", profile.name));
                }
                0
            }
            Err(err) => {
                logger::failure(&format!("Failed to launch terminal: {err}"));
                1
            }
        }
    }

    /// `GetWindowsTerminalPath`: vendor path → PATH → WindowsApps default.
    fn windows_terminal_path(&self) -> Option<PathBuf> {
        if let Some(vendor_path) = self.config.vendor_paths.get("WindowsTerminal")
            && Path::new(vendor_path).is_file()
        {
            return Some(PathBuf::from(vendor_path));
        }

        if let Some(found) = find_executable_in_path(constants::executables::WINDOWS_TERMINAL) {
            return Some(found);
        }

        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let default_path = Path::new(&local_app_data)
                .join("Microsoft")
                .join("WindowsApps")
                .join("wt.exe");
            if default_path.is_file() {
                return Some(default_path);
            }
        }

        None
    }

    /// Additive (no C# counterpart): rusty_term discovery — vendor path →
    /// PATH. Mirrors the Windows Terminal chain minus the WindowsApps step
    /// (rusty_term has no store-installed default location).
    fn rusty_term_path(&self) -> Option<PathBuf> {
        if let Some(vendor_path) = self.config.vendor_paths.get("RustyTerm")
            && Path::new(vendor_path).is_file()
        {
            return Some(PathBuf::from(vendor_path));
        }

        find_executable_in_path(rusty_term_executable_name())
    }

    /// Additive (no C# counterpart): argument string for rusty_term's CLI
    /// (`--gui --title <t> --cwd <dir> [--maximized|--fullscreen] -- prog
    /// args`), quoting and path expansion mirroring the wt builder.
    fn build_rusty_term_arguments(
        &self,
        profile: &ProfileConfig,
        starting_directory_override: Option<&str>,
    ) -> String {
        let root = self.naner_root.to_string_lossy();
        let mut args = String::from("--gui ");

        if !profile.name.is_empty() {
            args.push_str(&format!("--title \"{}\" ", profile.name));
        }

        let start_dir = starting_directory_override.unwrap_or(&profile.starting_directory);
        if !start_dir.is_empty() {
            // Same double expansion pass as the wt builder — preserved.
            let expanded = paths::expand_naner_path(start_dir, &root);
            let expanded = paths::expand_naner_path(&expanded, &root);
            args.push_str(&format!("--cwd \"{expanded}\" "));
        }

        // rusty_term only knows --maximized/--fullscreen; naner's "default"
        // and "focus" launch modes map to no flag.
        match self
            .config
            .windows_terminal
            .launch_mode
            .to_lowercase()
            .as_str()
        {
            "maximized" => args.push_str("--maximized "),
            "fullscreen" => args.push_str("--fullscreen "),
            _ => {}
        }

        match &profile.custom_shell {
            Some(custom) if !custom.executable_path.is_empty() => {
                let shell_path = paths::expand_naner_path(&custom.executable_path, &root);
                match &custom.arguments {
                    Some(shell_args) if !shell_args.is_empty() => {
                        let expanded = paths::expand_naner_path(shell_args, &root);
                        let expanded = paths::expand_naner_path(&expanded, &root);
                        args.push_str(&format!("-- \"{shell_path}\" {expanded}"));
                    }
                    _ => args.push_str(&format!("-- \"{shell_path}\"")),
                }
            }
            _ => {
                if let Some(shell_path) = self.default_shell_path(&profile.shell) {
                    args.push_str(&format!("-- \"{shell_path}\""));
                }
            }
        }

        args.trim().to_string()
    }

    /// `BuildTerminalArguments`, string-identical to the C# builder.
    fn build_terminal_arguments(
        &self,
        profile: &naner_core::config::ProfileConfig,
        starting_directory_override: Option<&str>,
    ) -> String {
        let root = self.naner_root.to_string_lossy();
        let mut args = String::new();

        let launch_mode = &self.config.windows_terminal.launch_mode;
        if !launch_mode.is_empty() && launch_mode != "default" {
            args.push_str(&format!("--{launch_mode} "));
        }

        if !profile.name.is_empty() {
            args.push_str(&format!("--title \"{}\" ", profile.name));
        }

        let start_dir = starting_directory_override.unwrap_or(&profile.starting_directory);
        if !start_dir.is_empty() {
            // The C# code runs ExpandNanerPath and then a second plain
            // %VAR% expansion pass — preserved.
            let expanded = paths::expand_naner_path(start_dir, &root);
            let expanded = paths::expand_naner_path(&expanded, &root);
            args.push_str(&format!("--startingDirectory \"{expanded}\" "));
        }

        match &profile.custom_shell {
            Some(custom) if !custom.executable_path.is_empty() => {
                let shell_path = paths::expand_naner_path(&custom.executable_path, &root);
                match &custom.arguments {
                    Some(shell_args) if !shell_args.is_empty() => {
                        let expanded = paths::expand_naner_path(shell_args, &root);
                        let expanded = paths::expand_naner_path(&expanded, &root);
                        args.push_str(&format!("-- \"{shell_path}\" {expanded}"));
                    }
                    _ => args.push_str(&format!("-- \"{shell_path}\"")),
                }
            }
            _ => {
                if let Some(shell_path) = self.default_shell_path(&profile.shell) {
                    args.push_str(&format!("-- \"{shell_path}\""));
                }
            }
        }

        args.trim().to_string()
    }

    /// `GetDefaultShellPath`.
    fn default_shell_path(&self, shell_type: &str) -> Option<String> {
        match shell_type.to_lowercase().as_str() {
            "powershell" => Some(
                self.config
                    .vendor_paths
                    .get("PowerShell")
                    .cloned()
                    .unwrap_or_else(|| "pwsh.exe".to_string()),
            ),
            "bash" => Some(
                self.config
                    .vendor_paths
                    .get("GitBash")
                    .cloned()
                    .unwrap_or_else(|| "bash.exe".to_string()),
            ),
            "cmd" => Some("cmd.exe".to_string()),
            _ => None,
        }
    }

    /// `SetupPathEnvironment`: rebuild the unified PATH and set it on the
    /// process so the spawned terminal inherits it.
    fn setup_path_environment(&self) {
        let unified = paths::build_unified_path(
            &self.config.environment.path_precedence,
            &self.naner_root.to_string_lossy(),
            self.config.advanced.inherit_system_path,
        );
        // SAFETY: single-threaded launcher flow; no concurrent env access.
        unsafe { std::env::set_var("PATH", &unified) };

        if self.debug_mode {
            let cut = unified
                .char_indices()
                .nth(constants::MAX_PATH_DISPLAY_LENGTH)
                .map(|(i, _)| i)
                .unwrap_or(unified.len());
            logger::debug(&format!("PATH set to: {}...", &unified[..cut]), true);
        }
    }
}

/// Additive: platform name of the rusty_term binary (`rusty_term.exe` on
/// Windows, `rusty_term` elsewhere).
fn rusty_term_executable_name() -> &'static str {
    if cfg!(windows) {
        constants::executables::RUSTY_TERM
    } else {
        "rusty_term"
    }
}

/// `FindExecutableInPath` over the `;`-separated PATH.
fn find_executable_in_path(executable_name: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(';').filter(|d| !d.is_empty()) {
        let candidate = Path::new(dir.trim()).join(executable_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Fire-and-forget spawn with the pre-built argument string (the C# code
/// passes `ProcessStartInfo.Arguments` as one raw string, which only Windows
/// can reproduce via `raw_arg`).
#[cfg(windows)]
fn spawn_terminal(wt_path: &Path, arguments: &str, working_dir: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(wt_path)
        .raw_arg(arguments)
        .current_dir(working_dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn spawn_terminal(_wt_path: &Path, _arguments: &str, _working_dir: &Path) -> Result<(), String> {
    Err("terminal launch is only supported on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use naner_core::config::load_json;

    const CONFIG: &str = r#"{
        "DefaultProfile": "Unified",
        "VendorPaths": {
            "PowerShell": "C:\\naner\\vendor\\powershell\\pwsh.exe",
            "GitBash": "C:\\naner\\vendor\\msys64\\usr\\bin\\bash.exe"
        },
        "WindowsTerminal": { "LaunchMode": "default" },
        "Profiles": {
            "Unified": {
                "Name": "Naner (Unified)",
                "Shell": "PowerShell",
                "StartingDirectory": "C:\\work",
                "CustomShell": {
                    "ExecutablePath": "C:\\naner\\vendor\\powershell\\pwsh.exe",
                    "Arguments": "-NoExit -NoLogo"
                }
            },
            "Bash": { "Name": "Bash", "Shell": "Bash", "StartingDirectory": "C:\\work" },
            "Plain": { "Name": "CMD", "Shell": "CMD", "StartingDirectory": "C:\\work" },
            "Weird": { "Name": "W", "Shell": "fish", "StartingDirectory": "C:\\work" }
        }
    }"#;

    fn config() -> NanerConfig {
        load_json(CONFIG).unwrap()
    }

    #[test]
    fn custom_shell_arguments_form() {
        let cfg = config();
        let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);
        let args = launcher.build_terminal_arguments(cfg.profiles.get("Unified").unwrap(), None);
        assert_eq!(
            args,
            "--title \"Naner (Unified)\" --startingDirectory \"C:\\work\" -- \"C:\\naner\\vendor\\powershell\\pwsh.exe\" -NoExit -NoLogo"
        );
    }

    #[test]
    fn default_shell_by_type_and_directory_override() {
        let cfg = config();
        let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);
        let args =
            launcher.build_terminal_arguments(cfg.profiles.get("Bash").unwrap(), Some("D:\\proj"));
        assert_eq!(
            args,
            "--title \"Bash\" --startingDirectory \"D:\\proj\" -- \"C:\\naner\\vendor\\msys64\\usr\\bin\\bash.exe\""
        );
    }

    #[test]
    fn cmd_is_hardcoded_and_unknown_shell_omits_command() {
        let cfg = config();
        let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);
        let cmd = launcher.build_terminal_arguments(cfg.profiles.get("Plain").unwrap(), None);
        assert!(cmd.ends_with("-- \"cmd.exe\""));
        let weird = launcher.build_terminal_arguments(cfg.profiles.get("Weird").unwrap(), None);
        assert!(
            !weird.contains("--\u{20}\""),
            "unknown shell adds no -- section: {weird}"
        );
        assert_eq!(weird, "--title \"W\" --startingDirectory \"C:\\work\"");
    }

    #[test]
    fn non_default_launch_mode_is_prefixed() {
        let mut cfg = config();
        cfg.windows_terminal.launch_mode = "maximized".into();
        let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);
        let args = launcher.build_terminal_arguments(cfg.profiles.get("Plain").unwrap(), None);
        assert!(args.starts_with("--maximized --title"));
    }

    // ---- Additive terminal abstraction (RustyTerm) ----

    fn with_terminal(
        cfg: &NanerConfig,
        profile: &str,
        terminal: &str,
    ) -> naner_core::config::ProfileConfig {
        let mut p = cfg.profiles.get(profile).unwrap().clone();
        p.terminal = Some(terminal.to_string());
        p
    }

    #[test]
    fn rusty_term_custom_shell_arguments_form() {
        let cfg = config();
        let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);
        let profile = with_terminal(&cfg, "Unified", "RustyTerm");
        let args = launcher.build_rusty_term_arguments(&profile, None);
        assert_eq!(
            args,
            "--gui --title \"Naner (Unified)\" --cwd \"C:\\work\" -- \"C:\\naner\\vendor\\powershell\\pwsh.exe\" -NoExit -NoLogo"
        );
    }

    #[test]
    fn rusty_term_default_shell_and_directory_override() {
        let cfg = config();
        let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);
        let profile = with_terminal(&cfg, "Bash", "RustyTerm");
        let args = launcher.build_rusty_term_arguments(&profile, Some("D:\\proj"));
        assert_eq!(
            args,
            "--gui --title \"Bash\" --cwd \"D:\\proj\" -- \"C:\\naner\\vendor\\msys64\\usr\\bin\\bash.exe\""
        );
    }

    #[test]
    fn rusty_term_launch_mode_mapping() {
        let expect = |mode: &str, expected: &str| {
            let mut cfg = config();
            cfg.windows_terminal.launch_mode = mode.into();
            let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);
            let profile = with_terminal(&cfg, "Plain", "RustyTerm");
            let args = launcher.build_rusty_term_arguments(&profile, None);
            assert_eq!(args, expected, "mode {mode}");
        };
        // default/focus (and empty) map to no flag; maximized/fullscreen
        // map to rusty_term's flags, placed after --cwd.
        expect(
            "default",
            "--gui --title \"CMD\" --cwd \"C:\\work\" -- \"cmd.exe\"",
        );
        expect(
            "focus",
            "--gui --title \"CMD\" --cwd \"C:\\work\" -- \"cmd.exe\"",
        );
        expect(
            "",
            "--gui --title \"CMD\" --cwd \"C:\\work\" -- \"cmd.exe\"",
        );
        expect(
            "maximized",
            "--gui --title \"CMD\" --cwd \"C:\\work\" --maximized -- \"cmd.exe\"",
        );
        expect(
            "Fullscreen",
            "--gui --title \"CMD\" --cwd \"C:\\work\" --fullscreen -- \"cmd.exe\"",
        );
    }

    #[test]
    fn rusty_term_unknown_shell_omits_command() {
        let cfg = config();
        let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);
        let profile = with_terminal(&cfg, "Weird", "RustyTerm");
        let args = launcher.build_rusty_term_arguments(&profile, None);
        assert_eq!(args, "--gui --title \"W\" --cwd \"C:\\work\"");
    }

    #[test]
    fn terminal_kind_mapping_and_unknown_error() {
        let cfg = config();
        let plain = cfg.profiles.get("Plain").unwrap();
        assert_eq!(
            TerminalKind::from_profile(plain).unwrap(),
            TerminalKind::WindowsTerminal
        );
        for (value, expected) in [
            ("WindowsTerminal", TerminalKind::WindowsTerminal),
            ("windowsterminal", TerminalKind::WindowsTerminal),
            ("", TerminalKind::WindowsTerminal),
            ("  ", TerminalKind::WindowsTerminal),
            ("RustyTerm", TerminalKind::RustyTerm),
            ("rustyterm", TerminalKind::RustyTerm),
        ] {
            let p = with_terminal(&cfg, "Plain", value);
            assert_eq!(
                TerminalKind::from_profile(&p).unwrap(),
                expected,
                "{value:?}"
            );
        }
        let p = with_terminal(&cfg, "Plain", "Kitty");
        assert_eq!(
            TerminalKind::from_profile(&p).unwrap_err(),
            "Unknown terminal 'Kitty' (supported: WindowsTerminal, RustyTerm)"
        );
    }

    #[test]
    fn terminal_field_deserializes_with_aliases() {
        let cfg = load_json(
            r#"{
                "DefaultProfile": "A",
                "Profiles": {
                    "A": { "Name": "A", "Shell": "CMD", "Terminal": "RustyTerm" },
                    "B": { "Name": "B", "Shell": "CMD", "terminal": "WindowsTerminal" },
                    "C": { "Name": "C", "Shell": "CMD" }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.profiles["A"].terminal.as_deref(), Some("RustyTerm"));
        assert_eq!(
            cfg.profiles["B"].terminal.as_deref(),
            Some("WindowsTerminal")
        );
        assert_eq!(cfg.profiles["C"].terminal, None);
    }

    #[test]
    fn rusty_term_discovery_prefers_vendor_path_then_path_env() {
        let tmp = tempfile::tempdir().unwrap();
        let vendor_exe = tmp.path().join("vendored-rusty-term.exe");
        std::fs::write(&vendor_exe, "x").unwrap();

        // 1. Vendor path wins when it points at an existing file.
        let mut cfg = config();
        cfg.vendor_paths.insert(
            "RustyTerm".to_string(),
            vendor_exe.to_string_lossy().to_string(),
        );
        let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);
        assert_eq!(launcher.rusty_term_path(), Some(vendor_exe.clone()));

        // 2. Missing vendor path falls back to the PATH scan.
        let path_dir = tempfile::tempdir().unwrap();
        let path_exe = path_dir.path().join(rusty_term_executable_name());
        std::fs::write(&path_exe, "x").unwrap();

        let mut cfg = config();
        cfg.vendor_paths.insert(
            "RustyTerm".to_string(),
            tmp.path().join("does-not-exist.exe").display().to_string(),
        );
        let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);

        let saved_path = std::env::var("PATH").ok();
        // SAFETY: test-local mutation, restored below; no other test in this
        // binary reads PATH.
        unsafe { std::env::set_var("PATH", path_dir.path()) };
        let found = launcher.rusty_term_path();
        let vendor_first = {
            // 3. With PATH populated, an existing vendor path still wins.
            let mut cfg = config();
            cfg.vendor_paths.insert(
                "RustyTerm".to_string(),
                vendor_exe.to_string_lossy().to_string(),
            );
            TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false).rusty_term_path()
        };
        match saved_path {
            Some(p) => unsafe { std::env::set_var("PATH", p) },
            None => unsafe { std::env::remove_var("PATH") },
        }
        assert_eq!(found, Some(path_exe));
        assert_eq!(vendor_first, Some(vendor_exe));
    }

    #[test]
    fn profile_without_terminal_keeps_wt_behavior_bit_for_bit() {
        // Parity guard: no Terminal field → the WindowsTerminal kind and the
        // exact pre-existing wt argument string, untouched by the seam.
        let cfg = config();
        let launcher = TerminalLauncher::new(Path::new("C:\\naner"), &cfg, false);
        let profile = cfg.profiles.get("Unified").unwrap();
        assert_eq!(profile.terminal, None);
        let kind = TerminalKind::from_profile(profile).unwrap();
        assert_eq!(kind, TerminalKind::WindowsTerminal);
        let args = match kind {
            TerminalKind::WindowsTerminal => launcher.build_terminal_arguments(profile, None),
            TerminalKind::RustyTerm => launcher.build_rusty_term_arguments(profile, None),
        };
        assert_eq!(
            args,
            "--title \"Naner (Unified)\" --startingDirectory \"C:\\work\" -- \"C:\\naner\\vendor\\powershell\\pwsh.exe\" -NoExit -NoLogo"
        );
    }
}
