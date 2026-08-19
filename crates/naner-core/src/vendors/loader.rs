//! Port of `VendorConfigurationLoader`: parse vendors.json (case-insensitive,
//! comments + trailing commas tolerated), convert to definitions, fall back
//! to the hardcoded essential set when missing/empty/invalid.
//!
//! Post-parity fixes (see docs/post-parity-fix-wave.md): B1 — `assetPattern`
//! globs now match for real in the installer (`asset_pattern_end` remains a
//! built-in-defaults-only mechanism); B2 — an optional `checksum`
//! `{algorithm, value, required}` object per vendor entry is wired through to
//! the verifier.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{
    ChecksumSource, VendorDefinition, VendorSourceType, WebScrapeConfig,
    essential_vendor_definitions,
};
use crate::config::strip_json_comments;
use crate::{constants, logger};

// ---- vendors.json wire models (`VendorJsonModels`) ----

#[derive(Debug, Deserialize)]
#[serde(default)]
struct VendorJsonEntry {
    name: String,
    description: String,
    #[serde(rename = "extractDir", alias = "extractdir")]
    extract_dir: String,
    /// Defaults to *true*: `#[serde(default)]` on the struct would otherwise
    /// give a bool `false`, so omitting the field would silently disable a
    /// vendor. Opting out has to be deliberate.
    #[serde(default = "default_true")]
    enabled: bool,
    required: bool,
    dependencies: Option<Vec<String>>,
    #[serde(rename = "releaseSource", alias = "releasesource")]
    release_source: Option<ReleaseSourceJson>,
    #[serde(rename = "installType", alias = "installtype")]
    install_type: Option<String>,
    #[serde(rename = "installerArgs", alias = "installerargs")]
    installer_args: Option<Vec<String>>,
    #[serde(rename = "pathPriority", alias = "pathpriority")]
    path_priority: Option<i64>,
    #[serde(rename = "pathPrecedence", alias = "pathprecedence")]
    path_precedence: Option<Vec<String>>,
    #[serde(rename = "environmentVariables", alias = "environmentvariables")]
    environment_variables: Option<crate::collections::OrderedMap<String>>,
    checksum: Option<ChecksumJson>,
    #[serde(rename = "checksumSource", alias = "checksumsource")]
    checksum_source: Option<ChecksumSourceJson>,
    provides: Option<Vec<String>>,
    #[serde(rename = "binaryName", alias = "binaryname")]
    binary_name: Option<String>,
}

impl Default for VendorJsonEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            extract_dir: String::new(),
            enabled: true,
            required: false,
            dependencies: None,
            release_source: None,
            install_type: None,
            installer_args: None,
            path_priority: None,
            path_precedence: None,
            environment_variables: None,
            checksum: None,
            checksum_source: None,
            provides: None,
            binary_name: None,
        }
    }
}

/// Where to fetch a digest for a dynamically-resolved artifact.
/// `type: "sidecar"` reads `<download-url><suffix>`; `type: "scrape"` pulls
/// capture group 1 out of `url`, with `{FILE}` in `pattern` replaced by the
/// resolved file name.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ChecksumSourceJson {
    #[serde(rename = "type")]
    source_type: String,
    suffix: Option<String>,
    url: Option<String>,
    pattern: Option<String>,
}

/// Optional per-vendor checksum (fix for B2 — the C# schema never had one).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ChecksumJson {
    /// `SHA256` (default), `SHA512`, `SHA384`, `SHA1`, or `MD5`.
    algorithm: Option<String>,
    value: Option<String>,
    /// When true a mismatch blocks installation; otherwise it only warns.
    required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ReleaseSourceJson {
    #[serde(rename = "type")]
    source_type: String,
    repo: Option<String>,
    #[serde(rename = "assetPattern", alias = "assetpattern")]
    asset_pattern: Option<String>,
    url: Option<String>,
    pattern: Option<String>,
    #[serde(rename = "fileName", alias = "filename")]
    file_name: Option<String>,
    /// Package name, for `type: "npm"` and `type: "pip"`.
    package: Option<String>,
    fallback: Option<FallbackJson>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FallbackJson {
    version: Option<String>,
    url: Option<String>,
    #[serde(rename = "fileName", alias = "filename")]
    file_name: Option<String>,
}

pub struct VendorConfigurationLoader {
    config_dir: PathBuf,
    legacy_config_path: PathBuf,
    vendor_dir: PathBuf,
}

impl VendorConfigurationLoader {
    pub fn new(naner_root: &Path) -> Self {
        let config = naner_root.join(constants::directory_names::CONFIG);
        Self {
            config_dir: config.join(constants::VENDORS_CONFIG_DIR_NAME),
            legacy_config_path: config.join(constants::LEGACY_VENDORS_CONFIG_FILE_NAME),
            vendor_dir: naner_root.join(constants::directory_names::VENDOR),
        }
    }

    /// A loader over an explicit vendor-definitions directory, for tooling
    /// that operates outside a naner tree — `refresh-pins` pointed at this
    /// repo's own `dist-assets/config/vendors`, for instance. Install-state
    /// queries (`is_vendor_installed`, `vendor_version`) resolve against
    /// `<dir>/../../vendor`, which simply won't exist for such a directory
    /// and correctly answers "not installed."
    pub fn from_vendors_dir(vendors_dir: &Path) -> Self {
        let root = vendors_dir
            .parent()
            .and_then(Path::parent)
            .unwrap_or(vendors_dir);
        Self {
            config_dir: vendors_dir.to_path_buf(),
            legacy_config_path: vendors_dir
                .parent()
                .unwrap_or(vendors_dir)
                .join(constants::LEGACY_VENDORS_CONFIG_FILE_NAME),
            vendor_dir: root.join(constants::directory_names::VENDOR),
        }
    }

    /// `LoadVendors`: file → parse → convert, with the default-essential
    /// fallback and the same warnings.
    /// Vendors the user has actually opted into.
    ///
    /// `enabled: false` was parsed and then ignored everywhere, so
    /// `install --all` installed vendors the config had switched off and
    /// `install --list` advertised them. Filtering here means every caller
    /// gets the same answer.
    pub fn load_vendors(&self) -> Vec<VendorDefinition> {
        self.load_all_vendors()
            .into_iter()
            .filter(|v| v.enabled)
            .collect()
    }

    /// Every vendor in the file, disabled ones included — for tooling that
    /// needs to show what exists rather than what is switched on.
    pub fn load_all_vendors(&self) -> Vec<VendorDefinition> {
        if !self.config_dir.is_dir() {
            self.report_missing_config_dir();
            return essential_vendor_definitions();
        }

        match self.read_vendor_files() {
            Ok(vendors) if !vendors.is_empty() => convert(vendors),
            Ok(_) => {
                logger::warning("Vendor configuration is empty");
                logger::info("Using default vendor definitions");
                essential_vendor_definitions()
            }
            Err(e) => {
                logger::warning(&format!("Failed to load vendor configuration: {e}"));
                logger::info("Using default vendor definitions");
                essential_vendor_definitions()
            }
        }
    }

    /// Read every `*.json` in the vendor directory, in sorted file-name order
    /// so the listing is stable rather than whatever `read_dir` returns.
    ///
    /// One unreadable or malformed file does not take the whole catalog down
    /// with it: it is reported and skipped. The old single-file layout had no
    /// such choice -- a stray comma cost the user every vendor at once, which
    /// is a large part of why the split is worth having.
    fn read_vendor_files(&self) -> Result<crate::collections::OrderedMap<VendorJsonEntry>, String> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&self.config_dir)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            })
            .collect();
        paths.sort();

        let mut vendors = crate::collections::OrderedMap::new();
        for path in paths {
            match Self::read_one_vendor_file(&path) {
                Ok(file) => {
                    for (key, entry) in file {
                        vendors.insert(key, entry);
                    }
                }
                Err(e) => logger::warning(&format!("Skipping {}: {e}", path.display())),
            }
        }
        Ok(vendors)
    }

    /// A vendor file is `{"<Key>": { ...definition... }}` -- the key inside
    /// rather than taken from the file name, so renaming a file cannot
    /// silently re-key a vendor and orphan its `naner.lock` pin.
    fn read_one_vendor_file(
        path: &Path,
    ) -> Result<crate::collections::OrderedMap<VendorJsonEntry>, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&strip_json_comments(&text)).map_err(|e| e.to_string())
    }

    /// The pre-split layout is not read any more. Saying only "not found"
    /// would leave a user staring at a `vendors.json` that is plainly right
    /// there, while naner quietly falls back to four essentials and drops the
    /// other eighteen.
    fn report_missing_config_dir(&self) {
        logger::warning(&format!(
            "Vendor configuration not found: {}",
            self.config_dir.display()
        ));
        if self.legacy_config_path.is_file() {
            logger::warning(&format!(
                "{} is the pre-split layout and is no longer read.",
                self.legacy_config_path.display()
            ));
            logger::info(
                "Each vendor now lives in its own file under config/vendors/. \
                 Update this installation to get them.",
            );
        }
        logger::info("Using default vendor definitions");
    }

    pub fn optional_vendors(&self) -> Vec<VendorDefinition> {
        self.load_vendors()
            .into_iter()
            .filter(|v| !v.required)
            .collect()
    }

    pub fn essential_vendors(&self) -> Vec<VendorDefinition> {
        self.load_vendors()
            .into_iter()
            .filter(|v| v.required)
            .collect()
    }

    /// `GetVendorByKey`: case-insensitive on key or display name.
    /// Searches *all* vendors, disabled included. Lookup by name is a
    /// different question from whether the vendor may be installed — a caller
    /// that resolves a disabled name needs to say "that one is switched off",
    /// not "no such vendor", which would send the user hunting for a typo.
    pub fn vendor_by_key(&self, key: &str) -> Option<VendorDefinition> {
        self.load_all_vendors()
            .into_iter()
            .find(|v| v.key.eq_ignore_ascii_case(key) || v.name.eq_ignore_ascii_case(key))
    }

    /// "Installed" = extract dir exists and is non-empty.
    pub fn is_vendor_installed(&self, vendor: &VendorDefinition) -> bool {
        let target = self.vendor_dir.join(&vendor.extract_dir);
        target.is_dir()
            && std::fs::read_dir(&target)
                .map(|mut d| d.next().is_some())
                .unwrap_or(false)
    }

    /// Read a vendor's `.vendor-version` file, if present (used by the
    /// additive `--porcelain` listing).
    pub fn vendor_version(&self, vendor: &VendorDefinition) -> Option<String> {
        let file = self
            .vendor_dir
            .join(&vendor.extract_dir)
            .join(super::VENDOR_VERSION_FILE);
        std::fs::read_to_string(file)
            .ok()
            .map(|s| s.trim().to_string())
    }
}

/// `ConvertToVendorDefinitions`.
fn convert(vendors: crate::collections::OrderedMap<VendorJsonEntry>) -> Vec<VendorDefinition> {
    let mut definitions = Vec::new();

    for (key, entry) in vendors {
        let mut def = VendorDefinition {
            key,
            name: entry.name,
            description: entry.description,
            extract_dir: entry.extract_dir,
            enabled: entry.enabled,
            required: entry.required,
            dependencies: entry.dependencies.unwrap_or_default(),
            install_type: entry.install_type,
            installer_args: entry.installer_args,
            path_priority: entry.path_priority,
            path_precedence: entry.path_precedence.unwrap_or_default(),
            environment_variables: entry.environment_variables.unwrap_or_default(),
            provides: entry.provides.unwrap_or_default(),
            binary_name: entry.binary_name,
            // B2 fixed: an optional checksum object flows to the verifier.
            checksum: entry.checksum.and_then(|c| {
                let value = c.value.unwrap_or_default();
                (!value.is_empty()).then(|| crate::checksum::ChecksumInfo {
                    algorithm: c.algorithm.unwrap_or_else(|| "SHA256".into()),
                    value,
                    required: c.required,
                })
            }),
            checksum_source: entry.checksum_source.and_then(convert_checksum_source),
            ..Default::default()
        };

        if let Some(source) = entry.release_source {
            def.source_type = parse_source_type(&source.source_type);
            match def.source_type {
                VendorSourceType::GitHub => {
                    if let Some(repo) = &source.repo {
                        let parts: Vec<&str> = repo.split('/').collect();
                        if parts.len() == 2 {
                            def.github_owner = Some(parts[0].to_string());
                            def.github_repo = Some(parts[1].to_string());
                        }
                    }
                    // B1 fixed: glob patterns now match in the installer;
                    // asset_pattern_end stays a built-in-defaults mechanism.
                    def.asset_pattern = source.asset_pattern.clone();
                }
                VendorSourceType::WebScrape => {
                    def.web_scrape = Some(WebScrapeConfig {
                        url: source.url.clone().unwrap_or_default(),
                        pattern: source.pattern.clone().unwrap_or_default(),
                        base_url: base_url_of(source.url.as_deref()),
                    });
                }
                VendorSourceType::StaticUrl => {
                    def.static_url = source.url.clone();
                    def.file_name = source.file_name.clone();
                }
                VendorSourceType::Npm | VendorSourceType::Pip => {
                    def.package_name = source.package.clone();
                }
                // The three API types carry no extra config.
                _ => {}
            }

            if let Some(fallback) = source.fallback {
                def.fallback_url = fallback.url;
                def.fallback_version = fallback.version;
                def.fallback_file_name = fallback.file_name;
            }
        }

        definitions.push(def);
    }

    definitions
}

/// An entry missing the fields its `type` needs is dropped rather than
/// half-applied — a malformed checksum source must not look like a
/// successfully configured one.
fn convert_checksum_source(json: ChecksumSourceJson) -> Option<ChecksumSource> {
    match json.source_type.to_lowercase().as_str() {
        "sidecar" => Some(ChecksumSource::Sidecar {
            suffix: json.suffix.filter(|s| !s.is_empty())?,
        }),
        "scrape" => Some(ChecksumSource::Scrape {
            url: json.url.filter(|s| !s.is_empty())?,
            pattern: json.pattern.filter(|s| !s.is_empty())?,
        }),
        other => {
            logger::warning(&format!("Unknown checksumSource type: {other}"));
            None
        }
    }
}

/// `ParseSourceType`: unknown types silently parse as `static`
/// (MIGRATION_ANALYSIS §3 drift note).
fn parse_source_type(source_type: &str) -> VendorSourceType {
    match source_type.to_lowercase().as_str() {
        "github" => VendorSourceType::GitHub,
        "web-scrape" => VendorSourceType::WebScrape,
        "static" => VendorSourceType::StaticUrl,
        "golang-api" => VendorSourceType::GolangApi,
        "nodejs-api" => VendorSourceType::NodeJsApi,
        "dotnet-api" => VendorSourceType::DotNetApi,
        "npm" => VendorSourceType::Npm,
        "pip" => VendorSourceType::Pip,
        _ => VendorSourceType::StaticUrl,
    }
}

/// `GetBaseUrl`: `scheme://host` + the path up to (and including) the last
/// `/`; the raw string when parsing fails.
fn base_url_of(url: Option<&str>) -> String {
    let Some(url) = url else {
        return String::new();
    };
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let after_scheme = &url[scheme_end + 3..];
    let (host, path) = match after_scheme.find('/') {
        Some(idx) => (&after_scheme[..idx], &after_scheme[idx..]),
        None => (after_scheme, "/"),
    };
    // Drop query/fragment from the path portion before trimming to the last '/'.
    let path = path.split(['?', '#']).next().unwrap_or("/");
    let last_slash = path.rfind('/').unwrap_or(0);
    format!(
        "{}{}",
        &url[..scheme_end + 3],
        format_args!("{host}{}", &path[..last_slash + 1])
    )
}

#[cfg(test)]
/// Write a `{"vendors": {...}}` fixture out as the per-vendor files the
/// loader now reads, one file per key. Lets the fixtures below stay in the
/// shape they document -- the whole catalog in one readable literal --
/// while exercising the real on-disk layout.
fn write_vendor_files(config_dir: &Path, json: &str) {
    let vendors_dir = config_dir.join("vendors");
    std::fs::create_dir_all(&vendors_dir).unwrap();

    let Ok(root) = serde_json::from_str::<serde_json::Value>(&strip_json_comments(json)) else {
        // A deliberately-malformed fixture is the point of some tests: put it
        // on disk as-is so the loader meets the same garbage it used to.
        std::fs::write(vendors_dir.join("malformed.json"), json).unwrap();
        return;
    };
    let Some(vendors) = root.get("vendors").and_then(|v| v.as_object()) else {
        // A fixture with no `vendors` key at all still has to produce a
        // directory: that is the "present but empty" case.
        return;
    };
    for (key, definition) in vendors {
        let mut one = serde_json::Map::new();
        one.insert(key.clone(), definition.clone());
        std::fs::write(
            vendors_dir.join(format!("{key}.json")),
            serde_json::to_string_pretty(&serde_json::Value::Object(one)).unwrap(),
        )
        .unwrap();
    }
}

#[cfg(test)]
mod enabled_tests {
    use super::*;

    fn loader_for(json: &str) -> (tempfile::TempDir, VendorConfigurationLoader) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        super::write_vendor_files(&config, json);
        let loader = VendorConfigurationLoader::new(tmp.path());
        (tmp, loader)
    }

    const MIXED: &str = r#"{
        "vendors": {
            "On":      { "name": "On",      "extractDir": "on",      "enabled": true,  "required": false },
            "Off":     { "name": "Off",     "extractDir": "off",     "enabled": false, "required": false },
            "Omitted": { "name": "Omitted", "extractDir": "omitted", "required": false }
        }
    }"#;

    #[test]
    fn disabled_vendors_are_not_offered_or_installed() {
        let (_tmp, loader) = loader_for(MIXED);
        let names: Vec<String> = loader.load_vendors().into_iter().map(|v| v.key).collect();
        assert!(names.contains(&"On".to_string()));
        assert!(
            !names.contains(&"Off".to_string()),
            "enabled:false must be honoured"
        );
    }

    /// The dangerous half of honouring the flag: `#[serde(default)]` gives a
    /// bool `false`, so an entry that omits `enabled` would have been switched
    /// off by the very change that started reading it.
    #[test]
    fn omitting_enabled_means_enabled() {
        let (_tmp, loader) = loader_for(MIXED);
        let names: Vec<String> = loader.load_vendors().into_iter().map(|v| v.key).collect();
        assert!(
            names.contains(&"Omitted".to_string()),
            "a vendor that does not mention `enabled` must stay enabled"
        );
    }

    #[test]
    fn load_all_vendors_still_sees_the_disabled_ones() {
        let (_tmp, loader) = loader_for(MIXED);
        assert_eq!(loader.load_all_vendors().len(), 3);
        assert_eq!(loader.load_vendors().len(), 2);
    }

    /// Regression: a vendor whose release asset is a bare `.exe` and that
    /// sets its own `installerArgs` overrides `build_installer_arguments`'s
    /// smart per-installer-technology fallback entirely (`archives.rs`) --
    /// if none of those args reference `%TARGETDIR%`/`$TARGETDIR`, the
    /// installer runs silently and successfully, but installs to wherever
    /// its own default is (Program Files, AppData, ...) instead of its
    /// vendor directory. `naner install` still reports success and pins a
    /// version; nothing lands where every other vendor expects it, and the
    /// only visible symptom is an empty `vendor/<name>/` folder. Caught for
    /// real: Obsidian shipped with `["/S"]`, no target-dir switch at all.
    #[test]
    fn every_installer_arg_exe_vendor_redirects_into_its_target_dir() {
        // The catalog `build.rs` assembles from dist-assets/config/vendors/*.json,
        // which is both what ships and what gets compiled into the binary --
        // so this checks the real shipped set, not a fixture of it.
        const SHIPPED: &str = include_str!(concat!(env!("OUT_DIR"), "/vendors_catalog.json"));
        let (_tmp, loader) = loader_for(SHIPPED);
        let vendors = loader.load_all_vendors();
        assert!(!vendors.is_empty());

        for vendor in &vendors {
            let Some(args) = &vendor.installer_args else {
                continue;
            };
            if vendor.key.eq_ignore_ascii_case("Rust") {
                // Redirected via RUSTUP_HOME/CARGO_HOME env vars in
                // archives::run_exe_installer, not a command-line switch --
                // the one documented exception.
                continue;
            }
            assert!(
                args.iter()
                    .any(|a| a.contains("%TARGETDIR%") || a.contains("$TARGETDIR")),
                "{}'s installerArgs {args:?} never redirect into its own vendor \
                 directory -- it will install to the installer's own default \
                 location instead of vendor/{}",
                vendor.name,
                vendor.extract_dir,
            );
        }
    }

    /// `naner suggest` answers with the first vendor whose `provides` lists
    /// the queried name, so two shipped vendors claiming the same executable
    /// would make the answer depend on file-name sort order.
    #[test]
    fn shipped_provides_entries_are_unique_across_vendors() {
        const SHIPPED: &str = include_str!(concat!(env!("OUT_DIR"), "/vendors_catalog.json"));
        let (_tmp, loader) = loader_for(SHIPPED);
        let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for vendor in loader.load_all_vendors() {
            for name in &vendor.provides {
                let normalized = name.to_lowercase();
                assert!(
                    !normalized.is_empty() && !normalized.contains('.'),
                    "{}'s provides entry {name:?} must be a bare lowercase \
                     executable name (no extension)",
                    vendor.key
                );
                if let Some(previous) = seen.insert(normalized, vendor.key.clone()) {
                    panic!(
                        "{} and {previous} both claim to provide {name:?}",
                        vendor.key
                    );
                }
            }
        }
    }

    /// The other way this change could have gone catastrophically wrong: the
    /// hardcoded fallback set never sets `enabled`, so a derived `Default` of
    /// `false` would have made a missing vendors.json install nothing at all.
    #[test]
    fn the_builtin_essential_set_survives_the_filter() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("config")).unwrap();
        let loader = VendorConfigurationLoader::new(tmp.path());

        let essentials = essential_vendor_definitions();
        assert!(!essentials.is_empty());
        assert!(
            essentials.iter().all(|v| v.enabled),
            "built-in defaults must be enabled or a missing vendors.json installs nothing"
        );
        assert_eq!(loader.load_vendors().len(), essentials.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loader_with(vendors_json: Option<&str>) -> (tempfile::TempDir, VendorConfigurationLoader) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        if let Some(json) = vendors_json {
            super::write_vendor_files(&config, json);
        }
        let loader = VendorConfigurationLoader::new(tmp.path());
        (tmp, loader)
    }

    const SAMPLE: &str = r#"{
        "$schema": "./vendors-schema.json",
        "version": "1.0.0",
        "vendors": {
            "PowerShell": {
                "name": "PowerShell",
                "description": "Cross-platform shell",
                "extractDir": "powershell",
                "enabled": true,
                "required": true,
                "dependencies": ["SevenZip"],
                "releaseSource": {
                    "type": "github",
                    "repo": "PowerShell/PowerShell",
                    "assetPattern": "*win-x64.zip",
                    "fallback": {
                        "version": "7.4.6",
                        "url": "https://example.com/PowerShell-7.4.6-win-x64.zip",
                        "fileName": "PowerShell-7.4.6-win-x64.zip"
                    }
                }
            },
            "SevenZip": {
                "name": "7-Zip",
                "description": "Archiver",
                "extractDir": "7zip",
                "enabled": true,
                "required": true,
                "releaseSource": {
                    "type": "web-scrape",
                    "url": "https://www.7-zip.org/download.html",
                    "pattern": "href=\"([^\"]*7z(\\d+)-x64\\.msi)\""
                }
            },
            "Ruby": {
                "name": "Ruby",
                "description": "Ruby language",
                "extractDir": "ruby",
                "enabled": true,
                "required": false,
                "releaseSource": { "type": "mystery-type", "url": "https://x.example/r.7z", "fileName": "r.7z" }
            }
        }
    }"#;

    #[test]
    fn loads_and_converts_the_real_shape() {
        let (_tmp, loader) = loader_with(Some(SAMPLE));
        let vendors = loader.load_vendors();
        assert_eq!(vendors.len(), 3);

        let by_key = |key: &str| {
            vendors
                .iter()
                .find(|v| v.key == key)
                .unwrap_or_else(|| panic!("{key} missing from the loaded set"))
        };

        let ps = by_key("PowerShell");
        assert_eq!(ps.key, "PowerShell");
        assert_eq!(ps.source_type, VendorSourceType::GitHub);
        assert_eq!(ps.github_owner.as_deref(), Some("PowerShell"));
        assert_eq!(ps.github_repo.as_deref(), Some("PowerShell"));
        // The glob string is kept verbatim (the installer glob-matches it
        // since the B1 fix); no pattern-end is ever set from JSON.
        assert_eq!(ps.asset_pattern.as_deref(), Some("*win-x64.zip"));
        assert!(ps.asset_pattern_end.is_none());
        // No checksum object in the JSON → none on the definition.
        assert!(ps.checksum.is_none());
        assert_eq!(ps.dependencies, vec!["SevenZip"]);
        assert_eq!(
            ps.fallback_file_name.as_deref(),
            Some("PowerShell-7.4.6-win-x64.zip")
        );

        let sz = by_key("SevenZip");
        assert_eq!(sz.source_type, VendorSourceType::WebScrape);
        let scrape = sz.web_scrape.as_ref().unwrap();
        assert_eq!(scrape.base_url, "https://www.7-zip.org/");

        // Unknown source type silently parses as static (drift preserved).
        let ruby = by_key("Ruby");
        assert_eq!(ruby.source_type, VendorSourceType::StaticUrl);
        assert_eq!(ruby.static_url.as_deref(), Some("https://x.example/r.7z"));
    }

    #[test]
    fn b2_checksum_object_is_wired_through() {
        let json = r#"{ "vendors": { "Tool": {
            "name": "Tool", "extractDir": "tool", "enabled": true, "required": false,
            "checksum": { "algorithm": "SHA512", "value": "AB CD", "required": true }
        } } }"#;
        let (_tmp, loader) = loader_with(Some(json));
        let tool = &loader.load_vendors()[0];
        let checksum = tool.checksum.as_ref().expect("checksum wired");
        assert_eq!(checksum.algorithm, "SHA512");
        assert_eq!(checksum.value, "AB CD");
        assert!(checksum.required);

        // Algorithm defaults to SHA256; an empty value means no checksum.
        let json = r#"{ "vendors": { "Tool": {
            "name": "Tool", "extractDir": "tool", "enabled": true, "required": false,
            "checksum": { "value": "ff00" }
        } } }"#;
        let (_tmp, loader) = loader_with(Some(json));
        let tool = &loader.load_vendors()[0];
        assert_eq!(tool.checksum.as_ref().unwrap().algorithm, "SHA256");

        let json = r#"{ "vendors": { "Tool": {
            "name": "Tool", "extractDir": "tool", "enabled": true, "required": false,
            "checksum": { "algorithm": "SHA256" }
        } } }"#;
        let (_tmp, loader) = loader_with(Some(json));
        assert!(loader.load_vendors()[0].checksum.is_none());
    }

    #[test]
    fn npm_source_type_carries_its_package() {
        let json = r#"{ "vendors": { "Tool": {
            "name": "Tool", "extractDir": "tool", "enabled": true, "required": false,
            "releaseSource": { "type": "npm", "package": "@scope/tool" }
        } } }"#;
        let (_tmp, loader) = loader_with(Some(json));
        let tool = &loader.load_vendors()[0];
        assert_eq!(tool.source_type, VendorSourceType::Npm);
        assert_eq!(tool.package_name.as_deref(), Some("@scope/tool"));
    }

    #[test]
    fn provides_is_wired_through_and_defaults_empty() {
        let json = r#"{ "vendors": {
            "Tool":  { "name": "Tool",  "extractDir": "tool",  "enabled": true, "required": false,
                       "provides": ["tool", "toolctl"] },
            "Other": { "name": "Other", "extractDir": "other", "enabled": true, "required": false }
        } }"#;
        let (_tmp, loader) = loader_with(Some(json));
        let vendors = loader.load_vendors();
        let tool = vendors.iter().find(|v| v.key == "Tool").unwrap();
        assert_eq!(tool.provides, vec!["tool", "toolctl"]);
        let other = vendors.iter().find(|v| v.key == "Other").unwrap();
        assert!(other.provides.is_empty(), "omitted provides must mean none");
    }

    #[test]
    fn missing_or_invalid_file_falls_back_to_defaults() {
        let (_tmp, loader) = loader_with(None);
        let vendors = loader.load_vendors();
        assert_eq!(vendors.len(), 6);
        assert_eq!(vendors[0].name, "7-Zip"); // 7-Zip first: extraction dependency

        let (_tmp, loader) = loader_with(Some("{ not json"));
        assert_eq!(loader.load_vendors().len(), 6);

        let (_tmp, loader) = loader_with(Some(r#"{ "vendors": {} }"#));
        assert_eq!(loader.load_vendors().len(), 6);
    }

    #[test]
    fn vendor_lookup_is_case_insensitive_on_key_and_name() {
        let (_tmp, loader) = loader_with(Some(SAMPLE));
        assert!(loader.vendor_by_key("powershell").is_some());
        assert!(loader.vendor_by_key("7-ZIP").is_some()); // by display name
        assert!(loader.vendor_by_key("nope").is_none());
    }

    #[test]
    fn installed_means_nonempty_extract_dir() {
        let (tmp, loader) = loader_with(Some(SAMPLE));
        let ruby = loader.vendor_by_key("Ruby").unwrap();
        assert!(!loader.is_vendor_installed(&ruby));

        std::fs::create_dir_all(tmp.path().join("vendor/ruby")).unwrap();
        assert!(!loader.is_vendor_installed(&ruby)); // empty dir

        std::fs::write(tmp.path().join("vendor/ruby/ruby.exe"), "x").unwrap();
        assert!(loader.is_vendor_installed(&ruby));
    }

    #[test]
    fn base_url_extraction() {
        assert_eq!(
            base_url_of(Some("https://www.7-zip.org/download.html")),
            "https://www.7-zip.org/"
        );
        assert_eq!(
            base_url_of(Some("https://repo.msys2.org/distrib/x86_64/")),
            "https://repo.msys2.org/distrib/x86_64/"
        );
        assert_eq!(
            base_url_of(Some("https://host.example")),
            "https://host.example/"
        );
        assert_eq!(base_url_of(Some("not a url")), "not a url");
        assert_eq!(base_url_of(None), "");
    }
}
