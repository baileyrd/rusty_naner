//! Writing back a file the user owns.
//!
//! `migrate` and `profile import` both rewrite `config/naner.json`. Getting
//! that wrong costs the user their configuration, so the discipline lives in
//! one place: back up first, write to a temp path, rename into position.

use std::fs;
use std::path::{Path, PathBuf};

use naner_core::{logger, timestamp};

/// Copy `target` aside before it is overwritten.
///
/// Timestamped so a second run cannot clobber the only copy of the original.
/// Returns the backup path, or `None` when there was nothing to back up.
pub fn back_up(target: &Path) -> Result<Option<PathBuf>, String> {
    if !target.is_file() {
        return Ok(None);
    }
    let backup = target.with_extension(format!("{}.bak", timestamp::file_stamp()));
    fs::copy(target, &backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    Ok(Some(backup))
}

/// Write `contents` to `target` via a temp file and a rename.
///
/// The rename is what makes it safe: an interrupted write leaves the previous
/// file intact rather than a truncated one the launcher cannot parse.
pub fn write_atomic(target: &Path, contents: &str) -> Result<(), String> {
    let temp = target.with_extension("tmp");
    fs::write(&temp, contents).map_err(|e| format!("{}: {e}", temp.display()))?;
    if let Err(e) = fs::rename(&temp, target) {
        let _ = fs::remove_file(&temp);
        return Err(format!("{}: {e}", target.display()));
    }
    Ok(())
}

/// Back up, then write. Reports the backup path so the user knows the way
/// back without having to guess the name.
pub fn replace(target: &Path, contents: &str) -> Result<(), String> {
    if let Some(path) = back_up(target)? {
        logger::info(&format!("Backup: {}", path.display()));
    }
    write_atomic(target, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_keeps_a_backup_of_the_previous_contents() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("naner.json");
        fs::write(&target, "original").unwrap();

        replace(&target, "updated").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "updated");

        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
            .collect();
        assert_eq!(backups.len(), 1, "exactly one backup expected");
        assert_eq!(fs::read_to_string(backups[0].path()).unwrap(), "original");
    }

    #[test]
    fn writing_a_new_file_needs_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("naner.json");
        assert!(back_up(&target).unwrap().is_none());
        replace(&target, "fresh").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "fresh");
    }

    #[test]
    fn no_temp_file_survives_a_successful_write() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("naner.json");
        write_atomic(&target, "x").unwrap();
        assert!(!target.with_extension("tmp").exists());
    }
}
