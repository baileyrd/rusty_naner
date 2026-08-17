//! Small shared helpers for editing a file the user owns: back it up before
//! touching it, and write the replacement via a temp file + rename so an
//! interrupted write leaves the previous file intact rather than a
//! truncated one nothing downstream can parse. Used by every in-place
//! config merge in this crate (Windows Terminal's `settings.json`, and the
//! `naner.json`/`vendors.json` shipped-defaults merge) so a second call site
//! reuses this instead of a third hand-rolled copy.

use std::path::{Path, PathBuf};

use crate::timestamp;

/// Copy `target` aside before it is overwritten. Timestamped so a second run
/// cannot clobber the only copy of the original.
pub(crate) fn back_up(target: &Path) -> std::io::Result<Option<PathBuf>> {
    if !target.is_file() {
        return Ok(None);
    }
    let backup = target.with_extension(format!("{}.bak", timestamp::file_stamp()));
    std::fs::copy(target, &backup)?;
    Ok(Some(backup))
}

/// Write via a temp file and a rename.
pub(crate) fn write_atomic(target: &Path, contents: &str) -> std::io::Result<()> {
    let temp = target.with_extension("tmp");
    std::fs::write(&temp, contents)?;
    if let Err(e) = std::fs::rename(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok(())
}
