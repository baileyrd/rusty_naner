//! Port of `GitHubReleasesClient` (Naner.Init): release-by-tag (the update
//! protocol's primary lookup), latest-release (with the all-releases
//! fallback), and asset download with the octet-stream Accept swap for
//! `api.github.com` URLs. `GITHUB_TOKEN` is honored as a Bearer token.

use std::io::{Read, Write};
use std::path::Path;

use serde::Deserialize;

use crate::{constants, logger};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    pub assets: Option<Vec<GitHubAsset>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct GitHubAsset {
    pub name: Option<String>,
    /// API URL — needs `Accept: application/octet-stream` to download.
    pub url: Option<String>,
    pub browser_download_url: Option<String>,
    #[serde(default)]
    pub size: i64,
}

/// Seam for updater tests (MIGRATION_ANALYSIS §2.2).
pub trait ReleasesApi {
    fn get_latest_release(&self) -> Option<GitHubRelease>;
    fn get_release_by_tag(&self, tag_name: &str) -> Option<GitHubRelease>;
    fn download_asset(&self, download_url: &str, output_path: &Path, asset_name: &str) -> bool;
}

pub struct GitHubReleasesClient {
    agent: ureq::Agent,
    owner: String,
    repo: String,
    token: Option<String>,
}

impl GitHubReleasesClient {
    pub fn new(owner: &str, repo: &str) -> Self {
        let tls = native_tls::TlsConnector::new().expect("failed to initialize TLS");
        let agent = ureq::AgentBuilder::new()
            .tls_connector(std::sync::Arc::new(tls))
            .timeout(std::time::Duration::from_secs(
                constants::DEFAULT_HTTP_TIMEOUT_MINUTES * 60,
            ))
            .user_agent(&constants::default_user_agent())
            .build();
        Self {
            agent,
            owner: owner.to_string(),
            repo: repo.to_string(),
            token: std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty()),
        }
    }

    fn request(&self, url: &str, accept: &str) -> ureq::Request {
        let mut req = self
            .agent
            .get(url)
            .set("Accept", accept)
            .set("Cache-Control", "no-cache");
        if let Some(token) = &self.token {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
        req
    }

    fn get_json(&self, url: &str) -> Result<(u16, String), String> {
        match self.request(url, "application/vnd.github+json").call() {
            Ok(response) => {
                let status = response.status();
                let body = response.into_string().map_err(|e| e.to_string())?;
                Ok((status, body))
            }
            Err(ureq::Error::Status(status, response)) => {
                Ok((status, response.into_string().unwrap_or_default()))
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

impl ReleasesApi for GitHubReleasesClient {
    /// `GetLatestReleaseAsync`: `/releases/latest`, falling back to the full
    /// `/releases` list (which includes prereleases) on failure.
    fn get_latest_release(&self) -> Option<GitHubRelease> {
        let result = (|| -> Result<Option<GitHubRelease>, String> {
            let url = format!(
                "https://api.github.com/repos/{}/{}/releases/latest",
                self.owner, self.repo
            );
            let (status, body) = self.get_json(&url)?;
            if (200..300).contains(&status)
                && let Ok(release) = serde_json::from_str::<GitHubRelease>(&body)
            {
                return Ok(Some(release));
            }

            let url = format!(
                "https://api.github.com/repos/{}/{}/releases",
                self.owner, self.repo
            );
            let (status, body) = self.get_json(&url)?;
            if !(200..300).contains(&status) {
                logger::warning(&format!("Failed to fetch latest release: {status}"));
                return Ok(None);
            }
            let releases: Vec<GitHubRelease> =
                serde_json::from_str(&body).map_err(|e| e.to_string())?;
            Ok(releases.into_iter().next())
        })();

        match result {
            Ok(release) => release,
            Err(e) => {
                logger::failure(&format!("Error fetching latest release: {e}"));
                None
            }
        }
    }

    /// `GetReleaseByTagAsync`: tag gets a `v` prefix if missing.
    fn get_release_by_tag(&self, tag_name: &str) -> Option<GitHubRelease> {
        let tag = if tag_name.to_lowercase().starts_with('v') {
            tag_name.to_string()
        } else {
            format!("v{tag_name}")
        };

        let url = format!(
            "https://api.github.com/repos/{}/{}/releases/tags/{tag}",
            self.owner, self.repo
        );
        match self.get_json(&url) {
            Ok((status, body)) if (200..300).contains(&status) => serde_json::from_str(&body).ok(),
            Ok((status, _)) => {
                logger::warning(&format!(
                    "Failed to fetch release for tag '{tag}': {status}"
                ));
                None
            }
            Err(e) => {
                logger::failure(&format!("Error fetching release for tag '{tag_name}': {e}"));
                None
            }
        }
    }

    /// `DownloadAssetAsync`: octet-stream Accept for API URLs, and the
    /// init-flavored progress format (`\r  Progress: N%` — two spaces, vs
    /// the vendor pipeline's four).
    fn download_asset(&self, download_url: &str, output_path: &Path, asset_name: &str) -> bool {
        logger::status(&format!("Downloading {asset_name}..."));

        let accept = if download_url.starts_with("https://api.github.com/") {
            "application/octet-stream"
        } else {
            "application/vnd.github+json"
        };

        let response = match self.request(download_url, accept).call() {
            Ok(r) => r,
            Err(e) => {
                logger::failure(&format!("Download failed: {e}"));
                return false;
            }
        };

        let total_bytes: u64 = response
            .header("Content-Length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut reader = response.into_reader();
        let file = match std::fs::File::create(output_path) {
            Ok(f) => f,
            Err(e) => {
                logger::failure(&format!("Download failed: {e}"));
                return false;
            }
        };
        let mut writer = std::io::BufWriter::new(file);
        let mut buffer = vec![0u8; constants::HTTP_DOWNLOAD_BUFFER_SIZE];
        let mut total_read: u64 = 0;
        let mut last_percent: i64 = -1;

        loop {
            let n = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    logger::failure(&format!("Download failed: {e}"));
                    return false;
                }
            };
            if writer.write_all(&buffer[..n]).is_err() {
                logger::failure("Download failed: write error");
                return false;
            }
            total_read += n as u64;
            if let Some(pct) = (total_read * 100).checked_div(total_bytes) {
                let percent = pct as i64;
                if percent != last_percent
                    && percent % constants::PROGRESS_UPDATE_INTERVAL as i64 == 0
                {
                    print!("\r  Progress: {percent}%");
                    let _ = std::io::stdout().flush();
                    last_percent = percent;
                }
            }
        }
        if writer.flush().is_err() {
            logger::failure("Download failed: flush error");
            return false;
        }
        if total_bytes > 0 {
            print!("\r  Progress: 100%");
            println!();
        }

        logger::success(&format!("Downloaded {asset_name}"));
        true
    }
}
