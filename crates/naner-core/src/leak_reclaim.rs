//! Sweep dotfolders/files that leaked into the real Windows profile despite
//! naner's `Environment.EnvironmentVariables` redirects, back into
//! `%NANER_ROOT%\home` -- and bridge the original location back to the new
//! one so future writes land there too, even from a tool that resolves its
//! own path via a native OS call and never reads
//! `USERPROFILE`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME` at all (`naner reclaim`,
//! additive, no C# counterpart).
//!
//! Two bridging mechanisms, one per entry kind, mirroring
//! [`crate::home_junctions`]:
//! - Directories (`.claude/`, `.codex/`, `.gemini/`) get a junction
//!   (`mklink /J`, via [`crate::home_junctions::create_junction`]) -- no
//!   admin privilege or Developer Mode required.
//! - `.claude.json` is a single *file*; NTFS reparse points only redirect
//!   directories, so there is no junction equivalent for it. It falls back
//!   to a real symlink (`std::os::windows::fs::symlink_file`), which DOES
//!   need `SeCreateSymbolicLinkPrivilege` (admin, or Developer Mode). A
//!   failure there is reported, not fatal -- the file has already been
//!   moved into naner's tree either way, just not durably re-protected.
//!
//! Never silently discards data: if naner's home already has a copy of
//! something that also leaked to the real profile, the leaked copy is
//! preserved under a timestamped name rather than overwritten or dropped.

use std::path::{Path, PathBuf};

use crate::{home_junctions, logger, timestamp};

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Dir,
    File,
}

struct LeakTarget {
    /// Human-readable label used in log lines.
    name: &'static str,
    /// Path relative to the real user profile (and, mirrored, relative to
    /// `%NANER_ROOT%\home`).
    relative: &'static str,
    kind: EntryKind,
}

/// Every dotfolder/file this repo has root-caused as leaking into the real
/// Windows profile despite the redirects `naner.json` ships (see
/// `docs/VALIDATION.md`'s "Known limitations" for the per-tool reasoning).
const KNOWN_LEAKS: &[LeakTarget] = &[
    LeakTarget {
        name: "Claude Code (.claude)",
        relative: ".claude",
        kind: EntryKind::Dir,
    },
    LeakTarget {
        name: "Claude Code (.claude.json)",
        relative: ".claude.json",
        kind: EntryKind::File,
    },
    LeakTarget {
        name: "Codex CLI (.codex)",
        relative: ".codex",
        kind: EntryKind::Dir,
    },
    LeakTarget {
        name: "Gemini CLI / Antigravity (.gemini)",
        relative: ".gemini",
        kind: EntryKind::Dir,
    },
];

/// Claude Code's own timestamped backups of `.claude.json`
/// (`.claude.json.backup.<epoch>`) have no fixed path to bridge -- each one
/// names itself uniquely -- so they are swept into naner's home for
/// consolidation and never symlinked.
const BACKUP_SWEEP_PREFIX: &str = ".claude.json.backup.";
const BACKUP_SWEEP_LABEL: &str = "Claude Code (.claude.json backups)";

/// Tally of what [`reclaim`] actually did, for the CLI layer's summary line.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReclaimSummary {
    /// Leaked entries moved into `naner_home` (including conflict backups).
    pub migrated: usize,
    /// Of those, how many also got a working redirect link back.
    pub linked: usize,
    /// Of those, how many could not be linked (privilege, most likely).
    pub link_failed: usize,
    /// Loose backup files swept in, no linking attempted.
    pub swept_backups: usize,
}

/// Sweep every entry in [`KNOWN_LEAKS`] plus the `.claude.json` backup glob
/// from `real_profile` into `naner_home`, narrating every decision via
/// [`logger`] the same way [`crate::home_junctions::create_junction`] does.
/// `dry_run` previews without touching the filesystem.
pub fn reclaim(real_profile: &Path, naner_home: &Path, dry_run: bool) -> ReclaimSummary {
    let mut summary = ReclaimSummary::default();
    for target in KNOWN_LEAKS {
        reclaim_one(target, real_profile, naner_home, dry_run, &mut summary);
    }
    sweep_backups(real_profile, naner_home, dry_run, &mut summary);
    summary
}

enum Plan {
    /// Nothing at the real-profile path; there is no leak to sweep.
    Nothing,
    /// Already bridged to `naner_home` by a prior run.
    AlreadyLinked,
    /// A link exists but doesn't point where naner would have put it --
    /// left alone rather than risking someone else's deliberate symlink.
    ForeignLink,
    /// Real data sitting at the leaked path. `conflict` is true when
    /// `naner_home` already has its own copy too.
    Migrate { conflict: bool },
}

fn plan_for(real_path: &Path, home_path: &Path) -> Plan {
    let Ok(meta) = std::fs::symlink_metadata(real_path) else {
        return Plan::Nothing;
    };
    if meta.file_type().is_symlink() {
        return match std::fs::read_link(real_path) {
            Ok(dest) if paths_match(&dest, home_path) => Plan::AlreadyLinked,
            _ => Plan::ForeignLink,
        };
    }
    Plan::Migrate {
        conflict: home_path.exists(),
    }
}

fn reclaim_one(
    target: &LeakTarget,
    real_profile: &Path,
    naner_home: &Path,
    dry_run: bool,
    summary: &mut ReclaimSummary,
) {
    let real_path = real_profile.join(target.relative);
    let home_path = naner_home.join(target.relative);
    let name = target.name;

    match plan_for(&real_path, &home_path) {
        Plan::Nothing => logger::info(&format!("{name}: nothing leaked here")),
        Plan::AlreadyLinked => logger::success(&format!("{name}: already redirected")),
        Plan::ForeignLink => logger::warning(&format!(
            "{name}: {} is already a link naner didn't create; left alone",
            real_path.display()
        )),
        Plan::Migrate { conflict } => {
            if dry_run {
                let verb = if conflict {
                    "would back up the leaked copy (naner's home already has one) and link"
                } else {
                    "would move and link"
                };
                logger::info(&format!(
                    "{name}: {verb} {} -> {}",
                    real_path.display(),
                    home_path.display()
                ));
                summary.migrated += 1;
                return;
            }

            let move_dest = if conflict {
                conflict_backup_path(&home_path)
            } else {
                home_path.clone()
            };
            if let Some(parent) = move_dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = move_path(&real_path, &move_dest, target.kind) {
                logger::warning(&format!(
                    "{name}: could not move {}: {e}",
                    real_path.display()
                ));
                return;
            }
            if conflict {
                logger::warning(&format!(
                    "{name}: naner's home already had a copy; the leaked copy was preserved at {}",
                    move_dest.display()
                ));
            } else {
                logger::success(&format!("{name}: moved to {}", move_dest.display()));
            }
            summary.migrated += 1;

            let link_target = if conflict { &home_path } else { &move_dest };
            if create_link(&real_path, link_target, target.kind) {
                summary.linked += 1;
            } else {
                summary.link_failed += 1;
            }
        }
    }
}

fn sweep_backups(
    real_profile: &Path,
    naner_home: &Path,
    dry_run: bool,
    summary: &mut ReclaimSummary,
) {
    let Ok(entries) = std::fs::read_dir(real_profile) else {
        return;
    };
    let mut moved = 0usize;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with(BACKUP_SWEEP_PREFIX) {
            continue;
        }
        let dest = naner_home.join(&*file_name);
        if dest.exists() {
            // A prior sweep already claimed this exact name (backups are
            // epoch-stamped, so a real collision is essentially never
            // organic) -- leave it rather than overwrite.
            continue;
        }
        if dry_run {
            moved += 1;
            continue;
        }
        if std::fs::rename(entry.path(), &dest).is_ok() {
            moved += 1;
        }
    }
    if moved > 0 {
        let verb = if dry_run { "would move" } else { "moved" };
        logger::success(&format!("{BACKUP_SWEEP_LABEL}: {verb} {moved} file(s)"));
        summary.swept_backups += moved;
    } else {
        logger::info(&format!("{BACKUP_SWEEP_LABEL}: nothing to sweep"));
    }
}

/// A leaked-then-reclaimed name never collides with the live target: the
/// stamp is appended to the whole file/dir name (`.claude.json` ->
/// `.claude.json.reclaimed-<stamp>`), not substituted for an "extension" --
/// `Path::with_extension` would mistake `.claude.json`'s `.json` for a
/// replaceable extension and silently drop it.
fn conflict_backup_path(home_path: &Path) -> PathBuf {
    let stamp = timestamp::file_stamp();
    let mut name = home_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.push_str(".reclaimed-");
    name.push_str(&stamp);
    home_path.with_file_name(name)
}

fn move_path(src: &Path, dst: &Path, kind: EntryKind) -> std::io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    // `rename` fails across volumes (NANER_ROOT on a different drive than
    // the profile) -- fall back to copy + delete so that setup still works.
    match kind {
        EntryKind::Dir => {
            copy_dir_all(src, dst)?;
            std::fs::remove_dir_all(src)
        }
        EntryKind::File => {
            std::fs::copy(src, dst)?;
            std::fs::remove_file(src)
        }
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else if file_type.is_symlink() {
            // Best-effort: an embedded symlink materializing as a plain
            // copy would be surprising, so it's skipped rather than
            // followed or failing the whole sweep.
            continue;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

fn create_link(link: &Path, target: &Path, kind: EntryKind) -> bool {
    match kind {
        EntryKind::Dir => home_junctions::create_junction(link, target),
        EntryKind::File => create_file_symlink(link, target),
    }
}

#[cfg(windows)]
fn create_file_symlink(link: &Path, target: &Path) -> bool {
    match std::os::windows::fs::symlink_file(target, link) {
        Ok(()) => {
            logger::success(&format!(
                "Linked {} -> {}",
                link.display(),
                target.display()
            ));
            true
        }
        Err(e) => {
            logger::warning(&format!(
                "Could not create a symlink at {} (needs Developer Mode or Administrator): {e}. \
                 The file is safely moved into naner's tree, but until this is fixed and \
                 'naner reclaim' is run again, a fresh copy may reappear at the real location.",
                link.display()
            ));
            false
        }
    }
}

#[cfg(not(windows))]
fn create_file_symlink(link: &Path, target: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

fn paths_match(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// The real Windows profile directory, resolved independently of
/// `USERPROFILE` (which may already be naner's own redirected value if this
/// process was launched from inside a naner shell -- unlike
/// [`crate::home_junctions`]'s `host_userprofile`, which is only reliable
/// when captured before that redirect happens in the *same* process,
/// `SHGetKnownFolderPath` asks the OS directly and is correct regardless of
/// how deeply nested the invocation is).
#[cfg(windows)]
pub fn real_user_profile() -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Profile, SHGetKnownFolderPath};
    use windows_sys::core::PWSTR;

    unsafe {
        let mut raw: PWSTR = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(&FOLDERID_Profile, 0, std::ptr::null_mut(), &mut raw);
        if hr < 0 || raw.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *raw.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw as *const std::ffi::c_void);
        if text.is_empty() {
            None
        } else {
            Some(PathBuf::from(text))
        }
    }
}

#[cfg(not(windows))]
pub fn real_user_profile() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dir(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("marker.txt"), "leaked").unwrap();
    }

    #[test]
    fn nothing_leaked_is_a_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&home).unwrap();

        let summary = reclaim(&real, &home, false);
        assert_eq!(summary, ReclaimSummary::default());
        assert!(!home.join(".claude").exists());
    }

    #[test]
    fn dry_run_previews_without_touching_the_filesystem() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        let home = tmp.path().join("home");
        make_dir(&real.join(".claude"));
        std::fs::create_dir_all(&home).unwrap();

        let summary = reclaim(&real, &home, true);
        assert_eq!(summary.migrated, 1);
        assert_eq!(summary.linked, 0);
        // Untouched: still real data at the original path, nothing under home.
        assert!(real.join(".claude/marker.txt").is_file());
        assert!(!home.join(".claude").exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_leaked_directory_is_moved_and_linked_back() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        let home = tmp.path().join("home");
        make_dir(&real.join(".codex"));
        std::fs::create_dir_all(&home).unwrap();

        let summary = reclaim(&real, &home, false);
        assert_eq!(summary.migrated, 1);
        assert_eq!(summary.linked, 1);
        assert!(home.join(".codex/marker.txt").is_file());
        // The original location now resolves through the link to the same data.
        assert!(real.join(".codex/marker.txt").is_file());
        assert!(
            std::fs::symlink_metadata(real.join(".codex"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(windows)]
    #[test]
    fn re_running_after_a_successful_link_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        let home = tmp.path().join("home");
        make_dir(&real.join(".codex"));
        std::fs::create_dir_all(&home).unwrap();

        reclaim(&real, &home, false);
        let second = reclaim(&real, &home, false);
        assert_eq!(second, ReclaimSummary::default());
    }

    #[test]
    fn a_conflicting_copy_in_home_is_backed_up_not_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        let home = tmp.path().join("home");
        make_dir(&real.join(".gemini"));
        std::fs::create_dir_all(home.join(".gemini")).unwrap();
        std::fs::write(home.join(".gemini/marker.txt"), "already naner's").unwrap();

        let summary = reclaim(&real, &home, false);
        assert_eq!(summary.migrated, 1);
        // naner's own copy is untouched.
        assert_eq!(
            std::fs::read_to_string(home.join(".gemini/marker.txt")).unwrap(),
            "already naner's"
        );
        // The leaked copy is preserved somewhere under home, not deleted.
        let backups: Vec<_> = std::fs::read_dir(&home)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".gemini.reclaimed-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(
            std::fs::read_to_string(home.join(&backups[0]).join("marker.txt"))
                .unwrap()
                .contains("leaked")
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_foreign_symlink_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        let home = tmp.path().join("home");
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        make_dir(&elsewhere);

        create_link(&real.join(".codex"), &elsewhere, EntryKind::Dir);
        // A directory junction to somewhere naner did not just create it
        // for -- reclaim must not touch it.
        let summary = reclaim(&real, &home, false);
        assert_eq!(summary, ReclaimSummary::default());
        assert!(!home.join(".codex").exists());
    }

    #[test]
    fn backup_files_are_swept_without_linking() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(real.join(".claude.json.backup.1700000000000"), "a").unwrap();
        std::fs::write(real.join(".claude.json.backup.1700000001000"), "b").unwrap();
        std::fs::write(real.join("unrelated.txt"), "c").unwrap();

        let summary = reclaim(&real, &home, false);
        assert_eq!(summary.swept_backups, 2);
        assert!(home.join(".claude.json.backup.1700000000000").is_file());
        assert!(home.join(".claude.json.backup.1700000001000").is_file());
        assert!(real.join("unrelated.txt").is_file());
        assert!(!real.join(".claude.json.backup.1700000000000").exists());
    }

    #[test]
    fn conflict_backup_path_keeps_the_full_original_name() {
        let path = conflict_backup_path(Path::new("C:/naner/home/.claude.json"));
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with(".claude.json.reclaimed-"));
    }
}
