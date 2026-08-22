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
        Self {
            agent: build_agent(),
        }
    }

    /// One shared client resolves every vendor source -- GitHub releases,
    /// npm/PyPI registries, static JSON feeds (nodejs.org, go.dev, dotnet's
    /// channel manifest), and HTML scrapes. Reported live: `naner install
    /// codex` (npm) failed with a bare "Failed to fetch Codex CLI,
    /// skipping..." and no detail -- `registry.npmjs.org` was returning 406
    /// Not Acceptable. This client sent `Accept: application/vnd.github+json`
    /// unconditionally, correct for GitHub's REST API (the only caller that
    /// actually needs a non-default Accept) but nonsensical everywhere else;
    /// npmjs.org's Fastly frontend does real content negotiation on it and
    /// rejects a media type it doesn't recognize, where every other resolver
    /// happened to ignore it and return normal JSON regardless.
    fn request(&self, url: &str) -> ureq::Request {
        let accept = if url.starts_with("https://api.github.com/") {
            "application/vnd.github+json"
        } else {
            "application/json"
        };
        self.agent.get(url).set("Accept", accept)
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
        // A dropped connection mid-transfer is a single `download_once`
        // failure, not a reason to give up outright -- the larger the file,
        // the longer it holds a connection open and the likelier some
        // network hop in between resets it before the last byte arrives.
        // Observed for real on Anaconda (~1 GB, by far the biggest artifact
        // naner downloads): "response body closed before all bytes were
        // read" at 60% with no retry at all, and `static`-type vendors like
        // it have no separate fallback URL the way `github`-type ones do,
        // so that one failure was the whole install failing.
        const MAX_ATTEMPTS: u32 = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            if self.download_once(url, output_path) {
                return true;
            }
            if attempt < MAX_ATTEMPTS {
                logger::warning(&format!(
                    "    Retrying download (attempt {}/{})...",
                    attempt + 1,
                    MAX_ATTEMPTS
                ));
                std::thread::sleep(std::time::Duration::from_secs(2 * u64::from(attempt)));
            }
        }
        false
    }
}

impl UreqHttp {
    fn download_once(&self, url: &str, output_path: &Path) -> bool {
        // Stream into `<name>.part` and publish with a rename only once the
        // transfer is known complete. Writing straight to `output_path` meant
        // an interrupted process — Ctrl-C, a crash, a lost machine — left a
        // truncated file exactly where the next run looks for a finished one.
        // Deleting on the error paths cannot cover that case, because nothing
        // gets to run. Staging makes it safe by construction: a file at
        // `output_path` was renamed there after a verified-complete transfer.
        let part_path = partial_path(output_path);
        let _ = std::fs::remove_file(&part_path); // stale one from a previous run

        match self.request(url).call() {
            Ok(response) => {
                let total_bytes: u64 = response
                    .header("Content-Length")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let mut reader = response.into_reader();
                let file = match std::fs::File::create(&part_path) {
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
                            return discard_partial(writer, &part_path);
                        }
                    };
                    if writer.write_all(&buffer[..bytes_read]).is_err() {
                        logger::failure("    Download error: write failed");
                        return discard_partial(writer, &part_path);
                    }
                    total_read += bytes_read as u64;

                    if let Some(pct) = (total_read * 100).checked_div(total_bytes) {
                        let percent = pct as i64;
                        if percent != last_percent
                            && percent % constants::PROGRESS_UPDATE_INTERVAL as i64 == 0
                            && !logger::quiet()
                        {
                            print!("\r    Progress: {percent}%");
                            let _ = std::io::stdout().flush();
                            last_percent = percent;
                        }
                    }
                }
                if writer.flush().is_err() {
                    logger::failure("    Download error: flush failed");
                    return discard_partial(writer, &part_path);
                }
                // A truncated transfer that ends cleanly is otherwise
                // indistinguishable from a complete one, and the partial file
                // would be picked up as a cache hit on the next run.
                if total_bytes > 0 && total_read != total_bytes {
                    logger::failure(&format!(
                        "    Download error: expected {total_bytes} bytes, received {total_read}"
                    ));
                    return discard_partial(writer, &part_path);
                }
                if total_bytes > 0 && !logger::quiet() {
                    print!("\r    Progress: 100%");
                    println!();
                }

                // Publish. Only now is the artifact allowed to exist under the
                // name the cache probe trusts.
                drop(writer);
                if let Err(e) = std::fs::rename(&part_path, output_path) {
                    logger::failure(&format!("    Download error: could not finalize: {e}"));
                    let _ = std::fs::remove_file(&part_path);
                    return false;
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

/// The one place an HTTP agent is configured.
///
/// Both callers — the vendor pipeline here and the releases client in
/// `github` — need the same TLS stack, timeout, user agent and proxy. They
/// used to build their own, and only this one read the proxy variables, so a
/// user behind a corporate proxy could install vendors but could not bootstrap
/// or update: `naner-init` failed with a bare "Failed to fetch release".
/// Owning the configuration once is also what `ATLAS-BOUND-0001` asks for —
/// one component responsible for translating across the outbound-HTTP
/// boundary.
pub(crate) fn build_agent() -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(
            constants::DEFAULT_HTTP_TIMEOUT_MINUTES * 60,
        ))
        .user_agent(&constants::default_user_agent());

    // native-tls: schannel on Windows, OpenSSL elsewhere. A connector that
    // will not build means a broken TLS stack; say so and fall back to ureq's
    // own default rather than aborting the process — the launcher builds with
    // `panic = "abort"`, so an expect() here would take the whole run down
    // with no message on a GUI launch.
    match native_tls::TlsConnector::new() {
        Ok(tls) => builder = builder.tls_connector(std::sync::Arc::new(tls)),
        Err(e) => logger::warning(&format!("TLS init failed ({e}); using default TLS")),
    }

    if let Some(proxy) = configured_proxy() {
        builder = builder.proxy(proxy);
    }
    builder.build()
}

/// Proxy from the environment, honouring `NO_PROXY` as a blanket opt-out.
///
/// Both spellings of each variable are read because callers set them
/// inconsistently and naner cannot control which one a given shell exports.
fn configured_proxy() -> Option<ureq::Proxy> {
    let no_proxy = std::env::var("NO_PROXY")
        .or_else(|_| std::env::var("no_proxy"))
        .unwrap_or_default();
    if no_proxy.trim() == "*" {
        return None;
    }

    let url = ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .filter(|v| !v.trim().is_empty())?;

    match ureq::Proxy::new(&url) {
        Ok(proxy) => Some(proxy),
        Err(e) => {
            logger::warning(&format!("Ignoring unusable proxy setting {url:?}: {e}"));
            None
        }
    }
}

/// Staging name for an in-flight download: `foo.zip` -> `foo.zip.part`.
///
/// A suffix rather than `with_extension`, which would turn `foo.tar.xz` into
/// `foo.tar.part` and collide with the intermediate `.tar` the xz extractor
/// writes beside it.
pub fn partial_path(output_path: &Path) -> std::path::PathBuf {
    let mut name = output_path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    output_path.with_file_name(name)
}

/// Drop a partially-written download and report failure.
///
/// Always returns `false` so callers can `return discard_partial(..)`.
fn discard_partial(writer: std::io::BufWriter<std::fs::File>, output_path: &Path) -> bool {
    // Takes the writer by value so the handle is closed before the unlink:
    // Windows refuses to remove a file that is still open.
    drop(writer);
    let _ = std::fs::remove_file(output_path);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_name_appends_rather_than_replacing_the_extension() {
        assert_eq!(
            partial_path(Path::new("/d/node.zip")),
            Path::new("/d/node.zip.part")
        );
        // `with_extension` would yield `msys2-base.tar.part` here, colliding
        // with the intermediate `.tar` the xz extractor writes into the same
        // folder.
        assert_eq!(
            partial_path(Path::new("/d/msys2-base.tar.xz")),
            Path::new("/d/msys2-base.tar.xz.part")
        );
        // No extension at all.
        assert_eq!(
            partial_path(Path::new("/d/installer")),
            Path::new("/d/installer.part")
        );
    }

    /// Proves the staging discipline against a real transfer: the artifact
    /// lands under its final name and no `.part` survives. Excluded from CI —
    /// run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "hits the network"]
    fn a_real_download_publishes_by_rename_and_leaves_no_part_file() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("SHASUMS256.txt");

        let http = UreqHttp::new();
        assert!(http.download("https://nodejs.org/dist/index.json", &out));

        assert!(out.is_file(), "artifact published under its final name");
        assert!(out.metadata().unwrap().len() > 0);
        assert!(
            !partial_path(&out).exists(),
            "staging file must not survive a successful download"
        );
    }

    #[test]
    #[ignore = "hits the network"]
    fn a_failed_download_leaves_neither_the_artifact_nor_a_part_file() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("nope.bin");

        let http = UreqHttp::new();
        assert!(!http.download("https://nodejs.org/dist/definitely-not-here", &out));

        assert!(!out.exists(), "nothing may be left under the final name");
        assert!(!partial_path(&out).exists());
    }

    /// The regression this was built for: a connection that dies mid-body
    /// (Anaconda, ~1 GB, real-world observed) must not fail the whole
    /// install on the first bad connection. A tiny local server plays the
    /// truncated-then-complete server so this runs offline and
    /// deterministically, unlike the network-hitting tests above.
    #[test]
    fn a_connection_dropped_mid_body_is_retried_and_then_succeeds() {
        use std::net::TcpListener;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let connections_srv = Arc::clone(&connections);

        let body = b"the complete payload, present in full on this attempt".to_vec();
        let full_len = body.len();
        let server_body = body.clone();

        let server = std::thread::spawn(move || {
            let body = server_body;
            // First connection: truncated. Second: complete. A third
            // `accept()` would hang forever if the client retried more than
            // once, which is exactly why the test asserts the exact count.
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                connections_srv.fetch_add(1, Ordering::SeqCst);

                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {full_len}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(header.as_bytes());
                if attempt == 0 {
                    let _ = stream.write_all(&body[..full_len / 2]);
                    // Dropping here closes the socket before the promised
                    // Content-Length is satisfied -- the truncation itself.
                } else {
                    let _ = stream.write_all(&body);
                }
            }
        });

        // Not `UreqHttp::new()`: this sandbox's own outbound-proxy env vars
        // only get bypassed by `configured_proxy()` for the literal
        // `NO_PROXY=*` wildcard, not a specific-host list that includes
        // 127.0.0.1 -- routing this local test server through a real proxy
        // that has never heard of it is a proxy-config question, not what
        // this test is about. Build a bare agent directly instead.
        let http = UreqHttp {
            agent: ureq::AgentBuilder::new().build(),
        };
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("payload.bin");

        assert!(http.download(&format!("http://{addr}/file"), &out));
        assert_eq!(std::fs::read(&out).unwrap(), body);
        assert_eq!(
            connections.load(Ordering::SeqCst),
            2,
            "must retry exactly once after the truncated first attempt"
        );
        assert!(!partial_path(&out).exists());

        server.join().unwrap();
    }
}
