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
    ChecksumSource, VENDOR_VERSION_FILE, VendorDefinition, VendorSourceType,
    WindowsTerminalConfigurator, is_windows_terminal,
};
use crate::http::Http;
use crate::lockfile::{LockedVendor, NanerLockfile};
use crate::{archives, checksum, logger};

/// `VendorDownloadInfo`.
#[derive(Clone, Debug)]
pub struct VendorDownloadInfo {
    pub url: String,
    pub file_name: String,
    pub version: Option<String>,
    /// Digest published by the upstream source for *this* resolved artifact,
    /// when the source exposes one. Additive: the C# model had no such field
    /// because it never verified anything. A vendor's static `checksum` in
    /// vendors.json takes precedence over this (see `resolved_checksum`).
    pub checksum: Option<checksum::ChecksumInfo>,
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
    ///
    /// Honours `naner.lock`: a pinned vendor installs the exact artifact
    /// recorded there rather than re-resolving to whatever upstream now calls
    /// latest. That is what makes an environment reproducible, and it is the
    /// only verification MSYS2 and the GitHub-sourced vendors get, since their
    /// distributors publish no digest (ADR-0002).
    pub fn install_vendor(&self, vendor_name: &str) -> bool {
        self.install_vendor_inner(vendor_name, true, true)
    }

    fn install_vendor_inner(
        &self,
        vendor_name: &str,
        skip_if_exists: bool,
        use_lock: bool,
    ) -> bool {
        let Some(vendor) = self.find(vendor_name) else {
            logger::failure(&format!("Unknown vendor: {vendor_name}"));
            return false;
        };

        let target_dir = self.vendor_dir.join(&vendor.extract_dir);

        if skip_if_exists && dir_is_nonempty(&target_dir) {
            logger::info(&format!("Skipping {} (already installed)", vendor.name));
            return true;
        }

        let pinned = use_lock
            .then(|| NanerLockfile::load(&self.naner_root))
            .flatten()
            .and_then(|lock| lock.get(&vendor.key).cloned());

        let Some(mut info) = (match &pinned {
            Some(locked) => {
                logger::status(&format!(
                    "Using pinned {} ({})",
                    vendor.name, locked.version
                ));
                Some(locked_download_info(locked))
            }
            None => {
                logger::status(&format!("Fetching latest {}...", vendor.name));
                self.fetch_download_info(vendor)
            }
        }) else {
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

        if !self.verify_checksum(&download_path, vendor, &info) {
            logger::failure(&format!(
                "  Checksum verification failed for {}",
                vendor.name
            ));
            return false;
        }

        logger::status(&format!("  Installing {}...", vendor.name));
        let staging_root = self.vendor_dir.join(".staging");
        let staging_target = staging_root.join(&vendor.extract_dir);
        let _ = std::fs::remove_dir_all(&staging_target);
        let _ = std::fs::create_dir_all(&staging_target);

        let seven_zip = self.vendor_dir.join("7zip").join("7z.exe");
        if !archives::extract_archive(
            &download_path,
            &staging_target,
            &vendor.name,
            Some(&seven_zip),
            vendor.installer_args.as_deref(),
        ) {
            logger::warning(&format!("Failed to install {}, skipping...", vendor.name));
            let _ = std::fs::remove_dir_all(&staging_target);
            return false;
        }

        // Move the staged tree into place. Windows Terminal merges over its
        // existing install to preserve `settings/`; everything else is a clean
        // swap. Either way a failure here is a failed install — reporting
        // success over a half-populated directory is how a broken vendor gets
        // recorded as installed and skipped by every later run.
        let placed = if is_windows_terminal(&vendor.name) {
            merge_over(&staging_target, &target_dir)
        } else {
            swap_into_place(&staging_target, &target_dir)
        };
        if let Err(e) = placed {
            logger::failure(&format!("    Failed to install {}: {e}", vendor.name));
            let _ = std::fs::remove_dir_all(&staging_target);
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

        self.record_lock_entry(vendor, &info, &download_path, pinned.is_some());

        logger::success(&format!("  Installed {}", vendor.name));
        true
    }

    /// Pin what was just installed, so the next install reproduces it.
    ///
    /// Best-effort by design: a lock that cannot be written must not fail an
    /// otherwise-successful install, but it is reported rather than swallowed —
    /// silently not pinning is exactly the failure this file exists to prevent.
    fn record_lock_entry(
        &self,
        vendor: &VendorDefinition,
        info: &VendorDownloadInfo,
        download_path: &Path,
        already_pinned: bool,
    ) {
        // Re-installing from an existing pin changes nothing; don't rewrite the
        // file (and don't re-hash a 400 MB archive) for a no-op.
        if already_pinned {
            return;
        }

        let sha256 = match checksum::compute(download_path, "SHA256") {
            Ok(hex) => Some(hex.to_lowercase()),
            Err(e) => {
                logger::debug(&format!("Could not hash for {LOCKFILE_LABEL}: {e}"), false);
                None
            }
        };

        let mut lock = NanerLockfile::load_or_default(&self.naner_root);
        lock.record(
            &vendor.key,
            LockedVendor {
                version: info.version.clone().unwrap_or_default(),
                url: info.url.clone(),
                sha256,
            },
        );
        match lock.save(&self.naner_root) {
            Ok(()) => logger::debug(
                &format!("  Pinned {} in {LOCKFILE_LABEL}", vendor.name),
                false,
            ),
            Err(e) => logger::warning(&format!("    Could not update {LOCKFILE_LABEL}: {e}")),
        }
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

        // An update is an explicit request for a newer artifact, so it ignores
        // the pin and rewrites it. Honouring the lock here would make
        // `update-vendors` a no-op on every pinned vendor.
        self.install_vendor_inner(vendor_name, false, false)
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
            Ok(Some(mut info)) => {
                if info.checksum.is_none() {
                    info.checksum = self.fetch_checksum_source(vendor, &info);
                }
                Some(info)
            }
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
            // The releases API exposes no digest for older assets; a vendor
            // can still pin one via `checksum` in vendors.json.
            checksum: None,
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
        let regex = crate::regex_shim::compile_ci(&scrape.pattern)?;
        let Some(captures) = regex.captures(&html) else {
            return Ok(None);
        };
        let relative = captures.get(1).unwrap_or_default();
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
            checksum: None,
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
        // nodejs.org publishes a per-release SHASUMS256.txt; a resolution that
        // succeeds without it still installs (unverified) rather than failing
        // the whole vendor.
        let checksum = match self
            .http
            .get_text(&format!("https://nodejs.org/dist/{version}/SHASUMS256.txt"))
        {
            Ok((status, body)) if (200..300).contains(&status) => {
                upstream_sha256(sha256_from_sums_file(&body, &file_name).as_deref())
            }
            _ => {
                logger::debug("    No SHASUMS256.txt available for this release", false);
                None
            }
        };

        Ok(Some(VendorDownloadInfo {
            url: format!("https://nodejs.org/dist/{version}/{file_name}"),
            file_name,
            version: Some(version),
            checksum,
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
            /// go.dev publishes the digest inline — no extra request needed.
            sha256: Option<String>,
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
        let Some(file) = file else { return Ok(None) };
        let Some(file_name) = file.filename.clone() else {
            return Ok(None);
        };

        Ok(Some(VendorDownloadInfo {
            url: format!("https://go.dev/dl/{file_name}"),
            file_name,
            version: Some(version.clone()),
            checksum: upstream_sha256(file.sha256.as_deref()),
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
            #[serde(rename = "releases.json")]
            releases_json: Option<String>,
        }
        // Channel manifest: carries the real download URL and a SHA-512 per
        // file, so following it beats hand-building the URL from the version.
        #[derive(Deserialize)]
        struct ChannelReleases {
            releases: Option<Vec<ChannelRelease>>,
        }
        #[derive(Deserialize)]
        struct ChannelRelease {
            sdk: Option<Sdk>,
        }
        #[derive(Deserialize)]
        struct Sdk {
            version: Option<String>,
            files: Option<Vec<SdkFile>>,
        }
        #[derive(Deserialize)]
        struct SdkFile {
            name: Option<String>,
            rid: Option<String>,
            url: Option<String>,
            hash: Option<String>,
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
        let Some(channel) = lts else { return Ok(None) };
        let Some(version) = channel.latest_sdk else {
            return Ok(None);
        };

        let file_name = format!("dotnet-sdk-{version}-win-x64.zip");
        let built_url =
            format!("https://builds.dotnet.microsoft.com/dotnet/Sdk/{version}/{file_name}");

        // Prefer the channel manifest's own URL + hash; fall back to the
        // hand-built URL (unverified) if the manifest is unavailable.
        if let Some(releases_url) = channel.releases_json.as_deref()
            && let Ok((status, body)) = self.http.get_text(releases_url)
            && (200..300).contains(&status)
            && let Ok(channel_releases) = serde_json::from_str::<ChannelReleases>(&body)
            && let Some(sdk) = channel_releases
                .releases
                .unwrap_or_default()
                .into_iter()
                .filter_map(|r| r.sdk)
                .find(|s| s.version.as_deref() == Some(version.as_str()))
            && let Some(file) = sdk.files.unwrap_or_default().into_iter().find(|f| {
                f.rid.as_deref() == Some("win-x64")
                    && f.name.as_deref().is_some_and(|n| n.ends_with(".zip"))
            })
        {
            return Ok(Some(VendorDownloadInfo {
                url: file.url.unwrap_or(built_url),
                file_name,
                version: Some(version),
                // 128 hex chars — the .NET manifest publishes SHA-512.
                checksum: upstream_digest(file.hash.as_deref()),
            }));
        }

        logger::debug(
            "    No .NET channel manifest available; URL unverified",
            false,
        );
        Ok(Some(VendorDownloadInfo {
            url: built_url,
            file_name,
            version: Some(version),
            checksum: None,
        }))
    }

    /// Fetch the digest described by a vendor's `checksumSource`, if any.
    /// A source that is configured but unreachable logs and yields `None`
    /// rather than failing resolution — the download still happens, just
    /// unverified, which is the pre-existing behavior for every vendor.
    fn fetch_checksum_source(
        &self,
        vendor: &VendorDefinition,
        info: &VendorDownloadInfo,
    ) -> Option<checksum::ChecksumInfo> {
        let source = vendor.checksum_source.as_ref()?;
        let url = match source {
            ChecksumSource::Sidecar { suffix } => format!("{}{suffix}", info.url),
            ChecksumSource::Scrape { url, .. } => url.clone(),
        };

        let body = match self.http.get_text(&url) {
            Ok((status, body)) if (200..300).contains(&status) => body,
            Ok((status, _)) => {
                logger::warning(&format!("    Checksum source {url} returned {status}"));
                return None;
            }
            Err(e) => {
                logger::warning(&format!("    Checksum source {url} unreachable: {e}"));
                return None;
            }
        };

        let found = match source {
            ChecksumSource::Sidecar { .. } => digest_from_sidecar(&body, &info.file_name),
            ChecksumSource::Scrape { pattern, .. } => {
                // `{FILE}` lets one pattern serve any resolved artifact.
                let pattern =
                    pattern.replace("{FILE}", &crate::regex_shim::escape(&info.file_name));
                crate::regex_shim::compile_ci(&pattern)
                    .ok()
                    .and_then(|regex| regex.captures(&body)?.get(1).map(str::to_string))
            }
        };

        match upstream_digest(found.as_deref()) {
            Some(digest) => Some(digest),
            None => {
                logger::warning(&format!("    No digest for {} in {url}", info.file_name));
                None
            }
        }
    }

    /// `VerifyChecksum` (`VendorInstallerBase`) with the same log lines.
    fn verify_checksum(
        &self,
        file_path: &Path,
        vendor: &VendorDefinition,
        download: &VendorDownloadInfo,
    ) -> bool {
        let Some(info) = resolved_checksum(vendor, download) else {
            logger::debug("    No checksum provided, skipping verification", false);
            return true;
        };
        let info = &info;

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

/// Label used in log lines about the lockfile.
const LOCKFILE_LABEL: &str = crate::lockfile::LOCKFILE_NAME;

/// Build download info from a lockfile pin.
///
/// The pinned digest becomes the download's checksum and is `required`: the
/// whole point of a pin is that a different artifact at that URL is a failure,
/// not a silent upgrade. A pin without a digest still fixes the URL and version.
fn locked_download_info(locked: &LockedVendor) -> VendorDownloadInfo {
    VendorDownloadInfo {
        url: locked.url.clone(),
        file_name: file_name_of(&locked.url),
        version: Some(locked.version.clone()),
        checksum: locked
            .sha256
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(|value| checksum::ChecksumInfo {
                algorithm: "SHA256".into(),
                value: value.to_string(),
                required: true,
            }),
    }
}

/// Which digest to verify a download against.
///
/// A `checksum` pinned in vendors.json wins: an operator who has pinned an
/// artifact is asserting something stronger than "whatever the distributor
/// currently serves", and silently preferring the upstream value would let a
/// compromised manifest overrule the pin. Otherwise use whatever the resolver
/// discovered upstream.
fn resolved_checksum(
    vendor: &VendorDefinition,
    download: &VendorDownloadInfo,
) -> Option<checksum::ChecksumInfo> {
    vendor
        .checksum
        .clone()
        .filter(|c| !c.value.is_empty())
        .or_else(|| download.checksum.clone())
        .filter(|c| !c.value.is_empty())
}

/// Wrap an upstream-published hex digest as a *required* checksum.
///
/// Required is the right default here, unlike the optional `checksum` object
/// in vendors.json: the value came from the distributor's own manifest for
/// this exact artifact, so a mismatch means the bytes are not what the source
/// says they are. The algorithm is inferred from the hex length, which keeps
/// callers from having to state it.
fn upstream_digest(value: Option<&str>) -> Option<checksum::ChecksumInfo> {
    let value = value.map(str::trim).filter(|v| !v.is_empty())?;
    if !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let algorithm = match value.len() {
        64 => "SHA256",
        96 => "SHA384",
        128 => "SHA512",
        _ => return None,
    };
    Some(checksum::ChecksumInfo {
        algorithm: algorithm.into(),
        value: value.to_string(),
        required: true,
    })
}

/// [`upstream_digest`] restricted to SHA-256, for sources that document that
/// algorithm specifically (go.dev, nodejs.org).
fn upstream_sha256(value: Option<&str>) -> Option<checksum::ChecksumInfo> {
    upstream_digest(value).filter(|c| c.algorithm == "SHA256")
}

/// Pull the digest for `file_name` out of a `sha256sum`-style listing
/// (`<hex>  <name>` per line), as published at
/// `nodejs.org/dist/<version>/SHASUMS256.txt`.
fn sha256_from_sums_file(body: &str, file_name: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        // The name column is `*name` for binary mode, plain otherwise.
        let name = parts.next()?.trim_start_matches('*');
        (name == file_name).then(|| hash.to_string())
    })
}

/// Digest from a sidecar file whose body is either a bare hex string or a
/// `sha256sum`-style line (`static.rust-lang.org` publishes the bare form).
fn digest_from_sidecar(body: &str, file_name: &str) -> Option<String> {
    let token = body.split_whitespace().next()?;
    if token.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(token.to_string());
    }
    sha256_from_sums_file(body, file_name)
}

fn fetch_static(vendor: &VendorDefinition) -> Option<VendorDownloadInfo> {
    let (Some(url), Some(file_name)) = (&vendor.static_url, &vendor.file_name) else {
        return None;
    };
    Some(VendorDownloadInfo {
        url: url.clone(),
        file_name: file_name.clone(),
        version: Some(version_from_file_name(file_name)),
        checksum: None,
    })
}

fn fallback_info(vendor: &VendorDefinition, fallback_url: &str) -> VendorDownloadInfo {
    VendorDownloadInfo {
        checksum: None,
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
    let regex = crate::regex_shim::compile(r"(\d+\.?\d*\.?\d*\.?\d*)").unwrap();
    regex
        .captures(file_name)
        .and_then(|c| c.get(1))
        .map(|s| s.to_string())
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
/// Replace `target` with `staging`, keeping the old tree until the new one is
/// in place.
///
/// The rename is the fast path and the only one that is actually atomic. It
/// works here because `target` is moved aside first rather than pre-created:
/// Windows' `MoveFileExW` cannot replace an existing directory, so creating
/// the destination beforehand — as this used to — guaranteed the rename failed
/// on the one platform naner ships to, silently demoting every install to a
/// recursive copy and losing symlinks with it.
///
/// The previous tree is restored if placement fails, so a failed install
/// leaves the working vendor it was replacing.
fn swap_into_place(staging: &Path, target: &Path) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let backup = with_suffix(target, ".old");
    let _ = std::fs::remove_dir_all(&backup); // stale one from an interrupted run
    let had_previous = target.exists();
    if had_previous {
        std::fs::rename(target, &backup)?;
    }

    let placed = std::fs::rename(staging, target).or_else(|rename_err| {
        // Cross-device: staging and target normally share `vendor/`, so this
        // is rare. Copy, then drop staging.
        copy_tree(staging, target)
            .inspect_err(|_| {
                let _ = std::fs::remove_dir_all(target);
            })
            .map_err(|copy_err| {
                std::io::Error::other(format!(
                    "rename failed ({rename_err}); copy failed ({copy_err})"
                ))
            })?;
        let _ = std::fs::remove_dir_all(staging);
        Ok(())
    });

    match placed {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&backup);
            Ok(())
        }
        Err(e) => {
            if had_previous {
                let _ = std::fs::rename(&backup, target);
            }
            Err(e)
        }
    }
}

/// Overlay `staging` onto `target`, leaving files that only exist in `target`.
///
/// Windows Terminal only: an update must not lose `settings/`. This cannot be
/// a swap, so a failure part-way leaves a mixed tree — the caller reports the
/// failure rather than pretending otherwise, which is the best available
/// outcome while preserving settings is the requirement.
fn merge_over(staging: &Path, target: &Path) -> std::io::Result<()> {
    copy_tree(staging, target)?;
    let _ = std::fs::remove_dir_all(staging);
    Ok(())
}

/// Recursive directory copy, overwriting existing files.
///
/// Symlinks are followed and materialised as regular files — the fallback path
/// only, and only across devices, so the symlink-heavy trees (MSYS2) take the
/// rename instead.
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    // A missing source is an error, not a no-op. The version this replaced
    // returned Ok here, so a vanished staging tree copied nothing and reported
    // success — the same silent-failure shape this whole path is being fixed
    // for.
    if !src.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("source is not a directory: {}", src.display()),
        ));
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// `foo` -> `foo.old`, preserving the parent directory.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Network-dependent checks against the real distributor endpoints. Excluded
/// from CI (`#[ignore]`); run with `cargo test -- --ignored --nocapture` when
/// touching a resolver, since these are the only tests that catch an upstream
/// manifest changing shape.
#[cfg(test)]
mod live_digest_tests {
    use super::*;
    use crate::http::UreqHttp;

    fn installer<'a>(http: &'a UreqHttp) -> UnifiedVendorInstaller<'a> {
        UnifiedVendorInstaller::new(Path::new("/nonexistent"), Vec::new(), http)
    }

    fn assert_sha(info: &VendorDownloadInfo, bits: usize, label: &str) {
        let c = info
            .checksum
            .as_ref()
            .unwrap_or_else(|| panic!("{label}: no digest resolved"));
        assert_eq!(c.value.len(), bits / 4, "{label}: unexpected digest length");
        assert!(c.required, "{label}: upstream digest must be required");
        println!("{label}: {} {} = {}", info.file_name, c.algorithm, c.value);
    }

    #[test]
    #[ignore = "hits the network"]
    fn go_resolves_a_sha256() {
        let http = UreqHttp::new();
        let info = installer(&http).fetch_golang().unwrap().unwrap();
        assert_sha(&info, 256, "go");
    }

    #[test]
    #[ignore = "hits the network"]
    fn nodejs_resolves_a_sha256() {
        let http = UreqHttp::new();
        let info = installer(&http).fetch_nodejs().unwrap().unwrap();
        assert_sha(&info, 256, "nodejs");
    }

    #[test]
    #[ignore = "hits the network"]
    fn dotnet_resolves_a_sha512_and_an_authoritative_url() {
        let http = UreqHttp::new();
        let info = installer(&http).fetch_dotnet().unwrap().unwrap();
        assert_sha(&info, 512, "dotnet");
        assert!(info.url.starts_with("https://"), "url: {}", info.url);
    }

    #[test]
    #[ignore = "hits the network"]
    fn rustup_sidecar_resolves() {
        let http = UreqHttp::new();
        let vendor = VendorDefinition {
            name: "Rust".into(),
            checksum_source: Some(ChecksumSource::Sidecar {
                suffix: ".sha256".into(),
            }),
            ..Default::default()
        };
        let info = VendorDownloadInfo {
            url: "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"
                .into(),
            file_name: "rustup-init.exe".into(),
            version: None,
            checksum: None,
        };
        let resolved = installer(&http).fetch_checksum_source(&vendor, &info);
        assert_sha(
            &VendorDownloadInfo {
                checksum: resolved,
                ..info.clone()
            },
            256,
            "rustup",
        );
    }

    #[test]
    #[ignore = "hits the network"]
    fn miniconda_scrape_resolves() {
        let http = UreqHttp::new();
        let vendor = VendorDefinition {
            name: "Miniconda".into(),
            checksum_source: Some(ChecksumSource::Scrape {
                url: "https://repo.anaconda.com/miniconda/".into(),
                pattern: "{FILE}</a></td>[^<]*<td[^>]*>[^<]*</td>[^<]*<td[^>]*>[^<]*</td>[^<]*<td>([0-9a-f]{64})".into(),
            }),
            ..Default::default()
        };
        let info = VendorDownloadInfo {
            url: "https://repo.anaconda.com/miniconda/Miniconda3-latest-Windows-x86_64.exe".into(),
            file_name: "Miniconda3-latest-Windows-x86_64.exe".into(),
            version: None,
            checksum: None,
        };
        let resolved = installer(&http).fetch_checksum_source(&vendor, &info);
        assert_sha(
            &VendorDownloadInfo {
                checksum: resolved,
                ..info.clone()
            },
            256,
            "miniconda",
        );
    }
}

#[cfg(test)]
mod digest_tests {
    use super::*;

    /// The real shape of a repo.anaconda.com/miniconda/ row, including the
    /// newline+indent runs between tags and the duplicate filename in href.
    const MINICONDA_LISTING: &str = r#"    <tr>
      <th>Last Modified</th>
      <th>SHA256</th>
    </tr>
    <tr>
      <td><a href="Miniconda3-py39_4.9.2-Windows-x86_64.exe">Miniconda3-py39_4.9.2-Windows-x86_64.exe</a></td>
      <td class="s">70.7M</td>
      <td>2021-01-12 20:03:36</td>
      <td>1111111111111111111111111111111111111111111111111111111111111111</td>
    </tr>
    <tr>
      <td><a href="Miniconda3-latest-Windows-x86_64.exe">Miniconda3-latest-Windows-x86_64.exe</a></td>
      <td class="s">124.7M</td>
      <td>2026-07-29 18:22:05</td>
      <td>4441b50816f866f4e6e774e90f90a71bde756f06c94144407a6d93677c539e46</td>
    </tr>
"#;

    /// The pattern shipped in dist-assets/config/vendors.json.
    const MINICONDA_PATTERN: &str =
        "{FILE}</a></td>[^<]*<td[^>]*>[^<]*</td>[^<]*<td[^>]*>[^<]*</td>[^<]*<td>([0-9a-f]{64})";

    /// Guards the shipped config against the engine: `rusty_regx` is POSIX-ERE
    /// and leftmost-*longest*, so this proves `{64}` intervals work and that
    /// the bounded `[^<]*` runs keep the match inside the requested row rather
    /// than sliding to a neighbouring one.
    #[test]
    fn miniconda_scrape_pattern_selects_the_right_row() {
        let file = "Miniconda3-latest-Windows-x86_64.exe";
        let pattern = MINICONDA_PATTERN.replace("{FILE}", &crate::regex_shim::escape(file));
        let regex = crate::regex_shim::compile_ci(&pattern).expect("pattern compiles");
        let captured = regex
            .captures(MINICONDA_LISTING)
            .and_then(|c| c.get(1))
            .expect("hash captured");
        assert_eq!(
            captured,
            "4441b50816f866f4e6e774e90f90a71bde756f06c94144407a6d93677c539e46"
        );
    }

    #[test]
    fn miniconda_pattern_picks_the_named_file_not_the_first_row() {
        let file = "Miniconda3-py39_4.9.2-Windows-x86_64.exe";
        let pattern = MINICONDA_PATTERN.replace("{FILE}", &crate::regex_shim::escape(file));
        let regex = crate::regex_shim::compile_ci(&pattern).unwrap();
        assert_eq!(
            regex.captures(MINICONDA_LISTING).and_then(|c| c.get(1)),
            Some("1111111111111111111111111111111111111111111111111111111111111111")
        );
    }

    #[test]
    fn algorithm_is_inferred_from_digest_length() {
        let sha256 = "a".repeat(64);
        let sha512 = "b".repeat(128);
        assert_eq!(upstream_digest(Some(&sha256)).unwrap().algorithm, "SHA256");
        assert_eq!(upstream_digest(Some(&sha512)).unwrap().algorithm, "SHA512");
        assert_eq!(
            upstream_digest(Some(&"c".repeat(96))).unwrap().algorithm,
            "SHA384"
        );
    }

    #[test]
    fn upstream_digests_are_required_so_a_mismatch_blocks_install() {
        assert!(upstream_digest(Some(&"a".repeat(64))).unwrap().required);
    }

    #[test]
    fn malformed_digests_are_rejected_rather_than_trusted() {
        assert!(upstream_digest(None).is_none());
        assert!(upstream_digest(Some("")).is_none());
        assert!(upstream_digest(Some("  ")).is_none());
        // Wrong length for every supported algorithm.
        assert!(upstream_digest(Some(&"a".repeat(40))).is_none());
        // Non-hex, e.g. an error page captured by a bad pattern.
        assert!(upstream_digest(Some(&"z".repeat(64))).is_none());
        // SHA-512 offered where only SHA-256 is documented.
        assert!(upstream_sha256(Some(&"a".repeat(128))).is_none());
    }

    #[test]
    fn shasums_file_is_parsed_by_exact_file_name() {
        let body = "d3bd72755141ed32bbcd841228ee81897c8a98d50dfa7dae2179399a0a7c90f8  node-v26.7.0-win-x64.zip
aaaa72755141ed32bbcd841228ee81897c8a98d50dfa7dae2179399a0a7c90f8  node-v26.7.0-win-x86.zip
";
        assert_eq!(
            sha256_from_sums_file(body, "node-v26.7.0-win-x64.zip").as_deref(),
            Some("d3bd72755141ed32bbcd841228ee81897c8a98d50dfa7dae2179399a0a7c90f8")
        );
        // A near-miss name must not fall through to another line's digest.
        assert_eq!(sha256_from_sums_file(body, "node-v26.7.0-win.zip"), None);
    }

    #[test]
    fn sidecar_accepts_bare_hex_and_sums_form() {
        // static.rust-lang.org serves the bare form.
        assert_eq!(
            digest_from_sidecar(
                "86478e53f769379d7f0ebfa7c9aa97cb76ca92233f79aa2cc0dbee2efaac73c7\n",
                "rustup-init.exe"
            )
            .as_deref(),
            Some("86478e53f769379d7f0ebfa7c9aa97cb76ca92233f79aa2cc0dbee2efaac73c7")
        );
        assert_eq!(
            digest_from_sidecar(
                "86478e53f769379d7f0ebfa7c9aa97cb76ca92233f79aa2cc0dbee2efaac73c7  rustup-init.exe",
                "rustup-init.exe"
            )
            .as_deref(),
            Some("86478e53f769379d7f0ebfa7c9aa97cb76ca92233f79aa2cc0dbee2efaac73c7")
        );
    }

    #[test]
    fn a_pinned_checksum_outranks_the_upstream_one() {
        let pinned = checksum::ChecksumInfo {
            algorithm: "SHA256".into(),
            value: "a".repeat(64),
            required: true,
        };
        let upstream = upstream_digest(Some(&"b".repeat(64)));
        let vendor = VendorDefinition {
            checksum: Some(pinned.clone()),
            ..Default::default()
        };
        let download = VendorDownloadInfo {
            url: String::new(),
            file_name: String::new(),
            version: None,
            checksum: upstream.clone(),
        };
        assert_eq!(
            resolved_checksum(&vendor, &download).unwrap().value,
            pinned.value
        );

        // With no pin, the upstream digest is what gets verified.
        let unpinned = VendorDefinition::default();
        assert_eq!(
            resolved_checksum(&unpinned, &download).unwrap().value,
            upstream.unwrap().value
        );

        // Neither present: nothing to verify.
        let empty = VendorDownloadInfo {
            url: String::new(),
            file_name: String::new(),
            version: None,
            checksum: None,
        };
        assert!(resolved_checksum(&unpinned, &empty).is_none());
    }
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

    /// End-to-end proof that a resolver-supplied digest is enforced: same
    /// vendor and same bytes, only the sidecar digest differs.
    fn install_with_sidecar_digest(sidecar_body: &str) -> (tempfile::TempDir, bool) {
        let root = tempfile::tempdir().unwrap();
        let payload = zip_bytes("pwsh.exe", b"fake");
        let mut vendor = github_vendor();
        vendor.checksum_source = Some(ChecksumSource::Sidecar {
            suffix: ".sha256".into(),
        });

        let mut http = StubHttp::default();
        http.text.insert(
            "https://api.github.com/repos/PowerShell/PowerShell/releases/latest".into(),
            (200, RELEASE_JSON.into()),
        );
        http.text.insert(
            "https://gh.example/PowerShell-7.5.0-win-x64.zip.sha256".into(),
            (200, sidecar_body.into()),
        );
        http.files.insert(
            "https://gh.example/PowerShell-7.5.0-win-x64.zip".into(),
            payload,
        );

        let installed = {
            let installer = UnifiedVendorInstaller::new(root.path(), vec![vendor], &http);
            installer.install_vendor("PowerShell")
        };
        (root, installed)
    }

    fn sha256_of_payload() -> String {
        let bytes = zip_bytes("pwsh.exe", b"fake");
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &bytes).unwrap();
        crate::checksum::compute(tmp.path(), "SHA256").unwrap()
    }

    #[test]
    fn matching_upstream_digest_allows_the_install() {
        let (root, installed) = install_with_sidecar_digest(&sha256_of_payload());
        assert!(installed);
        assert!(root.path().join("vendor/powershell/pwsh.exe").is_file());
    }

    #[test]
    fn mismatched_upstream_digest_blocks_the_install() {
        let (root, installed) = install_with_sidecar_digest(&"a".repeat(64));
        assert!(
            !installed,
            "install must fail when the upstream digest does not match"
        );
        // Nothing may be left behind for a later run to treat as installed.
        assert!(!root.path().join("vendor/powershell/pwsh.exe").exists());
    }

    /// An unreachable checksum source must not become a silent hard failure
    /// for every vendor that configures one — it degrades to the pre-existing
    /// unverified install, loudly.
    #[test]
    fn unreachable_checksum_source_degrades_to_unverified() {
        let root = tempfile::tempdir().unwrap();
        let mut vendor = github_vendor();
        vendor.checksum_source = Some(ChecksumSource::Sidecar {
            suffix: ".sha256".into(),
        });
        let mut http = StubHttp::default();
        http.text.insert(
            "https://api.github.com/repos/PowerShell/PowerShell/releases/latest".into(),
            (200, RELEASE_JSON.into()),
        );
        // No route for the .sha256 URL at all.
        http.files.insert(
            "https://gh.example/PowerShell-7.5.0-win-x64.zip".into(),
            zip_bytes("pwsh.exe", b"fake"),
        );

        let installer = UnifiedVendorInstaller::new(root.path(), vec![vendor], &http);
        assert!(installer.install_vendor("PowerShell"));
    }

    // ---- placement / swap failure handling (#14) ----

    fn tree(root: &Path, files: &[(&str, &[u8])]) {
        for (rel, content) in files {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
    }

    #[test]
    fn swap_replaces_the_previous_tree_entirely() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging/pwsh");
        let target = tmp.path().join("vendor/pwsh");
        tree(&staging, &[("new.exe", b"new"), ("sub/a.txt", b"a")]);
        tree(&target, &[("stale.exe", b"old")]);

        swap_into_place(&staging, &target).unwrap();

        assert!(target.join("new.exe").is_file());
        assert!(target.join("sub/a.txt").is_file());
        // A swap is a replacement: nothing from the old tree survives.
        assert!(!target.join("stale.exe").exists());
        assert!(!staging.exists(), "staging consumed by the rename");
        assert!(
            !tmp.path().join("vendor/pwsh.old").exists(),
            "backup cleaned up on success"
        );
    }

    #[test]
    fn swap_into_a_fresh_location_needs_no_previous_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging/go");
        let target = tmp.path().join("vendor/go");
        tree(&staging, &[("go.exe", b"go")]);

        swap_into_place(&staging, &target).unwrap();
        assert!(target.join("go.exe").is_file());
    }

    /// The bug this issue is about: a failed placement used to be discarded, so
    /// `.vendor-version` was written and "Installed" logged over a directory
    /// that never received the new tree.
    #[test]
    fn a_failed_placement_is_reported_and_leaves_the_previous_install_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("vendor/pwsh");
        tree(&target, &[("working.exe", b"the version that works")]);

        // Staging does not exist, so both the rename and the copy fail.
        let staging = tmp.path().join("staging/pwsh");
        let err = swap_into_place(&staging, &target).unwrap_err();
        assert!(
            err.to_string().contains("rename failed"),
            "error should name both attempts: {err}"
        );

        // The previously working install is restored, not left deleted.
        assert_eq!(
            std::fs::read(target.join("working.exe")).unwrap(),
            b"the version that works"
        );
        assert!(!tmp.path().join("vendor/pwsh.old").exists());
    }

    #[test]
    fn a_stale_backup_from_an_interrupted_run_does_not_block_the_swap() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging/pwsh");
        let target = tmp.path().join("vendor/pwsh");
        tree(&staging, &[("new.exe", b"new")]);
        tree(&target, &[("old.exe", b"old")]);
        tree(&tmp.path().join("vendor/pwsh.old"), &[("junk", b"junk")]);

        swap_into_place(&staging, &target).unwrap();
        assert!(target.join("new.exe").is_file());
        assert!(!tmp.path().join("vendor/pwsh.old").exists());
    }

    /// Windows Terminal is the one vendor that must not be swapped — an update
    /// extracts over-top so `settings/` survives.
    #[test]
    fn merge_preserves_files_the_new_tree_does_not_carry() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging/wt");
        let target = tmp.path().join("vendor/wt");
        tree(&staging, &[("wt.exe", b"v2")]);
        tree(
            &target,
            &[("wt.exe", b"v1"), ("settings/settings.json", b"{mine}")],
        );

        merge_over(&staging, &target).unwrap();

        assert_eq!(std::fs::read(target.join("wt.exe")).unwrap(), b"v2");
        assert_eq!(
            std::fs::read(target.join("settings/settings.json")).unwrap(),
            b"{mine}",
            "user settings must survive a Windows Terminal update"
        );
    }

    /// End-to-end: the installer must not report success, write
    /// `.vendor-version`, or pin the vendor when placement fails.
    ///
    /// The failure is injected on the Windows Terminal merge path, where a
    /// directory sitting where the new tree has a file makes the copy fail
    /// deterministically on every platform. Contrived, but it exercises the
    /// real branch — before this change, all three assertions below failed.
    #[test]
    fn install_fails_loudly_when_the_tree_cannot_be_placed() {
        let root = tempfile::tempdir().unwrap();
        let vendor = VendorDefinition {
            name: "Windows Terminal".into(),
            key: "WindowsTerminal".into(),
            extract_dir: "windows-terminal".into(),
            source_type: VendorSourceType::StaticUrl,
            static_url: Some("https://static.example/wt.zip".into()),
            file_name: Some("wt.zip".into()),
            ..Default::default()
        };

        let mut http = StubHttp::default();
        http.files.insert(
            "https://static.example/wt.zip".into(),
            zip_bytes("wt.exe", b"new"),
        );

        // `wt.exe` already exists as a directory, so copying the file over it
        // cannot succeed.
        std::fs::create_dir_all(root.path().join("vendor/windows-terminal/wt.exe")).unwrap();

        let installer = UnifiedVendorInstaller::new(root.path(), vec![vendor], &http);
        // skip_if_exists would short-circuit on the non-empty dir; go through
        // the update path so placement is actually attempted.
        let installed = installer.update_vendor("Windows Terminal");

        assert!(!installed, "a failed placement must not report success");
        assert!(
            !root
                .path()
                .join("vendor/windows-terminal/.vendor-version")
                .exists(),
            "no version marker for an install that did not happen"
        );
        assert!(
            NanerLockfile::load(root.path()).is_none(),
            "a failed install must not be pinned"
        );
    }

    // ---- lockfile pinning (#20) ----

    /// Only the *pinned* URL is routable, so a successful install proves
    /// resolution was skipped entirely rather than merely agreeing.
    fn stub_with_only_pinned_url(payload: &[u8]) -> StubHttp {
        let mut http = StubHttp::default();
        http.files.insert(
            "https://pinned.example/pwsh-7.4.0.zip".into(),
            payload.to_vec(),
        );
        http
    }

    fn write_pin(root: &Path, sha256: Option<&str>) {
        let mut lock = NanerLockfile::default();
        lock.record(
            "PowerShell",
            LockedVendor {
                version: "7.4.0".into(),
                url: "https://pinned.example/pwsh-7.4.0.zip".into(),
                sha256: sha256.map(str::to_string),
            },
        );
        lock.save(root).unwrap();
    }

    fn sha256_of(bytes: &[u8]) -> String {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), bytes).unwrap();
        crate::checksum::compute(tmp.path(), "SHA256")
            .unwrap()
            .to_lowercase()
    }

    #[test]
    fn a_pinned_vendor_installs_the_pin_without_resolving() {
        let root = tempfile::tempdir().unwrap();
        let payload = zip_bytes("pwsh.exe", b"pinned build");
        write_pin(root.path(), Some(&sha256_of(&payload)));

        // No route for the GitHub API or the "latest" asset — if the installer
        // resolved, it would fail.
        let http = stub_with_only_pinned_url(&payload);
        let installer = UnifiedVendorInstaller::new(root.path(), vec![github_vendor()], &http);
        assert!(installer.install_vendor("PowerShell"));

        let target = root.path().join("vendor/powershell");
        assert!(target.join("pwsh.exe").is_file());
        assert_eq!(
            std::fs::read_to_string(target.join(".vendor-version")).unwrap(),
            "7.4.0"
        );
    }

    #[test]
    fn a_pin_whose_digest_does_not_match_blocks_the_install() {
        let root = tempfile::tempdir().unwrap();
        let payload = zip_bytes("pwsh.exe", b"pinned build");
        // Same URL, different bytes than the pin attests.
        write_pin(root.path(), Some(&"a".repeat(64)));

        let http = stub_with_only_pinned_url(&payload);
        let installer = UnifiedVendorInstaller::new(root.path(), vec![github_vendor()], &http);
        assert!(!installer.install_vendor("PowerShell"));
        assert!(!root.path().join("vendor/powershell/pwsh.exe").exists());
    }

    /// The MSYS2 / GitHub-asset case: no upstream digest exists, so the first
    /// install cannot be verified — but it must still be pinned, because that
    /// is what makes every later install verifiable.
    #[test]
    fn an_unpinned_install_records_url_version_and_digest() {
        let root = tempfile::tempdir().unwrap();
        let payload = zip_bytes("pwsh.exe", b"fake");
        let mut http = StubHttp::default();
        http.text.insert(
            "https://api.github.com/repos/PowerShell/PowerShell/releases/latest".into(),
            (200, RELEASE_JSON.into()),
        );
        http.files.insert(
            "https://gh.example/PowerShell-7.5.0-win-x64.zip".into(),
            payload.clone(),
        );

        let installer = UnifiedVendorInstaller::new(root.path(), vec![github_vendor()], &http);
        assert!(installer.install_vendor("PowerShell"));

        let lock = NanerLockfile::load(root.path()).expect("lock written");
        let entry = lock.get("PowerShell").expect("vendor pinned");
        assert_eq!(entry.version, "v7.5.0");
        assert_eq!(entry.url, "https://gh.example/PowerShell-7.5.0-win-x64.zip");
        assert_eq!(entry.sha256.as_deref(), Some(sha256_of(&payload).as_str()));
    }

    /// A pin without a digest still fixes the artifact; it just cannot verify
    /// the bytes. It must not be treated as "no pin".
    #[test]
    fn a_digestless_pin_still_fixes_the_url_and_version() {
        let root = tempfile::tempdir().unwrap();
        let payload = zip_bytes("pwsh.exe", b"pinned build");
        write_pin(root.path(), None);

        let http = stub_with_only_pinned_url(&payload);
        let installer = UnifiedVendorInstaller::new(root.path(), vec![github_vendor()], &http);
        assert!(installer.install_vendor("PowerShell"));
        assert_eq!(
            std::fs::read_to_string(root.path().join("vendor/powershell/.vendor-version")).unwrap(),
            "7.4.0"
        );
    }

    /// `update-vendors` means "get me a newer one" — honouring the pin would
    /// make it a permanent no-op on every pinned vendor.
    #[test]
    fn update_ignores_the_pin_and_repins_what_it_resolved() {
        let root = tempfile::tempdir().unwrap();
        write_pin(root.path(), Some(&"a".repeat(64)));

        let mut http = StubHttp::default();
        http.text.insert(
            "https://api.github.com/repos/PowerShell/PowerShell/releases/latest".into(),
            (200, RELEASE_JSON.into()),
        );
        let payload = zip_bytes("pwsh.exe", b"newer");
        http.files.insert(
            "https://gh.example/PowerShell-7.5.0-win-x64.zip".into(),
            payload.clone(),
        );

        let installer = UnifiedVendorInstaller::new(root.path(), vec![github_vendor()], &http);
        assert!(installer.update_vendor("PowerShell"));

        let entry = NanerLockfile::load(root.path())
            .unwrap()
            .get("PowerShell")
            .cloned()
            .expect("re-pinned");
        assert_eq!(entry.version, "v7.5.0");
        assert_eq!(entry.sha256.as_deref(), Some(sha256_of(&payload).as_str()));
    }

    /// A vendors.json `checksum` is the operator's explicit assertion and still
    /// outranks the pin, so a compromised lock cannot overrule it.
    #[test]
    fn a_pinned_checksum_in_config_still_outranks_the_lock() {
        let locked = LockedVendor {
            version: "7.4.0".into(),
            url: "https://pinned.example/pwsh-7.4.0.zip".into(),
            sha256: Some("b".repeat(64)),
        };
        let download = locked_download_info(&locked);
        assert_eq!(download.checksum.as_ref().unwrap().value, "b".repeat(64));
        assert!(download.checksum.as_ref().unwrap().required);

        let vendor = VendorDefinition {
            checksum: Some(checksum::ChecksumInfo {
                algorithm: "SHA256".into(),
                value: "c".repeat(64),
                required: true,
            }),
            ..Default::default()
        };
        assert_eq!(
            resolved_checksum(&vendor, &download).unwrap().value,
            "c".repeat(64)
        );
    }

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
