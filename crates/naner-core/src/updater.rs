//! Bootstrap and self-update for the single naner binary.
//!
//! Descended from `NanerUpdater` (Naner.Init), which lived in the separate
//! `naner-init.exe`. Two models met here and one survived:
//! `initialize` is still sync-to-embedded — the bundle a binary bootstraps
//! from is always the release matching its own compiled-in version, so a
//! fresh install can never strand — while `update_from_release` installs the
//! *latest* release, into every copy of the binary the tree is known to
//! carry, replacing the running one via rename-aside.

use std::path::{Path, PathBuf};

use crate::github::{GitHubRelease, ReleasesApi};
use crate::{archives, checksum, constants, logger, version};

const NANER_BUNDLE_NAME: &str = "naner-bundle.zip";

/// `sha256sum`-style manifest published alongside the release assets by the
/// release workflow. Every asset this updater installs must appear in it.
const SHA256SUMS_NAME: &str = "SHA256SUMS";

pub struct NanerUpdater<'a> {
    naner_root: PathBuf,
    vendor_bin_dir: PathBuf,
    github: &'a dyn ReleasesApi,
    /// Baked at compile time; the release workflow guarantees tag == this
    /// (MIGRATION_ANALYSIS §4.2).
    init_version: String,
}

impl<'a> NanerUpdater<'a> {
    pub fn new(naner_root: &Path, github: &'a dyn ReleasesApi) -> Self {
        Self {
            naner_root: naner_root.to_path_buf(),
            vendor_bin_dir: naner_root.join("vendor").join("bin"),
            github,
            init_version: constants::VERSION.to_string(),
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.naner_root
            .join(constants::INITIALIZATION_MARKER_FILE)
            .is_file()
    }

    /// `GetInstalledVersion`: the `.naner-version` file, else "0.0.0" when
    /// naner.exe exists without one (the C# FileVersionInfo fallback reads
    /// a PE version resource our Rust exe doesn't carry), else None.
    pub fn installed_version(&self) -> Option<String> {
        let version_file = self.vendor_bin_dir.join(constants::VERSION_FILE);
        if let Ok(content) = std::fs::read_to_string(&version_file) {
            return Some(content.trim().to_string());
        }
        if self
            .vendor_bin_dir
            .join(constants::executables::NANER)
            .is_file()
        {
            return Some("0.0.0".to_string());
        }
        None
    }

    pub fn target_version(&self) -> &str {
        &self.init_version
    }

    /// Verify `file` against the release's `SHA256SUMS` manifest.
    ///
    /// Fails closed. The release workflow enforces tag == embedded version,
    /// so a release this binary is willing to install from is always one the
    /// same workflow built — a missing or non-matching manifest means the
    /// artifact is not what the release says it is, not that the release is
    /// merely old.
    fn verify_asset(&self, release: &GitHubRelease, file: &Path, asset_name: &str) -> bool {
        let sums_asset = release.assets.as_deref().and_then(|assets| {
            assets.iter().find(|a| {
                a.name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(SHA256SUMS_NAME))
            })
        });
        let Some(sums_url) = sums_asset
            .and_then(|a| a.url.as_deref().or(a.browser_download_url.as_deref()))
            .filter(|u| !u.is_empty())
        else {
            logger::failure(&format!("{SHA256SUMS_NAME} not found in release assets"));
            logger::info("Refusing to install an unverified download.");
            return false;
        };

        let sums_path = file.with_extension("sha256sums");
        if !self
            .github
            .download_asset(sums_url, &sums_path, SHA256SUMS_NAME)
        {
            logger::failure(&format!("Could not download {SHA256SUMS_NAME}"));
            return false;
        }
        let sums = std::fs::read_to_string(&sums_path).unwrap_or_default();
        let _ = std::fs::remove_file(&sums_path);

        let Some(expected) = sha256_for(&sums, asset_name) else {
            logger::failure(&format!("{asset_name} is not listed in {SHA256SUMS_NAME}"));
            return false;
        };

        let info = checksum::ChecksumInfo {
            algorithm: "SHA256".into(),
            value: expected,
            required: true,
        };
        let result = checksum::verify(file, &info);
        if result.success {
            logger::success(&format!("Verified {asset_name}"));
            return true;
        }

        logger::failure(&format!("Checksum verification failed for {asset_name}!"));
        logger::failure(&format!(
            "Expected: {}",
            result.expected.as_deref().unwrap_or_default()
        ));
        logger::failure(&format!(
            "Actual:   {}",
            result.actual.as_deref().unwrap_or_default()
        ));
        false
    }

    /// `CheckForUpdateAsync`: canonical-form inequality against the embedded
    /// version (B5 fixed: "1.2" == "1.2.0"; B6 handled by canonicalizing the
    /// leading `v`). Still sync-to-embedded — any difference, including a
    /// prerelease suffix or a downgrade, triggers the swap.
    pub fn check_for_update(&self) -> (bool, Option<String>) {
        let Some(installed) = self.installed_version() else {
            return (true, Some(self.init_version.clone()));
        };
        let update_needed =
            version::canonical(&installed) != version::canonical(&self.init_version);
        (
            update_needed,
            update_needed.then(|| self.init_version.clone()),
        )
    }

    /// The newest published release, with the failure logged.
    ///
    /// Callers pair this with [`update_from_release`](Self::update_from_release)
    /// — fetch, show the tag, prompt, then act — which together replaced the
    /// sync-to-embedded `update_naner_exe`. That model was "replace naner-init
    /// by hand, and it pulls naner.exe up to match"; its check compared two
    /// local values, so nothing ever told a user a newer release existed —
    /// the mechanism worked and the discovery did not.
    pub fn fetch_latest(&self) -> Option<GitHubRelease> {
        let release = self.github.get_latest_release();
        if release.is_none() {
            logger::failure("Failed to fetch the latest release from GitHub");
        }
        release
    }

    /// Install `release`'s single binary into every copy the tree is known
    /// to carry, replacing the running one at `self_path` via rename-aside
    /// (Windows renames a running exe but will not overwrite one).
    ///
    /// One download, verified against the release's `SHA256SUMS` before any
    /// file is touched, then installed to: `self_path` first — if a later
    /// step fails, a new binary over an old tree offers the update again,
    /// where the other order leaves a stale binary that could offer a
    /// downgrade — then the canonical `vendor/bin/naner.exe`, then any
    /// pre-single-binary `naner-init.exe` leftovers (vendor/bin and the
    /// root). Refreshing the leftovers is not politeness: an old naner-init
    /// syncs the tree to its own embedded version, so one stale copy is a
    /// standing downgrade hazard.
    pub fn update_from_release(&self, release: &GitHubRelease, self_path: &Path) -> bool {
        logger::header("Updating Naner");
        logger::newline();

        let tag = release.tag_name.clone().unwrap_or_default();
        logger::info(&format!("Target version: {tag}"));
        logger::newline();

        let staged = self
            .vendor_bin_dir
            .join(format!("{}.staged", constants::executables::NANER));
        if !self.fetch_and_verify(release, constants::executables::NANER, &staged) {
            let _ = std::fs::remove_file(&staged);
            return false;
        }

        // 1. The running binary, by rename-aside. The `.old` leftover is
        // swept on the next launch.
        let old_self = with_appended_extension(self_path, "old");
        let _ = std::fs::remove_file(&old_self);
        if let Err(e) = std::fs::rename(self_path, &old_self) {
            logger::failure(&format!("Could not move the running binary aside: {e}"));
            let _ = std::fs::remove_file(&staged);
            return false;
        }
        if let Err(e) = std::fs::copy(&staged, self_path) {
            logger::failure(&format!("Could not install the new binary: {e}"));
            // Put the working binary back under its own name.
            let _ = std::fs::rename(&old_self, self_path);
            let _ = std::fs::remove_file(&staged);
            return false;
        }
        logger::success(&format!("Installed {}", self_path.display()));

        // 2. The canonical launcher location, when the running copy is
        // somewhere else (e.g. the root).
        let canonical = self.vendor_bin_dir.join(constants::executables::NANER);
        if !same_path(&canonical, self_path) {
            if !install_copy(&staged, &canonical) {
                let _ = std::fs::remove_file(&staged);
                return false;
            }
            logger::success(&format!("Installed {}", canonical.display()));
        }

        // 3. Pre-single-binary leftovers. Best effort: a locked copy fails
        // its own refresh, not the update -- but say exactly what it means.
        for legacy in [
            self.vendor_bin_dir.join(constants::executables::NANER_INIT),
            self.naner_root.join(constants::executables::NANER_INIT),
        ] {
            if legacy.is_file() && !same_path(&legacy, self_path) && !install_copy(&staged, &legacy)
            {
                logger::warning(&format!(
                    "Could not refresh {} -- it is now an OUTDATED updater whose \
                     own update would downgrade this installation. Delete it.",
                    legacy.display()
                ));
            }
        }

        let _ = std::fs::remove_file(&staged);

        if let Err(e) = std::fs::write(self.vendor_bin_dir.join(constants::VERSION_FILE), &tag) {
            logger::failure(&format!("Update failed: {e}"));
            return false;
        }

        logger::newline();
        logger::success(&format!("Naner updated to version {tag}"));
        true
    }

    /// Find `asset_name` in the release, download it to `dest`, and verify it
    /// against the release's manifest. `dest` is left in place on success and
    /// is the caller's to clean up on failure.
    fn fetch_and_verify(&self, release: &GitHubRelease, asset_name: &str, dest: &Path) -> bool {
        let asset = release.assets.as_deref().and_then(|assets| {
            assets.iter().find(|a| {
                a.name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(asset_name))
            })
        });
        let Some(asset) = asset else {
            logger::failure(&format!("{asset_name} not found in release assets"));
            return false;
        };

        let download_url = asset
            .url
            .as_deref()
            .or(asset.browser_download_url.as_deref());
        let Some(download_url) = download_url.filter(|u| !u.is_empty()) else {
            logger::failure(&format!("Download URL for {asset_name} is missing"));
            return false;
        };

        if !self.github.download_asset(download_url, dest, asset_name) {
            return false;
        }
        self.verify_asset(release, dest, asset_name)
    }

    /// `InitializeAsync`: download `naner-bundle.zip` from the embedded-tag
    /// release, extract it over the root (no flattening), verify
    /// vendor/bin/naner.exe, write `.naner-version` and the init marker.
    pub fn initialize(&self) -> bool {
        logger::header("Initializing Naner");
        logger::newline();

        logger::info(&format!("Fetching release v{}...", self.init_version));
        let Some(release) = self.github.get_release_by_tag(&self.init_version) else {
            logger::failure(&format!(
                "Failed to fetch release v{} from GitHub",
                self.init_version
            ));
            return false;
        };

        let tag = release.tag_name.clone().unwrap_or_default();
        logger::info(&format!("Target version: {tag}"));
        logger::newline();

        let asset = release.assets.as_deref().and_then(|assets| {
            assets.iter().find(|a| {
                a.name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(NANER_BUNDLE_NAME))
            })
        });
        let Some(asset) = asset else {
            logger::failure(&format!("{NANER_BUNDLE_NAME} not found in release assets"));
            return false;
        };
        let download_url = asset
            .url
            .as_deref()
            .or(asset.browser_download_url.as_deref());
        let Some(download_url) = download_url.filter(|u| !u.is_empty()) else {
            logger::failure(&format!("Download URL for {NANER_BUNDLE_NAME} is missing"));
            return false;
        };

        let temp_bundle = self.naner_root.join(format!("{NANER_BUNDLE_NAME}.tmp"));
        if !self
            .github
            .download_asset(download_url, &temp_bundle, NANER_BUNDLE_NAME)
        {
            return false;
        }

        logger::newline();
        logger::status("Extracting bundle...");
        if !self.verify_asset(&release, &temp_bundle, NANER_BUNDLE_NAME) {
            let _ = std::fs::remove_file(&temp_bundle);
            return false;
        }

        let extract_result = archives::extract_zip_plain(&temp_bundle, &self.naner_root);
        let _ = std::fs::remove_file(&temp_bundle);
        match extract_result {
            Ok(()) => logger::success("Bundle extracted"),
            Err(e) => {
                logger::failure(&format!("Failed to extract bundle: {e}"));
                return false;
            }
        }

        let naner_path = self.vendor_bin_dir.join(constants::executables::NANER);
        if !naner_path.is_file() {
            logger::failure(&format!(
                "{} not found in bundle (expected at vendor/bin/{})",
                constants::executables::NANER,
                constants::executables::NANER
            ));
            return false;
        }
        logger::success(&format!("Found {}", constants::executables::NANER));

        if std::fs::write(self.vendor_bin_dir.join(constants::VERSION_FILE), &tag).is_err() {
            return false;
        }

        let marker = format!(
            "# Naner Initialization Marker\n# Created: {}\n# Version: {tag}\n",
            crate::timestamp::now_local()
        );
        if std::fs::write(
            self.naner_root.join(constants::INITIALIZATION_MARKER_FILE),
            marker,
        )
        .is_err()
        {
            return false;
        }

        logger::newline();
        logger::success(&format!("Naner initialized successfully (version {tag})"));
        true
    }

    /// `LaunchNaner`: pass-through spawn of naner.exe with NANER_ROOT set,
    /// fire-and-forget (naner itself spawns the terminal and exits).
    pub fn launch_naner(&self, args: &[String]) -> i32 {
        let naner_path = self.vendor_bin_dir.join(constants::executables::NANER);
        if !naner_path.is_file() {
            logger::failure(&format!("Naner not found at: {}", naner_path.display()));
            logger::info("Run 'naner init' to install Naner first");
            return 1;
        }

        let mut command = std::process::Command::new(&naner_path);
        command
            .args(args)
            .current_dir(&self.naner_root)
            .env("NANER_ROOT", &self.naner_root);

        // CreateNoWindow: don't flash a console for the GUI-subsystem child.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        match command.spawn() {
            Ok(_) => 0,
            Err(e) => {
                logger::failure(&format!("Failed to launch Naner: {e}"));
                1
            }
        }
    }
}

/// Look up `asset_name` in a `sha256sum`-style manifest (`<hex>  <name>`).
/// The name column may be `*name` when the sum was taken in binary mode.
fn sha256_for(sums: &str, asset_name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name.eq_ignore_ascii_case(asset_name) && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_string())
    })
}

/// Whether two paths name the same file, tolerant of `.`/`..`/case noise on
/// Windows: canonicalized when possible, raw comparison otherwise.
fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Replace `target` with a copy of `staged`, atomically enough for an exe
/// swap: copy to a sibling temp file, remove the old target, rename into
/// place. The target must not be the running binary.
fn install_copy(staged: &Path, target: &Path) -> bool {
    let tmp = with_appended_extension(target, "tmp");
    if let Err(e) = std::fs::copy(staged, &tmp) {
        logger::failure(&format!("Could not stage {}: {e}", target.display()));
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    if target.is_file()
        && let Err(e) = std::fs::remove_file(target)
    {
        logger::warning(&format!("Could not delete old {}: {e}", target.display()));
    }
    if let Err(e) = std::fs::rename(&tmp, target) {
        logger::failure(&format!("Could not install {}: {e}", target.display()));
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

/// `path` with `.{ext}` appended to its full file name — `naner-init.exe`
/// becomes `naner-init.exe.old`, not `naner-init.old`.
fn with_appended_extension(path: &Path, ext: &str) -> std::path::PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(ext);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{GitHubAsset, GitHubRelease};
    use std::io::Write;

    struct StubApi {
        release: Option<GitHubRelease>,
        /// Bytes served for any download_asset call; None = download fails.
        asset_bytes: Option<Vec<u8>>,
        /// Body served when the updater asks for SHA256SUMS; None = the
        /// manifest download itself fails.
        sums: Option<String>,
    }

    impl ReleasesApi for StubApi {
        fn get_latest_release(&self) -> Option<GitHubRelease> {
            self.release.clone()
        }
        fn get_release_by_tag(&self, _tag: &str) -> Option<GitHubRelease> {
            self.release.clone()
        }
        fn download_asset(&self, _url: &str, output_path: &Path, name: &str) -> bool {
            let bytes = if name.eq_ignore_ascii_case(SHA256SUMS_NAME) {
                match &self.sums {
                    Some(body) => body.clone().into_bytes(),
                    None => return false,
                }
            } else {
                match &self.asset_bytes {
                    Some(bytes) => bytes.clone(),
                    None => return false,
                }
            };
            std::fs::File::create(output_path)
                .unwrap()
                .write_all(&bytes)
                .unwrap();
            true
        }
    }

    /// A `SHA256SUMS` body listing `name` with the real digest of `bytes`.
    fn sums_for(name: &str, bytes: &[u8]) -> String {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), bytes).unwrap();
        let hash = crate::checksum::compute(tmp.path(), "SHA256").unwrap();
        format!("{}  {name}\n", hash.to_lowercase())
    }

    /// The manifest asset every release now carries.
    fn sums_asset() -> (&'static str, &'static str) {
        (
            SHA256SUMS_NAME,
            "https://api.github.com/repos/x/y/releases/assets/99",
        )
    }

    fn release_with(assets: Vec<(&str, &str)>) -> GitHubRelease {
        GitHubRelease {
            tag_name: Some(format!("v{}", constants::VERSION)),
            assets: Some(
                assets
                    .into_iter()
                    .map(|(name, url)| GitHubAsset {
                        name: Some(name.to_string()),
                        url: Some(url.to_string()),
                        ..Default::default()
                    })
                    .collect(),
            ),
            ..Default::default()
        }
    }

    fn bundle_zip() -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default();
            for dir in ["bin/", "config/", "home/", "vendor/", "vendor/bin/"] {
                writer
                    .add_directory(dir.trim_end_matches('/'), options)
                    .unwrap();
            }
            writer.start_file("vendor/bin/naner.exe", options).unwrap();
            writer.write_all(b"fake naner exe").unwrap();
            writer.start_file("config/naner.json", options).unwrap();
            writer.write_all(b"{}").unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn initialize_extracts_bundle_and_writes_markers() {
        let root = tempfile::tempdir().unwrap();
        let api = StubApi {
            release: Some(release_with(vec![
                (
                    "naner-bundle.zip",
                    "https://api.github.com/repos/x/y/releases/assets/1",
                ),
                sums_asset(),
            ])),
            asset_bytes: Some(bundle_zip()),
            sums: Some(sums_for(NANER_BUNDLE_NAME, &bundle_zip())),
        };
        let updater = NanerUpdater::new(root.path(), &api);

        assert!(!updater.is_initialized());
        assert!(updater.initialize());

        // Bundle extracted with NO flattening: tree is exactly the archive's.
        assert!(root.path().join("vendor/bin/naner.exe").is_file());
        assert!(root.path().join("config/naner.json").is_file());
        // Version written in tag form (B6 canonical), marker created.
        assert_eq!(
            std::fs::read_to_string(root.path().join("vendor/bin/.naner-version")).unwrap(),
            format!("v{}", constants::VERSION)
        );
        assert!(updater.is_initialized());
        let marker = std::fs::read_to_string(root.path().join(".naner-initialized")).unwrap();
        assert!(marker.starts_with("# Naner Initialization Marker"));

        // Bundle temp file cleaned up.
        assert!(!root.path().join("naner-bundle.zip.tmp").exists());

        // check_for_update now sees matching versions (v-normalized).
        let (update_needed, _) = updater.check_for_update();
        assert!(!update_needed);
    }

    #[test]
    fn initialize_fails_when_bundle_asset_missing() {
        let root = tempfile::tempdir().unwrap();
        let api = StubApi {
            release: Some(release_with(vec![("other.zip", "https://x/other.zip")])),
            asset_bytes: Some(vec![]),
            sums: None,
        };
        let updater = NanerUpdater::new(root.path(), &api);
        assert!(!updater.initialize());
        assert!(!updater.is_initialized());
    }

    #[test]
    fn update_replaces_naner_exe_and_version_file() {
        let root = tempfile::tempdir().unwrap();
        let vendor_bin = root.path().join("vendor/bin");
        std::fs::create_dir_all(&vendor_bin).unwrap();
        std::fs::write(vendor_bin.join("naner.exe"), "old exe").unwrap();
        std::fs::write(vendor_bin.join(".naner-version"), "v0.0.1").unwrap();

        let api = StubApi {
            release: Some(exe_release()),
            asset_bytes: Some(b"new exe".to_vec()),
            sums: Some(sums_for(constants::executables::NANER, b"new exe")),
        };
        let updater = NanerUpdater::new(root.path(), &api);

        // Sync check: installed v0.0.1 != embedded version -> update needed.
        let (update_needed, target) = updater.check_for_update();
        assert!(update_needed);
        assert_eq!(target.as_deref(), Some(constants::VERSION));

        // The tree a real upgrade meets: the running copy at the root, the
        // canonical launcher in vendor/bin, and a pre-single-binary
        // naner-init.exe leftover in each place.
        let self_path = staged_self(root.path());
        for legacy in [
            vendor_bin.join(constants::executables::NANER_INIT),
            root.path().join(constants::executables::NANER_INIT),
        ] {
            std::fs::write(legacy, b"stale init").unwrap();
        }

        assert!(updater.update_from_release(&exe_release(), &self_path));

        // Every copy now carries the new bytes -- the leftovers included,
        // because a stale naner-init is a standing downgrade hazard.
        assert_eq!(std::fs::read(&self_path).unwrap(), b"new exe");
        for refreshed in [
            vendor_bin.join(constants::executables::NANER),
            vendor_bin.join(constants::executables::NANER_INIT),
            root.path().join(constants::executables::NANER_INIT),
        ] {
            assert_eq!(
                std::fs::read(&refreshed).unwrap(),
                b"new exe",
                "{} was not refreshed",
                refreshed.display()
            );
        }
        assert_eq!(
            std::fs::read_to_string(vendor_bin.join(".naner-version")).unwrap(),
            format!("v{}", constants::VERSION)
        );

        // The displaced running binary parks as `.old` for the next launch
        // to sweep; no staging files survive.
        assert_eq!(
            std::fs::read(root.path().join("naner.exe.old")).unwrap(),
            b"old self"
        );
        assert!(!vendor_bin.join("naner.exe.staged").exists());
        assert!(!vendor_bin.join("naner.exe.tmp").exists());
    }

    /// The common steady-state: the running binary IS vendor/bin/naner.exe.
    /// The self swap and the canonical install are the same file; nothing
    /// may be double-handled or left behind.
    #[test]
    fn updating_the_canonical_copy_in_place_parks_old_beside_it() {
        let root = tempfile::tempdir().unwrap();
        let vendor_bin = root.path().join("vendor/bin");
        std::fs::create_dir_all(&vendor_bin).unwrap();
        let self_path = vendor_bin.join(constants::executables::NANER);
        std::fs::write(&self_path, b"old self").unwrap();

        let api = StubApi {
            release: Some(exe_release()),
            asset_bytes: Some(b"new exe".to_vec()),
            sums: Some(sums_for(constants::executables::NANER, b"new exe")),
        };
        let updater = NanerUpdater::new(root.path(), &api);
        assert!(updater.update_from_release(&exe_release(), &self_path));

        assert_eq!(std::fs::read(&self_path).unwrap(), b"new exe");
        assert_eq!(
            std::fs::read(vendor_bin.join("naner.exe.old")).unwrap(),
            b"old self"
        );
        assert!(!vendor_bin.join("naner.exe.staged").exists());
    }

    fn exe_release() -> GitHubRelease {
        release_with(vec![
            (
                "naner.exe",
                "https://api.github.com/repos/x/y/releases/assets/2",
            ),
            sums_asset(),
        ])
    }

    /// A fake running binary on disk at the root, standing in for
    /// current_exe() -- the single binary a user downloaded and ran.
    fn staged_self(root: &Path) -> std::path::PathBuf {
        let path = root.join(constants::executables::NANER);
        std::fs::write(&path, b"old self").unwrap();
        path
    }

    /// Stage an installed naner.exe so a refused update is visibly a refusal
    /// rather than a no-op on an empty tree.
    fn root_with_installed_exe() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("vendor/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(constants::executables::NANER), b"original exe").unwrap();
        root
    }

    #[test]
    fn tampered_naner_exe_is_refused_and_the_installed_binary_survives() {
        let root = root_with_installed_exe();
        let api = StubApi {
            release: Some(exe_release()),
            // Served bytes differ from what the manifest attests.
            asset_bytes: Some(b"trojan".to_vec()),
            sums: Some(sums_for(constants::executables::NANER, b"new exe")),
        };
        let updater = NanerUpdater::new(root.path(), &api);

        let self_path = staged_self(root.path());
        assert!(
            !updater.update_from_release(&exe_release(), &self_path),
            "mismatch must abort the update"
        );
        assert_eq!(
            std::fs::read(&self_path).unwrap(),
            b"old self",
            "the running binary must survive a refused update"
        );
        let installed = root
            .path()
            .join("vendor/bin")
            .join(constants::executables::NANER);
        assert_eq!(
            std::fs::read(&installed).unwrap(),
            b"original exe",
            "the existing binary must not be replaced or deleted"
        );
        // The rejected download must not be left lying around.
        assert!(
            !root
                .path()
                .join("vendor/bin")
                .join(format!("{}.tmp", constants::executables::NANER))
                .exists()
        );
    }

    #[test]
    fn release_without_a_manifest_is_refused() {
        let root = root_with_installed_exe();
        let api = StubApi {
            // No SHA256SUMS asset in the release at all.
            release: Some(release_with(vec![(
                "naner.exe",
                "https://api.github.com/repos/x/y/releases/assets/2",
            )])),
            asset_bytes: Some(b"new exe".to_vec()),
            sums: None,
        };
        let updater = NanerUpdater::new(root.path(), &api);
        let no_manifest = release_with(vec![(
            "naner.exe",
            "https://api.github.com/repos/x/y/releases/assets/2",
        )]);
        assert!(!updater.update_from_release(&no_manifest, &staged_self(root.path())));
        assert_eq!(
            std::fs::read(
                root.path()
                    .join("vendor/bin")
                    .join(constants::executables::NANER)
            )
            .unwrap(),
            b"original exe"
        );
    }

    #[test]
    fn asset_absent_from_the_manifest_is_refused() {
        let root = root_with_installed_exe();
        let api = StubApi {
            release: Some(exe_release()),
            asset_bytes: Some(b"new exe".to_vec()),
            // Manifest is well-formed but lists a different file.
            sums: Some(sums_for("something-else.zip", b"new exe")),
        };
        let updater = NanerUpdater::new(root.path(), &api);
        assert!(!updater.update_from_release(&exe_release(), &staged_self(root.path())));
    }

    #[test]
    fn tampered_bundle_is_refused_and_leaves_the_root_uninitialized() {
        let root = tempfile::tempdir().unwrap();
        let api = StubApi {
            release: Some(release_with(vec![
                (
                    "naner-bundle.zip",
                    "https://api.github.com/repos/x/y/releases/assets/1",
                ),
                sums_asset(),
            ])),
            asset_bytes: Some(bundle_zip()),
            sums: Some(sums_for(NANER_BUNDLE_NAME, b"a different bundle")),
        };
        let updater = NanerUpdater::new(root.path(), &api);

        assert!(!updater.initialize());
        assert!(!updater.is_initialized());
        // Nothing extracted from an unverified archive.
        assert!(!root.path().join("vendor/bin/naner.exe").exists());
        assert!(
            !root
                .path()
                .join(NANER_BUNDLE_NAME.to_owned() + ".tmp")
                .exists()
        );
    }

    #[test]
    fn manifest_lookup_is_name_exact_and_tolerates_binary_mode() {
        let sums = "\
aa11111111111111111111111111111111111111111111111111111111111111  naner.exe
bb22222222222222222222222222222222222222222222222222222222222222 *naner-bundle.zip
";
        assert_eq!(
            sha256_for(sums, "naner.exe").as_deref(),
            Some("aa11111111111111111111111111111111111111111111111111111111111111")
        );
        // Binary-mode `*` prefix is stripped.
        assert_eq!(
            sha256_for(sums, "naner-bundle.zip").as_deref(),
            Some("bb22222222222222222222222222222222222222222222222222222222222222")
        );
        // A name that merely appears in the file must not match another row.
        assert_eq!(sha256_for(sums, "naner"), None);
        assert_eq!(sha256_for(sums, "naner-init.exe"), None);
    }

    #[test]
    fn installed_version_fallbacks() {
        let root = tempfile::tempdir().unwrap();
        let api = StubApi {
            release: None,
            asset_bytes: None,
            sums: None,
        };
        let updater = NanerUpdater::new(root.path(), &api);

        // Nothing installed.
        assert_eq!(updater.installed_version(), None);
        let (needed, v) = updater.check_for_update();
        assert!(needed);
        assert_eq!(v.as_deref(), Some(constants::VERSION));

        // Exe without version file -> "0.0.0" (C# FileVersionInfo fallback).
        std::fs::create_dir_all(root.path().join("vendor/bin")).unwrap();
        std::fs::write(root.path().join("vendor/bin/naner.exe"), "x").unwrap();
        assert_eq!(updater.installed_version().as_deref(), Some("0.0.0"));
    }

    #[test]
    fn failed_download_aborts_update() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("vendor/bin")).unwrap();
        let api = StubApi {
            release: Some(release_with(vec![(
                "naner.exe",
                "https://api.github.com/repos/x/y/releases/assets/2",
            )])),
            asset_bytes: None, // download fails
            sums: None,
        };
        let updater = NanerUpdater::new(root.path(), &api);
        assert!(!updater.update_from_release(&exe_release(), &staged_self(root.path())));
    }
}
