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

use super::{VendorDefinition, VendorSourceType, WebScrapeConfig, essential_vendor_definitions};
use crate::config::strip_json_comments;
use crate::{constants, logger};

// ---- vendors.json wire models (`VendorJsonModels`) ----

#[derive(Debug, Deserialize)]
struct VendorsJsonRoot {
    vendors: Option<crate::collections::OrderedMap<VendorJsonEntry>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct VendorJsonEntry {
    name: String,
    description: String,
    #[serde(rename = "extractDir", alias = "extractdir")]
    extract_dir: String,
    enabled: bool,
    required: bool,
    dependencies: Option<Vec<String>>,
    #[serde(rename = "releaseSource", alias = "releasesource")]
    release_source: Option<ReleaseSourceJson>,
    #[serde(rename = "installType", alias = "installtype")]
    install_type: Option<String>,
    #[serde(rename = "installerArgs", alias = "installerargs")]
    installer_args: Option<Vec<String>>,
    checksum: Option<ChecksumJson>,
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
    config_path: PathBuf,
    vendor_dir: PathBuf,
}

impl VendorConfigurationLoader {
    pub fn new(naner_root: &Path) -> Self {
        Self {
            config_path: naner_root
                .join(constants::directory_names::CONFIG)
                .join(constants::VENDORS_CONFIG_FILE_NAME),
            vendor_dir: naner_root.join(constants::directory_names::VENDOR),
        }
    }

    /// `LoadVendors`: file → parse → convert, with the default-essential
    /// fallback and the same warnings.
    pub fn load_vendors(&self) -> Vec<VendorDefinition> {
        if !self.config_path.is_file() {
            logger::warning(&format!(
                "Vendor configuration not found: {}",
                self.config_path.display()
            ));
            logger::info("Using default vendor definitions");
            return essential_vendor_definitions();
        }

        let parsed: Result<VendorsJsonRoot, String> = std::fs::read_to_string(&self.config_path)
            .map_err(|e| e.to_string())
            .and_then(|json| {
                serde_json::from_str(&strip_json_comments(&json)).map_err(|e| e.to_string())
            });

        match parsed {
            Ok(root) => match root.vendors {
                Some(vendors) if !vendors.is_empty() => convert(vendors),
                _ => {
                    logger::warning("Vendor configuration is empty");
                    logger::info("Using default vendor definitions");
                    essential_vendor_definitions()
                }
            },
            Err(e) => {
                logger::warning(&format!("Failed to load vendor configuration: {e}"));
                logger::info("Using default vendor definitions");
                essential_vendor_definitions()
            }
        }
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
    pub fn vendor_by_key(&self, key: &str) -> Option<VendorDefinition> {
        self.load_vendors()
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
            // B2 fixed: an optional checksum object flows to the verifier.
            checksum: entry.checksum.and_then(|c| {
                let value = c.value.unwrap_or_default();
                (!value.is_empty()).then(|| crate::checksum::ChecksumInfo {
                    algorithm: c.algorithm.unwrap_or_else(|| "SHA256".into()),
                    value,
                    required: c.required,
                })
            }),
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
mod tests {
    use super::*;

    fn loader_with(vendors_json: Option<&str>) -> (tempfile::TempDir, VendorConfigurationLoader) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("config")).unwrap();
        if let Some(json) = vendors_json {
            std::fs::write(tmp.path().join("config/vendors.json"), json).unwrap();
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

        let ps = &vendors[0];
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

        let sz = &vendors[1];
        assert_eq!(sz.source_type, VendorSourceType::WebScrape);
        let scrape = sz.web_scrape.as_ref().unwrap();
        assert_eq!(scrape.base_url, "https://www.7-zip.org/");

        // Unknown source type silently parses as static (drift preserved).
        let ruby = &vendors[2];
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
    fn missing_or_invalid_file_falls_back_to_defaults() {
        let (_tmp, loader) = loader_with(None);
        let vendors = loader.load_vendors();
        assert_eq!(vendors.len(), 4);
        assert_eq!(vendors[0].name, "7-Zip"); // 7-Zip first: extraction dependency

        let (_tmp, loader) = loader_with(Some("{ not json"));
        assert_eq!(loader.load_vendors().len(), 4);

        let (_tmp, loader) = loader_with(Some(r#"{ "vendors": {} }"#));
        assert_eq!(loader.load_vendors().len(), 4);
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
