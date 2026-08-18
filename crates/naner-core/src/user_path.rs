//! Per-user Windows PATH editing for `naner add-to-path`.
//!
//! Edits the `Path` value under `HKCU\Environment` directly rather than
//! shelling out to `setx`, which silently truncates the stored value at
//! 1024 characters. The value's registry type is preserved (`REG_EXPAND_SZ`
//! stays `REG_EXPAND_SZ`, so other entries' `%VAR%` references keep
//! expanding), and a `WM_SETTINGCHANGE` broadcast tells Explorer and new
//! shells to re-read the environment. Shells already open keep the PATH
//! they started with — Windows has no mechanism to change that.
//!
//! Only the *user* PATH is touched, never `HKLM`, so no elevation is needed
//! and nothing naner does outlives deleting `NANER_ROOT` plus one
//! `add-to-path --remove`.

/// Compare form for one PATH entry: Windows paths are case-insensitive, a
/// trailing separator is meaningless, and surrounding quotes are cmd-era
/// armor for embedded spaces, not part of the path.
fn normalized(entry: &str) -> String {
    entry
        .trim()
        .trim_matches('"')
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

/// Whether `entry` is already one of the semicolon-separated entries.
pub fn contains(path_value: &str, entry: &str) -> bool {
    let want = normalized(entry);
    path_value.split(';').any(|e| normalized(e) == want)
}

/// The value with `entry` appended, or `None` when it is already present.
/// Everything already stored is preserved byte-for-byte; only a dangling
/// trailing `;` is absorbed rather than doubled.
pub fn appended(path_value: &str, entry: &str) -> Option<String> {
    if contains(path_value, entry) {
        return None;
    }
    let existing = path_value.trim_end().trim_end_matches(';');
    if existing.is_empty() {
        Some(entry.to_string())
    } else {
        Some(format!("{existing};{entry}"))
    }
}

/// The value with every occurrence of `entry` removed, or `None` when it is
/// not present. Other entries are preserved byte-for-byte; empty segments
/// (doubled semicolons) are dropped in passing.
pub fn removed(path_value: &str, entry: &str) -> Option<String> {
    if !contains(path_value, entry) {
        return None;
    }
    let want = normalized(entry);
    let kept: Vec<&str> = path_value
        .split(';')
        .filter(|e| !e.trim().is_empty() && normalized(e) != want)
        .collect();
    Some(kept.join(";"))
}

#[cfg(windows)]
pub mod registry {
    //! The `HKCU\Environment` side. Read returns the raw stored value (no
    //! `%VAR%` expansion — expansion would be destructive on write-back)
    //! along with its type so write can preserve it.

    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };

    const VALUE_NAME: &str = "Path";

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open_environment(access: u32) -> io::Result<Key> {
        let subkey = wide("Environment");
        let mut handle: HKEY = std::ptr::null_mut();
        let rc =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &mut handle) };
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        Ok(Key(handle))
    }

    /// The stored user `Path` value and its registry type. A missing value
    /// reads as an empty `REG_EXPAND_SZ` — the type a fresh profile gets.
    pub fn read_user_path() -> io::Result<(String, u32)> {
        let key = open_environment(KEY_QUERY_VALUE)?;
        let name = wide(VALUE_NAME);
        let mut kind: u32 = REG_EXPAND_SZ;
        let mut len: u32 = 0;
        let rc = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                std::ptr::null_mut(),
                &mut len,
            )
        };
        if rc == ERROR_FILE_NOT_FOUND {
            return Ok((String::new(), REG_EXPAND_SZ));
        }
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        let mut buf = vec![0u8; len as usize];
        let rc = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                buf.as_mut_ptr(),
                &mut len,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        let units: Vec<u16> = buf[..len as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        Ok((String::from_utf16_lossy(&units[..end]), kind))
    }

    /// Write the user `Path` value back. `kind` should be what
    /// [`read_user_path`] reported; anything that is not plain `REG_SZ` is
    /// written as `REG_EXPAND_SZ` so `%VAR%` entries keep expanding.
    pub fn write_user_path(value: &str, kind: u32) -> io::Result<()> {
        let kind = if kind == REG_SZ { REG_SZ } else { REG_EXPAND_SZ };
        let key = open_environment(KEY_SET_VALUE)?;
        let name = wide(VALUE_NAME);
        let data = wide(value);
        let rc = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                kind,
                data.as_ptr().cast::<u8>(),
                (data.len() * 2) as u32,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        Ok(())
    }

    /// Broadcast `WM_SETTINGCHANGE("Environment")` so Explorer — and thus
    /// anything launched from it after this call — re-reads the registry
    /// environment. Best-effort by design: a hung window must not hold the
    /// command hostage, hence the timeout, and the result is ignored.
    pub fn broadcast_environment_change() {
        let param = wide("Environment");
        let mut result: usize = 0;
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                param.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                5000,
                &mut result,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str = "C:\\naner\\vendor\\bin";

    #[test]
    fn matching_ignores_case_trailing_separators_and_quotes() {
        for stored in [
            "C:\\naner\\vendor\\bin",
            "c:\\NANER\\Vendor\\BIN",
            "C:\\naner\\vendor\\bin\\",
            "\"C:\\naner\\vendor\\bin\"",
            " C:\\naner\\vendor\\bin ",
        ] {
            let value = format!("C:\\Windows;{stored};C:\\Tools");
            assert!(contains(&value, ENTRY), "{stored:?} should match");
        }
    }

    #[test]
    fn a_prefix_is_not_a_match() {
        assert!(!contains("C:\\naner\\vendor;C:\\naner\\vendor\\bin2", ENTRY));
        assert!(!contains("", ENTRY));
    }

    #[test]
    fn appending_preserves_what_is_already_stored() {
        let value = "C:\\Windows;%JAVA_HOME%\\bin";
        assert_eq!(
            appended(value, ENTRY).as_deref(),
            Some("C:\\Windows;%JAVA_HOME%\\bin;C:\\naner\\vendor\\bin")
        );
    }

    #[test]
    fn appending_to_an_empty_or_missing_value_is_just_the_entry() {
        assert_eq!(appended("", ENTRY).as_deref(), Some(ENTRY));
    }

    #[test]
    fn a_dangling_trailing_semicolon_is_absorbed_not_doubled() {
        assert_eq!(
            appended("C:\\Windows;", ENTRY).as_deref(),
            Some("C:\\Windows;C:\\naner\\vendor\\bin")
        );
    }

    #[test]
    fn appending_is_idempotent() {
        let once = appended("C:\\Windows", ENTRY).unwrap();
        assert_eq!(appended(&once, ENTRY), None, "second add must be a no-op");
        assert_eq!(
            appended("c:\\naner\\VENDOR\\bin\\", ENTRY),
            None,
            "case/slash variants of an existing entry must count as present"
        );
    }

    #[test]
    fn removal_drops_every_occurrence_and_keeps_the_rest_verbatim() {
        let value = "C:\\Windows;C:\\naner\\vendor\\bin;%JAVA_HOME%\\bin;c:\\naner\\vendor\\bin\\";
        assert_eq!(
            removed(value, ENTRY).as_deref(),
            Some("C:\\Windows;%JAVA_HOME%\\bin")
        );
    }

    #[test]
    fn removing_an_absent_entry_is_a_no_op() {
        assert_eq!(removed("C:\\Windows;C:\\Tools", ENTRY), None);
        assert_eq!(removed("", ENTRY), None);
    }
}
