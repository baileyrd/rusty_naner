//! Port of `UnifiedVendorInstaller` (+ `VendorInstallerBase`): resolve a
//! download URL per source type, two-level fallback cascade (fallback URL on
//! resolution failure AND a second attempt on download failure), download to
//! `vendor/.downloads/`, optional checksum, extract, flatten, post-install
//! (Windows Terminal only), write `.vendor-version`, delete `.downloads`.
//! Update = delete-and-reinstall, except Windows Terminal which extracts
//! over-top to preserve settings.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{
    VENDOR_VERSION_FILE, VendorDefinition, VendorSourceType, WindowsTerminalConfigurator,
    is_windows_terminal,
};
use crate::http::Http;
use crate::{archives, checksum, logger};

/// `VendorDownloadInfo`.
#[derive(Clone, Debug)]
pub struct VendorDownloadInfo {
    pub url: String,
    pub file_name: String,
    pub version: Option<String>,
}

pub struct UnifiedVendorInstaller<'a> {
    naner_root: PathBuf,
    vendor_dir: PathBuf,
    download_dir: PathBuf,
    http: &'a dyn Http,
    vendors: Vec<VendorDefinition>,
}

impl<'a> UnifiedVendorInstaller<'a> {
    pub fn new(naner_root: &Path, vendors: Vec<VendorDefinition>, http: &'a dyn Http) -> Self {
        let vendor_dir = naner_root.join("vendor");
        Self {
            naner_root: naner_root.to_path_buf(),
            download_dir: vendor_dir.join(".downloads"),
            vendor_dir,
            http,
            vendors,
        }
    }

    fn find(&self, vendor_name: &str) -> Option<&VendorDefinition> {
        self.vendors
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(vendor_name))
    }

    /// `InstallVendorAsync(name)` — skip when already installed.
    pub fn install_vendor(&self, vendor_name: &str) -> bool {
        self.install_vendor_inner(vendor_name, true)
    }

    fn install_vendor_inner(&self, vendor_name: &str, skip_if_exists: bool) -> bool {
        let Some(vendor) = self.find(vendor_name) else {
            logger::failure(&format!("Unknown vendor: {vendor_name}"));
            return false;
        };

        let target_dir = self.vendor_dir.join(&vendor.extract_dir);

        if skip_if_exists && dir_is_nonempty(&target_dir) {
            logger::info(&format!("Skipping {} (already installed)", vendor.name));
            return true;
        }

        logger::status(&format!("Fetching latest {}...", vendor.name));

        // Resolve (with resolution-level fallback).
        let Some(mut info) = self.fetch_download_info(vendor) else {
            logger::warning(&format!("Failed to fetch {}, skipping...", vendor.name));
            return false;
        };

        logger::info(&format!(
            "  Latest version: {}",
            info.version.as_deref().unwrap_or("Unknown")
        ));
        logger::status(&format!("  Downloading {}...", info.file_name));

        if std::fs::create_dir_all(&self.download_dir).is_err() {
            return false;
        }
        let mut download_path = self.download_dir.join(&info.file_name);

        // Download (with download-level fallback).
        if !self.http.download(&info.url, &download_path) {
            let Some(fallback_url) = vendor.fallback_url.as_deref() else {
                logger::warning(&format!("Failed to download {}, skipping...", vendor.name));
                return false;
            };
            logger::warning("  Primary download failed, trying fallback version...");
            info = fallback_info(vendor, fallback_url);
            download_path = self.download_dir.join(&info.file_name);
            logger::status(&format!("  Downloading {}...", info.file_name));
            if !self.http.download(&info.url, &download_path) {
                logger::warning(&format!("Failed to download {}, skipping...", vendor.name));
                return false;
            }
        }

        logger::success(&format!("  Downloaded {}", info.file_name));

        if !self.verify_checksum(&download_path, vendor) {
            logger::failure(&format!(
                "  Checksum verification failed for {}",
                vendor.name
            ));
            return false;
        }

        logger::status(&format!("  Installing {}...", vendor.name));
        let seven_zip = self.vendor_dir.join("7zip").join("7z.exe");
        if !archives::extract_archive(
            &download_path,
            &target_dir,
            &vendor.name,
            Some(&seven_zip),
            vendor.installer_args.as_deref(),
        ) {
            logger::warning(&format!("Failed to install {}, skipping...", vendor.name));
            return false;
        }

        // Post-install (Windows Terminal portable mode only).
        if is_windows_terminal(&vendor.name)
            && let Err(e) = WindowsTerminalConfigurator::new(&self.naner_root)
                .configure_portable_mode(&target_dir)
        {
            logger::warning(&format!("    Post-install configuration warning: {e}"));
        }

        if let Some(version) = &info.version
            && !version.is_empty()
            && std::fs::write(target_dir.join(VENDOR_VERSION_FILE), version).is_err()
        {
            logger::debug("Failed to save vendor version", false);
        }

        logger::success(&format!("  Installed {}", vendor.name));
        true
    }

    /// `UpdateVendorAsync`: delete-and-reinstall, except Windows Terminal
    /// (extract over-top, preserving settings/).
    pub fn update_vendor(&self, vendor_name: &str) -> bool {
        let Some(vendor) = self.find(vendor_name) else {
            logger::failure(&format!("Unknown vendor: {vendor_name}"));
            return false;
        };

        let target_dir = self.vendor_dir.join(&vendor.extract_dir);
        let is_wt = is_windows_terminal(&vendor.name);

        if target_dir.is_dir() {
            let current = read_version(&target_dir);
            let suffix = current
                .as_deref()
                .map(|v| format!(" (v{v})"))
                .unwrap_or_default();

            if is_wt {
                logger::info(&format!("Updating {}{suffix}...", vendor.name));
                logger::info("  Preserving settings configuration");
            } else {
                logger::info(&format!(
                    "Removing existing {} installation{suffix}...",
                    vendor.name
                ));
                if let Err(e) = std::fs::remove_dir_all(&target_dir) {
                    logger::warning(&format!("Failed to remove existing installation: {e}"));
                    return false;
                }
            }
        }

        self.install_vendor_inner(vendor_name, false)
    }

    /// `InstallAllVendorsAsync` (essential bootstrap path).
    pub fn install_all_vendors(&self) -> bool {
        logger::header("Downloading Vendor Dependencies");
        logger::newline();
        logger::status("This may take several minutes depending on your connection...");
        logger::newline();

        let _ = std::fs::create_dir_all(&self.download_dir);
        for vendor in dependency_order(&self.vendors) {
            self.install_vendor(&vendor.name);
            logger::newline();
        }
        self.cleanup_downloads();

        logger::newline();
        logger::success("Vendor setup completed!");
        // B4: the C# "MSYS2 packages will be installed on first launch" note
        // was removed — nothing ever installed them. If pacman bootstrap is
        // ever implemented, it belongs in a post-install hook here.
        true
    }

    /// `UpdateAllVendorsAsync`.
    pub fn update_all_vendors(&self) -> bool {
        logger::status("This may take several minutes depending on your connection...");
        logger::newline();

        let _ = std::fs::create_dir_all(&self.download_dir);
        for vendor in dependency_order(&self.vendors) {
            self.update_vendor(&vendor.name);
            logger::newline();
        }
        self.cleanup_downloads();
        true
    }

    pub fn cleanup_downloads(&self) {
        if self.download_dir.is_dir() && std::fs::remove_dir_all(&self.download_dir).is_err() {
            logger::debug("Failed to cleanup download directory", false);
        }
    }

    /// `FetchVendorDownloadInfoAsync`: per-source resolution, then the
    /// resolution-level fallback (both the returned-None and the threw-error
    /// paths use it — the cascade that hides bug B1 in production).
    fn fetch_download_info(&self, vendor: &VendorDefinition) -> Option<VendorDownloadInfo> {
        let resolved = match vendor.source_type {
            VendorSourceType::StaticUrl => Ok(fetch_static(vendor)),
            VendorSourceType::GitHub => self.fetch_github(vendor),
            VendorSourceType::WebScrape => self.fetch_web_scrape(vendor),
            VendorSourceType::NodeJsApi => self.fetch_nodejs(),
            VendorSourceType::GolangApi => self.fetch_golang(),
            VendorSourceType::DotNetApi => self.fetch_dotnet(),
        };

        match resolved {
            Ok(Some(info)) => Some(info),
            Ok(None) => {
                let fallback_url = vendor.fallback_url.as_deref()?;
                // Tier-3: fallback use is loud (stderr) — a silently pinned
                // old version is how B1 went unnoticed for years.
                logger::warning("  No matching release found, using fallback URL");
                Some(fallback_info(vendor, fallback_url))
            }
            Err(e) => {
                logger::warning(&format!("  Failed to fetch dynamically: {e}"));
                logger::warning("  Using fallback URL");
                vendor
                    .fallback_url
                    .as_deref()
                    .map(|url| fallback_info(vendor, url))
            }
        }
    }

    fn fetch_github(
        &self,
        vendor: &VendorDefinition,
    ) -> Result<Option<VendorDownloadInfo>, String> {
        let (Some(owner), Some(repo)) = (&vendor.github_owner, &vendor.github_repo) else {
            return Ok(None);
        };

        let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
        let (status, body) = self.http.get_text(&url)?;
        if !(200..300).contains(&status) {
            return Ok(None);
        }

        #[derive(Deserialize)]
        struct GitHubRelease {
            tag_name: Option<String>,
            assets: Option<Vec<GitHubAsset>>,
        }
        #[derive(Deserialize)]
        struct GitHubAsset {
            name: Option<String>,
            browser_download_url: Option<String>,
        }

        let release: GitHubRelease = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let assets = release.assets.unwrap_or_default();

        // B1 fixed: patterns containing `*`/`?` are matched as whole-name
        // globs (case-insensitive), so `*win-x64.zip` from vendors.json now
        // works. Wildcard-free patterns keep the C# substring semantics the
        // built-in defaults rely on (`win-x64.zip`, `Microsoft.WindowsTerminal_`).
        let matches = |name: &str, pattern: &Option<String>| match pattern {
            None => true,
            Some(p) if p.is_empty() => true,
            Some(p) if p.contains(['*', '?']) => glob_matches(name, p),
            Some(p) => name.to_lowercase().contains(&p.to_lowercase()),
        };
        let asset = assets.iter().find(|a| {
            a.name.as_deref().is_some_and(|name| {
                matches(name, &vendor.asset_pattern) && matches(name, &vendor.asset_pattern_end)
            })
        });

        let Some(asset) = asset else { return Ok(None) };
        let Some(download_url) = &asset.browser_download_url else {
            return Ok(None);
        };

        Ok(Some(VendorDownloadInfo {
            url: download_url.clone(),
            file_name: asset
                .name
                .clone()
                .unwrap_or_else(|| file_name_of(download_url)),
            version: release.tag_name,
        }))
    }

    fn fetch_web_scrape(
        &self,
        vendor: &VendorDefinition,
    ) -> Result<Option<VendorDownloadInfo>, String> {
        let Some(scrape) = &vendor.web_scrape else {
            return Ok(None);
        };

        let (status, html) = self.http.get_text(&scrape.url)?;
        if !(200..300).contains(&status) {
            return Ok(None);
        }

        // RegexOptions.IgnoreCase.
        let regex = regex_lite::Regex::new(&format!("(?i){}", scrape.pattern))
            .map_err(|e| e.to_string())?;
        let Some(captures) = regex.captures(&html) else {
            return Ok(None);
        };
        let relative = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
        let full_url = format!(
            "{}/{}",
            scrape.base_url.trim_end_matches('/'),
            relative.trim_start_matches('/')
        );
        let file_name = file_name_of(relative);

        Ok(Some(VendorDownloadInfo {
            url: full_url,
            version: Some(version_from_file_name(&file_name)),
            file_name,
        }))
    }

    fn fetch_nodejs(&self) -> Result<Option<VendorDownloadInfo>, String> {
        #[derive(Deserialize)]
        struct NodeRelease {
            version: Option<String>,
            files: Option<Vec<String>>,
        }

        let (status, body) = self.http.get_text("https://nodejs.org/dist/index.json")?;
        if !(200..300).contains(&status) {
            return Ok(None);
        }
        let releases: Vec<NodeRelease> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let latest = releases.iter().find(|r| {
            r.files
                .as_ref()
                .is_some_and(|f| f.iter().any(|x| x == "win-x64-zip"))
        });
        let Some(version) = latest.and_then(|r| r.version.clone()) else {
            return Ok(None);
        };

        let file_name = format!("node-{version}-win-x64.zip");
        Ok(Some(VendorDownloadInfo {
            url: format!("https://nodejs.org/dist/{version}/{file_name}"),
            file_name,
            version: Some(version),
        }))
    }

    fn fetch_golang(&self) -> Result<Option<VendorDownloadInfo>, String> {
        #[derive(Deserialize)]
        struct GoRelease {
            version: Option<String>,
            stable: bool,
            files: Option<Vec<GoFile>>,
        }
        #[derive(Deserialize)]
        struct GoFile {
            filename: Option<String>,
            os: Option<String>,
            arch: Option<String>,
            kind: Option<String>,
        }

        let (status, body) = self.http.get_text("https://go.dev/dl/?mode=json")?;
        if !(200..300).contains(&status) {
            return Ok(None);
        }
        let releases: Vec<GoRelease> = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let Some(latest) = releases.iter().find(|r| r.stable) else {
            return Ok(None);
        };
        let (Some(version), Some(files)) = (&latest.version, &latest.files) else {
            return Ok(None);
        };
        let file = files.iter().find(|f| {
            f.os.as_deref() == Some("windows")
                && f.arch.as_deref() == Some("amd64")
                && f.kind.as_deref() == Some("archive")
        });
        let Some(file_name) = file.and_then(|f| f.filename.clone()) else {
            return Ok(None);
        };

        Ok(Some(VendorDownloadInfo {
            url: format!("https://go.dev/dl/{file_name}"),
            file_name,
            version: Some(version.clone()),
        }))
    }

    fn fetch_dotnet(&self) -> Result<Option<VendorDownloadInfo>, String> {
        #[derive(Deserialize)]
        struct ReleasesIndex {
            #[serde(rename = "releases-index")]
            releases_index: Option<Vec<Channel>>,
        }
        #[derive(Deserialize)]
        struct Channel {
            #[serde(rename = "latest-sdk")]
            latest_sdk: Option<String>,
            #[serde(rename = "release-type")]
            release_type: Option<String>,
            #[serde(rename = "support-phase")]
            support_phase: Option<String>,
        }

        let (status, body) = self.http.get_text(
            "https://dotnetcli.azureedge.net/dotnet/release-metadata/releases-index.json",
        )?;
        if !(200..300).contains(&status) {
            return Ok(None);
        }
        let index: ReleasesIndex = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let lts = index
            .releases_index
            .unwrap_or_default()
            .into_iter()
            .find(|c| {
                c.release_type.as_deref() == Some("lts")
                    && c.support_phase.as_deref() == Some("active")
            });
        let Some(version) = lts.and_then(|c| c.latest_sdk) else {
            return Ok(None);
        };

        let file_name = format!("dotnet-sdk-{version}-win-x64.zip");
        Ok(Some(VendorDownloadInfo {
            url: format!("https://builds.dotnet.microsoft.com/dotnet/Sdk/{version}/{file_name}"),
            file_name,
            version: Some(version),
        }))
    }

    /// `VerifyChecksum` (`VendorInstallerBase`) with the same log lines.
    fn verify_checksum(&self, file_path: &Path, vendor: &VendorDefinition) -> bool {
        let Some(info) = &vendor.checksum else {
            logger::debug("    No checksum provided, skipping verification", false);
            return true;
        };
        if info.value.is_empty() {
            logger::debug("    No checksum provided, skipping verification", false);
            return true;
        }

        logger::status(&format!("    Verifying {} checksum...", info.algorithm));
        let result = checksum::verify(file_path, info);

        if result.skipped {
            logger::debug(
                &format!("    {}", result.message.as_deref().unwrap_or_default()),
                false,
            );
            return true;
        }
        if result.success {
            let actual = result.actual.as_deref().unwrap_or_default();
            logger::success(&format!(
                "    Checksum verified: {}...",
                &actual[..actual.len().min(16)]
            ));
            return true;
        }

        let expected = result.expected.as_deref().unwrap_or_default();
        let actual = result.actual.as_deref().unwrap_or_default();
        if info.required {
            logger::failure("    Checksum verification failed!");
            logger::failure(&format!("    Expected: {expected}"));
            logger::failure(&format!("    Actual:   {actual}"));
            false
        } else {
            logger::warning("    Checksum mismatch (verification not required)");
            logger::warning(&format!("    Expected: {expected}"));
            logger::warning(&format!("    Actual:   {actual}"));
            true
        }
    }
}

fn fetch_static(vendor: &VendorDefinition) -> Option<VendorDownloadInfo> {
    let (Some(url), Some(file_name)) = (&vendor.static_url, &vendor.file_name) else {
        return None;
    };
    Some(VendorDownloadInfo {
        url: url.clone(),
        file_name: file_name.clone(),
        version: Some(version_from_file_name(file_name)),
    })
}

fn fallback_info(vendor: &VendorDefinition, fallback_url: &str) -> VendorDownloadInfo {
    VendorDownloadInfo {
        url: fallback_url.to_string(),
        file_name: vendor
            .fallback_file_name
            .clone()
            .unwrap_or_else(|| file_name_of(fallback_url)),
        version: Some(
            vendor
                .fallback_version
                .clone()
                .unwrap_or_else(|| "fallback".to_string()),
        ),
    }
}

/// `Path.GetFileName` over a URL-ish string.
fn file_name_of(url: &str) -> String {
    url.rsplit(['/', '\\']).next().unwrap_or(url).to_string()
}

/// Whole-name, case-insensitive glob match (`*` = any run, `?` = any char).
/// Everything else is matched literally (fix for B1). Classic two-pointer
/// backtracking matcher — no regex, no escaping edge cases.
fn glob_matches(name: &str, pattern: &str) -> bool {
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let pat: Vec<char> = pattern.to_lowercase().chars().collect();
    let (mut n, mut p) = (0usize, 0usize);
    let (mut star, mut restart) = (None::<usize>, 0usize);

    while n < name.len() {
        if p < pat.len() && (pat[p] == '?' || pat[p] == name[n]) {
            n += 1;
            p += 1;
        } else if p < pat.len() && pat[p] == '*' {
            star = Some(p);
            restart = n;
            p += 1;
        } else if let Some(s) = star {
            p = s + 1;
            restart += 1;
            n = restart;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == '*' {
        p += 1;
    }
    p == pat.len()
}

/// `ExtractVersionFromFileName`: first `(\d+\.?\d*\.?\d*\.?\d*)` match, else
/// "latest".
fn version_from_file_name(file_name: &str) -> String {
    let regex = regex_lite::Regex::new(r"(\d+\.?\d*\.?\d*\.?\d*)").unwrap();
    regex
        .captures(file_name)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "latest".to_string())
}

/// Dependency-first ordering (fix for B3): repeatedly emit the first vendor
/// whose dependencies (matched by key or name, case-insensitive) are either
/// already emitted or not present in the set at all. Cycles degrade to the
/// original order for the remainder — never an infinite loop, never a panic.
fn dependency_order(vendors: &[VendorDefinition]) -> Vec<&VendorDefinition> {
    let matches_dep = |v: &VendorDefinition, dep: &str| {
        v.key.eq_ignore_ascii_case(dep) || v.name.eq_ignore_ascii_case(dep)
    };
    let mut ordered: Vec<&VendorDefinition> = Vec::new();
    let mut remaining: Vec<&VendorDefinition> = vendors.iter().collect();
    while !remaining.is_empty() {
        let ready = remaining.iter().position(|v| {
            v.dependencies.iter().all(|dep| {
                ordered.iter().any(|o| matches_dep(o, dep))
                    || !remaining.iter().any(|r| matches_dep(r, dep))
            })
        });
        match ready {
            Some(i) => ordered.push(remaining.remove(i)),
            None => {
                // Dependency cycle: fall back to the given order.
                ordered.append(&mut remaining);
            }
        }
    }
    ordered
}

fn dir_is_nonempty(dir: &Path) -> bool {
    dir.is_dir()
        && std::fs::read_dir(dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
}

fn read_version(target_dir: &Path) -> Option<String> {
    std::fs::read_to_string(target_dir.join(VENDOR_VERSION_FILE))
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::super::WebScrapeConfig;
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    /// Stub HTTP: canned text responses per URL; downloads write canned
    /// bytes (or fail when the URL is marked bad).
    #[derive(Default)]
    struct StubHttp {
        text: HashMap<String, (u16, String)>,
        files: HashMap<String, Vec<u8>>,
    }

    impl Http for StubHttp {
        fn get_text(&self, url: &str) -> Result<(u16, String), String> {
            self.text
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no route for {url}"))
        }
        fn download(&self, url: &str, output_path: &Path) -> bool {
            match self.files.get(url) {
                Some(bytes) => {
                    let mut f = std::fs::File::create(output_path).unwrap();
                    f.write_all(bytes).unwrap();
                    true
                }
                None => false,
            }
        }
    }

    fn zip_bytes(name: &str, content: &[u8]) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content).unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn github_vendor() -> VendorDefinition {
        VendorDefinition {
            name: "PowerShell".into(),
            key: "PowerShell".into(),
            extract_dir: "powershell".into(),
            source_type: VendorSourceType::GitHub,
            github_owner: Some("PowerShell".into()),
            github_repo: Some("PowerShell".into()),
            asset_pattern: Some("*win-x64.zip".into()), // glob — matches since the B1 fix
            fallback_url: Some("https://fallback.example/PowerShell-7.4.6-win-x64.zip".into()),
            fallback_version: Some("7.4.6".into()),
            fallback_file_name: Some("PowerShell-7.4.6-win-x64.zip".into()),
            ..Default::default()
        }
    }

    const RELEASE_JSON: &str = r#"{
        "tag_name": "v7.5.0",
        "assets": [
            { "name": "PowerShell-7.5.0-win-x64.zip",
              "browser_download_url": "https://gh.example/PowerShell-7.5.0-win-x64.zip" }
        ]
    }"#;

    #[test]
    fn b1_fixed_glob_pattern_matches_release_asset() {
        let root = tempfile::tempdir().unwrap();
        let mut http = StubHttp::default();
        http.text.insert(
            "https://api.github.com/repos/PowerShell/PowerShell/releases/latest".into(),
            (200, RELEASE_JSON.into()),
        );
        // Only the PRIMARY asset download is routable — proving the glob
        // `*win-x64.zip` resolved the release asset instead of falling back.
        http.files.insert(
            "https://gh.example/PowerShell-7.5.0-win-x64.zip".into(),
            zip_bytes("pwsh.exe", b"fake"),
        );

        let installer = UnifiedVendorInstaller::new(root.path(), vec![github_vendor()], &http);
        assert!(installer.install_vendor("PowerShell"));

        let target = root.path().join("vendor/powershell");
        assert!(target.join("pwsh.exe").is_file());
        assert_eq!(
            std::fs::read_to_string(target.join(".vendor-version")).unwrap(),
            "v7.5.0"
        );
    }

    #[test]
    fn glob_matcher_semantics() {
        assert!(glob_matches("PowerShell-7.5.0-win-x64.zip", "*win-x64.zip"));
        assert!(glob_matches("7z2602-x64.msi", "7z*-x64.msi"));
        assert!(glob_matches(
            "Microsoft.WindowsTerminal_1.21_x64.zip",
            "microsoft.windowsterminal_*_x64.zip"
        ));
        // Whole-name semantics: no implicit substring.
        assert!(!glob_matches(
            "PowerShell-7.5.0-win-x64.zip.sha256",
            "*win-x64.zip"
        ));
        assert!(!glob_matches("7z2602-arm64.msi", "7z*-x64.msi"));
        assert!(glob_matches("abc", "a?c"));
        assert!(!glob_matches("abbc", "a?c"));
    }

    #[test]
    fn b3_dependency_order_puts_dependencies_first() {
        let dep = |name: &str, deps: &[&str]| VendorDefinition {
            name: name.into(),
            key: name.into(),
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        // Node depends on SevenZip which comes later in the given order.
        let vendors = vec![
            dep("Node", &["SevenZip"]),
            dep("Terminal", &[]),
            dep("SevenZip", &[]),
        ];
        let order: Vec<&str> = dependency_order(&vendors)
            .iter()
            .map(|v| v.name.as_str())
            .collect();
        assert_eq!(order, vec!["Terminal", "SevenZip", "Node"]);

        // A cycle degrades to the given order instead of hanging.
        let cyclic = vec![dep("A", &["B"]), dep("B", &["A"])];
        assert_eq!(dependency_order(&cyclic).len(), 2);
    }

    #[test]
    fn substring_pattern_matches_and_uses_release_asset() {
        let root = tempfile::tempdir().unwrap();
        let mut vendor = github_vendor();
        vendor.asset_pattern = Some("win-x64.zip".into()); // substring — matches
        let mut http = StubHttp::default();
        http.text.insert(
            "https://api.github.com/repos/PowerShell/PowerShell/releases/latest".into(),
            (200, RELEASE_JSON.into()),
        );
        http.files.insert(
            "https://gh.example/PowerShell-7.5.0-win-x64.zip".into(),
            zip_bytes("pwsh.exe", b"fake"),
        );

        let installer = UnifiedVendorInstaller::new(root.path(), vec![vendor], &http);
        assert!(installer.install_vendor("PowerShell"));
        assert_eq!(
            std::fs::read_to_string(root.path().join("vendor/powershell/.vendor-version")).unwrap(),
            "v7.5.0"
        );
    }

    #[test]
    fn download_failure_falls_back_then_succeeds() {
        let root = tempfile::tempdir().unwrap();
        let mut vendor = github_vendor();
        vendor.asset_pattern = Some("win-x64.zip".into());
        let mut http = StubHttp::default();
        http.text.insert(
            "https://api.github.com/repos/PowerShell/PowerShell/releases/latest".into(),
            (200, RELEASE_JSON.into()),
        );
        // Primary asset URL not routable (download fails); fallback is.
        http.files.insert(
            "https://fallback.example/PowerShell-7.4.6-win-x64.zip".into(),
            zip_bytes("pwsh.exe", b"fake"),
        );

        let installer = UnifiedVendorInstaller::new(root.path(), vec![vendor], &http);
        assert!(installer.install_vendor("PowerShell"));
        assert_eq!(
            std::fs::read_to_string(root.path().join("vendor/powershell/.vendor-version")).unwrap(),
            "7.4.6"
        );
    }

    #[test]
    fn api_error_uses_fallback_and_no_fallback_fails() {
        let root = tempfile::tempdir().unwrap();
        let mut http = StubHttp::default();
        // GitHub API rate-limited.
        http.text.insert(
            "https://api.github.com/repos/PowerShell/PowerShell/releases/latest".into(),
            (403, "rate limited".into()),
        );
        http.files.insert(
            "https://fallback.example/PowerShell-7.4.6-win-x64.zip".into(),
            zip_bytes("pwsh.exe", b"fake"),
        );
        let installer = UnifiedVendorInstaller::new(root.path(), vec![github_vendor()], &http);
        assert!(installer.install_vendor("PowerShell"));

        // Same but with no fallback URL: resolution yields fallback=None → fail.
        let root2 = tempfile::tempdir().unwrap();
        let mut vendor = github_vendor();
        vendor.fallback_url = None;
        let installer = UnifiedVendorInstaller::new(root2.path(), vec![vendor], &http);
        assert!(!installer.install_vendor("PowerShell"));
    }

    #[test]
    fn already_installed_is_skipped_but_update_reinstalls() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("vendor/powershell");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("pwsh.exe"), "old").unwrap();
        std::fs::write(target.join(".vendor-version"), "7.0.0").unwrap();

        let mut http = StubHttp::default();
        http.text.insert(
            "https://api.github.com/repos/PowerShell/PowerShell/releases/latest".into(),
            (200, RELEASE_JSON.into()),
        );
        http.files.insert(
            "https://fallback.example/PowerShell-7.4.6-win-x64.zip".into(),
            zip_bytes("pwsh.exe", b"new"),
        );

        let installer = UnifiedVendorInstaller::new(root.path(), vec![github_vendor()], &http);

        // install: skip (dir non-empty), old content untouched.
        assert!(installer.install_vendor("PowerShell"));
        assert_eq!(
            std::fs::read_to_string(target.join("pwsh.exe")).unwrap(),
            "old"
        );

        // update: delete-and-reinstall.
        assert!(installer.update_vendor("PowerShell"));
        assert_eq!(
            std::fs::read_to_string(target.join("pwsh.exe")).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(target.join(".vendor-version")).unwrap(),
            "7.4.6"
        );
    }

    #[test]
    fn windows_terminal_update_preserves_settings() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("vendor/terminal");
        std::fs::create_dir_all(target.join("settings")).unwrap();
        std::fs::write(
            target.join("settings/settings.json"),
            "{\"user\":\"edited\"}",
        )
        .unwrap();
        std::fs::write(target.join("WindowsTerminal.exe"), "old").unwrap();

        let vendor = VendorDefinition {
            name: "Windows Terminal".into(),
            extract_dir: "terminal".into(),
            source_type: VendorSourceType::StaticUrl,
            static_url: Some("https://dl.example/wt.zip".into()),
            file_name: Some("wt.zip".into()),
            ..Default::default()
        };
        let mut http = StubHttp::default();
        http.files.insert(
            "https://dl.example/wt.zip".into(),
            zip_bytes("WindowsTerminal.exe", b"new"),
        );

        let installer = UnifiedVendorInstaller::new(root.path(), vec![vendor], &http);
        assert!(installer.update_vendor("Windows Terminal"));

        // Extracted over-top: exe updated, user's settings file NOT deleted...
        assert_eq!(
            std::fs::read_to_string(target.join("WindowsTerminal.exe")).unwrap(),
            "new"
        );
        assert!(target.join(".portable").is_file());
        assert!(target.join("settings/settings.json").is_file());
    }

    #[test]
    fn static_and_scrape_resolution() {
        let root = tempfile::tempdir().unwrap();
        let vendor = VendorDefinition {
            name: "MSYS2".into(),
            extract_dir: "msys64".into(),
            source_type: VendorSourceType::WebScrape,
            web_scrape: Some(WebScrapeConfig {
                url: "https://repo.msys2.org/distrib/x86_64/".into(),
                pattern: r#"href="(msys2-base-x86_64-(\d{8})\.tar\.xz)""#.into(),
                base_url: "https://repo.msys2.org/distrib/x86_64/".into(),
            }),
            ..Default::default()
        };
        let mut http = StubHttp::default();
        http.text.insert(
            "https://repo.msys2.org/distrib/x86_64/".into(),
            (
                200,
                r#"<a href="msys2-base-x86_64-20240727.tar.xz">latest</a>"#.into(),
            ),
        );
        let installer = UnifiedVendorInstaller::new(root.path(), vec![vendor.clone()], &http);
        let info = installer.fetch_download_info(&vendor).unwrap();
        assert_eq!(
            info.url,
            "https://repo.msys2.org/distrib/x86_64/msys2-base-x86_64-20240727.tar.xz"
        );
        assert_eq!(info.file_name, "msys2-base-x86_64-20240727.tar.xz");
        // Quirk preserved: the C# `(\d+\.?\d*\.?\d*\.?\d*)` regex matches the
        // FIRST digit run — the "2" in "msys2" — not the date.
        assert_eq!(info.version.as_deref(), Some("2"));

        assert_eq!(version_from_file_name("7z2408-x64.msi"), "7"); // "7" in "7z"
        assert_eq!(
            version_from_file_name("PowerShell-7.4.6-win-x64.zip"),
            "7.4.6"
        );
        assert_eq!(version_from_file_name("no-digits.zip"), "latest");
    }
}
