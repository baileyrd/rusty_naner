//! Port of `CommandRouter` + `CommandNames`: first-layer dispatch on a
//! case-insensitive `args[0]`. Returns `None` when nothing matched, meaning
//! the launcher path should run (the C# `NoCommandMatch = -1` sentinel that
//! never escapes to the OS).

pub mod bench;
pub mod checksum;
pub mod completions;
pub mod diagnose;
pub mod diff;
pub mod doctor;
pub mod help;
pub mod lock;
pub mod migrate;
pub mod pack;
pub mod profile;
pub mod repair;
pub mod root;
pub mod schema;
pub mod self_update;
pub mod setup_shell;
pub mod shell_integration;
pub mod vendors;
pub mod version;

pub mod names {
    pub const VERSION: &str = "--version";
    pub const VERSION_SHORT: &str = "-v";
    pub const HELP: &str = "--help";
    pub const HELP_SHORT: &str = "-h";
    pub const HELP_ALTERNATE: &str = "/?";
    pub const DIAGNOSE: &str = "--diagnose";
    pub const DOCTOR: &str = "doctor";
    pub const DOCTOR_ALT: &str = "--doctor";
    pub const SCHEMA: &str = "schema";
    pub const COMPLETIONS: &str = "completions";
    pub const SHELL_INTEGRATION: &str = "shell-integration";
    pub const SETUP_SHELL: &str = "setup-shell";
    pub const REPAIR: &str = "repair";
    pub const PROFILE: &str = "profile";
    pub const CHECKSUM: &str = "checksum";
    pub const DIFF: &str = "diff";
    pub const BENCH: &str = "bench";
    pub const MIGRATE: &str = "migrate";
    pub const PACK: &str = "pack";
    pub const SELF_UPDATE: &str = "self-update";
    pub const UPDATE_VENDORS: &str = "update-vendors";
    pub const INSTALL: &str = "install";
    pub const DEBUG: &str = "--debug";
    pub const ROOT: &str = "root";
    pub const LOCK: &str = "lock";

    pub const CONSOLE_COMMANDS: [&str; 25] = [
        VERSION,
        VERSION_SHORT,
        HELP,
        HELP_SHORT,
        HELP_ALTERNATE,
        DIAGNOSE,
        DOCTOR,
        DOCTOR_ALT,
        SCHEMA,
        COMPLETIONS,
        SHELL_INTEGRATION,
        SETUP_SHELL,
        REPAIR,
        PROFILE,
        CHECKSUM,
        DIFF,
        BENCH,
        MIGRATE,
        PACK,
        SELF_UPDATE,
        UPDATE_VENDORS,
        INSTALL,
        DEBUG,
        ROOT,
        LOCK,
    ];
}

/// Which verb an argument names, decided without running anything.
///
/// Split out from [`route`] so dispatch is testable: calling `route` in a test
/// would actually install vendors or hit the network. `parse` is pure, so the
/// table can be covered exhaustively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Version,
    Help,
    Diagnose,
    Doctor,
    Schema,
    Completions,
    ShellIntegration,
    SetupShell,
    Repair,
    Profile,
    Checksum,
    Diff,
    Bench,
    Migrate,
    Pack,
    SelfUpdate,
    UpdateVendors,
    Install,
    Root,
    Lock,
}

impl Verb {
    /// Match `arg` case-insensitively, as the C# router did.
    pub fn parse(arg: &str) -> Option<Self> {
        let first = arg.to_lowercase();
        Some(match first.as_str() {
            names::VERSION | names::VERSION_SHORT => Self::Version,
            names::HELP | names::HELP_SHORT | names::HELP_ALTERNATE => Self::Help,
            names::DIAGNOSE => Self::Diagnose,
            names::DOCTOR | names::DOCTOR_ALT => Self::Doctor,
            names::SCHEMA => Self::Schema,
            names::COMPLETIONS => Self::Completions,
            names::SHELL_INTEGRATION => Self::ShellIntegration,
            names::SETUP_SHELL => Self::SetupShell,
            names::REPAIR => Self::Repair,
            names::PROFILE => Self::Profile,
            names::CHECKSUM => Self::Checksum,
            names::DIFF => Self::Diff,
            names::BENCH => Self::Bench,
            names::MIGRATE => Self::Migrate,
            names::PACK => Self::Pack,
            names::SELF_UPDATE => Self::SelfUpdate,
            names::UPDATE_VENDORS => Self::UpdateVendors,
            names::INSTALL => Self::Install,
            names::ROOT => Self::Root,
            names::LOCK => Self::Lock,
            _ => return None,
        })
    }

    fn run(self, rest: &[String]) -> i32 {
        match self {
            Self::Version => version::execute(),
            Self::Help => help::execute(),
            Self::Diagnose => diagnose::execute(),
            Self::Doctor => doctor::execute(rest),
            Self::Schema => schema::execute(rest),
            Self::Completions => completions::execute(rest),
            Self::ShellIntegration => shell_integration::execute(rest),
            Self::SetupShell => setup_shell::execute(rest),
            Self::Repair => repair::execute(rest),
            Self::Profile => profile::execute(rest),
            Self::Checksum => checksum::execute(rest),
            Self::Diff => diff::execute(rest),
            Self::Bench => bench::execute(rest),
            Self::Migrate => migrate::execute(rest),
            Self::Pack => pack::execute(rest),
            Self::SelfUpdate => self_update::execute(rest),
            Self::UpdateVendors => vendors::execute_update(rest),
            Self::Install => vendors::execute_install(rest),
            Self::Root => root::execute(),
            Self::Lock => lock::execute(rest),
        }
    }
}

/// Route `args` to a command. `Some(exit_code)` when a command ran; `None`
/// when the launcher should proceed.
pub fn route(args: &[String]) -> Option<i32> {
    let first = args.first()?;
    Verb::parse(first).map(|verb| verb.run(&args[1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--debug` is in the console list because it forces a console to be
    /// attached, not because it is a verb — the launcher consumes it.
    const NOT_VERBS: [&str; 1] = [names::DEBUG];

    #[test]
    fn every_console_command_is_routed_or_a_known_launcher_flag() {
        for name in names::CONSOLE_COMMANDS {
            let routed = Verb::parse(name).is_some();
            let expected = !NOT_VERBS.contains(&name);
            assert_eq!(
                routed,
                expected,
                "{name:?} is listed as a console command but {}",
                if expected {
                    "does not route"
                } else {
                    "unexpectedly routes"
                }
            );
        }
    }

    /// The console list drives whether a console window is attached at all, so
    /// a verb missing from it runs with nowhere to print on a GUI launch.
    #[test]
    fn every_routed_verb_appears_in_the_console_list() {
        for name in [
            names::VERSION,
            names::VERSION_SHORT,
            names::HELP,
            names::HELP_SHORT,
            names::HELP_ALTERNATE,
            names::DIAGNOSE,
            names::DOCTOR,
            names::DOCTOR_ALT,
            names::SCHEMA,
            names::COMPLETIONS,
            names::SHELL_INTEGRATION,
            names::SETUP_SHELL,
            names::REPAIR,
            names::PROFILE,
            names::CHECKSUM,
            names::DIFF,
            names::BENCH,
            names::MIGRATE,
            names::PACK,
            names::SELF_UPDATE,
            names::UPDATE_VENDORS,
            names::INSTALL,
            names::ROOT,
            names::LOCK,
        ] {
            assert!(
                names::CONSOLE_COMMANDS.contains(&name),
                "{name:?} routes but is missing from CONSOLE_COMMANDS, so it would \
                 run with no console attached"
            );
        }
    }

    #[test]
    fn dispatch_is_case_insensitive() {
        assert_eq!(Verb::parse("DOCTOR"), Some(Verb::Doctor));
        assert_eq!(Verb::parse("Install"), Some(Verb::Install));
        assert_eq!(Verb::parse("--VERSION"), Some(Verb::Version));
        assert_eq!(Verb::parse("Self-Update"), Some(Verb::SelfUpdate));
    }

    #[test]
    fn both_spellings_reach_the_same_verb() {
        assert_eq!(Verb::parse("doctor"), Verb::parse("--doctor"));
        assert_eq!(Verb::parse("--help"), Verb::parse("-h"));
        assert_eq!(Verb::parse("--help"), Verb::parse("/?"));
        assert_eq!(Verb::parse("--version"), Verb::parse("-v"));
    }

    #[test]
    fn a_non_verb_falls_through_to_the_launcher() {
        for arg in ["--profile", "-p", "--debug", "--export-env", "unknown", ""] {
            assert_eq!(Verb::parse(arg), None, "{arg:?} must not be routed");
        }
    }

    #[test]
    fn no_arguments_means_no_command() {
        assert_eq!(route(&[]), None);
    }

    /// Every verb must be reachable by at least one name, or it is dead code
    /// that looks alive.
    #[test]
    fn every_verb_has_a_name_that_reaches_it() {
        let all = [
            Verb::Version,
            Verb::Help,
            Verb::Diagnose,
            Verb::Doctor,
            Verb::Schema,
            Verb::Completions,
            Verb::ShellIntegration,
            Verb::SetupShell,
            Verb::Repair,
            Verb::Profile,
            Verb::Checksum,
            Verb::Diff,
            Verb::Bench,
            Verb::Migrate,
            Verb::Pack,
            Verb::SelfUpdate,
            Verb::UpdateVendors,
            Verb::Install,
            Verb::Root,
            Verb::Lock,
        ];
        for verb in all {
            assert!(
                names::CONSOLE_COMMANDS
                    .iter()
                    .any(|n| Verb::parse(n) == Some(verb)),
                "{verb:?} is not reachable from any console command name"
            );
        }
    }
}
