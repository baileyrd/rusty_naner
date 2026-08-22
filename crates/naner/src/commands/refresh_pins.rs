//! Command: `naner refresh-pins [dir] [--dry-run] [--porcelain]`
//!
//! Re-resolves what upstream currently calls latest for every vendor with a
//! dynamic release source and rewrites the hardcoded `fallback` pin
//! (`version`/`url`/`fileName`) in that vendor's `config/vendors/<Key>.json`.
//! The pins exist for installs whose dynamic resolution fails — but nothing
//! ever refreshed them, so they rot until a degraded install silently gets a
//! years-old version or a 404. `[dir]` points at a vendor-definitions
//! directory explicitly (this repo's own `dist-assets/config/vendors` when
//! run from a checkout); default is the discovered root's `config/vendors`.
//!
//! Also rewrites a vendor's static top-level `checksum`, when GitHub's
//! release API publishes a `digest` for the resolved asset. Reported live:
//! `naner install zed` failed checksum verification on a `checksum` value
//! that had simply gone stale — nothing had ever refreshed it, and the
//! `fallback.version` pin already matched latest, so the version-drift check
//! below wouldn't have caught it either. GitHub's digest is authoritative
//! for the exact resolved artifact, so it wins here the way an operator's
//! pin wins over it at install time (`resolved_checksum`) -- refreshing
//! *is* the operator re-asserting the pin, not upstream overriding it.
//! Vendors with no static `checksum` are left alone; this never adds one
//! that wasn't already there.
//!
//! Static-URL vendors have no "latest" to resolve — their pinned version IS
//! the install — so they are reported as manual-only, never rewritten.

use std::path::{Path, PathBuf};

use naner_core::config::strip_json_comments;
use naner_core::http::UreqHttp;
use naner_core::{constants, logger, paths, vendors, version};
use serde_json::json;

pub fn execute(args: &[String]) -> i32 {
    let dry_run = args.iter().any(|a| a.eq_ignore_ascii_case("--dry-run"));
    let porcelain = args
        .iter()
        .any(|a| a.eq_ignore_ascii_case("--porcelain") || a.eq_ignore_ascii_case("-p"));

    let vendors_dir = match args.iter().find(|a| !a.starts_with('-')) {
        Some(dir) => PathBuf::from(dir),
        None => match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
            Ok(root) => root
                .join(constants::directory_names::CONFIG)
                .join(constants::VENDORS_CONFIG_DIR_NAME),
            Err(err) => {
                logger::failure(
                    "Could not locate a Naner root; pass the vendor-definitions \
                         directory explicitly: naner refresh-pins <dir>",
                );
                println!("{}", err.message);
                return 1;
            }
        },
    };
    if !vendors_dir.is_dir() {
        logger::failure(&format!("Not a directory: {}", vendors_dir.display()));
        return 1;
    }

    let loader = vendors::VendorConfigurationLoader::from_vendors_dir(&vendors_dir);
    // Disabled vendors keep their pins too — a pin's staleness has nothing to
    // do with whether this install switched the vendor on.
    let all = loader.load_all_vendors();
    let http = UreqHttp::new();
    let resolver = vendors::UnifiedVendorInstaller::new(
        vendors_dir.parent().unwrap_or(&vendors_dir),
        Vec::new(),
        &http,
    );

    let mut rows = Vec::new();
    let mut updated = 0usize;
    let mut failed = 0usize;

    for vendor in &all {
        let row = refresh_one(&vendors_dir, vendor, &resolver, dry_run);
        match row.state.as_str() {
            "updated" => updated += 1,
            "failed" => failed += 1,
            _ => {}
        }
        if !porcelain {
            let line = match (row.pinned.as_deref(), row.latest.as_deref()) {
                (Some(old), Some(new)) if row.state == "updated" => {
                    format!("{}: {} {old} -> {new}{}", row.state, row.vendor, row.note)
                }
                _ => format!("{}: {}{}", row.state, row.vendor, row.note),
            };
            match row.state.as_str() {
                "failed" => logger::warning(&line),
                "updated" => logger::success(&line),
                _ => logger::info(&line),
            }
        }
        rows.push(row);
    }

    if porcelain {
        let out = json!({
            "dry_run": dry_run,
            "vendors_dir": vendors_dir.display().to_string(),
            "vendors": rows.iter().map(|r| json!({
                "vendor": r.vendor,
                "state": r.state,
                "pinned": r.pinned,
                "latest": r.latest,
                "major": r.major,
            })).collect::<Vec<_>>(),
        });
        println!("{out}");
    } else {
        logger::newline();
        let verb = if dry_run { "would update" } else { "updated" };
        logger::status(&format!(
            "{updated} pin(s) {verb}, {failed} failed, {} total",
            rows.len()
        ));
    }
    // Failures are the whole point of running this: a pin that cannot be
    // checked is a pin that cannot be trusted.
    i32::from(failed > 0)
}

struct Row {
    vendor: String,
    state: String,
    pinned: Option<String>,
    latest: Option<String>,
    major: bool,
    note: String,
}

fn refresh_one(
    vendors_dir: &Path,
    vendor: &vendors::VendorDefinition,
    resolver: &vendors::UnifiedVendorInstaller,
    dry_run: bool,
) -> Row {
    let mut row = Row {
        vendor: vendor.key.clone(),
        state: String::new(),
        pinned: vendor.fallback_version.clone(),
        latest: None,
        major: false,
        note: String::new(),
    };

    if vendor.source_type == vendors::VendorSourceType::StaticUrl {
        row.state = "manual".into();
        row.note = " (static URL - the pin IS the install; bump it by hand)".into();
        return row;
    }
    if vendor.fallback_url.is_none() {
        row.state = "no-pin".into();
        row.note = " (no fallback block to refresh)".into();
        return row;
    }

    let info = match resolver.resolve_upstream(vendor) {
        Ok(Some(info)) => info,
        Ok(None) => {
            row.state = "failed".into();
            row.note = " (no matching release found upstream)".into();
            return row;
        }
        Err(e) => {
            row.state = "failed".into();
            row.note = format!(" ({e})");
            return row;
        }
    };
    let Some(latest_version) = info.version.clone() else {
        row.state = "failed".into();
        row.note = " (upstream reported no version)".into();
        return row;
    };
    row.latest = Some(latest_version.clone());
    if let Some(pinned) = &row.pinned {
        row.major = version::vendor_major_differs(pinned, &latest_version);
        if row.major {
            row.note = " (major)".into();
        }
    }

    // GitHub's own digest for the resolved asset (when it published one)
    // wins over a stale pin here -- refreshing the pin file *is* the
    // operator re-asserting it, the same authority `resolved_checksum` gives
    // a hand-edited `checksum` at install time.
    let checksum_update = vendor.checksum.as_ref().and_then(|pinned| {
        info.checksum
            .as_ref()
            .filter(|upstream| !pinned.value.eq_ignore_ascii_case(&upstream.value))
    });

    let version_current = vendor.fallback_url.as_deref() == Some(info.url.as_str())
        && row.pinned.as_deref() == Some(latest_version.as_str());

    if version_current && checksum_update.is_none() {
        row.state = "current".into();
        return row;
    }
    if checksum_update.is_some() {
        row.note = format!("{} (checksum)", row.note);
    }

    if dry_run {
        row.state = "updated".into();
        return row;
    }
    match rewrite_pin(
        &vendors_dir.join(format!("{}.json", vendor.key)),
        &vendor.key,
        &latest_version,
        &info.url,
        &info.file_name,
        checksum_update.map(|c| c.value.as_str()),
    ) {
        Ok(()) => row.state = "updated".into(),
        Err(e) => {
            row.state = "failed".into();
            row.note = format!(" ({e})");
        }
    }
    row
}

/// Rewrite one vendor file's `releaseSource.fallback` (always) and its
/// top-level `checksum.value` (only when `checksum` is `Some`, and only if
/// the vendor file already has a `checksum` object to update -- this never
/// adds one that wasn't already there) in place, preserving key order
/// (`serde_json` here carries `preserve_order`) and refusing files whose
/// comments a parse-and-reserialize round trip would silently delete.
fn rewrite_pin(
    path: &Path,
    key: &str,
    version: &str,
    url: &str,
    file_name: &str,
    checksum: Option<&str>,
) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if strip_json_comments(&text) != text {
        return Err("file contains comments a rewrite would delete; update it by hand".into());
    }
    let mut root: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let vendor_obj = root
        .get_mut(key)
        .ok_or_else(|| format!("no top-level \"{key}\" object in file"))?;

    let fallback = vendor_obj
        .get_mut("releaseSource")
        .and_then(|v| v.get_mut("fallback"))
        .and_then(|v| v.as_object_mut())
        .ok_or("no releaseSource.fallback object in file")?;
    fallback.insert("version".into(), json!(version));
    fallback.insert("url".into(), json!(url));
    fallback.insert("fileName".into(), json!(file_name));

    if let Some(digest) = checksum
        && let Some(checksum_obj) = vendor_obj.get_mut("checksum").and_then(|v| v.as_object_mut())
    {
        checksum_obj.insert("value".into(), json!(digest));
    }

    let pretty = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(path, pretty + "\n").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{
  "Tool": {
    "name": "Tool",
    "extractDir": "tool",
    "enabled": false,
    "required": false,
    "releaseSource": {
      "type": "github",
      "repo": "example/tool",
      "assetPattern": "tool-win-x64.zip",
      "fallback": {
        "version": "1.0.0",
        "url": "https://example.com/1.0.0/tool-win-x64.zip",
        "fileName": "tool-win-x64.zip",
        "size": "~10"
      }
    },
    "pathPrecedence": [
      "%NANER_ROOT%\\vendor\\tool"
    ]
  }
}
"#;

    #[test]
    fn rewrites_only_the_fallback_and_preserves_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Tool.json");
        std::fs::write(&path, FILE).unwrap();

        rewrite_pin(
            &path,
            "Tool",
            "2.1.0",
            "https://example.com/2.1.0/tool-win-x64.zip",
            "tool-win-x64.zip",
            None,
        )
        .unwrap();

        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("\"version\": \"2.1.0\""), "{out}");
        assert!(out.contains("2.1.0/tool-win-x64.zip"), "{out}");
        // Untouched fields survive, in place.
        assert!(out.contains("\"size\": \"~10\""), "{out}");
        assert!(
            out.contains("\"assetPattern\": \"tool-win-x64.zip\""),
            "{out}"
        );
        assert!(out.ends_with('\n'), "trailing newline preserved");
        // preserve_order: name still precedes extractDir, fallback still
        // inside releaseSource.
        let name = out.find("\"name\"").unwrap();
        let extract = out.find("\"extractDir\"").unwrap();
        assert!(name < extract, "key order drifted:\n{out}");

        // Round trip is still one vendor under the same key.
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            parsed["Tool"]["releaseSource"]["fallback"]["version"],
            "2.1.0"
        );
    }

    /// The bug this exists for: `naner install zed` failed checksum
    /// verification on a pin that had simply gone stale, with
    /// `fallback.version` already matching latest -- the version-drift path
    /// alone would never have caught it. A vendor with a `checksum` object
    /// gets its `value` rewritten too, without disturbing `algorithm` or
    /// `required`.
    #[test]
    fn a_stale_checksum_is_rewritten_alongside_the_fallback() {
        const WITH_CHECKSUM: &str = r#"{
  "Tool": {
    "name": "Tool",
    "extractDir": "tool",
    "enabled": true,
    "required": false,
    "releaseSource": {
      "type": "github",
      "repo": "example/tool",
      "assetPattern": "tool-win-x64.zip",
      "fallback": {
        "version": "1.0.0",
        "url": "https://example.com/1.0.0/tool-win-x64.zip",
        "fileName": "tool-win-x64.zip",
        "size": "~10"
      }
    },
    "checksum": {
      "algorithm": "SHA256",
      "value": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "required": true
    }
  }
}
"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Tool.json");
        std::fs::write(&path, WITH_CHECKSUM).unwrap();

        let fresh_digest = "b".repeat(64);
        rewrite_pin(
            &path,
            "Tool",
            "1.0.0",
            "https://example.com/1.0.0/tool-win-x64.zip",
            "tool-win-x64.zip",
            Some(&fresh_digest),
        )
        .unwrap();

        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed["Tool"]["checksum"]["value"], fresh_digest);
        assert_eq!(parsed["Tool"]["checksum"]["algorithm"], "SHA256");
        assert_eq!(parsed["Tool"]["checksum"]["required"], true);
    }

    /// A vendor with no `checksum` object never gets one added — refreshing
    /// only ever updates a pin that already opted in.
    #[test]
    fn no_checksum_object_is_never_added() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Tool.json");
        std::fs::write(&path, FILE).unwrap();

        let digest = "c".repeat(64);
        rewrite_pin(&path, "Tool", "1.0.0", "u", "f", Some(&digest)).unwrap();

        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(parsed["Tool"].get("checksum").is_none());
    }

    #[test]
    fn a_commented_file_is_refused_not_flattened() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Tool.json");
        std::fs::write(&path, format!("// hands off\n{FILE}")).unwrap();

        let err = rewrite_pin(&path, "Tool", "2.0.0", "u", "f", None).unwrap_err();
        assert!(err.contains("comments"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("// hands off\n{FILE}"),
            "file must be untouched"
        );
    }

    #[test]
    fn a_file_without_a_fallback_block_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Tool.json");
        std::fs::write(
            &path,
            r#"{ "Tool": { "name": "Tool", "releaseSource": { "type": "github" } } }"#,
        )
        .unwrap();
        assert!(rewrite_pin(&path, "Tool", "2.0.0", "u", "f", None).is_err());
    }
}
