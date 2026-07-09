//! Port of `NanerUpdater` (Naner.Init). The update model is
//! sync-to-embedded-version, NOT install-latest: this binary fetches the
//! GitHub release whose tag matches its own compile-time version and makes
//! the installed `naner.exe`/`.naner-version` match it — a string-inequality
//! check that will happily "downgrade" (MIGRATION_ANALYSIS §1.3).

use std::path::{Path, PathBuf};

use naner_core::github::ReleasesApi;
use naner_core::{archives, constants, logger, version};

const NANER_BUNDLE_NAME: &str = "naner-bundle.zip";

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

    /// `CheckForUpdateAsync`: normalized string inequality against the
    /// embedded version (bugs B5/B6 live here — preserved).
    pub fn check_for_update(&self) -> (bool, Option<String>) {
        let Some(installed) = self.installed_version() else {
            return (true, Some(self.init_version.clone()));
        };
        let update_needed =
            version::normalize(&installed) != version::normalize(&self.init_version);
        (
            update_needed,
            update_needed.then(|| self.init_version.clone()),
        )
    }

    /// `UpdateNanerExeAsync`: fetch release by embedded tag, download the
    /// `naner.exe` asset over the API URL, swap it in, write the tag to
    /// `.naner-version` (tag form — the canonical choice for B6).
    pub fn update_naner_exe(&self) -> bool {
        logger::header("Updating Naner");
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
                    .is_some_and(|n| n.eq_ignore_ascii_case(constants::executables::NANER))
            })
        });
        let Some(asset) = asset else {
            logger::failure(&format!(
                "{} not found in release assets",
                constants::executables::NANER
            ));
            return false;
        };

        let download_url = asset
            .url
            .as_deref()
            .or(asset.browser_download_url.as_deref());
        let Some(download_url) = download_url.filter(|u| !u.is_empty()) else {
            logger::failure(&format!(
                "Download URL for {} is missing",
                constants::executables::NANER
            ));
            return false;
        };

        let naner_path = self.vendor_bin_dir.join(constants::executables::NANER);
        let temp_path = self
            .vendor_bin_dir
            .join(format!("{}.tmp", constants::executables::NANER));

        if !self
            .github
            .download_asset(download_url, &temp_path, constants::executables::NANER)
        {
            return false;
        }

        if naner_path.is_file()
            && let Err(e) = std::fs::remove_file(&naner_path)
        {
            logger::warning(&format!("Could not delete old naner.exe: {e}"));
            logger::info("Will attempt to overwrite...");
        }
        if let Err(e) = std::fs::rename(&temp_path, &naner_path) {
            logger::failure(&format!("Update failed: {e}"));
            return false;
        }
        logger::success(&format!("Installed {}", constants::executables::NANER));

        if let Err(e) = std::fs::write(self.vendor_bin_dir.join(constants::VERSION_FILE), &tag) {
            logger::failure(&format!("Update failed: {e}"));
            return false;
        }

        logger::newline();
        logger::success(&format!("Naner updated to version {tag}"));
        true
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
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
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
            logger::info("Run 'naner-init' to install Naner first");
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

#[cfg(test)]
mod tests {
    use super::*;
    use naner_core::github::{GitHubAsset, GitHubRelease};
    use std::io::Write;

    struct StubApi {
        release: Option<GitHubRelease>,
        /// Bytes served for any download_asset call; None = download fails.
        asset_bytes: Option<Vec<u8>>,
    }

    impl ReleasesApi for StubApi {
        fn get_latest_release(&self) -> Option<GitHubRelease> {
            self.release.clone()
        }
        fn get_release_by_tag(&self, _tag: &str) -> Option<GitHubRelease> {
            self.release.clone()
        }
        fn download_asset(&self, _url: &str, output_path: &Path, _name: &str) -> bool {
            match &self.asset_bytes {
                Some(bytes) => {
                    std::fs::File::create(output_path)
                        .unwrap()
                        .write_all(bytes)
                        .unwrap();
                    true
                }
                None => false,
            }
        }
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
            release: Some(release_with(vec![(
                "naner-bundle.zip",
                "https://api.github.com/repos/x/y/releases/assets/1",
            )])),
            asset_bytes: Some(bundle_zip()),
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
            release: Some(release_with(vec![(
                "naner.exe",
                "https://api.github.com/repos/x/y/releases/assets/2",
            )])),
            asset_bytes: Some(b"new exe".to_vec()),
        };
        let updater = NanerUpdater::new(root.path(), &api);

        // Sync check: installed v0.0.1 != embedded version -> update needed.
        let (update_needed, target) = updater.check_for_update();
        assert!(update_needed);
        assert_eq!(target.as_deref(), Some(constants::VERSION));

        assert!(updater.update_naner_exe());
        assert_eq!(
            std::fs::read_to_string(vendor_bin.join("naner.exe")).unwrap(),
            "new exe"
        );
        assert_eq!(
            std::fs::read_to_string(vendor_bin.join(".naner-version")).unwrap(),
            format!("v{}", constants::VERSION)
        );
        assert!(!vendor_bin.join("naner.exe.tmp").exists());
    }

    #[test]
    fn installed_version_fallbacks() {
        let root = tempfile::tempdir().unwrap();
        let api = StubApi {
            release: None,
            asset_bytes: None,
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
        };
        let updater = NanerUpdater::new(root.path(), &api);
        assert!(!updater.update_naner_exe());
    }
}
