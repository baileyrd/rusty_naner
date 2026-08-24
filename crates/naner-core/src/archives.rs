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
    naner_root: &Path,
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
        run_exe_installer(
            archive_path,
            target_dir,
            vendor_name,
            installer_args,
            naner_root,
        )
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
    // Matched against the original string, not a lowercased copy: offsets
    // taken from a lowercased copy can land mid-character (see
    // `paths::match_ranges_ignore_case`).
    match crate::paths::match_ranges_ignore_case(&s, ".tar.xz").last() {
        Some(range) => PathBuf::from(format!("{}.tar", &s[..range.start])),
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

/// `USERPROFILE`/`HOME`/`APPDATA`/`LOCALAPPDATA`/`TEMP`/`TMP`, pointed into
/// naner's own home tree -- the same redirect a launched terminal profile
/// gets from `naner.json`'s `EnvironmentVariables` (see `run_launcher`'s
/// `setup_environment`), for a subprocess spawned by `naner install`/
/// `update-vendors` instead. That code path never runs the launcher's
/// environment setup, so without this a spawned installer or package
/// manager inherits the host's raw environment and writes any home-relative
/// dotfile straight into the real Windows profile. Empty when `home/`
/// doesn't exist yet (a naner root mid-init), which callers treat as "set
/// nothing" by iterating zero pairs.
pub(crate) fn home_isolation_envs(naner_root: &Path) -> Vec<(String, String)> {
    let home = naner_root.join("home");
    if !home.is_dir() {
        return Vec::new();
    }
    vec![
        ("USERPROFILE".into(), home.display().to_string()),
        ("HOME".into(), home.display().to_string()),
        (
            "APPDATA".into(),
            home.join("AppData").join("Roaming").display().to_string(),
        ),
        (
            "LOCALAPPDATA".into(),
            home.join("AppData").join("Local").display().to_string(),
        ),
        ("TEMP".into(), home.join(".tmp").display().to_string()),
        ("TMP".into(), home.join(".tmp").display().to_string()),
    ]
}

/// `USERNAME`/`USERDOMAIN`, resolved directly from the process token via
/// `GetUserNameExW` rather than trusted from the inherited environment.
///
/// Reported live: Anaconda's silent installer (`/S /D=<target>`) exited
/// with code 2 on every attempt, `install.log` showing `CreateDirectory:
/// can't create "$INSTDIR\tmp" (err=5)` -- ACCESS_DENIED -- immediately
/// after `$INSTDIR` itself was created, before a single package was
/// written. Constructor-built installers (Anaconda/Miniconda) hardened
/// against CVE-2025-64343 by revoking generic write on `$INSTDIR` for
/// Authenticated Users/BUILTIN Users/Domain Users right after creating it
/// (`main.nsi.tmpl`'s `AccessControl::RevokeOnFile`/`SetOnFile ...
/// "GenericRead + GenericExecute"`), then compensate for a non-elevated
/// run by granting `FullAccess` back to `$USERDOMAIN\$USERNAME` --
/// read from the environment with `ReadEnvStr`, not queried from Windows.
/// A process tree that never had those two variables set (observed here:
/// present env had `USERPROFILE` but no `USERNAME`/`USERDOMAIN` at all) or
/// which lost them somewhere upstream ends up compensating an empty
/// principal, and every write under `$INSTDIR` fails from that point on --
/// reproduced identically running the installer directly, bypassing naner
/// entirely, confirming the gap is in the ambient environment naner (or
/// whatever launched it) handed the subprocess, not in `naner`'s own
/// argument construction. `GetUserNameExW(NameSamCompatible)` returns
/// `DOMAIN\username` (or `COMPUTER\username` for a local account) from the
/// actual token regardless of what the parent process's environment
/// carried, so the compensating grant always targets a real, resolvable
/// principal. Empty when the call fails, which callers treat as "set
/// nothing" the same way `home_isolation_envs` does for a missing `home/`.
#[cfg(windows)]
fn identity_envs() -> Vec<(String, String)> {
    use windows_sys::Win32::Security::Authentication::Identity::{
        GetUserNameExW, NameSamCompatible,
    };

    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let ok = unsafe { GetUserNameExW(NameSamCompatible, buf.as_mut_ptr(), &mut len) };
    if ok == 0 {
        return Vec::new();
    }
    let sam = String::from_utf16_lossy(&buf[..len as usize]);
    match sam.split_once('\\') {
        Some((domain, user)) if !domain.is_empty() && !user.is_empty() => vec![
            ("USERDOMAIN".into(), domain.to_string()),
            ("USERNAME".into(), user.to_string()),
        ],
        _ => Vec::new(),
    }
}

#[cfg(not(windows))]
fn identity_envs() -> Vec<(String, String)> {
    Vec::new()
}

/// `ExeInstallerExtractor`: run the installer silently with per-vendor
/// argument defaults and `%TARGETDIR%`/`$TARGETDIR` substitution.
///
/// Deliberately does NOT pre-create `target_dir` the way the archive
/// extractors above do -- an installer .exe creates its own destination.
/// Reported live: Anaconda's silent installer (`/S /D=<target>`) exited
/// with code 2 on every attempt. Anaconda/Miniconda's constructor-based
/// installer refuses to proceed when the target directory already exists,
/// even empty -- interactively it prompts "directory already exists,
/// continue?", and in silent mode that prompt has no way to be answered,
/// so it aborts instead. Pre-creating an empty `target_dir` here tripped
/// that check on the very first install.
fn run_exe_installer(
    installer_path: &Path,
    target_dir: &Path,
    vendor_name: &str,
    installer_args: Option<&[String]>,
    naner_root: &Path,
) -> bool {
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

    // Every home-relative dotfile an installer might write on its own
    // initiative -- Anaconda's NSIS installer registers its base env into
    // `~/.conda/environments.txt` as its very last step, no user action
    // involved -- must land inside naner's tree, not the real profile.
    // `naner install`/`update-vendors` run this subprocess outside the
    // launcher path (`run_launcher`'s `setup_environment` never executes),
    // so without this override the installer inherits the host's raw
    // environment and writes there instead, unnoticed until someone goes
    // looking at `%USERPROFILE%\.conda\environments.txt` and finds every
    // naner install/reinstall ever run on the box listed in it.
    for (key, value) in home_isolation_envs(naner_root) {
        command.env(key, value);
    }

    // See `identity_envs`: a constructor-built installer (Anaconda,
    // Miniconda) needs a real `USERDOMAIN`/`USERNAME` to grant itself
    // write access back after its own CVE-2025-64343 hardening revokes it;
    // an inherited environment that lacks (or lost) those two variables
    // silently breaks every subsequent package write.
    for (key, value) in identity_envs() {
        command.env(key, value);
    }

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

    // A real installer (Anaconda's NSIS installer, `rustup-init.exe`) writes
    // an Add/Remove Programs entry and, for some, a Start Menu folder,
    // regardless of the target directory naner gave it -- unlike every
    // archive-extracted vendor, whose whole footprint is `target_dir`. Snap
    // both before running it so the diff below can strip exactly what this
    // run added, on success or failure alike (a failed install can still
    // have self-registered before it errored out).
    let registry_before = os_registration::uninstall_keys();
    let start_menu_before = os_registration::start_menu_entries();

    let result = match command.output() {
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
    };

    for key in os_registration::uninstall_keys().difference(&registry_before) {
        os_registration::delete_uninstall_key(key);
    }
    for entry in os_registration::start_menu_entries().difference(&start_menu_before) {
        let _ = if entry.is_dir() {
            std::fs::remove_dir_all(entry)
        } else {
            std::fs::remove_file(entry)
        };
    }

    result
}

/// OS-level state a real installer `.exe` can register outside the vendor's
/// own directory tree, diffed before/after rather than guessed by name: an
/// installer's own Start Menu folder name, or a versioned Add/Remove
/// Programs display name (e.g. "Anaconda3 2026.07-1 (Python 3.14.6
/// 64-bit)"), is per-vendor, per-release knowledge that breaks on the next
/// version bump. "Whatever showed up during this specific run" is not, and
/// never touches an entry that existed beforehand.
#[cfg(windows)]
mod os_registration {
    use std::collections::HashSet;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, KEY_ENUMERATE_SUB_KEYS, RegCloseKey,
        RegDeleteKeyW, RegEnumKeyExW, RegOpenKeyExW,
    };

    use crate::logger;

    const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open_uninstall(access: u32) -> Option<Key> {
        let subkey = wide(UNINSTALL_KEY);
        let mut handle: HKEY = std::ptr::null_mut();
        let rc =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &mut handle) };
        (rc == ERROR_SUCCESS).then(|| Key(handle))
    }

    /// Current Add/Remove Programs subkey names. Best-effort: any failure to
    /// open or enumerate the key just yields an empty set, which makes the
    /// later diff a no-op rather than an error.
    pub(super) fn uninstall_keys() -> HashSet<String> {
        let Some(key) = open_uninstall(KEY_ENUMERATE_SUB_KEYS) else {
            return HashSet::new();
        };
        let mut names = HashSet::new();
        let mut index = 0u32;
        loop {
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            let rc = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index,
                    buf.as_mut_ptr(),
                    &mut len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if rc != ERROR_SUCCESS {
                if rc != ERROR_NO_MORE_ITEMS {
                    logger::debug(
                        &format!("Uninstall key enumeration stopped early: {rc}"),
                        false,
                    );
                }
                break;
            }
            names.insert(String::from_utf16_lossy(&buf[..len as usize]));
            index += 1;
        }
        names
    }

    /// Delete one Add/Remove Programs subkey by name. Best-effort, and safe
    /// to call on a name that no longer exists. These entries carry no
    /// subkeys of their own (only values), so a plain `RegDeleteKeyW` --
    /// which refuses to delete a key that still has children -- is enough;
    /// no recursive `RegDeleteTree` is needed.
    pub(super) fn delete_uninstall_key(name: &str) {
        let Some(key) = open_uninstall(KEY_ALL_ACCESS) else {
            return;
        };
        let sub = wide(name);
        let rc = unsafe { RegDeleteKeyW(key.0, sub.as_ptr()) };
        if rc != ERROR_SUCCESS {
            logger::debug(&format!("Could not remove Uninstall\\{name}: {rc}"), false);
        }
    }

    /// Current Start Menu top-level entries (files and folders) for this
    /// user. Best-effort: no `APPDATA` or an unreadable directory just
    /// yields an empty set.
    pub(super) fn start_menu_entries() -> HashSet<PathBuf> {
        let Ok(appdata) = std::env::var("APPDATA") else {
            return HashSet::new();
        };
        let programs = Path::new(&appdata).join(r"Microsoft\Windows\Start Menu\Programs");
        let Ok(entries) = std::fs::read_dir(&programs) else {
            return HashSet::new();
        };
        entries.filter_map(|e| e.ok()).map(|e| e.path()).collect()
    }
}

/// Non-Windows builds (Linux CI) never spawn a real installer `.exe`; every
/// helper is a no-op so call sites need no `#[cfg(windows)]` of their own.
#[cfg(not(windows))]
mod os_registration {
    use std::collections::HashSet;
    use std::path::PathBuf;

    pub(super) fn uninstall_keys() -> HashSet<String> {
        HashSet::new()
    }
    pub(super) fn delete_uninstall_key(_name: &str) {}
    pub(super) fn start_menu_entries() -> HashSet<PathBuf> {
        HashSet::new()
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

    #[test]
    fn tar_path_strips_the_suffix_case_insensitively() {
        assert_eq!(
            tar_path_for(Path::new("/d/msys2.TAR.XZ")),
            Path::new("/d/msys2.tar")
        );
        assert_eq!(
            tar_path_for(Path::new("/d/msys2.tar.xz")),
            Path::new("/d/msys2.tar")
        );
        // No suffix: fall back to swapping the extension.
        assert_eq!(
            tar_path_for(Path::new("/d/msys2.zip")),
            Path::new("/d/msys2.tar")
        );
    }

    /// Regression: the suffix used to be located in a lowercased copy of the
    /// path, whose byte offsets drift from the original on characters like
    /// `ẞ` whose lowercase form is a different length.
    #[test]
    fn tar_path_handles_non_ascii_directories() {
        assert_eq!(
            tar_path_for(Path::new("/d/\u{1E9E}/msys2.tar.xz")),
            Path::new("/d/\u{1E9E}/msys2.tar")
        );
        assert_eq!(
            tar_path_for(Path::new("/d/\u{0130}x/msys2.TAR.xz")),
            Path::new("/d/\u{0130}x/msys2.tar")
        );
    }

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
        assert!(extract_archive(
            zip.path(),
            &target,
            "Tool",
            None,
            None,
            out.path()
        ));
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
        assert!(extract_archive(
            zip.path(),
            &target,
            "Multi",
            None,
            None,
            out.path()
        ));
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
        assert!(extract_archive(
            &archive,
            &target,
            "Pkg",
            None,
            None,
            dir.path()
        ));
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
            None,
            out.path()
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
            Path::new("Anaconda3-2026.07-1-Windows-x86_64.exe"),
            Path::new("C:\\v\\conda"),
            "Anaconda",
            None,
        );
        assert_eq!(conda, vec!["/S", "/D=C:\\v\\conda"]);
    }

    /// Reported live: `naner install anaconda` failed every attempt with
    /// "Installer exited with code 2". Anaconda's constructor-based silent
    /// installer aborts if its `/D=` target directory already exists, even
    /// empty -- and this function used to `create_dir_all` it before ever
    /// spawning the installer. A missing installer binary fails to spawn
    /// without touching the filesystem either way, so this only proves the
    /// directory-creation side effect is gone.
    #[test]
    fn run_exe_installer_does_not_precreate_the_target_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("vendor").join(".staging").join("anaconda");
        let missing_installer = tmp.path().join("does-not-exist.exe");

        let ok = run_exe_installer(
            &missing_installer,
            &target_dir,
            "Anaconda",
            None,
            tmp.path(),
        );

        assert!(!ok, "a missing installer binary should fail to spawn");
        assert!(
            !target_dir.exists(),
            "run_exe_installer must not pre-create target_dir"
        );
    }
}
