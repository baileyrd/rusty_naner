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

        // Only on the resolving path. A pinned install checked nothing about
        // what is current, and the "Using pinned" line above already carries
        // the version -- printing "Latest version" under it reads as though
        // naner confirmed the pin is up to date, which is the opposite of what
        // a pin means.
        if pinned.is_none() {
            logger::info(&format!(
                "  Latest version: {}",
                info.version.as_deref().unwrap_or("Unknown")
            ));
        }

        // npm-published vendors install through npm itself — it resolves the
        // dependency tree and verifies tarball integrity — instead of the
        // download/extract path below. `naner.lock` never pins these, so
        // `pinned` cannot be `Some` here.
        if matches!(
            vendor.source_type,
            VendorSourceType::Npm | VendorSourceType::Pip
        ) {
            return self.install_package_vendor(vendor, &info, &target_dir);
        }

        logger::status(&format!("  Downloading {}...", info.file_name));

        if std::fs::create_dir_all(&self.download_dir).is_err() {
            return false;
        }
        let mut download_path = self.download_dir.join(&info.file_name);

        // Download (with download-level fallback). Reusing a cached asset is a
        // policy decision that needs the expected digest, so it lives here
        // rather than in the transport.
        if !self.reuse_cached(&download_path, vendor, &info)
            && !self.http.download(&info.url, &download_path)
        {
            let Some(fallback_url) = vendor.fallback_url.as_deref() else {
                logger::warning(&format!("Failed to download {}, skipping...", vendor.name));
                return false;
            };
            logger::warning("  Primary download failed, trying fallback version...");
            info = fallback_info(vendor, fallback_url);
            download_path = self.download_dir.join(&info.file_name);
            logger::status(&format!("  Downloading {}...", info.file_name));
            if !self.reuse_cached(&download_path, vendor, &info)
                && !self.http.download(&info.url, &download_path)
            {
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

        // `installType: "binary"`: the download IS the tool — a single
        // verified executable placed as-is (no archive to extract, no
        // installer to run; running it would launch the tool). `binaryName`
        // names the file users will type; without it the download's own
        // name is kept.
        let staged = if vendor.install_type.as_deref() == Some("binary") {
            let placed_name = vendor
                .binary_name
                .clone()
                .unwrap_or_else(|| info.file_name.clone());
            // The lone copy target below needs `staging_target` to already
            // exist; every archive extractor in the other branch already
            // creates it internally (and an .exe installer must NOT find it
            // pre-created -- see `run_exe_installer`), so this stays scoped
            // to just the binary path.
            std::fs::create_dir_all(&staging_target).is_ok()
                && std::fs::copy(&download_path, staging_target.join(&placed_name)).is_ok()
        } else {
            let seven_zip = self.vendor_dir.join("7zip").join("7z.exe");
            archives::extract_archive(
                &download_path,
                &staging_target,
                &vendor.name,
                Some(&seven_zip),
                vendor.installer_args.as_deref(),
            )
        };
        if !staged {
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

    /// Install an `Npm`- or `Pip`-type vendor by running the corresponding
    /// vendored package manager into the tree (`home\.npm-global` /
    /// `home\.local`, both already on the exported PATH). The vendor
    /// directory receives only a marker `.vendor-version`, which is what
    /// keeps `is_vendor_installed`, `doctor`, and `outdated` truthful for
    /// these vendors.
    fn install_package_vendor(
        &self,
        vendor: &VendorDefinition,
        info: &VendorDownloadInfo,
        target_dir: &Path,
    ) -> bool {
        let Some(package) = vendor.package_name.as_deref() else {
            logger::failure(&format!("{} has no package configured", vendor.name));
            return false;
        };
        let (program, args, envs) = match vendor.source_type {
            VendorSourceType::Npm => {
                let npm = self.vendor_dir.join("nodejs").join("npm.cmd");
                if !npm.is_file() {
                    logger::failure(&format!(
                        "  {} installs through npm, but {} does not exist - install the NodeJS vendor first",
                        vendor.name,
                        npm.display()
                    ));
                    return false;
                }
                npm_install_command(&npm, &self.naner_root, package, info.version.as_deref())
            }
            VendorSourceType::Pip => {
                let python = self.vendor_dir.join("anaconda").join("python.exe");
                if !python.is_file() {
                    logger::failure(&format!(
                        "  {} installs through pip, but {} does not exist - install the Anaconda vendor first",
                        vendor.name,
                        python.display()
                    ));
                    return false;
                }
                pip_install_command(&python, &self.naner_root, package, info.version.as_deref())
            }
            _ => {
                logger::failure(&format!("{} is not a package-manager vendor", vendor.name));
                return false;
            }
        };
        logger::status(&format!("  Installing {package} via package manager..."));
        let mut command = std::process::Command::new(&program);
        command.args(&args);
        for (key, value) in &envs {
            command.env(key, value);
        }
        match command.status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                logger::failure(&format!("  npm install failed with {status}"));
                return false;
            }
            Err(e) => {
                logger::failure(&format!("  Could not run npm: {e}"));
                return false;
            }
        }

        if std::fs::create_dir_all(target_dir).is_err() {
            logger::failure(&format!(
                "  Could not create marker directory {}",
                target_dir.display()
            ));
            return false;
        }
        if let Some(version) = info.version.as_deref()
            && !version.is_empty()
            && std::fs::write(target_dir.join(VENDOR_VERSION_FILE), version).is_err()
        {
            logger::debug("Failed to save vendor version", false);
        }
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
                .map(|v| format!(" ({})", with_v_prefix(v)))
                .unwrap_or_default();

            if is_wt {
                logger::info(&format!("Updating {}{suffix}...", vendor.name));
                logger::info("  Extracting over-top; your settings are kept");
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

    /// What upstream currently calls latest, resolved fresh: no `naner.lock`,
    /// no download, and — unlike [`Self::install_vendor`]'s path — **no
    /// fallback cascade**. `outdated` and `refresh-pins` exist to check the
    /// hardcoded pins against reality; answering resolution failure with the
    /// hardcoded pin would report the stale value as the truth it was meant
    /// to be checked against. A `StaticUrl` vendor resolves to its own
    /// configured artifact — the caller decides what "latest" means there.
    pub fn resolve_upstream(
        &self,
        vendor: &VendorDefinition,
    ) -> Result<Option<VendorDownloadInfo>, String> {
        match vendor.source_type {
            VendorSourceType::StaticUrl => Ok(fetch_static(vendor)),
            VendorSourceType::GitHub => self.fetch_github(vendor),
            VendorSourceType::WebScrape => self.fetch_web_scrape(vendor),
            VendorSourceType::NodeJsApi => self.fetch_nodejs(),
            VendorSourceType::GolangApi => self.fetch_golang(),
            VendorSourceType::DotNetApi => self.fetch_dotnet(),
            VendorSourceType::Npm => self.fetch_npm(vendor),
            VendorSourceType::Pip => self.fetch_pip(vendor),
        }
    }

    /// npm registry resolution for `source_type == Npm`: the `latest`
    /// dist-tag, plus the tarball URL for the record — npm itself downloads
    /// the tarball and verifies its integrity, so no checksum flows here.
    fn fetch_npm(&self, vendor: &VendorDefinition) -> Result<Option<VendorDownloadInfo>, String> {
        let Some(package) = &vendor.package_name else {
            return Ok(None);
        };
        let url = format!("https://registry.npmjs.org/{package}");
        let (status, body) = self.http.get_text(&url)?;
        if !(200..300).contains(&status) {
            return Ok(None);
        }

        #[derive(Deserialize)]
        struct Registry {
            #[serde(rename = "dist-tags")]
            dist_tags: Option<DistTags>,
        }
        #[derive(Deserialize)]
        struct DistTags {
            latest: Option<String>,
        }

        let registry: Registry = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let Some(version) = registry.dist_tags.and_then(|t| t.latest) else {
            return Ok(None);
        };
        // Scoped packages publish their tarball under the bare name:
        // `@scope/tool` -> `tool-<version>.tgz`.
        let bare = package.rsplit('/').next().unwrap_or(package);
        Ok(Some(VendorDownloadInfo {
            url: format!("https://registry.npmjs.org/{package}/-/{bare}-{version}.tgz"),
            file_name: format!("{bare}-{version}.tgz"),
            version: Some(version),
            checksum: None,
        }))
    }

    /// PyPI resolution for `source_type == Pip` — the JSON API's
    /// `info.version` is the latest release. pip downloads and verifies the
    /// artifact itself, so the URL here is informational.
    fn fetch_pip(&self, vendor: &VendorDefinition) -> Result<Option<VendorDownloadInfo>, String> {
        let Some(package) = &vendor.package_name else {
            return Ok(None);
        };
        let url = format!("https://pypi.org/pypi/{package}/json");
        let (status, body) = self.http.get_text(&url)?;
        if !(200..300).contains(&status) {
            return Ok(None);
        }

        #[derive(Deserialize)]
        struct PyPi {
            info: Option<PyPiInfo>,
        }
        #[derive(Deserialize)]
        struct PyPiInfo {
            version: Option<String>,
        }

        let pypi: PyPi = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let Some(version) = pypi.info.and_then(|i| i.version) else {
            return Ok(None);
        };
        Ok(Some(VendorDownloadInfo {
            url: format!("https://pypi.org/project/{package}/{version}/"),
            file_name: format!("{package}=={version}"),
            version: Some(version),
            checksum: None,
        }))
    }

    /// `FetchVendorDownloadInfoAsync`: per-source resolution, then the
    /// resolution-level fallback (both the returned-None and the threw-error
    /// paths use it — the cascade that hides bug B1 in production).
    fn fetch_download_info(&self, vendor: &VendorDefinition) -> Option<VendorDownloadInfo> {
        let resolved = self.resolve_upstream(vendor);

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
        let Some(relative) = newest_scrape_match(&regex, &html) else {
            return Ok(None);
        };
        let relative = relative.as_str();
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

    /// Whether an already-downloaded asset can stand in for a fresh download.
    ///
    /// A cached file is complete by construction — `Http::download` publishes
    /// with a rename, so nothing truncated ever carries the final name. What
    /// completeness cannot tell us is whether it is the *right* artifact: file
    /// names like `rustup-init.exe` are stable while their contents move.
    /// So when a digest is known the cache entry has to match
    /// it, and a stale one is deleted and re-fetched instead of being handed to
    /// the verifier, which would fail the install rather than fix it.
    fn reuse_cached(
        &self,
        download_path: &Path,
        vendor: &VendorDefinition,
        info: &VendorDownloadInfo,
    ) -> bool {
        if !download_path.is_file() || download_path.metadata().map_or(0, |m| m.len()) == 0 {
            return false;
        }

        if let Some(expected) = resolved_checksum(vendor, info) {
            let result = checksum::verify(download_path, &expected);
            if !result.success && !result.skipped {
                logger::info("    Cached download does not match the expected digest, re-fetching");
                let _ = std::fs::remove_file(download_path);
                return false;
            }
        }

        logger::info(&format!(
            "    Using cached download asset: {}",
            download_path.display()
        ));
        true
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

/// The newest artifact a scrape pattern matches, not the first one.
///
/// `regex.captures` returns the leftmost match, and a vendor's directory index
/// is conventionally sorted ascending -- so taking the first match meant
/// `install MSYS2` fetched the *oldest* base published, two years stale, under
/// a line reading "Fetching latest MSYS2".
///
/// Ordering prefers the pattern's second capture group where there is one. The
/// convention in `vendors.json` is that group 1 is the file name and group 2 is
/// the version or date inside it, and comparing that numerically puts `1.10`
/// after `1.9`, which a string comparison does not.
///
/// Without a second group there is nothing to parse and the file names are
/// compared as strings -- correct for a zero-padded date embedded in a name,
/// wrong for an unpadded version. A vendor that needs better should capture its
/// version as group 2.
fn newest_scrape_match(regex: &crate::regex_shim::Regex, html: &str) -> Option<String> {
    let mut best: Option<(Option<String>, String)> = None;
    for captures in regex.captures_iter(html) {
        let Some(relative) = captures.get(1).filter(|s| !s.is_empty()) else {
            continue;
        };
        let key = captures
            .get(2)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let candidate = (key, relative.to_string());
        let replace = match &best {
            None => true,
            Some(current) => scrape_match_is_newer(&candidate, current),
        };
        if replace {
            best = Some(candidate);
        }
    }
    best.map(|(_, relative)| relative)
}

/// Strictly-newer test for two scraped matches, by version key where both have
/// one and by file name otherwise. Ties keep the incumbent, so the leftmost
/// match wins a genuine draw and the result stays deterministic.
fn scrape_match_is_newer(
    candidate: &(Option<String>, String),
    current: &(Option<String>, String),
) -> bool {
    match (&candidate.0, &current.0) {
        (Some(a), Some(b)) => crate::version::is_newer(a, b),
        _ => candidate.1 > current.1,
    }
}

/// Exactly one `v`, however the vendor spelled its version.
///
/// Windows Terminal's recorded version is already `v1.24.11911.0`, so
/// unconditionally prefixing produced `vv1.24.11911.0`. Deliberately not
/// `version::normalize`, which also strips a `-prerelease` suffix -- correct
/// for comparison, silently lossy for display.
fn with_v_prefix(version: &str) -> String {
    format!("v{}", version.trim_start_matches(['v', 'V']))
}

/// `ExtractVersionFromFileName`: first `(\d+\.?\d*\.?\d*\.?\d*)` match, else
/// "latest".
/// The exact npm invocation an `Npm`-type vendor installs with: a global
/// install into the tree's `home\.npm-global` prefix with the cache in the
/// tree too, pinned by explicit environment so the install stays portable
/// even when `naner install` runs from a shell without the exported
/// environment. A resolved version is pinned in the spec; without one the
/// registry's `latest` is npm's own default.
fn npm_install_command(
    npm: &Path,
    naner_root: &Path,
    package: &str,
    version: Option<&str>,
) -> (PathBuf, Vec<String>, Vec<(String, String)>) {
    let spec = match version {
        Some(v) if !v.is_empty() => format!("{package}@{v}"),
        _ => package.to_string(),
    };
    let home = naner_root.join("home");
    (
        npm.to_path_buf(),
        vec!["install".into(), "-g".into(), spec],
        vec![
            (
                "NPM_CONFIG_PREFIX".into(),
                home.join(".npm-global").display().to_string(),
            ),
            (
                "NPM_CONFIG_CACHE".into(),
                home.join(".npm-cache").display().to_string(),
            ),
        ],
    )
}

/// The pip twin of [`npm_install_command`]: a `--user` install through the
/// vendored Anaconda's interpreter, with `PYTHONUSERBASE` pinned into the
/// tree so console scripts land in `home\.local\Scripts` (already on the
/// exported PATH) and the cache stays portable too.
fn pip_install_command(
    python: &Path,
    naner_root: &Path,
    package: &str,
    version: Option<&str>,
) -> (PathBuf, Vec<String>, Vec<(String, String)>) {
    let spec = match version {
        Some(v) if !v.is_empty() => format!("{package}=={v}"),
        _ => package.to_string(),
    };
    let home = naner_root.join("home");
    (
        python.to_path_buf(),
        vec![
            "-m".into(),
            "pip".into(),
            "install".into(),
            "--user".into(),
            spec,
        ],
        vec![
            (
                "PYTHONUSERBASE".into(),
                home.join(".local").display().to_string(),
            ),
            (
                "PIP_CACHE_DIR".into(),
                home.join(".cache").join("pip").display().to_string(),
            ),
        ],
    )
}

/// Best version-looking run in a file name. The *first* match is usually
/// wrong: `msys2-base-x86_64-20240727.tar.xz` starts with the `2` in "msys2"
/// and `7z2408-x64.msi` with the `7` in "7z", so every MSYS2 install recorded
/// `.vendor-version` as literally `"2"`. The match carrying the most digits
/// is the version; on a tie the later one wins (versions sit near the end).
fn version_from_file_name(file_name: &str) -> String {
    let regex = crate::regex_shim::compile(r"(\d+\.?\d*\.?\d*\.?\d*)").unwrap();
    let mut best: Option<String> = None;
    for captures in regex.captures_iter(file_name) {
        // The pattern's optional dots can trail into a file extension
        // (`20240727.` out of `20240727.tar.xz`); a version never ends in one.
        let Some(candidate) = captures.get(1).map(|s| s.trim_end_matches('.').to_string()) else {
            continue;
        };
        let digits = |s: &str| s.chars().filter(char::is_ascii_digit).count();
        if best
            .as_deref()
            .is_none_or(|b| digits(&candidate) >= digits(b))
        {
            best = Some(candidate);
        }
    }
    best.unwrap_or_else(|| "latest".to_string())
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
    fn anaconda_scrape_resolves() {
        let http = UreqHttp::new();
        let vendor = VendorDefinition {
            name: "Anaconda".into(),
            checksum_source: Some(ChecksumSource::Scrape {
                url: "https://repo.anaconda.com/archive/".into(),
                pattern: "{FILE}</a></td>[^<]*<td[^>]*>[^<]*</td>[^<]*<td[^>]*>[^<]*</td>[^<]*<td>([0-9a-f]{64})".into(),
            }),
            ..Default::default()
        };
        let info = VendorDownloadInfo {
            url: "https://repo.anaconda.com/archive/Anaconda3-2026.07-1-Windows-x86_64.exe".into(),
            file_name: "Anaconda3-2026.07-1-Windows-x86_64.exe".into(),
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
            "anaconda",
        );
    }
}

#[cfg(test)]
mod digest_tests {
    use super::*;

    /// The real shape of a repo.anaconda.com/archive/ row, including the
    /// newline+indent runs between tags and the duplicate filename in href.
    /// Captured from the live listing (2026-08-17).
    const ANACONDA_LISTING: &str = r#"    <tr>
      <th>Last Modified</th>
      <th>SHA256</th>
    </tr>
    <tr>
      <td><a href="Anaconda3-2025.12-2-Windows-x86_64.exe">Anaconda3-2025.12-2-Windows-x86_64.exe</a></td>
      <td class="s">1.1G</td>
      <td>2026-01-13 11:06:18</td>
      <td>2e0b8e40ec7600793f116250f5c1775c866833bac32d184ad575ecc0d360a88f</td>
    </tr>
    <tr>
      <td><a href="Anaconda3-2026.07-1-Windows-x86_64.exe">Anaconda3-2026.07-1-Windows-x86_64.exe</a></td>
      <td class="s">1.0G</td>
      <td>2026-07-29 16:08:18</td>
      <td>b545f4bd8ab3bf32d99002a0779a887668ebfe479ee32ecbf060375670d5ee09</td>
    </tr>
"#;

    /// The pattern shipped in dist-assets/config/vendors.json.
    const ANACONDA_PATTERN: &str =
        "{FILE}</a></td>[^<]*<td[^>]*>[^<]*</td>[^<]*<td[^>]*>[^<]*</td>[^<]*<td>([0-9a-f]{64})";

    /// Guards the shipped config against the engine: `rusty_regx` is POSIX-ERE
    /// and leftmost-*longest*, so this proves `{64}` intervals work and that
    /// the bounded `[^<]*` runs keep the match inside the requested row rather
    /// than sliding to a neighbouring one.
    #[test]
    fn anaconda_scrape_pattern_selects_the_right_row() {
        let file = "Anaconda3-2026.07-1-Windows-x86_64.exe";
        let pattern = ANACONDA_PATTERN.replace("{FILE}", &crate::regex_shim::escape(file));
        let regex = crate::regex_shim::compile_ci(&pattern).expect("pattern compiles");
        let captured = regex
            .captures(ANACONDA_LISTING)
            .and_then(|c| c.get(1))
            .expect("hash captured");
        assert_eq!(
            captured,
            "b545f4bd8ab3bf32d99002a0779a887668ebfe479ee32ecbf060375670d5ee09"
        );
    }

    #[test]
    fn anaconda_pattern_picks_the_named_file_not_the_first_row() {
        let file = "Anaconda3-2025.12-2-Windows-x86_64.exe";
        let pattern = ANACONDA_PATTERN.replace("{FILE}", &crate::regex_shim::escape(file));
        let regex = crate::regex_shim::compile_ci(&pattern).unwrap();
        assert_eq!(
            regex.captures(ANACONDA_LISTING).and_then(|c| c.get(1)),
            Some("2e0b8e40ec7600793f116250f5c1775c866833bac32d184ad575ecc0d360a88f")
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

    #[test]
    fn npm_and_pip_resolution_read_the_registry_latest() {
        let mut http = StubHttp::default();
        http.text.insert(
            "https://registry.npmjs.org/@scope/tool".into(),
            (200, r#"{"dist-tags":{"latest":"1.2.3"}}"#.into()),
        );
        http.text.insert(
            "https://pypi.org/pypi/pytool/json".into(),
            (200, r#"{"info":{"version":"0.8.1"}}"#.into()),
        );
        let tmp = tempfile::tempdir().unwrap();
        let installer = UnifiedVendorInstaller::new(tmp.path(), Vec::new(), &http);

        let npm_vendor = VendorDefinition {
            source_type: VendorSourceType::Npm,
            package_name: Some("@scope/tool".into()),
            ..Default::default()
        };
        let info = installer.resolve_upstream(&npm_vendor).unwrap().unwrap();
        assert_eq!(info.version.as_deref(), Some("1.2.3"));
        // Scoped tarballs live under the bare name.
        assert!(
            info.url.ends_with("/@scope/tool/-/tool-1.2.3.tgz"),
            "{}",
            info.url
        );

        let pip_vendor = VendorDefinition {
            source_type: VendorSourceType::Pip,
            package_name: Some("pytool".into()),
            ..Default::default()
        };
        let info = installer.resolve_upstream(&pip_vendor).unwrap().unwrap();
        assert_eq!(info.version.as_deref(), Some("0.8.1"));

        // No package configured resolves to nothing, not an error.
        let bare = VendorDefinition {
            source_type: VendorSourceType::Npm,
            ..Default::default()
        };
        assert!(installer.resolve_upstream(&bare).unwrap().is_none());
    }

    /// Both package managers must be pinned into the tree by explicit
    /// environment — an install run from a shell without naner's exported
    /// environment would otherwise write into the real user profile, which
    /// is the exact leak the redirection work exists to stop.
    #[test]
    fn package_manager_commands_stay_inside_the_tree() {
        let root = Path::new("/naner");
        let (program, args, envs) = npm_install_command(
            Path::new("/naner/vendor/nodejs/npm.cmd"),
            root,
            "@scope/tool",
            Some("1.2.3"),
        );
        assert!(program.ends_with("npm.cmd"));
        assert_eq!(args, vec!["install", "-g", "@scope/tool@1.2.3"]);
        assert!(
            envs.iter()
                .any(|(k, v)| k == "NPM_CONFIG_PREFIX" && v.contains(".npm-global"))
        );
        assert!(envs.iter().any(|(k, _)| k == "NPM_CONFIG_CACHE"));

        let (program, args, envs) = pip_install_command(
            Path::new("/naner/vendor/anaconda/python.exe"),
            root,
            "pytool",
            None,
        );
        assert!(program.ends_with("python.exe"));
        assert_eq!(args, vec!["-m", "pip", "install", "--user", "pytool"]);
        assert!(
            envs.iter()
                .any(|(k, v)| k == "PYTHONUSERBASE" && v.contains(".local"))
        );
        assert!(envs.iter().any(|(k, _)| k == "PIP_CACHE_DIR"));
    }

    /// `installType: "binary"`: the verified download is placed as-is under
    /// `binaryName` — running it or extracting it would both be wrong.
    #[test]
    fn a_binary_vendor_places_the_download_under_its_binary_name() {
        let root = tempfile::tempdir().unwrap();
        let payload = b"MZ fake exe".to_vec();
        let mut http = StubHttp::default();
        http.files.insert(
            "https://example.com/tool-windows-amd64.exe".into(),
            payload.clone(),
        );

        let vendor = VendorDefinition {
            name: "Tool".into(),
            key: "Tool".into(),
            extract_dir: "tool".into(),
            static_url: Some("https://example.com/tool-windows-amd64.exe".into()),
            file_name: Some("tool-windows-amd64.exe".into()),
            install_type: Some("binary".into()),
            binary_name: Some("tool.exe".into()),
            ..Default::default()
        };
        let installer = UnifiedVendorInstaller::new(root.path(), vec![vendor], &http);
        assert!(installer.install_vendor("Tool"));

        let placed = root.path().join("vendor/tool/tool.exe");
        assert_eq!(std::fs::read(&placed).unwrap(), payload, "placed verbatim");
        assert!(
            !root
                .path()
                .join("vendor/tool/tool-windows-amd64.exe")
                .exists(),
            "the upstream artifact name must not be what lands on PATH"
        );
    }

    /// The first digit run in a file name is usually part of the *name*
    /// (`msys2`, `7z`, `x86_64`), not the version — MSYS2 installs recorded
    /// `.vendor-version` as literally `"2"` because of it.
    #[test]
    fn version_from_file_name_picks_the_version_not_the_first_digit() {
        assert_eq!(
            version_from_file_name("msys2-base-x86_64-20240727.tar.xz"),
            "20240727"
        );
        assert_eq!(version_from_file_name("7z2408-x64.msi"), "2408");
        assert_eq!(
            version_from_file_name("node-v20.11.0-win-x64.zip"),
            "20.11.0"
        );
        assert_eq!(
            version_from_file_name("go1.21.6.windows-amd64.zip"),
            "1.21.6"
        );
        assert_eq!(version_from_file_name("no-digits-here.zip"), "latest");
    }

    /// Stub HTTP: canned text responses per URL; downloads write canned
    /// bytes (or fail when the URL is marked bad).
    #[derive(Default)]
    struct StubHttp {
        text: HashMap<String, (u16, String)>,
        files: HashMap<String, Vec<u8>>,
        /// Counts `download` calls so a test can prove a transfer was *skipped*
        /// rather than merely that the right bytes ended up on disk.
        downloads: std::cell::Cell<usize>,
    }

    impl Http for StubHttp {
        fn get_text(&self, url: &str) -> Result<(u16, String), String> {
            self.text
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no route for {url}"))
        }
        fn download(&self, url: &str, output_path: &Path) -> bool {
            self.downloads.set(self.downloads.get() + 1);
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

    // ---- download caching (#15) ----

    fn cached_vendor(sha256: Option<&str>) -> VendorDefinition {
        VendorDefinition {
            name: "Node.js".into(),
            key: "NodeJS".into(),
            extract_dir: "nodejs".into(),
            source_type: VendorSourceType::StaticUrl,
            static_url: Some("https://static.example/node.zip".into()),
            file_name: Some("node.zip".into()),
            checksum: sha256.map(|v| checksum::ChecksumInfo {
                algorithm: "SHA256".into(),
                value: v.into(),
                required: true,
            }),
            ..Default::default()
        }
    }

    /// Seed `vendor/.downloads/node.zip` as if a previous run had left it.
    fn seed_cache(root: &Path, bytes: &[u8]) -> PathBuf {
        let dir = root.join("vendor/.downloads");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("node.zip");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn payload_and_digest() -> (Vec<u8>, String) {
        let bytes = zip_bytes("node.exe", b"node");
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &bytes).unwrap();
        let digest = crate::checksum::compute(tmp.path(), "SHA256").unwrap();
        (bytes, digest)
    }

    #[test]
    fn a_matching_cached_asset_is_reused_without_downloading() {
        let root = tempfile::tempdir().unwrap();
        let (payload, digest) = payload_and_digest();
        seed_cache(root.path(), &payload);

        // No route for the URL at all: if it tried to download, it would fail.
        let http = StubHttp::default();
        let installer =
            UnifiedVendorInstaller::new(root.path(), vec![cached_vendor(Some(&digest))], &http);

        assert!(installer.install_vendor("Node.js"));
        assert_eq!(http.downloads.get(), 0, "cache hit must skip the transfer");
        assert!(root.path().join("vendor/nodejs/node.exe").is_file());
    }

    /// The stale-cache case that used to turn into a failed install: the file
    /// name is stable (`rustup-init.exe`) while the contents move, so the
    /// cached bytes no longer match the digest we now expect.
    #[test]
    fn a_stale_cached_asset_is_discarded_and_refetched() {
        let root = tempfile::tempdir().unwrap();
        let (payload, digest) = payload_and_digest();
        seed_cache(root.path(), b"an older release with the same file name");

        let mut http = StubHttp::default();
        http.files
            .insert("https://static.example/node.zip".into(), payload);
        let installer =
            UnifiedVendorInstaller::new(root.path(), vec![cached_vendor(Some(&digest))], &http);

        assert!(
            installer.install_vendor("Node.js"),
            "a stale cache must be re-fetched, not fail the install"
        );
        assert_eq!(http.downloads.get(), 1);
        assert!(root.path().join("vendor/nodejs/node.exe").is_file());
    }

    /// With no digest to check against, a complete cached file is still
    /// reusable — that is the offline/air-gapped case the README advertises.
    #[test]
    fn an_unverifiable_cached_asset_is_still_reused() {
        let root = tempfile::tempdir().unwrap();
        let (payload, _) = payload_and_digest();
        seed_cache(root.path(), &payload);

        let http = StubHttp::default();
        let installer = UnifiedVendorInstaller::new(root.path(), vec![cached_vendor(None)], &http);

        assert!(installer.install_vendor("Node.js"));
        assert_eq!(http.downloads.get(), 0);
    }

    /// The interruption this issue is about. A killed process leaves the
    /// staging file, never the final name — so the next run sees no cache and
    /// downloads properly instead of unpacking a truncated archive.
    #[test]
    fn an_interrupted_transfer_leaves_no_cache_hit() {
        let root = tempfile::tempdir().unwrap();
        let (payload, digest) = payload_and_digest();

        // Simulate the kill: a partial `.part` and nothing at the final name.
        let dir = root.path().join("vendor/.downloads");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("node.zip.part"), &payload[..payload.len() / 2]).unwrap();

        let mut http = StubHttp::default();
        http.files
            .insert("https://static.example/node.zip".into(), payload);
        let installer =
            UnifiedVendorInstaller::new(root.path(), vec![cached_vendor(Some(&digest))], &http);

        assert!(installer.install_vendor("Node.js"));
        assert_eq!(
            http.downloads.get(),
            1,
            "a .part file must not be mistaken for a finished download"
        );
    }

    #[test]
    fn an_empty_cached_file_is_not_a_cache_hit() {
        let root = tempfile::tempdir().unwrap();
        let (payload, digest) = payload_and_digest();
        seed_cache(root.path(), b"");

        let mut http = StubHttp::default();
        http.files
            .insert("https://static.example/node.zip".into(), payload);
        let installer =
            UnifiedVendorInstaller::new(root.path(), vec![cached_vendor(Some(&digest))], &http);

        assert!(installer.install_vendor("Node.js"));
        assert_eq!(http.downloads.get(), 1);
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

        // Windows Terminal's profiles are generated from naner.json's own
        // `Profiles` (#83) -- a real naner tree always has this by the time
        // any vendor gets installed, so the fixture needs one too.
        let config_dir = root.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("naner.json"),
            r#"{ "Profiles": { "Unified": {
                "Name": "Naner (Unified)", "Shell": "PowerShell",
                "CustomShell": { "ExecutablePath": "pwsh.exe" }
            } } }"#,
        )
        .unwrap();

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

        // Asserting `is_file()` here is what let the overwrite through: the
        // configurator used to rewrite settings.json from the template on
        // every update, wholesale, and a file was still present afterward,
        // so this passed while every colour scheme and key binding the user
        // had set was being destroyed. Read the contents.
        //
        // #52: naner now reconciles its own profiles into the file by GUID
        // instead of leaving it untouched or overwriting it whole. This
        // fixture's settings.json has never seen a Naner profile before, so
        // one is added -- but the user's own key survives right alongside
        // it, which is the property #50 needed and #52 keeps.
        let updated: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(target.join("settings/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            updated["user"], "edited",
            "an update must not drop the user's own Windows Terminal settings"
        );
        assert!(
            updated["profiles"]["list"]
                .as_array()
                .is_some_and(|list| !list.is_empty()),
            "a Naner profile the user never had should be offered: {updated}"
        );
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
        // The C# regex took the FIRST digit run — the "2" in "msys2" — so
        // every MSYS2 install recorded `.vendor-version` as literally "2".
        // Fixed: the run with the most digits is the version.
        assert_eq!(info.version.as_deref(), Some("20240727"));
    }

    /// A directory index lists many archives, ascending. Taking the leftmost
    /// match therefore took the OLDEST -- `install MSYS2` fetched a base two
    /// years stale under a line reading "Fetching latest MSYS2".
    ///
    /// The old fixture had exactly one archive on the page, so first and newest
    /// were the same document and it could not tell the two behaviours apart.
    #[test]
    fn a_scrape_takes_the_newest_match_not_the_first() {
        let regex = crate::regex_shim::compile_ci(r#"href="(msys2-base-x86_64-(\d{8})\.tar\.xz)""#)
            .unwrap();
        let index = r#"
            <a href="msys2-base-x86_64-20240507.tar.xz">a</a>
            <a href="msys2-base-x86_64-20240727.tar.xz">b</a>
            <a href="msys2-base-x86_64-20260611.tar.xz">c</a>
            <a href="msys2-base-x86_64-20251213.tar.xz">d</a>
        "#;
        assert_eq!(
            newest_scrape_match(&regex, index).as_deref(),
            Some("msys2-base-x86_64-20260611.tar.xz")
        );
    }

    /// Why the version group is compared numerically rather than as a string:
    /// `"1.9.0" > "1.10.0"` lexically, and that is the wrong answer.
    #[test]
    fn the_version_group_is_compared_numerically() {
        let regex = crate::regex_shim::compile_ci(r#"href="(tool-(\d+\.\d+\.\d+)\.zip)""#).unwrap();
        let index = r#"<a href="tool-1.9.0.zip">x</a><a href="tool-1.10.0.zip">y</a>"#;
        assert_eq!(
            newest_scrape_match(&regex, index).as_deref(),
            Some("tool-1.10.0.zip")
        );
    }

    /// No second group means nothing to parse, so file names are compared as
    /// strings. Right for a zero-padded date, and the documented limit.
    #[test]
    fn without_a_version_group_the_file_names_are_compared() {
        let regex = crate::regex_shim::compile_ci(r#"href="(base-\d{8}\.tar)""#).unwrap();
        let index = r#"<a href="base-20240507.tar">a</a><a href="base-20260611.tar">b</a>"#;
        assert_eq!(
            newest_scrape_match(&regex, index).as_deref(),
            Some("base-20260611.tar")
        );
    }

    #[test]
    fn a_scrape_with_no_match_resolves_to_nothing() {
        let regex = crate::regex_shim::compile_ci(r#"href="(nothing-\d+\.zip)""#).unwrap();
        assert_eq!(
            newest_scrape_match(&regex, "<a href=\"other.zip\">x</a>"),
            None
        );

        assert_eq!(version_from_file_name("7z2408-x64.msi"), "2408");
        assert_eq!(
            version_from_file_name("PowerShell-7.4.6-win-x64.zip"),
            "7.4.6"
        );
        assert_eq!(version_from_file_name("no-digits.zip"), "latest");
    }

    /// Windows Terminal records its version with the `v` already on it, so the
    /// update line read `Updating Windows Terminal (vv1.24.11911.0)...`.
    #[test]
    fn a_version_gets_exactly_one_v_however_it_was_recorded() {
        assert_eq!(with_v_prefix("v1.24.11911.0"), "v1.24.11911.0");
        assert_eq!(with_v_prefix("26.02"), "v26.02");
        assert_eq!(with_v_prefix("V7.6.5"), "v7.6.5");
        // A prerelease suffix survives: this is display, not comparison.
        assert_eq!(with_v_prefix("0.5.0-alpha.0"), "v0.5.0-alpha.0");
    }
}
