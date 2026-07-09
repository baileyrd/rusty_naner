//! Port of Naner.Archives: extractor dispatch by extension with the C#
//! strategy order, single-subdirectory flattening, and the same log lines.
//!
//! Differences from C# (deliberate, MIGRATION_ANALYSIS §2.3/§4.3):
//! - `.zip` extracts natively (as in C#, different library).
//! - `.tar.xz` extracts natively (pure Rust xz + tar) with the C# 7z.exe
//!   two-stage shell-out kept as a fallback when native extraction fails.
//! - `.7z` and `.msi` remain shell-outs (7z.exe / msiexec), `.exe` installers
//!   run directly with the per-vendor argument defaults.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::logger;

/// Extract an archive to a directory, mirroring
/// `ArchiveExtractorService.ExtractArchive`.
pub fn extract_archive(
    archive_path: &Path,
    target_dir: &Path,
    vendor_name: &str,
    seven_zip_path: Option<&Path>,
    installer_args: Option<&[String]>,
) -> bool {
    if !archive_path.is_file() {
        logger::failure(&format!(
            "    Extraction error: Archive not found: {}",
            archive_path.display()
        ));
        return false;
    }

    let name = archive_path.to_string_lossy().to_lowercase();
    if name.ends_with(".zip") {
        extract_zip(archive_path, target_dir)
    } else if name.ends_with(".tar.xz") {
        extract_tar_xz(archive_path, target_dir, vendor_name, seven_zip_path)
    } else if name.ends_with(".7z") {
        extract_7z(archive_path, target_dir, vendor_name, seven_zip_path)
    } else if name.ends_with(".msi") {
        extract_msi(archive_path, target_dir)
    } else if name.ends_with(".exe") {
        run_exe_installer(archive_path, target_dir, vendor_name, installer_args)
    } else {
        let extension = archive_path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        logger::warning(&format!("    Unsupported archive format: {extension}"));
        false
    }
}

/// Plain zip extraction with overwrite and NO flattening — the
/// naner-bundle.zip path (`ZipFile.ExtractToDirectory(..., overwrite:
/// true)` in `NanerUpdater`), where the archive's layout IS the tree.
pub fn extract_zip_plain(archive_path: &Path, target_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;
    let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    archive.extract(target_dir).map_err(|e| e.to_string())
}

/// `ArchiveUtilities.FlattenSingleSubdirectory`: when extraction produced
/// exactly one entry and it is a directory, hoist its contents via the
/// rename-based strategy (no copying, symlink-safe).
pub fn flatten_single_subdirectory(target_dir: &Path) -> std::io::Result<()> {
    let entries: Vec<PathBuf> = std::fs::read_dir(target_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    if entries.len() != 1 || !entries[0].is_dir() {
        return Ok(());
    }

    let parent = target_dir.parent().unwrap_or(Path::new("."));
    let target_name = target_dir.file_name().unwrap_or_default().to_os_string();
    let mut temp_name = target_name.clone();
    temp_name.push("_flatten_temp");
    let temp_parent = parent.join(&temp_name);

    // Rename target -> temp, move inner dir to target's name, delete temp.
    std::fs::rename(target_dir, &temp_parent)?;
    let inner_name = entries[0].file_name().unwrap_or_default().to_os_string();
    let inner_path = temp_parent.join(inner_name);
    std::fs::rename(&inner_path, target_dir)?;
    let _ = std::fs::remove_dir_all(&temp_parent);
    Ok(())
}

fn extract_zip(archive_path: &Path, target_dir: &Path) -> bool {
    let result = (|| -> Result<(), String> {
        std::fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;
        let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        archive.extract(target_dir).map_err(|e| e.to_string())?;
        flatten_single_subdirectory(target_dir).map_err(|e| e.to_string())
    })();

    match result {
        Ok(()) => true,
        Err(e) => {
            logger::failure(&format!("    ZIP extraction failed: {e}"));
            false
        }
    }
}

fn extract_tar_xz(
    archive_path: &Path,
    target_dir: &Path,
    vendor_name: &str,
    seven_zip_path: Option<&Path>,
) -> bool {
    // Native path first (new capability; removes the 7-Zip bootstrap
    // dependency for MSYS2).
    match extract_tar_xz_native(archive_path, target_dir) {
        Ok(()) => return true,
        Err(e) => {
            logger::warning(&format!(
                "    Native .tar.xz extraction failed: {e}; trying 7-Zip fallback"
            ));
        }
    }
    extract_tar_xz_via_7z(archive_path, target_dir, vendor_name, seven_zip_path)
}

fn extract_tar_xz_native(archive_path: &Path, target_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;

    logger::info("    Extracting .xz...");
    // Stream-decompress the xz into an intermediate .tar next to the source
    // (same two-stage shape as the C# 7z flow — bounded memory for the
    // ~400 MB MSYS2 archive).
    let tar_path = tar_path_for(archive_path);
    {
        let file = std::fs::File::open(archive_path).map_err(|e| e.to_string())?;
        let mut reader = std::io::BufReader::new(file);
        let tar_file = std::fs::File::create(&tar_path).map_err(|e| e.to_string())?;
        let mut writer = std::io::BufWriter::new(tar_file);
        lzma_rs::xz_decompress(&mut reader, &mut writer).map_err(|e| format!("{e:?}"))?;
    }

    logger::info("    Extracting .tar...");
    let result = (|| -> Result<(), String> {
        let tar_file = std::fs::File::open(&tar_path).map_err(|e| e.to_string())?;
        let mut archive = tar::Archive::new(std::io::BufReader::new(tar_file));
        archive.set_preserve_permissions(false);
        // Windows without developer mode can't create symlinks; skip entries
        // that fail rather than aborting the whole tree (§4.3 risk note).
        for entry in archive.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            if let Err(e) = entry.unpack_in(target_dir) {
                let path = entry.path().map(|p| p.display().to_string());
                logger::debug(
                    &format!("    Skipped entry {:?}: {e}", path.unwrap_or_default()),
                    false,
                );
            }
        }
        Ok(())
    })();

    let _ = std::fs::remove_file(&tar_path);
    result?;

    flatten_single_subdirectory(target_dir).map_err(|e| e.to_string())
}

fn extract_tar_xz_via_7z(
    archive_path: &Path,
    target_dir: &Path,
    vendor_name: &str,
    seven_zip_path: Option<&Path>,
) -> bool {
    let Some(seven_zip) = seven_zip_path.filter(|p| p.is_file()) else {
        logger::warning("    7-Zip not found");
        logger::info(&format!(
            "    {vendor_name} downloaded to: {}",
            archive_path.display()
        ));
        logger::info(&format!(
            "    Please extract manually to: {}",
            target_dir.display()
        ));
        return false;
    };

    if std::fs::create_dir_all(target_dir).is_err() {
        return false;
    }

    logger::info("    Extracting .xz...");
    let archive_dir = archive_path.parent().unwrap_or(Path::new("."));
    if !run_7z(seven_zip, archive_path, archive_dir, "extract .xz") {
        return false;
    }

    let tar_path = tar_path_for(archive_path);
    if !tar_path.is_file() {
        logger::warning("    .tar file not found after extraction");
        return false;
    }

    logger::info("    Extracting .tar...");
    let ok = run_7z(seven_zip, &tar_path, target_dir, "extract .tar");
    let _ = std::fs::remove_file(&tar_path);
    if !ok {
        return false;
    }

    flatten_single_subdirectory(target_dir).is_ok()
}

fn extract_7z(
    archive_path: &Path,
    target_dir: &Path,
    vendor_name: &str,
    seven_zip_path: Option<&Path>,
) -> bool {
    let Some(seven_zip) = seven_zip_path.filter(|p| p.is_file()) else {
        logger::warning("    7-Zip not found");
        logger::info(&format!(
            "    {vendor_name} downloaded to: {}",
            archive_path.display()
        ));
        logger::info(&format!(
            "    Please extract manually to: {}",
            target_dir.display()
        ));
        return false;
    };

    if std::fs::create_dir_all(target_dir).is_err() {
        return false;
    }

    logger::info("    Extracting .7z archive...");
    if !run_7z(seven_zip, archive_path, target_dir, "extract .7z") {
        return false;
    }
    flatten_single_subdirectory(target_dir).is_ok()
}

/// `foo.tar.xz` → `foo.tar` (suffix matched case-insensitively, rest of the
/// path untouched — the C# `Replace(..., OrdinalIgnoreCase)` on the suffix).
fn tar_path_for(archive_path: &Path) -> PathBuf {
    let s = archive_path.to_string_lossy();
    match s.to_lowercase().rfind(".tar.xz") {
        Some(idx) => PathBuf::from(format!("{}.tar", &s[..idx])),
        None => archive_path.with_extension("tar"),
    }
}

/// `x "<src>" -o"<dst>" -y`, draining output to avoid pipe deadlocks.
fn run_7z(seven_zip: &Path, archive: &Path, out_dir: &Path, operation: &str) -> bool {
    let output = Command::new(seven_zip)
        .arg("x")
        .arg(archive)
        .arg(format!("-o{}", out_dir.display()))
        .arg("-y")
        .output();

    match output {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            logger::warning(&format!(
                "    Failed to {operation} (exit code {})",
                out.status.code().unwrap_or(-1)
            ));
            false
        }
        Err(_) => {
            logger::warning(&format!("    Failed to start 7-Zip for {operation}"));
            false
        }
    }
}

/// `msiexec /a "<msi>" /qn TARGETDIR="<dst>"` + the 7-Zip-specific
/// `Files/7-Zip` hoist.
fn extract_msi(archive_path: &Path, target_dir: &Path) -> bool {
    let result = (|| -> Result<bool, String> {
        std::fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;

        let status = Command::new("msiexec.exe")
            .arg("/a")
            .arg(archive_path)
            .arg("/qn")
            .arg(format!("TARGETDIR={}", target_dir.display()))
            .status()
            .map_err(|e| e.to_string())?;

        // The 7-Zip MSI drops payload under Files/7-Zip; move it up.
        let files_dir = target_dir.join("Files").join("7-Zip");
        if files_dir.is_dir() {
            for entry in std::fs::read_dir(&files_dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.path().is_file() {
                    let dest = target_dir.join(entry.file_name());
                    let _ = std::fs::remove_file(&dest);
                    std::fs::rename(entry.path(), dest).map_err(|e| e.to_string())?;
                }
            }
            let _ = std::fs::remove_dir_all(target_dir.join("Files"));
        }

        Ok(status.success())
    })();

    match result {
        Ok(ok) => ok,
        Err(e) => {
            logger::failure(&format!("    MSI extraction failed: {e}"));
            false
        }
    }
}

/// `ExeInstallerExtractor`: run the installer silently with per-vendor
/// argument defaults and `%TARGETDIR%`/`$TARGETDIR` substitution.
fn run_exe_installer(
    installer_path: &Path,
    target_dir: &Path,
    vendor_name: &str,
    installer_args: Option<&[String]>,
) -> bool {
    if std::fs::create_dir_all(target_dir).is_err() {
        return false;
    }

    let arguments =
        build_installer_arguments(installer_path, target_dir, vendor_name, installer_args);
    logger::info(&format!(
        "    Running installer: {}",
        installer_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    logger::debug(&format!("    Arguments: {}", arguments.join(" ")), false);

    let mut command = Command::new(installer_path);
    command.args(&arguments);

    // rustup needs RUSTUP_HOME/CARGO_HOME pointed into the vendor dir.
    let file_name = installer_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let vendor = vendor_name.to_lowercase();
    if file_name.contains("rustup") || vendor.contains("rust") {
        command.env("RUSTUP_HOME", target_dir.join(".rustup"));
        command.env("CARGO_HOME", target_dir.join(".cargo"));
    }

    match command.output() {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            logger::failure(&format!(
                "    Installer exited with code {}",
                out.status.code().unwrap_or(-1)
            ));
            false
        }
        Err(e) => {
            logger::failure(&format!("    Installer execution failed: {e}"));
            false
        }
    }
}

/// `BuildInstallerArguments`, returned as discrete args (the C# raw string
/// split on the same boundaries it was joined on).
fn build_installer_arguments(
    installer_path: &Path,
    target_dir: &Path,
    vendor_name: &str,
    installer_args: Option<&[String]>,
) -> Vec<String> {
    let target = target_dir.display().to_string();

    if let Some(args) = installer_args
        && !args.is_empty()
    {
        return args
            .iter()
            .map(|a| {
                a.replace("%TARGETDIR%", &target)
                    .replace("$TARGETDIR", &target)
            })
            .collect();
    }

    let file_name = installer_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let vendor = vendor_name.to_lowercase();

    if file_name.contains("miniconda") || file_name.contains("anaconda") || vendor.contains("conda")
    {
        // NSIS: /D= must be last and unquoted.
        return vec!["/S".into(), format!("/D={target}")];
    }
    if file_name.contains("rustup") || vendor.contains("rust") {
        return vec![
            "-y".into(),
            "--default-toolchain".into(),
            "stable".into(),
            "--profile".into(),
            "default".into(),
            "--no-modify-path".into(),
        ];
    }
    vec!["/S".into(), format!("/D={target}")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(entries: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
        let file = tempfile::Builder::new().suffix(".zip").tempfile().unwrap();
        {
            let mut writer = zip::ZipWriter::new(file.reopen().unwrap());
            let options = zip::write::SimpleFileOptions::default();
            for (name, content) in entries {
                if name.ends_with('/') {
                    writer
                        .add_directory(name.trim_end_matches('/'), options)
                        .unwrap();
                } else {
                    writer.start_file(*name, options).unwrap();
                    writer.write_all(content).unwrap();
                }
            }
            writer.finish().unwrap();
        }
        file
    }

    #[test]
    fn zip_extracts_and_flattens_single_root() {
        let zip = make_zip(&[
            ("tool-1.0/", b"" as &[u8]),
            ("tool-1.0/bin/tool.exe", b"binary"),
            ("tool-1.0/readme.txt", b"hi"),
        ]);
        let out = tempfile::tempdir().unwrap();
        let target = out.path().join("tool");
        assert!(extract_archive(zip.path(), &target, "Tool", None, None));
        // Flattened: contents of tool-1.0/ hoisted into tool/.
        assert!(target.join("bin/tool.exe").is_file());
        assert!(target.join("readme.txt").is_file());
        assert!(!target.join("tool-1.0").exists());
    }

    #[test]
    fn zip_with_multiple_roots_is_not_flattened() {
        let zip = make_zip(&[("a.txt", b"a" as &[u8]), ("b/b.txt", b"b")]);
        let out = tempfile::tempdir().unwrap();
        let target = out.path().join("multi");
        assert!(extract_archive(zip.path(), &target, "Multi", None, None));
        assert!(target.join("a.txt").is_file());
        assert!(target.join("b/b.txt").is_file());
    }

    #[test]
    fn tar_xz_extracts_natively() {
        // Build a tar in memory, xz-compress it via lzma-rs, then extract.
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            header.set_size(5);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "pkg/hello.txt", &b"hello"[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let mut xz_bytes = Vec::new();
        lzma_rs::xz_compress(&mut std::io::Cursor::new(&tar_bytes), &mut xz_bytes).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("pkg.tar.xz");
        std::fs::write(&archive, &xz_bytes).unwrap();

        let target = dir.path().join("pkg");
        assert!(extract_archive(&archive, &target, "Pkg", None, None));
        // Single root "pkg/" flattened away.
        assert!(target.join("hello.txt").is_file());
    }

    #[test]
    fn unsupported_extension_is_rejected() {
        let f = tempfile::Builder::new().suffix(".rar").tempfile().unwrap();
        let out = tempfile::tempdir().unwrap();
        assert!(!extract_archive(
            f.path(),
            &out.path().join("x"),
            "X",
            None,
            None
        ));
    }

    #[test]
    fn installer_args_substitute_targetdir() {
        let args = build_installer_arguments(
            Path::new("setup.exe"),
            Path::new("C:\\naner\\vendor\\ruby"),
            "Ruby",
            Some(&["/silent".into(), "/dir=%TARGETDIR%".into()]),
        );
        assert_eq!(args, vec!["/silent", "/dir=C:\\naner\\vendor\\ruby"]);

        let rustup = build_installer_arguments(
            Path::new("rustup-init.exe"),
            Path::new("C:\\naner\\vendor\\rust"),
            "Rust",
            None,
        );
        assert!(rustup.contains(&"--no-modify-path".to_string()));

        let conda = build_installer_arguments(
            Path::new("Miniconda3-latest.exe"),
            Path::new("C:\\v\\conda"),
            "Miniconda",
            None,
        );
        assert_eq!(conda, vec!["/S", "/D=C:\\v\\conda"]);
    }
}
