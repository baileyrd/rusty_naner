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

/// Route `args` to a command. `Some(exit_code)` when a command ran; `None`
/// when the launcher should proceed.
pub fn route(args: &[String]) -> Option<i32> {
    let first = args.first()?.to_lowercase();
    let rest = &args[1..];

    match first.as_str() {
        names::VERSION | names::VERSION_SHORT => Some(version::execute()),
        names::HELP | names::HELP_SHORT | names::HELP_ALTERNATE => Some(help::execute()),
        names::DIAGNOSE => Some(diagnose::execute()),
        names::DOCTOR | names::DOCTOR_ALT => Some(doctor::execute(rest)),
        names::SCHEMA => Some(schema::execute(rest)),
        names::COMPLETIONS => Some(completions::execute(rest)),
        names::SHELL_INTEGRATION => Some(shell_integration::execute(rest)),
        names::SETUP_SHELL => Some(setup_shell::execute(rest)),
        names::REPAIR => Some(repair::execute(rest)),
        names::PROFILE => Some(profile::execute(rest)),
        names::CHECKSUM => Some(checksum::execute(rest)),
        names::DIFF => Some(diff::execute(rest)),
        names::BENCH => Some(bench::execute(rest)),
        names::MIGRATE => Some(migrate::execute(rest)),
        names::PACK => Some(pack::execute(rest)),
        names::SELF_UPDATE => Some(self_update::execute(rest)),
        names::UPDATE_VENDORS => Some(vendors::execute_update(rest)),
        names::INSTALL => Some(vendors::execute_install(rest)),
        names::ROOT => Some(root::execute()),
        names::LOCK => Some(lock::execute(rest)),
        _ => None,
    }
}
