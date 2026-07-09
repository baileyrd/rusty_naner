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
//!    console (re-open `CONOUT$` if needed) and enable VT processing so ANSI
//!    colors work.
//!
//! Everything Win32 lives behind `#[cfg(windows)]`; on other platforms
//! [`setup`] is a no-op that reports [`ConsoleState::Inherited`], which keeps
//! the pure-logic tests and the Linux CI leg meaningful.
//!
//! Manual validation matrix for this spike (per MIGRATION_ANALYSIS §4.1, to be
//! run on a real Windows box): launched from a shell, double-clicked, piped
//! (`| more`), and redirected to a file — for both binaries.

/// Which binary is asking. The two exes have different command surfaces and
/// therefore different "needs a console" decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exe {
    Naner,
    NanerInit,
}

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

/// Decide from the first argument whether this invocation writes to a
/// console. Mirrors the C# per-exe command lists: every sub-command and every
/// launch that prints status needs one; the bare launcher path does too (it
/// logs `[*]` progress before spawning the terminal).
///
/// The exact lists get refined against golden outputs in Phase 2; the shape
/// (per-exe, keyed on a case-insensitive `args[0]`) is the contract.
pub fn needs_console(exe: Exe, args: &[String]) -> bool {
    let first = args.first().map(|s| s.to_ascii_lowercase());
    match exe {
        // Every current naner code path emits console output (even a plain
        // launch logs status), so the launcher always wants a console — the
        // redirection check in `setup` is what keeps pipelines clean.
        Exe::Naner => true,
        // naner-init: everything except a bare pass-through launch prints.
        // A bare launch also prints ("[*] Starting naner..."), so: always.
        Exe::NanerInit => {
            let _ = first;
            true
        }
    }
}

/// Prepare the console per the contract above and report how it went.
/// Must be called before anything is written to stdout/stderr.
pub fn setup(needs_console: bool) -> ConsoleState {
    if !needs_console {
        return ConsoleState::Inherited;
    }
    imp::setup()
}

#[cfg(windows)]
mod imp {
    use super::ConsoleState;
    use windows_sys::Win32::Foundation::{
        GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_WRITE, FILE_TYPE_CHAR, FILE_TYPE_UNKNOWN, GetFileType,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
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

    /// Open `CONOUT$` and install it as a std handle. Needed when the process
    /// attached/allocated a console but its std handle slots are still empty
    /// (GUI-subsystem processes get none from the loader).
    fn reopen_conout(which: u32) {
        if std_handle(which).is_some() {
            return;
        }
        // "CONOUT$\0" as UTF-16.
        let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        // SAFETY: valid NUL-terminated wide string; null security attrs and
        // template are documented as acceptable; the returned handle is
        // checked before use.
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE && !h.is_null() {
            // SAFETY: `h` is a live CONOUT$ handle; SetStdHandle just stores it.
            unsafe { SetStdHandle(which, h) };
        }
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
        enable_vt(STD_OUTPUT_HANDLE);
        enable_vt(STD_ERROR_HANDLE);

        if state == ConsoleState::Attached {
            // The parent shell already printed its prompt on the current
            // line; step past it exactly like the C# implementation.
            println!();
        }
        state
    }
}

#[cfg(not(windows))]
mod imp {
    use super::ConsoleState;

    pub fn setup() -> ConsoleState {
        ConsoleState::Inherited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn naner_always_wants_a_console() {
        for case in [
            args(&[]),
            args(&["--version"]),
            args(&["--export-env"]),
            args(&["INSTALL", "--list"]),
        ] {
            assert!(needs_console(Exe::Naner, &case), "case: {case:?}");
        }
    }

    #[test]
    fn naner_init_always_wants_a_console() {
        for case in [args(&[]), args(&["update"]), args(&["--version"])] {
            assert!(needs_console(Exe::NanerInit, &case), "case: {case:?}");
        }
    }

    #[test]
    fn allocated_gates_the_exit_pause() {
        assert!(ConsoleState::Allocated.allocated());
        assert!(!ConsoleState::Attached.allocated());
        assert!(!ConsoleState::Redirected.allocated());
        assert!(!ConsoleState::Inherited.allocated());
    }
}
