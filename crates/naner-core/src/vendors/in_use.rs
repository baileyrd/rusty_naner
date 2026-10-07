//! Files held open by running processes: finding who holds a vendor's tree,
//! offering to close them, or deferring the replacement to the next launcher run.
//!
//! Replacing a vendor means deleting or overwriting files a running `pwsh.exe`
//! or `WindowsTerminal.exe` has mapped, which Windows refuses (os error 5,
//! 32, 33, 1224). Checking *before* the replacement starts -- rather than
//! reacting to the failure afterwards -- means the download is not wasted and
//! the tree is never left half-replaced. A replacement that cannot happen now
//! is queued in `vendor/.pending-upgrades` and applied by the launcher.

use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use crate::logger;

/// What to do when something is holding a vendor's folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InUsePolicy {
    /// Say who is holding it and stop. For pipes and scripts.
    #[default]
    Report,
    /// Ask: close them, retry at next launch, or skip.
    Prompt,
    /// Close them without asking (`--close-processes`).
    Close,
}

impl InUsePolicy {
    /// `Prompt` only when a person can answer; otherwise `Report`.
    pub fn interactive_default() -> Self {
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
            Self::Prompt
        } else {
            Self::Report
        }
    }
}

/// A running process with an executable under a vendor's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    pub name: String,
    /// This naner process, or one of its parents. Closing it would close the
    /// console naner is running in (or naner itself), so it is never killed.
    pub protected: bool,
}

/// What the caller should do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Nothing is holding the tree (or it was just freed): go ahead.
    Proceed,
    /// The user declined, or it could not be freed: leave the vendor alone.
    Skip,
    /// Queued for the next launcher run: leave the vendor alone for now.
    Scheduled,
}

/// Parse the tab-separated `pid<TAB>name<TAB>0|1` lines the lookup prints.
pub(crate) fn parse_holders(output: &str) -> Vec<Holder> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().split('\t');
            let pid = parts.next()?.trim().parse().ok()?;
            let name = parts.next()?.trim().to_string();
            let protected = parts.next().map(str::trim) == Some("1");
            (!name.is_empty()).then_some(Holder {
                pid,
                name,
                protected,
            })
        })
        .collect()
}

/// Running processes whose executable lives under `dir`. Best-effort (one
/// PowerShell call): empty when it cannot be asked or nothing matches.
pub fn find_holders(dir: &Path) -> Vec<Holder> {
    // Placeholders, not `format!`: the script is full of braces.
    let script = r#"
$dir = '@DIR@'
$anc = @{}
$cur = @SELF@
while ($cur -and -not $anc.ContainsKey($cur)) {
  $anc[$cur] = 1
  $p = Get-CimInstance Win32_Process -Filter "ProcessId=$cur" -ErrorAction SilentlyContinue
  if (-not $p) { break }
  $cur = $p.ParentProcessId
}
Get-Process | Where-Object { $_.Path -and $_.Path.StartsWith($dir, 'OrdinalIgnoreCase') } |
  ForEach-Object { "$($_.Id)`t$($_.ProcessName)`t$([int]$anc.ContainsKey([int]$_.Id))" }
"#
    .replace("@DIR@", &dir.display().to_string().replace('\'', "''"))
    .replace("@SELF@", &std::process::id().to_string());

    let Ok(output) = std::process::Command::new(system32(r"WindowsPowerShell\v1.0\powershell.exe"))
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encode_command(&script),
        ])
        .output()
    else {
        return Vec::new();
    };
    parse_holders(&String::from_utf8_lossy(&output.stdout))
}

/// A Windows system tool by absolute path. Naner rewrites `PATH`, and a bare
/// `powershell` was not on it: the launch failed and the lookup quietly
/// reported that nothing was holding the folder.
fn system32(tool: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Path::new(&root).join("System32").join(tool)
}

/// `powershell -EncodedCommand` takes base64 of the script's UTF-16LE bytes.
/// Passing the script via `-Command` instead loses its double quotes to
/// command-line parsing, which silently turned the lookup into "nobody is
/// holding anything".
pub(crate) fn encode_command(script: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Windows error codes for "something has this file open".
pub fn is_in_use_error(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5 | 32 | 33 | 1224))
}

fn describe(holders: &[Holder]) -> String {
    holders
        .iter()
        .map(|h| format!("{} ({})", h.name, h.pid))
        .collect::<Vec<_>>()
        .join(", ")
}

fn kill(holders: &[Holder]) {
    for holder in holders.iter().filter(|h| !h.protected) {
        let _ = std::process::Command::new(system32("taskkill.exe"))
            .args(["/F", "/PID", &holder.pid.to_string()])
            .output();
    }
}

/// One answer to the prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Close,
    Reboot,
    Skip,
}

pub(crate) fn parse_choice(input: &str, can_close: bool, allow_defer: bool) -> Choice {
    match input.trim().to_ascii_lowercase().as_str() {
        "k" | "kill" | "c" | "close" if can_close => Choice::Close,
        "r" | "reboot" | "l" | "logon" | "launch" if allow_defer => Choice::Reboot,
        _ => Choice::Skip,
    }
}

fn ask(can_close: bool, allow_defer: bool) -> Choice {
    let options = match (can_close, allow_defer) {
        (true, true) => "[k] close them and continue, [r] replace at next launch, [s] skip",
        (true, false) => "[k] close them and continue, [s] skip",
        (false, true) => "[r] replace at next launch, [s] skip",
        (false, false) => "[s] skip",
    };
    print!("  {options} (default: skip): ");
    let _ = std::io::stdout().flush();
    // Same sequence `naner update`'s prompt needed: a plain `stdin` read can
    // block forever in a GUI-subsystem process while the console input is
    // fought over, so read the console directly when there is one.
    crate::console::force_foreground();
    crate::console::refresh_std_handles();
    if let Some(line) = crate::console::read_line_raw() {
        return parse_choice(&line, can_close, allow_defer);
    }
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return Choice::Skip;
    }
    parse_choice(&line, can_close, allow_defer)
}

/// Make sure nothing is holding `dir` before `vendor` is replaced.
///
/// `allow_defer` offers "replace at next launch". Only a wholesale upgrade can
/// be queued: the queue runs `upgrade_vendor`, which is not what an update
/// through a vendor's own CLI updater means.
pub fn resolve(
    vendor: &str,
    dir: &Path,
    naner_root: &Path,
    policy: InUsePolicy,
    allow_defer: bool,
) -> Resolution {
    let holders = find_holders(dir);
    if holders.is_empty() {
        return Resolution::Proceed;
    }

    logger::warning(&format!("  {vendor} is in use by: {}", describe(&holders)));
    let can_close = holders.iter().any(|h| !h.protected);
    if holders.iter().any(|h| h.protected) {
        logger::warning(
            "  Some of those are running naner itself or the console it is in; \
             they cannot be closed from here.",
        );
    }

    let choice = match policy {
        InUsePolicy::Report => {
            logger::info(
                "  Close them and retry, or re-run with --close-processes to close them for you.",
            );
            return Resolution::Skip;
        }
        InUsePolicy::Close if can_close => Choice::Close,
        InUsePolicy::Close => Choice::Skip,
        InUsePolicy::Prompt => ask(can_close, allow_defer),
    };

    match choice {
        Choice::Close => {
            kill(&holders);
            std::thread::sleep(std::time::Duration::from_millis(800));
            let remaining = find_holders(dir);
            if remaining.is_empty() {
                logger::info(&format!("  Closed what was holding {vendor}"));
                Resolution::Proceed
            } else {
                logger::failure(&format!("  Still in use by: {}", describe(&remaining)));
                Resolution::Skip
            }
        }
        Choice::Reboot => match add_pending(naner_root, vendor) {
            Ok(()) => {
                logger::info(&format!(
                    "  Queued: {vendor} will be replaced the next time naner launches,                      before the terminal opens (once nothing is holding it)."
                ));
                Resolution::Scheduled
            }
            Err(e) => {
                logger::failure(&format!("  Could not queue it: {e}"));
                Resolution::Skip
            }
        },
        Choice::Skip => {
            logger::info(&format!("  Skipping {vendor}"));
            Resolution::Skip
        }
    }
}

/// Vendors whose upgrade is waiting for the next launcher run, because a
/// running process held their folder when it was asked for.
const PENDING_FILE: &str = ".pending-upgrades";

/// Give up on a pending upgrade after this many launcher runs that could not
/// complete it, so a permanently failing one cannot slow every launch forever.
pub const MAX_PENDING_ATTEMPTS: u32 = 3;

fn pending_path(naner_root: &Path) -> std::path::PathBuf {
    naner_root.join("vendor").join(PENDING_FILE)
}

/// `vendor<TAB>attempts` per line.
pub(crate) fn parse_pending(text: &str) -> Vec<(String, u32)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?.trim();
            let attempts = parts
                .next()
                .and_then(|a| a.trim().parse().ok())
                .unwrap_or(0);
            (!name.is_empty()).then(|| (name.to_string(), attempts))
        })
        .collect()
}

pub(crate) fn format_pending(entries: &[(String, u32)]) -> String {
    entries
        .iter()
        .map(|(name, attempts)| format!("{name}\t{attempts}\n"))
        .collect()
}

/// What is waiting for the next launcher run.
pub fn load_pending(naner_root: &Path) -> Vec<(String, u32)> {
    std::fs::read_to_string(pending_path(naner_root))
        .map(|text| parse_pending(&text))
        .unwrap_or_default()
}

/// Replace the pending list; an empty list removes the file so the launcher's
/// check stays a single failed `read`.
pub fn save_pending(naner_root: &Path, entries: &[(String, u32)]) -> std::io::Result<()> {
    let path = pending_path(naner_root);
    if entries.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    std::fs::write(path, format_pending(entries))
}

/// Queue `vendor` for the next launcher run (no duplicates).
fn add_pending(naner_root: &Path, vendor: &str) -> std::io::Result<()> {
    let mut entries = load_pending(naner_root);
    if !entries.iter().any(|(n, _)| n.eq_ignore_ascii_case(vendor)) {
        entries.push((vendor.to_string(), 0));
    }
    save_pending(naner_root, &entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holders_are_parsed_with_their_protection_flag() {
        let parsed = parse_holders("100\tpwsh\t1\n200\tWindowsTerminal\t0\n\nbad line\n");
        assert_eq!(
            parsed,
            vec![
                Holder {
                    pid: 100,
                    name: "pwsh".into(),
                    protected: true
                },
                Holder {
                    pid: 200,
                    name: "WindowsTerminal".into(),
                    protected: false
                },
            ]
        );
    }

    #[test]
    fn encoded_command_is_base64_of_utf16le() {
        // "Hi" -> 48 00 69 00 -> SABpAA==
        assert_eq!(encode_command("Hi"), "SABpAA==");
        // Quotes survive: the reason this exists.
        assert_eq!(encode_command("\"").len(), 4);
    }

    #[test]
    fn system_tools_resolve_under_system32() {
        let p = system32("taskkill.exe");
        assert!(p.ends_with("System32/taskkill.exe") || p.ends_with(r"System32	askkill.exe"));
    }

    #[test]
    fn choices_default_to_skip_and_respect_what_can_be_closed() {
        assert_eq!(parse_choice("k\n", true, true), Choice::Close);
        assert_eq!(parse_choice("K", true, true), Choice::Close);
        // An update cannot be queued: "r" must not be honoured there.
        assert_eq!(parse_choice("r", true, false), Choice::Skip);
        // Nothing closable: "k" must not be honoured.
        assert_eq!(parse_choice("k", false, true), Choice::Skip);
        assert_eq!(parse_choice("r", false, true), Choice::Reboot);
        assert_eq!(parse_choice("", true, true), Choice::Skip);
        assert_eq!(parse_choice("whatever", true, true), Choice::Skip);
    }

    #[test]
    fn pending_upgrades_round_trip_and_tolerate_junk() {
        let entries = vec![
            ("Windows Terminal".to_string(), 0),
            ("PowerShell".to_string(), 2),
        ];
        assert_eq!(parse_pending(&format_pending(&entries)), entries);
        // A bare name (hand-edited file) means "no attempts yet"; blanks vanish.
        assert_eq!(
            parse_pending(
                "PowerShell

  
Go	x
"
            ),
            vec![("PowerShell".to_string(), 0), ("Go".to_string(), 0)]
        );
    }

    #[test]
    fn queueing_dedupes_and_an_empty_list_removes_the_file() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("vendor")).unwrap();
        add_pending(root.path(), "PowerShell").unwrap();
        add_pending(root.path(), "powershell").unwrap();
        assert_eq!(
            load_pending(root.path()),
            vec![("PowerShell".to_string(), 0)]
        );
        save_pending(root.path(), &[]).unwrap();
        assert!(load_pending(root.path()).is_empty());
        assert!(!root.path().join("vendor/.pending-upgrades").exists());
        // Removing what is not there is not an error.
        save_pending(root.path(), &[]).unwrap();
    }

    #[test]
    fn in_use_errors_are_recognised_by_windows_code() {
        for code in [5, 32, 33, 1224] {
            assert!(is_in_use_error(&std::io::Error::from_raw_os_error(code)));
        }
        assert!(!is_in_use_error(&std::io::Error::from_raw_os_error(2)));
        assert!(!is_in_use_error(&std::io::Error::other("x")));
    }
}
