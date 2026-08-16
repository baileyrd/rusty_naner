//! Command: `naner migrate [--dry-run]`
//!
//! Rewrites the configuration file in canonical JSON form. This rewrites a
//! file the user owns and may have hand-edited, so it errs toward preserving
//! what it does not understand and toward leaving a way back.

use std::fs;
use std::path::Path;

use naner_core::{config, constants, logger, paths, timestamp};

/// Top-level keys the config model does not own but which are worth keeping —
/// `$schema` in particular, since losing it breaks the very IDE completion
/// `naner schema` exists to provide.
fn preserved_extras(source: &Path, canonical: &serde_json::Value) -> Vec<(String, String)> {
    let Ok(text) = fs::read_to_string(source) else {
        return Vec::new();
    };
    // YAML sources have no JSON extras to carry over.
    if source.extension().is_some_and(|e| e != "json") {
        return Vec::new();
    }
    let stripped = config::strip_json_comments(&text);
    let Ok(serde_json::Value::Object(raw)) = serde_json::from_str::<serde_json::Value>(&stripped)
    else {
        return Vec::new();
    };
    let owned = canonical.as_object().cloned().unwrap_or_default();

    raw.into_iter()
        .filter(|(k, _)| !owned.contains_key(k))
        .filter_map(|(k, v)| serde_json::to_string(&v).ok().map(|v| (k, v)))
        .collect()
}

/// Splice preserved keys in ahead of the canonical body, keeping the field
/// order serde produces rather than re-serializing through a map (which would
/// alphabetize everything).
fn render(canonical_pretty: &str, extras: &[(String, String)]) -> String {
    if extras.is_empty() {
        return format!("{canonical_pretty}\n");
    }
    let mut lines = String::from("{\n");
    for (key, value) in extras {
        lines.push_str(&format!(
            "  {}: {},\n",
            serde_json::to_string(key).unwrap(),
            value
        ));
    }
    // canonical_pretty starts with "{\n"; replace that opening brace.
    format!("{lines}{}\n", canonical_pretty.trim_start_matches("{\n"))
}

pub fn execute(args: &[String]) -> i32 {
    let dry_run = args.iter().any(|a| a.eq_ignore_ascii_case("--dry-run"));

    logger::header("Naner Configuration Migration");
    logger::newline();

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    let Some(cfg_file) = config::find_configuration_file(&naner_root) else {
        logger::failure("Configuration file not found");
        return 1;
    };

    // Verbatim: `config::load` folds in NANER_ENV_* / NANER_DEFAULT_PROFILE,
    // the telemetry opt-out defaults, and expands %NANER_ROOT% to a concrete
    // path. Writing any of that back would silently make a transient
    // environment permanent — `NANER_DEFAULT_PROFILE=Bash naner migrate` would
    // rewrite the user's DefaultProfile.
    let cfg = match config::load_verbatim(&cfg_file) {
        Ok(c) => c,
        Err(err) => {
            logger::failure(&format!("Configuration parse error: {err}"));
            return 1;
        }
    };

    let canonical_value = match serde_json::to_value(&cfg) {
        Ok(v) => v,
        Err(err) => {
            logger::failure(&format!("Could not serialize configuration: {err}"));
            return 1;
        }
    };
    let canonical_pretty = match serde_json::to_string_pretty(&cfg) {
        Ok(s) => s,
        Err(err) => {
            logger::failure(&format!("Could not serialize configuration: {err}"));
            return 1;
        }
    };

    let extras = preserved_extras(&cfg_file, &canonical_value);
    let output = render(&canonical_pretty, &extras);

    let target = naner_root
        .join(constants::directory_names::CONFIG)
        .join("naner.json");

    logger::info(&format!("Source: {}", cfg_file.display()));
    logger::info(&format!("Target: {}", target.display()));
    if !extras.is_empty() {
        let names: Vec<&str> = extras.iter().map(|(k, _)| k.as_str()).collect();
        logger::info(&format!(
            "Preserving unrecognized keys: {}",
            names.join(", ")
        ));
    }

    // Comments cannot survive a serde round-trip. Say so before doing it —
    // the shipped config documents its opt-in profiles in comments.
    let had_comments = fs::read_to_string(&cfg_file)
        .map(|t| t.lines().any(|l| l.trim_start().starts_with("//")))
        .unwrap_or(false);
    if had_comments {
        logger::warning("Comments cannot be preserved and will be dropped.");
    }

    if dry_run {
        logger::newline();
        logger::info("Dry run — nothing written. Output would be:");
        logger::newline();
        print!("{output}");
        return 0;
    }

    // Back up before overwriting. Timestamped so a second run cannot clobber
    // the only copy of the original.
    if target.is_file() {
        let backup = target.with_extension(format!("{}.bak", timestamp::file_stamp()));
        if let Err(err) = fs::copy(&target, &backup) {
            logger::failure(&format!("Could not write backup, aborting: {err}"));
            return 1;
        }
        logger::info(&format!("Backup: {}", backup.display()));
    }

    // Write via a temp file so an interrupted write cannot truncate the
    // config the launcher needs to start.
    let temp = target.with_extension("json.tmp");
    if let Err(err) = fs::write(&temp, &output) {
        logger::failure(&format!("Failed to write migrated configuration: {err}"));
        return 1;
    }
    if let Err(err) = fs::rename(&temp, &target) {
        let _ = fs::remove_file(&temp);
        logger::failure(&format!("Failed to finalize migrated configuration: {err}"));
        return 1;
    }

    logger::newline();
    logger::success(&format!("Migrated to {}", target.display()));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical() -> serde_json::Value {
        serde_json::json!({ "DefaultProfile": "Unified", "Profiles": {} })
    }

    fn source_with(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("naner.json");
        std::fs::write(&path, body).unwrap();
        (dir, path)
    }

    #[test]
    fn keys_the_model_does_not_own_are_preserved() {
        let (_d, path) = source_with(
            r#"{ "$schema": "https://x/s.json", "title": "T", "DefaultProfile": "Unified" }"#,
        );
        let extras = preserved_extras(&path, &canonical());
        let names: Vec<&str> = extras.iter().map(|(k, _)| k.as_str()).collect();
        assert!(
            names.contains(&"$schema"),
            "losing $schema breaks IDE completion"
        );
        assert!(names.contains(&"title"));
        assert!(
            !names.contains(&"DefaultProfile"),
            "a key the model owns must not be duplicated"
        );
    }

    #[test]
    fn extras_are_read_through_comments() {
        let (_d, path) =
            source_with("{\n  // a comment the loader tolerates\n  \"$schema\": \"https://x\"\n}");
        let extras = preserved_extras(&path, &canonical());
        assert_eq!(extras.len(), 1);
    }

    #[test]
    fn rendered_output_is_valid_json_with_extras_first() {
        let pretty = serde_json::to_string_pretty(&canonical()).unwrap();
        let extras = vec![("$schema".to_string(), "\"https://x\"".to_string())];
        let out = render(&pretty, &extras);

        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["$schema"], "https://x");
        assert_eq!(parsed["DefaultProfile"], "Unified");
        assert!(out.ends_with('\n'));
        // Extras lead, so the schema reference stays where an editor looks.
        assert!(out.find("$schema").unwrap() < out.find("DefaultProfile").unwrap());
    }

    #[test]
    fn no_extras_still_renders_valid_json() {
        let pretty = serde_json::to_string_pretty(&canonical()).unwrap();
        let out = render(&pretty, &[]);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["DefaultProfile"], "Unified");
    }

    #[test]
    fn a_yaml_source_contributes_no_json_extras() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("naner.yaml");
        std::fs::write(&path, "DefaultProfile: Unified\n").unwrap();
        assert!(preserved_extras(&path, &canonical()).is_empty());
    }
}
