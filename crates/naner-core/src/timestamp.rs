//! Timestamp formatting without `chrono`. The only consumer format is
//! `%Y-%m-%d %H:%M:%S` for cosmetic "Generated"/"Created" comments.
//!
//! On Windows (the shipped platform) this matches `chrono::Local` via
//! `GetLocalTime`. On Unix, std has no local-time access without extra
//! dependencies, so we fall back to UTC — the strings are cosmetic and no
//! test asserts local-vs-UTC.

/// Current wall-clock time formatted as `YYYY-MM-DD HH:MM:SS`
/// (local time on Windows, UTC elsewhere).
pub fn now_local() -> String {
    imp::now()
}

/// [`now_local`] reduced to a filename-safe stamp, `YYYYMMDD-HHMMSS`.
///
/// Used for backup names, where the separators the display format uses are
/// either illegal (`:` on Windows) or awkward (spaces) in a path.
pub fn file_stamp() -> String {
    now_local()
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == ' ')
        .collect::<String>()
        .replacen(' ', "-", 1)
        .replace(' ', "")
}

fn format(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> String {
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;

    pub fn now() -> String {
        let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
        // SAFETY: GetLocalTime fills the provided SYSTEMTIME and cannot fail.
        unsafe { GetLocalTime(&mut st) };
        super::format(
            st.wYear as i64,
            st.wMonth as u32,
            st.wDay as u32,
            st.wHour as u32,
            st.wMinute as u32,
            st.wSecond as u32,
        )
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn now() -> String {
        let secs =
            match std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH) {
                Ok(d) => d.as_secs() as i64,
                Err(e) => -(e.duration().as_secs() as i64),
            };
        super::format_unix_utc(secs)
    }
}

/// Format Unix seconds as a UTC `YYYY-MM-DD HH:MM:SS` string.
#[cfg_attr(windows, allow(dead_code))]
fn format_unix_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    format(year, month, day, sod / 3600, (sod / 60) % 60, sod % 60)
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (y, m, d)
/// in the proleptic Gregorian calendar.
#[cfg_attr(windows, allow(dead_code))]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (y + i64::from(m <= 2), m, d)
}

/// Runs on every platform, unlike `tests` below: the characters this guards
/// against (`:` in particular) are illegal in a Windows path, which is the
/// platform naner actually ships to.
#[cfg(test)]
mod stamp_tests {
    #[test]
    fn file_stamp_is_path_safe_and_sortable() {
        let stamp = super::file_stamp();
        assert_eq!(stamp.len(), 15, "expected YYYYMMDD-HHMMSS, got {stamp:?}");
        assert_eq!(&stamp[8..9], "-");
        assert!(
            stamp.chars().all(|c| c.is_ascii_digit() || c == '-'),
            "stamp must contain nothing illegal in a Windows path: {stamp:?}"
        );
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn known_epochs_format_correctly() {
        assert_eq!(format_unix_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_unix_utc(951_782_399), "2000-02-28 23:59:59");
        assert_eq!(format_unix_utc(951_782_400), "2000-02-29 00:00:00"); // leap day
        assert_eq!(format_unix_utc(1_752_303_600), "2025-07-12 07:00:00");
        assert_eq!(format_unix_utc(4_102_444_799), "2099-12-31 23:59:59");
    }

    #[test]
    fn output_shape_matches_chrono_format() {
        let s = now_local();
        assert_eq!(s.len(), 19);
        let bytes = s.as_bytes();
        assert_eq!(bytes[4], b'-');
        assert_eq!(bytes[7], b'-');
        assert_eq!(bytes[10], b' ');
        assert_eq!(bytes[13], b':');
        assert_eq!(bytes[16], b':');
    }
}
