//! Port of Naner.Vendors: definitions, vendors.json loader, the unified
//! installer with its six release-source resolvers and two-level fallback
//! cascade, and the Windows Terminal portable-mode configurator.

mod installer;
mod loader;
mod wt_config;

pub use installer::{UnifiedVendorInstaller, VendorDownloadInfo};
pub use loader::VendorConfigurationLoader;
pub use wt_config::{WindowsTerminalConfigurator, is_windows_terminal};

use crate::checksum::ChecksumInfo;
use crate::constants;

/// Per-vendor installed-version marker (`VendorInstallerBase`).
pub const VENDOR_VERSION_FILE: &str = ".vendor-version";

/// `VendorSourceType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VendorSourceType {
    #[default]
    StaticUrl,
    GitHub,
    WebScrape,
    NodeJsApi,
    GolangApi,
    DotNetApi,
}

/// `WebScrapeConfig`.
#[derive(Clone, Debug, Default)]
pub struct WebScrapeConfig {
    pub url: String,
    pub pattern: String,
    pub base_url: String,
}

/// `VendorDefinition` — how to fetch and install one vendor.
///
/// `Default` is hand-written rather than derived so `enabled` starts `true`.
/// A derived `bool` default of `false` would silently disable every built-in
/// essential vendor and every test fixture that uses `..Default::default()`,
/// which is a far worse failure than an unwanted vendor being offered.
#[derive(Clone, Debug)]
pub struct VendorDefinition {
    pub name: String,
    pub key: String,
    pub description: String,
    pub extract_dir: String,
    pub enabled: bool,
    pub required: bool,
    pub dependencies: Vec<String>,
    pub source_type: VendorSourceType,

    // Static URLs
    pub static_url: Option<String>,
    pub file_name: Option<String>,

    // GitHub releases. B1 fixed: patterns with `*`/`?` glob-match the whole
    // asset name; wildcard-free patterns keep substring semantics (which the
    // `asset_pattern` + `asset_pattern_end` built-in pairs rely on).
    pub github_owner: Option<String>,
    pub github_repo: Option<String>,
    pub asset_pattern: Option<String>,
    pub asset_pattern_end: Option<String>,

    // Web scraping
    pub web_scrape: Option<WebScrapeConfig>,

    // Fallback
    pub fallback_url: Option<String>,
    pub fallback_version: Option<String>,
    pub fallback_file_name: Option<String>,

    // Checksum (B2 fixed: populated from an optional vendors.json object).
    // A value here pins the artifact and outranks anything a resolver
    // discovers upstream.
    pub checksum: Option<ChecksumInfo>,

    /// Where to fetch a digest for a dynamically-resolved artifact, for
    /// sources that publish one outside the resolution response itself.
    /// The `golang-api` / `nodejs-api` / `dotnet-api` resolvers need no
    /// config — each has exactly one place to look.
    pub checksum_source: Option<ChecksumSource>,

    // Executable installers
    pub install_type: Option<String>,
    pub installer_args: Option<Vec<String>>,
}

impl Default for VendorDefinition {
    fn default() -> Self {
        Self {
            name: String::new(),
            key: String::new(),
            description: String::new(),
            extract_dir: String::new(),
            enabled: true,
            required: false,
            dependencies: Vec::new(),
            source_type: VendorSourceType::default(),
            static_url: None,
            file_name: None,
            github_owner: None,
            github_repo: None,
            asset_pattern: None,
            asset_pattern_end: None,
            web_scrape: None,
            fallback_url: None,
            fallback_version: None,
            fallback_file_name: None,
            checksum: None,
            checksum_source: None,
            install_type: None,
            installer_args: None,
        }
    }
}

/// How to obtain an upstream digest for an artifact whose URL is only known
/// after resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChecksumSource {
    /// A file alongside the download, named `<download-url><suffix>` — the
    /// form `static.rust-lang.org` uses (`rustup-init.exe.sha256`).
    Sidecar { suffix: String },
    /// A digest embedded in a listing page, captured by group 1 of `pattern`
    /// after `{FILE}` is replaced with the resolved file name — the form
    /// `repo.anaconda.com/miniconda/` uses.
    Scrape { url: String, pattern: String },
}

/// `VendorDefinitionFactory`: the hardcoded essential set used when
/// vendors.json is missing/invalid, 7-Zip deliberately first (it unblocks
/// the other extractions).
/// The built-in fallback set, used by `update-vendors` and whenever
/// `vendors.json` is missing, empty or unparseable.
///
/// Every entry must set `key`, and the key must match the one `vendors.json`
/// uses for the same vendor. `naner.lock` is keyed by it: six definitions
/// sharing the default `""` meant every `update-vendors` install overwrote the
/// previous one's pin, and -- on a tree without a readable `vendors.json` --
/// every vendor then resolved that single entry as its own pin, downloading
/// one vendor's artifact under another's name and verifying it successfully
/// against that artifact's genuine digest.
pub fn essential_vendor_definitions() -> Vec<VendorDefinition> {
    vec![
        VendorDefinition {
            name: constants::vendor_names::SEVEN_ZIP.into(),
            key: "SevenZip".into(),
            extract_dir: "7zip".into(),
            // 7-zip.org moved its binaries to GitHub releases; the old
            // download.html scrape now yields a mangled URL. GitHub source
            // with a glob (works since the B1 fix) is the real path.
            source_type: VendorSourceType::GitHub,
            github_owner: Some("ip7z".into()),
            github_repo: Some("7zip".into()),
            asset_pattern: Some("7z*-x64.msi".into()),
            fallback_url: Some("https://www.7-zip.org/a/7z2408-x64.msi".into()),
            ..Default::default()
        },
        VendorDefinition {
            name: constants::vendor_names::POWERSHELL.into(),
            key: "PowerShell".into(),
            extract_dir: "powershell".into(),
            source_type: VendorSourceType::GitHub,
            github_owner: Some("PowerShell".into()),
            github_repo: Some("PowerShell".into()),
            asset_pattern: Some("win-x64.zip".into()),
            fallback_url: Some(
                "https://github.com/PowerShell/PowerShell/releases/download/v7.4.6/PowerShell-7.4.6-win-x64.zip"
                    .into(),
            ),
            ..Default::default()
        },
        VendorDefinition {
            name: constants::vendor_names::WINDOWS_TERMINAL.into(),
            key: "WindowsTerminal".into(),
            extract_dir: "terminal".into(),
            source_type: VendorSourceType::GitHub,
            github_owner: Some("microsoft".into()),
            github_repo: Some("terminal".into()),
            asset_pattern: Some("Microsoft.WindowsTerminal_".into()),
            asset_pattern_end: Some("_x64.zip".into()),
            fallback_url: Some(
                "https://github.com/microsoft/terminal/releases/download/v1.21.2361.0/Microsoft.WindowsTerminal_1.21.2361.0_x64.zip"
                    .into(),
            ),
            ..Default::default()
        },
        VendorDefinition {
            name: constants::vendor_names::MSYS2.into(),
            key: "MSYS2".into(),
            extract_dir: "msys64".into(),
            source_type: VendorSourceType::WebScrape,
            web_scrape: Some(WebScrapeConfig {
                url: "https://repo.msys2.org/distrib/x86_64/".into(),
                pattern: r#"href="(msys2-base-x86_64-\d+\.tar\.xz)""#.into(),
                base_url: "https://repo.msys2.org/distrib/x86_64/".into(),
            }),
            fallback_url: Some(
                "https://repo.msys2.org/distrib/x86_64/msys2-base-x86_64-20240727.tar.xz".into(),
            ),
            ..Default::default()
        },
        VendorDefinition {
            name: constants::vendor_names::RUSTY_TERM.into(),
            key: "RustyTerm".into(),
            extract_dir: "rusty_term".into(),
            source_type: VendorSourceType::GitHub,
            github_owner: Some("baileyrd".into()),
            github_repo: Some("rusty_term".into()),
            asset_pattern: Some("*win-x64.zip".into()),
            fallback_url: Some(
                "https://github.com/baileyrd/rusty_term/releases/download/v0.1.0/rusty_term-v0.1.0-win-x64.zip".into(),
            ),
            ..Default::default()
        },
        VendorDefinition {
            name: constants::vendor_names::RUSH.into(),
            key: "Rush".into(),
            extract_dir: "rush".into(),
            source_type: VendorSourceType::GitHub,
            github_owner: Some("baileyrd".into()),
            github_repo: Some("rush".into()),
            asset_pattern: Some("*win-x64.zip".into()),
            fallback_url: Some(
                "https://github.com/baileyrd/rush/releases/download/v0.1.0/rush-v0.1.0-win-x64.zip".into(),
            ),
            ..Default::default()
        },
    ]
}

#[cfg(test)]
mod builtin_tests {
    use super::essential_vendor_definitions;
    use std::collections::HashSet;

    /// `naner.lock` is a map keyed by `VendorDefinition::key`. Every built-in
    /// left it at the `String::default()` empty string, so each install through
    /// `update-vendors` overwrote the previous one's pin and only the last
    /// survived -- visible as a nameless row in `naner lock`.
    ///
    /// The dangerous half is the read: on a tree where `vendors.json` is
    /// missing, empty or unparseable, `load_all_vendors` falls back to this set,
    /// and `lock.get(&vendor.key)` then hands every vendor the one `""` entry.
    /// Installing PowerShell would fetch whichever artifact wrote that entry
    /// last and verify it successfully, because the digest is genuine -- of the
    /// wrong file.
    #[test]
    fn every_builtin_has_a_key() {
        for vendor in essential_vendor_definitions() {
            assert!(
                !vendor.key.trim().is_empty(),
                "{:?} has no key; it would share a lockfile entry with every \
                 other keyless vendor",
                vendor.name
            );
        }
    }

    #[test]
    fn builtin_keys_are_unique() {
        let vendors = essential_vendor_definitions();
        let unique: HashSet<&str> = vendors.iter().map(|v| v.key.as_str()).collect();
        assert_eq!(
            unique.len(),
            vendors.len(),
            "two built-ins share a key, so they share a pin"
        );
    }

    /// The keys have to match `vendors.json`, or a vendor installed from the
    /// manifest and the same vendor installed from the fallback set would write
    /// two separate pins for one directory.
    #[test]
    fn builtin_keys_match_the_shipped_manifest() {
        // Kept as a literal rather than read from disk: this is the contract,
        // and a test that reads the file it is checking cannot catch the file
        // changing.
        const MANIFEST_KEYS: [&str; 6] = [
            "SevenZip",
            "PowerShell",
            "WindowsTerminal",
            "MSYS2",
            "RustyTerm",
            "Rush",
        ];
        for vendor in essential_vendor_definitions() {
            assert!(
                MANIFEST_KEYS.contains(&vendor.key.as_str()),
                "built-in key {:?} is not one vendors.json uses",
                vendor.key
            );
        }
    }
}
