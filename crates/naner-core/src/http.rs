//! Port of `HttpClientWrapper` + the download loop from
//! `VendorInstallerBase`: one blocking client, 10-minute timeout,
//! `Naner/{version}` user agent, and — as in C# — a default
//! `Accept: application/vnd.github+json` header on every request.

use std::io::{Read, Write};
use std::path::Path;

use crate::{constants, logger};

/// Minimal HTTP seam so the installer can be tested against a stub
/// (MIGRATION_ANALYSIS §2.2: traits only where tests need substitution).
pub trait Http {
    /// GET a URL; `Ok` carries (status, body-as-text). Transport errors are
    /// `Err`; non-2xx statuses are `Ok` with the status code (like
    /// `HttpResponseMessage.IsSuccessStatusCode` checks).
    fn get_text(&self, url: &str) -> Result<(u16, String), String>;

    /// Download a URL to a file with the C# progress format
    /// (`\r    Progress: N%` every 10%). Returns false and logs on failure.
    fn download(&self, url: &str, output_path: &Path) -> bool;
}

pub struct UreqHttp {
    agent: ureq::Agent,
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqHttp {
    pub fn new() -> Self {
        // native-tls: schannel on Windows, OpenSSL elsewhere. A connector
        // build failure would mean a broken TLS stack; surface it loudly.
        let tls = native_tls::TlsConnector::new().expect("failed to initialize TLS");
        let mut builder = ureq::AgentBuilder::new()
            .tls_connector(std::sync::Arc::new(tls))
            .timeout(std::time::Duration::from_secs(
                constants::DEFAULT_HTTP_TIMEOUT_MINUTES * 60,
            ))
            .user_agent(&constants::default_user_agent());

        let proxy_url = std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("http_proxy"))
            .ok();

        if let Some(proxy_str) = proxy_url {
            if let Ok(proxy) = ureq::Proxy::new(&proxy_str) {
                builder = builder.proxy(proxy);
            }
        }

        let agent = builder.build();
        Self { agent }
    }

    fn request(&self, url: &str) -> ureq::Request {
        self.agent
            .get(url)
            .set("Accept", "application/vnd.github+json")
    }
}

impl Http for UreqHttp {
    fn get_text(&self, url: &str) -> Result<(u16, String), String> {
        match self.request(url).call() {
            Ok(response) => {
                let status = response.status();
                let body = response.into_string().map_err(|e| e.to_string())?;
                Ok((status, body))
            }
            // ureq treats 4xx/5xx as Err(Status); normalize to Ok(status, body)
            // so callers branch on status exactly like the C# code.
            Err(ureq::Error::Status(status, response)) => {
                let body = response.into_string().unwrap_or_default();
                Ok((status, body))
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn download(&self, url: &str, output_path: &Path) -> bool {
        if output_path.is_file() && output_path.metadata().map_or(0, |m| m.len()) > 0 {
            logger::info(&format!("    Using cached download asset: {}", output_path.display()));
            return true;
        }
        match self.request(url).call() {
            Ok(response) => {
                let total_bytes: u64 = response
                    .header("Content-Length")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let mut reader = response.into_reader();
                let file = match std::fs::File::create(output_path) {
                    Ok(f) => f,
                    Err(e) => {
                        logger::failure(&format!("    Download error: {e}"));
                        return false;
                    }
                };
                let mut writer = std::io::BufWriter::new(file);
                let mut buffer = vec![0u8; constants::HTTP_DOWNLOAD_BUFFER_SIZE];
                let mut total_read: u64 = 0;
                let mut last_percent: i64 = -1;

                loop {
                    let bytes_read = match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) => {
                            logger::failure(&format!("    Download error: {e}"));
                            return false;
                        }
                    };
                    if writer.write_all(&buffer[..bytes_read]).is_err() {
                        logger::failure("    Download error: write failed");
                        return false;
                    }
                    total_read += bytes_read as u64;

                    if let Some(pct) = (total_read * 100).checked_div(total_bytes) {
                        let percent = pct as i64;
                        if percent != last_percent
                            && percent % constants::PROGRESS_UPDATE_INTERVAL as i64 == 0
                        {
                            print!("\r    Progress: {percent}%");
                            let _ = std::io::stdout().flush();
                            last_percent = percent;
                        }
                    }
                }
                if writer.flush().is_err() {
                    logger::failure("    Download error: flush failed");
                    return false;
                }
                if total_bytes > 0 {
                    print!("\r    Progress: 100%");
                    println!();
                }
                true
            }
            Err(e) => {
                logger::failure(&format!("    Download error: {e}"));
                false
            }
        }
    }
}
