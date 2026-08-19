//! Console subsystem management — the port of the C# `ConsoleManager`
//! (`Naner.Infrastructure`), and the subtlest behavior in the codebase
//! (MIGRATION_ANALYSIS §4.1).
//!
//! Both binaries are built with the Windows GUI subsystem
//! (`#![windows_subsystem = "windows"]`), so no console exists at startup.
//! The contract, in order:
//!
//! 1. Decide from `args[0]` whether this invocation produces console output
//!    at all ([`needs_console`] — pure logic, unit-tested on every platform).
//! 2. If stdout is already redirected (pipe or file), **do not attach** — this
//!    is what keeps `naner --export-env | Invoke-Expression` and file
//!    redirection working.
//! 3. Otherwise try `AttachConsole(ATTACH_PARENT_PROCESS)` (launched from a
//!    shell); on success print one leading newline to clear the shell's
//!    prompt line.
//! 4. Otherwise `AllocConsole()` (double-click launch). naner-init's
//!    "Press any key to exit" pause fires **only** in this case.
//! 5. After attach/alloc, make sure the std handles actually point at the
//!    console (re-open `CONOUT$`/`CONIN$` if needed) and enable VT
//!    processing so ANSI colors work.
//!
//! Everything Win32 lives behind `#[cfg(windows)]`; on other platforms
//! [`setup`] is a no-op that reports [`ConsoleState::Inherited`], which keeps
//! the pure-logic tests and the Linux CI leg meaningful.
//!
//! Manual validation matrix for this spike (per MIGRATION_ANALYSIS §4.1, to be
//! run on a real Windows box): launched from a shell, double-clicked, piped
//! (`| more`), and redirected to a file — for both binaries. Add: `naner
//! update`/`naner init` run from an attached shell, through the #81
//! `CREATE_NEW_CONSOLE` relaunch in `bootstrap.rs` — the scenario that
//! exposed stdin never being reopened (`Y`/Enter at the interactive prompt
//! had no effect, no error, in the relaunched console).

/// How this process ended up with (or without) a console.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleState {
    /// stdout was already captured (pipe/file); we deliberately left the
    /// console alone so the stream stays clean.
    Redirected,
    /// Attached to the parent process's console (launched from a shell).
    Attached,
    /// Allocated a fresh console (double-click launch). The only state in
    /// which naner-init's exit pause should fire.
    Allocated,
    /// Non-Windows platforms, or Windows when no console work was needed.
    Inherited,
}

impl ConsoleState {
    /// True only for a console we created ourselves — the gate for
    /// naner-init's "Press any key to exit" pause.
    pub fn allocated(self) -> bool {
        matches!(self, ConsoleState::Allocated)
    }
}

/// Port of `ConsoleManager.NeedsConsole`: true when `args[0]` (lowercased)
/// matches one of the exe's console commands. Each binary owns its command
/// list and ORs in its extra conditions (first-run, `--debug`,
/// `--export-env`) exactly as its C# `Main` did.
pub fn arg_needs_console(args: &[String], console_commands: &[&str]) -> bool {
    let Some(first) = args.first() else {
        return false;
    };
    let first = first.to_lowercase();
    console_commands.iter().any(|c| first == c.to_lowercase())
}

/// Prepare the console per the contract above and report how it went.
/// Must be called before anything is written to stdout/stderr.
pub fn setup(needs_console: bool) -> ConsoleState {
    if !needs_console {
        return ConsoleState::Inherited;
    }
    imp::setup()
}

/// Block for a single keypress, without echoing it or requiring Enter --
/// what naner-init's "Press any key to exit..." pause promises. Falls back
/// to a line-buffered `stdin` read (the old behavior) if raw console-mode
/// input can't be set up for any reason, so the pause can never wedge the
/// process waiting on an API that isn't going to answer.
pub fn wait_for_keypress() {
    imp::wait_for_keypress();
}

#[cfg(windows)]
mod imp {
    use super::ConsoleState;
    use std::io::BufRead;
    use windows_sys::Win32::Foundation::{
        GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_CHAR, FILE_TYPE_UNKNOWN,
        GetFileType, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT,
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, INPUT_RECORD, KEY_EVENT,
        ReadConsoleInputW, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
        SetStdHandle,
    };

    fn std_handle(which: u32) -> Option<HANDLE> {
        // SAFETY: GetStdHandle has no preconditions; a null/invalid result is
        // handled by the caller.
        let h = unsafe { GetStdHandle(which) };
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            None
        } else {
            Some(h)
        }
    }

    /// Port of the C# `IsStdoutCaptured`: stdout is "captured" when it is a
    /// valid handle to something that is not a character device — i.e. a pipe
    /// or a file. No handle at all (GUI launch) is *not* captured.
    pub fn is_stdout_captured() -> bool {
        match std_handle(STD_OUTPUT_HANDLE) {
            None => false,
            // SAFETY: `h` was just validated as a live handle.
            Some(h) => {
                let ft = unsafe { GetFileType(h) };
                ft != FILE_TYPE_CHAR && ft != FILE_TYPE_UNKNOWN
            }
        }
    }

    /// Open `CONOUT$`/`CONIN$` and install it as a std handle. Needed when
    /// the process attached/allocated a console but its std handle slots are
    /// still empty (GUI-subsystem processes get none from the loader) --
    /// which is exactly the state a `CREATE_NEW_CONSOLE`-relaunched child
    /// finds itself in: it already owns the console the parent's
    /// `CreateProcess` call just created, so both `AttachConsole` (already
    /// attached) and `AllocConsole` (already has one) fail with
    /// `ERROR_ACCESS_DENIED`, and `setup()` falls through to
    /// `ConsoleState::Inherited` -- so *this* is the only place stdin ever
    /// gets wired up for that process.
    fn reopen_con(which: u32, device: &str, share_mode: u32) {
        if std_handle(which).is_some() {
            return;
        }
        let name: Vec<u16> = format!("{device}\0").encode_utf16().collect();
        // SAFETY: valid NUL-terminated wide string; null security attrs and
        // template are documented as acceptable; the returned handle is
        // checked before use.
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                share_mode,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE && !h.is_null() {
            // SAFETY: `h` is a live console handle; SetStdHandle just stores it.
            unsafe { SetStdHandle(which, h) };
        }
    }

    fn reopen_conout(which: u32) {
        reopen_con(which, "CONOUT$", FILE_SHARE_WRITE);
    }

    fn reopen_conin() {
        reopen_con(STD_INPUT_HANDLE, "CONIN$", FILE_SHARE_READ);
    }

    /// Enable VT (ANSI escape) processing so the Phase 1 logger's colors work
    /// on every Windows 10+ console without WinAPI color calls.
    fn enable_vt(which: u32) {
        if let Some(h) = std_handle(which) {
            let mut mode = 0u32;
            // SAFETY: `h` is a live handle; `mode` is a valid out-pointer.
            unsafe {
                if GetConsoleMode(h, &mut mode) != 0 {
                    SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
                }
            }
        }
    }

    pub fn setup() -> ConsoleState {
        if is_stdout_captured() {
            // Piped/redirected: leave everything alone so the stream is pure.
            return ConsoleState::Redirected;
        }

        // SAFETY: AttachConsole/AllocConsole have no preconditions; failure
        // is reported via the return value and handled here.
        let state = unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
                ConsoleState::Attached
            } else if AllocConsole() != 0 {
                ConsoleState::Allocated
            } else {
                return ConsoleState::Inherited;
            }
        };

        reopen_conout(STD_OUTPUT_HANDLE);
        reopen_conout(STD_ERROR_HANDLE);
        // The one std handle `reopen_conout` never covered. A
        // `CREATE_NEW_CONSOLE`-relaunched child (bootstrap.rs's #81
        // keystroke-race fix) reliably needs exactly this: it already owns
        // the console `CreateProcess` just made for it, `AllocConsole`
        // succeeds in claiming it, output ends up wired (`GetStdHandle`
        // returns something usable for OUT/ERR), but STD_INPUT_HANDLE stays
        // unset -- so every keystroke the user types vanishes with no error,
        // no race, nothing to see. `reopen_conin` closes that gap the same
        // way the output side has always been closed.
        reopen_conin();
        enable_vt(STD_OUTPUT_HANDLE);
        enable_vt(STD_ERROR_HANDLE);

        if state == ConsoleState::Attached {
            // The parent shell already printed its prompt on the current
            // line; step past it exactly like the C# implementation.
            println!();
        }
        state
    }

    pub fn wait_for_keypress() {
        if !try_read_single_key() {
            let _ = std::io::stdin().lock().read_line(&mut String::new());
        }
    }

    /// Temporarily clear `ENABLE_LINE_INPUT`/`ENABLE_ECHO_INPUT` on CONIN$ and
    /// block on `ReadConsoleInputW` for the first key-down event, restoring
    /// the original mode before returning. `false` means raw mode couldn't be
    /// established or the read failed outright -- the caller falls back to a
    /// line read rather than leaving the console stuck in raw mode.
    fn try_read_single_key() -> bool {
        let Some(h) = std_handle(STD_INPUT_HANDLE) else {
            return false;
        };
        let mut mode = 0u32;
        // SAFETY: `h` is a live handle; `mode` is a valid out-pointer.
        if unsafe { GetConsoleMode(h, &mut mode) } == 0 {
            return false;
        }
        let raw_mode = mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT);
        // SAFETY: `h` is a live handle; `raw_mode` is a valid console-mode
        // bitmask.
        if unsafe { SetConsoleMode(h, raw_mode) } == 0 {
            return false;
        }

        let mut record: INPUT_RECORD = unsafe { std::mem::zeroed() };
        let mut read = 0u32;
        let ok = loop {
            // SAFETY: `h` is a live handle; `record`/`read` are valid
            // out-pointers sized for a single-element read.
            if unsafe { ReadConsoleInputW(h, &mut record, 1, &mut read) } == 0 || read == 0 {
                break false;
            }
            // SAFETY: `record.EventType` tags the union as a
            // `KEY_EVENT_RECORD` before `Event.KeyEvent` is read.
            let is_key_down = record.EventType as u32 == KEY_EVENT
                && unsafe { record.Event.KeyEvent.bKeyDown } != 0;
            if is_key_down {
                break true;
            }
        };

        // SAFETY: `h` is a live handle; `mode` is the value `GetConsoleMode`
        // reported before this function changed it.
        unsafe { SetConsoleMode(h, mode) };
        ok
    }
}

#[cfg(not(windows))]
mod imp {
    use super::ConsoleState;
    use std::io::BufRead;

    pub fn setup() -> ConsoleState {
        ConsoleState::Inherited
    }

    pub fn wait_for_keypress() {
        let _ = std::io::stdin().lock().read_line(&mut String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn first_arg_is_matched_case_insensitively() {
        let commands = ["--version", "-v", "install"];
        assert!(arg_needs_console(&args(&["--version"]), &commands));
        assert!(arg_needs_console(&args(&["INSTALL", "--list"]), &commands));
        assert!(!arg_needs_console(
            &args(&["--profile", "install"]),
            &commands
        ));
        assert!(!arg_needs_console(&args(&[]), &commands));
    }

    #[test]
    fn allocated_gates_the_exit_pause() {
        assert!(ConsoleState::Allocated.allocated());
        assert!(!ConsoleState::Attached.allocated());
        assert!(!ConsoleState::Redirected.allocated());
        assert!(!ConsoleState::Inherited.allocated());
    }
}
