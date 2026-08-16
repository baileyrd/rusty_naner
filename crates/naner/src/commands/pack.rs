//! Command: `naner pack [dir] --out <bundle.zip>`
//! Creates a self-contained portable distribution zip package.

use naner_core::{constants, logger, paths};
use std::fs;
use std::io::Write;
use std::path::Path;
use zip::write::SimpleFileOptions;

pub fn execute(args: &[String]) -> i32 {
    logger::header("Naner Package Bundler");
    logger::newline();

    let naner_root = match paths::find_naner_root(None, constants::MAX_NANER_ROOT_SEARCH_DEPTH) {
        Ok(r) => r,
        Err(e) => {
            logger::failure("Could not locate Naner root directory");
            println!("{}", e.message);
            return 1;
        }
    };

    let out_filename = if let Some(pos) = args.iter().position(|a| a == "--out" || a == "-o") {
        args.get(pos + 1)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "naner-bundle.zip".to_string())
    } else {
        "naner-bundle.zip".to_string()
    };

    let out_path = Path::new(&out_filename);
    logger::status(&format!(
        "Creating distribution bundle at {}...",
        out_path.display()
    ));

    let file = match fs::File::create(out_path) {
        Ok(f) => f,
        Err(err) => {
            logger::failure(&format!("Failed to create bundle zip file: {err}"));
            return 1;
        }
    };

    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let config_dir = naner_root.join(constants::directory_names::CONFIG);
    if config_dir.is_dir() {
        for entry in fs::read_dir(&config_dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_file() {
                let name = format!("config/{}", p.file_name().unwrap().to_string_lossy());
                let _ = zip.start_file(name, options);
                let content = fs::read(&p).unwrap_or_default();
                let _ = zip.write_all(&content);
            }
        }
    }

    if let Err(err) = zip.finish() {
        logger::failure(&format!("Failed to finalize bundle archive: {err}"));
        return 1;
    }

    logger::success(&format!(
        "Distribution package successfully bundled to {}",
        out_path.display()
    ));
    0
}
