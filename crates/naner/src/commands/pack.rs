//! Command: `naner pack [dir] --out <bundle.zip>`
//!
//! Bundles a naner installation into a portable zip — the same content set the
//! release workflow ships, not just the config directory.

use std::fs;
use std::io::Write;
use std::path::Path;

use naner_core::{constants, logger, paths};
use zip::write::SimpleFileOptions;

/// What a distribution actually consists of. Anything absent is skipped and
/// reported rather than silently producing a thinner bundle than the name
/// implies.
const BUNDLED: [&str; 4] = ["bin", "config", "home", "icons"];
const BUNDLED_FILES: [&str; 1] = ["naner.bat"];

/// Never bundle transient working files: vendor staging, in-flight downloads,
/// or the timestamped backups `migrate` and `profile import` leave behind. A
/// distribution carrying someone else's old config is worse than a thin one.
fn is_excluded(rel: &str) -> bool {
    rel.starts_with(".downloads")
        || rel.starts_with(".staging")
        || rel.ends_with(".part")
        || rel.ends_with(".tmp")
        || rel.ends_with(".bak")
}

fn add_dir<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    root: &Path,
    dir: &Path,
    options: SimpleFileOptions,
) -> std::io::Result<usize> {
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_excluded(&rel) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            count += add_dir(zip, root, &path, options)?;
        } else {
            zip.start_file(&rel, options)
                .map_err(std::io::Error::other)?;
            zip.write_all(&fs::read(&path)?)?;
            count += 1;
        }
    }
    Ok(count)
}

pub fn execute(args: &[String]) -> i32 {
    logger::header("Naner Package Bundler");
    logger::newline();

    let out_flag = args.iter().position(|a| a == "--out" || a == "-o");
    let out_name = out_flag
        .and_then(|pos| args.get(pos + 1))
        .cloned()
        .unwrap_or_else(|| "naner-bundle.zip".to_string());

    // First non-flag argument is the source root, per the documented usage.
    let source_arg = args
        .iter()
        .enumerate()
        .find(|(i, a)| !a.starts_with('-') && out_flag.map(|p| *i != p + 1).unwrap_or(true))
        .map(|(_, a)| a.clone());

    let source = match source_arg {
        Some(dir) => std::path::PathBuf::from(dir),
        None => match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
            Ok(r) => r,
            Err(e) => {
                logger::failure("Could not locate Naner root directory");
                println!("{}", e.message);
                return 1;
            }
        },
    };
    if !source.is_dir() {
        logger::failure(&format!("Not a directory: {}", source.display()));
        return 1;
    }

    let out_path = Path::new(&out_name);
    logger::info(&format!("Source: {}", source.display()));
    logger::status(&format!("Creating {}...", out_path.display()));

    let file = match fs::File::create(out_path) {
        Ok(f) => f,
        Err(err) => {
            logger::failure(&format!("Failed to create bundle: {err}"));
            return 1;
        }
    };
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut total = 0;
    let mut missing = Vec::new();
    for name in BUNDLED {
        let dir = source.join(name);
        if !dir.is_dir() {
            missing.push(name);
            continue;
        }
        match add_dir(&mut zip, &source, &dir, options) {
            Ok(n) => total += n,
            Err(err) => {
                logger::failure(&format!("Failed while adding {name}/: {err}"));
                return 1;
            }
        }
    }
    for name in BUNDLED_FILES {
        let path = source.join(name);
        if !path.is_file() {
            missing.push(name);
            continue;
        }
        let write = zip
            .start_file(name, options)
            .map_err(std::io::Error::other)
            .and_then(|()| fs::read(&path).and_then(|b| zip.write_all(&b)));
        if let Err(err) = write {
            logger::failure(&format!("Failed while adding {name}: {err}"));
            return 1;
        }
        total += 1;
    }

    if let Err(err) = zip.finish() {
        logger::failure(&format!("Failed to finalize bundle: {err}"));
        return 1;
    }

    if !missing.is_empty() {
        logger::warning(&format!(
            "Not present, so not bundled: {}",
            missing.join(", ")
        ));
    }
    logger::success(&format!(
        "Bundled {total} file(s) to {}",
        out_path.display()
    ));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_vendor_paths_are_excluded() {
        assert!(is_excluded(".downloads/node.zip"));
        assert!(is_excluded(".staging/nodejs/node.exe"));
        assert!(is_excluded("config/naner.json.part"));
        assert!(
            is_excluded("config/naner.20260816-163945.bak"),
            "backups must not ship"
        );
        assert!(is_excluded("config/naner.tmp"));
        assert!(!is_excluded("config/naner.json"));
        assert!(!is_excluded("bin/naner.exe"));
    }
}
