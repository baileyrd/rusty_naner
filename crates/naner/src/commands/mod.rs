//! Port of `CommandRouter` + `CommandNames`: first-layer dispatch on a
//! case-insensitive `args[0]`. Returns `None` when nothing matched, meaning
//! the launcher path should run (the C# `NoCommandMatch = -1` sentinel that
//! never escapes to the OS).

pub mod diagnose;
pub mod help;
pub mod root;
pub mod vendors;
pub mod version;

pub mod names {
    pub const VERSION: &str = "--version";
    pub const VERSION_SHORT: &str = "-v";
    pub const HELP: &str = "--help";
    pub const HELP_SHORT: &str = "-h";
    pub const HELP_ALTERNATE: &str = "/?";
    pub const DIAGNOSE: &str = "--diagnose";
    pub const UPDATE_VENDORS: &str = "update-vendors";
    pub const INSTALL: &str = "install";
    pub const DEBUG: &str = "--debug";
    /// Additive Unix-philosophy command (MIGRATION_ANALYSIS §2.4 tier 2):
    /// `naner root` prints the discovered NANER_ROOT and nothing else.
    pub const ROOT: &str = "root";

    /// Commands whose output requires a console (`CommandNames.ConsoleCommands`
    /// plus the additive `root`).
    pub const CONSOLE_COMMANDS: [&str; 10] = [
        VERSION,
        VERSION_SHORT,
        HELP,
        HELP_SHORT,
        HELP_ALTERNATE,
        DIAGNOSE,
        UPDATE_VENDORS,
        INSTALL,
        DEBUG,
        ROOT,
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
        names::UPDATE_VENDORS => Some(vendors::execute_update(rest)),
        names::INSTALL => Some(vendors::execute_install(rest)),
        names::ROOT => Some(root::execute()),
        _ => None,
    }
}
