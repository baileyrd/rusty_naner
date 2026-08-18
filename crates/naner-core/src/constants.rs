//! Port of `NanerConstants` (Naner.Core): names, directories, URLs, limits.
//! Exact strings matter — install-order code matches on vendor display names
//! (MIGRATION_ANALYSIS §3, drift note).

/// Version baked at compile time. The release workflow enforces tag == this
/// value, which is what keeps the naner-init update protocol working
/// (MIGRATION_ANALYSIS §4.2).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const PRODUCT_NAME: &str = "Naner Terminal Launcher";
/// C# says "Pure C# Implementation"; keeping the structure but telling the
/// truth is a deliberate, visible divergence (the version number on the same
/// screen already differs during the migration).
pub const PHASE_NAME: &str = "Production Release - Pure Rust Implementation";

pub const INITIALIZATION_MARKER_FILE: &str = ".naner-initialized";
pub const VERSION_FILE: &str = ".naner-version";
pub const CONFIG_FILE_NAME: &str = "naner.json";
/// Directory of per-vendor definition files, one JSON file per vendor,
/// under `config/`. Replaced the single `vendors.json`; the old name is
/// kept below only so the loader can recognise a pre-split tree and say so.
pub const VENDORS_CONFIG_DIR_NAME: &str = "vendors";
/// The pre-split monolithic file. No longer read -- see
/// `VendorConfigurationLoader::load_all_vendors`.
pub const LEGACY_VENDORS_CONFIG_FILE_NAME: &str = "vendors.json";

/// Supported configuration file names in priority order (no cross-file
/// merging: the first that exists wins).
pub const CONFIG_FILE_NAMES: [&str; 1] = ["naner.json"];

/// The YAML configuration files naner read before v0.7.0. No longer loaded --
/// kept so the loader can recognise one sitting where a config should be and
/// say "convert this" instead of a bare "not found" while a perfectly
/// good-looking file sits right there.
pub const LEGACY_YAML_CONFIG_FILE_NAMES: [&str; 2] = ["naner.yaml", "naner.yml"];

pub mod github {
    pub const OWNER: &str = "baileyrd";
    pub const REPO: &str = "rusty_naner";

    pub fn user_agent() -> String {
        format!("Naner/{}", super::VERSION)
    }
}

pub mod directory_names {
    pub const BIN: &str = "bin";
    pub const VENDOR: &str = "vendor";
    pub const VENDOR_BIN: &str = "vendor/bin";
    pub const CONFIG: &str = "config";
    pub const HOME: &str = "home";
    pub const LOGS: &str = "logs";
    pub const DOWNLOADS: &str = ".downloads";

    /// First-run "essential" set — note this includes `home`, while root
    /// *discovery* markers are only bin+vendor+config. The asymmetry is
    /// intentional behavior (MIGRATION_ANALYSIS §1.5).
    pub const ESSENTIAL: [&str; 4] = [BIN, VENDOR, CONFIG, HOME];
}

pub mod executables {
    pub const NANER: &str = "naner.exe";
    pub const NANER_INIT: &str = "naner-init.exe";
    pub const WINDOWS_TERMINAL: &str = "wt.exe";
    pub const POWERSHELL: &str = "pwsh.exe";
    pub const BASH: &str = "bash.exe";
    pub const SEVEN_ZIP: &str = "7z.exe";
    /// Additive (no C# counterpart): the rusty_term terminal binary.
    pub const RUSTY_TERM: &str = "rusty_term.exe";
}

pub mod vendor_names {
    // Essential vendors
    pub const SEVEN_ZIP: &str = "7-Zip";
    pub const POWERSHELL: &str = "PowerShell";
    pub const WINDOWS_TERMINAL: &str = "Windows Terminal";
    pub const GIT_FOR_WINDOWS: &str = "Git for Windows";

    // Optional vendors
    pub const NODEJS: &str = "Node.js";
    pub const BUN: &str = "Bun";
    pub const ANACONDA: &str = "Anaconda";
    pub const GO: &str = "Go";
    pub const RUST: &str = "Rust";
    pub const RUBY: &str = "Ruby";
    pub const DOTNET_SDK: &str = ".NET SDK";
    /// Demoted from essential: Git for Windows now provides the default
    /// Bash profile's shell. Still installable by name. vendors.json says
    /// plain "MSYS2"; this constant (and install-order matching) says this.
    /// Keep both exact (MIGRATION_ANALYSIS §3 drift).
    pub const MSYS2: &str = "MSYS2 (Git/Bash)";
    /// Additive (no C# counterpart): experimental rusty_term terminal.
    pub const RUSTY_TERM: &str = "Rusty Term";
    /// Additive (no C# counterpart): experimental rush shell.
    pub const RUSH: &str = "Rush";
}

// ===== HTTP configuration =====

pub const DEFAULT_HTTP_TIMEOUT_MINUTES: u64 = 10;
pub const HTTP_DOWNLOAD_BUFFER_SIZE: usize = 8192;
pub const PROGRESS_UPDATE_INTERVAL: u32 = 10;

// ===== Path resolution =====

pub const MAX_NANER_ROOT_SEARCH_DEPTH: usize = 10;
pub const MAX_PATH_DISPLAY_LENGTH: usize = 200;

pub fn default_user_agent() -> String {
    format!("Naner/{VERSION}")
}
