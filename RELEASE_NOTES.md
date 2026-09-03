# Release Notes

What shipped in each release of `rusty_naner`, newest first, with the reasoning
behind each change rather than just the diff. Unreleased work is listed by merged
PR until it is tagged. Terse per-category entries live in
[CHANGELOG.md](./CHANGELOG.md); this file is the narrative one.

## Unreleased

## v0.9.27 — 2026-09-03

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.26...v0.9.27).

Follow-up to 0.9.26's console-flash fix, reported live again against
that exact release: `naner update`'s "already up to date" result no
longer opened a second console window, but its final line still landed
spliced into the parent PowerShell tab's *next* prompt row instead of
appearing above it -- the prompt's own segments and naner's own text
sharing one line.

A before/after screenshot comparison against 0.9.25 ruled out 0.9.26's
change as the cause: the exact same splice happened whether the extra
console opened or not, so it was never about *where* naner's output
went, only about *when* the shell decided to draw its next prompt
relative to it.

The actual cause is a well-documented Windows quirk, not specific to
naner: a GUI-subsystem process (`#![windows_subsystem = "windows"]`,
naner's own choice, deliberately, to avoid a console flash on
double-click) that `AttachConsole`s to its parent shell's console is
never *waited for* by that shell. `cmd.exe`/PowerShell dispatch it and
move straight on to draw their own next prompt without knowing whether
it has actually finished writing, so naner's console output and the
shell's prompt redraw end up as two independent writers on one shared
console with no ordering guarantee between them (matches the
`AttachConsole`/GUI-subsystem class of bug documented against other
tools, e.g.
[microsoft/terminal#4921](https://github.com/microsoft/terminal/issues/4921)).

`FreeConsole()` right before exit -- after every byte naner will ever
write has already landed -- is the documented mitigation: it cannot
make the shell *wait*, but it hands the console back cleanly the
instant naner is actually done with it instead of leaving that to
whatever the OS does on process teardown, so there is nothing left for
the shell's own prompt-draw to race against. Every exit path in
`naner.exe` now funnels through a new `console::detach()` call
immediately before terminating, not just `update`/`init`'s.

**Known limitation, disclosed rather than silently assumed**: this
release's own build/CI environment has no way to launch a real
interactive Windows Terminal/PowerShell/oh-my-posh session, so this fix
was verified by `cargo fmt --all --check` (clean) and CI green on
`windows-latest` -- which confirm it compiles, links, and passes the
test suite -- but **not** by reproducing the original splice and
watching it disappear on a physical Windows box. If it recurs, report
it; the diagnosis above was reasoned from Windows/PowerShell's own
documented `AttachConsole` behavior and a screenshot comparison, not
from a fix verified against the actual bug in the same session.

## v0.9.26 — 2026-09-03

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.25...v0.9.26).

Reported live: running `naner update` (or `naner init`) from an
ordinary attached PowerShell tab flashed open a second, real console
window -- on every invocation, including the common case where there
was nothing to do. `naner update`'s "Naner is already up to date!"
result opened that window, printed the version check into it, and
closed it again in the same instant, with no "press any key" pause to
hold it open on that path. The flash raced the parent tab's own prompt
redraw, visibly corrupting the terminal -- output from the closing
console landing on the same line as PowerShell's next prompt instead of
above it.

The window itself is not a bug on its own: it's the `#81` fix, a
deliberate workaround for `cmd.exe`/PowerShell not waiting on a
GUI-subsystem process, so a Y/n prompt read from the parent shell's
console raced the shell's own next prompt for keystrokes. The bug was
in *when* naner decided to open it -- `execute_update` and
`execute_init` both called the re-exec unconditionally at the top of
the function, before either had done the read-only work that
determines whether a prompt will happen at all. `naner check-update`,
which never prompts, never had this problem -- confirming the fix:
move the re-exec past the non-interactive checks, to immediately before
each function's first actual prompt, so the extra console now opens
only when a prompt is genuinely imminent.

Verified on CI: `cargo fmt --all --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, and `cargo test --workspace` all green on
both `ubuntu-latest` and `windows-latest`. The `docs/VALIDATION.md`
interactive checklist -- specifically the console-window behavior it
covers -- was not re-run against this release on a physical Windows box;
flagged here rather than silently assumed.

## v0.9.25 — 2026-08-28

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.24...v0.9.25).

`naner reclaim [--dry-run]` is a new command: for the leaks the previous
entry below couldn't close with an environment variable (Claude's
`.claude.json`, and anything relying only on `USERPROFILE` when
Antigravity's bundled Gemini agent or a pre-`CODEX_HOME` Codex has
already left real data behind), it sweeps whatever already leaked into
`%NANER_ROOT%\home` and bridges the original real-profile location back
to it -- a directory junction for `.codex/`/`.gemini/`/`.claude/` (no
privilege needed, the same mechanism `Advanced.HomeJunctions` already
uses), a real symlink for the single-file `.claude.json` (NTFS reparse
points only redirect directories, so this one needs Developer Mode or
Administrator -- a failure there is reported, not fatal, since the move
itself already succeeded). Resolves the *real* profile directory via
`SHGetKnownFolderPath(FOLDERID_Profile)` rather than `USERPROFILE`,
which is unreliable here specifically because it may already be naner's
own redirected value if `naner reclaim` is run from inside an
already-launched naner shell. Never overwrites: a leaked copy that
conflicts with one naner's home already has is preserved under a
timestamped name, not discarded.

This is a mitigation, not a source fix: it plants a filesystem entry at
a real-profile path, a deliberate, opt-in exception to naner's "nothing
written outside `NANER_ROOT`" contract, made only when the user
explicitly runs the command.

Reported live: Claude Code, Codex CLI, and Gemini CLI all still leaving
dotfolders in the real Windows profile from a naner-launched shell, well
after the `CLAUDE_CONFIG_DIR`/`USERPROFILE` fixes in earlier releases.

Two separate causes, one per remaining tool (Claude's own leak stayed
fixed):

Codex CLI is the odd one out among naner's agentic-CLI vendors: it's a
native Rust binary shipped through npm, not a Node script, and it
resolves its home directory via the OS known-folder API rather than
reading `USERPROFILE` the way Node/Python/Go tools do. The `USERPROFILE`
redirect that already contains Claude, git, and everything else with only
an `os.homedir()`-style lookup simply never reaches it. Codex does
document its own override, though -- `CODEX_HOME` -- so the fix is the
same shape as `CLAUDE_CONFIG_DIR`: one more entry in the shipped
`Environment.EnvironmentVariables`, pointed at
`%NANER_ROOT%\home\.codex`.

Gemini CLI's leak had a different, more basic cause: naner never vendored
it at all. Unlike `ClaudeCode` and `Codex`, there was no `Gemini.json`, so
`naner install` couldn't put it in the tree's own `home\.npm-global`, and
anyone who wanted it had installed it with whatever `npm` and shell they
had lying around -- possibly never touching naner's redirected
environment in the first place. Gemini CLI itself has no config-dir
override upstream to plug in even if it had been a vendor
(`google-gemini/gemini-cli#2815` is still open), but it doesn't need one:
it's a Node CLI that reads `os.homedir()` like everything else the
`USERPROFILE` redirect already covers. Added the `Gemini` vendor
(`@google/gemini-cli`) so installing and running it goes through naner's
tree like `ClaudeCode`/`Codex` already do -- the existing `USERPROFILE`
redirect does the rest.

Verified on CI: `cargo fmt --all --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, and `cargo test --workspace` all green on
both `ubuntu-latest` and `windows-latest`, including new tests that
exercise the real `mklink /J` junction creation and idempotency on the
Windows runner. The full `docs/VALIDATION.md` interactive checklist
(console window behavior, first-run prompts, and the rest of what only a
live GUI session can show) was not re-run against this release on a
physical Windows box -- flagged here rather than silently assumed.

## v0.9.24 — 2026-08-27

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.23...v0.9.24).

Reported live: `naner --help`'s own documented PowerShell one-liner,
`Invoke-Expression (naner.exe --export-env)`, crashed every time with a
visible Rust panic -- `failed printing to stdout: The pipe is being
closed. (os error 232)`.

Root cause is on PowerShell's side, confirmed against
[PowerShell/PowerShell#25875](https://github.com/PowerShell/PowerShell/discussions/25875):
for a `/SUBSYSTEM:WINDOWS` process -- `naner.exe` is deliberately
GUI-subsystem, to avoid a console flash on double-click -- PowerShell's
subexpression-capture (`(...)`/`$(...)`) and native `>` redirection do
not reliably wait for the process before tearing down the handle they
redirected stdout to. naner loses that race and its `print!` panicked on
the resulting write failure. `console::is_stdout_captured` was already
correctly detecting the redirected case and leaving the console alone --
the crash happened after that, on the write itself, and can't be fixed by
any handle-management trick on naner's side.

What naner *can* control is not crashing when it loses an unwinnable
race: `handle_export_env` and `naner root` (the other documented
pipeline-composable primitive, `cd $(naner root)`) now write stdout
through a new `console::write_stdout_best_effort`, which swallows a write
failure instead of letting `print!`'s panic-on-error propagate -- the
same outcome Unix gets for free from the default `SIGPIPE` disposition.
`--help`'s PowerShell example now shows the reliable piped form
(`naner --export-env | Invoke-Expression`, what `setup_shell.rs` actually
installs into a user's profile) instead of the broken subexpression one.

Verified live on Windows: the exact panicking invocation now exits
cleanly, 3/3 runs, instead of crashing. The three working forms -- piped
`| Invoke-Expression`, `cmd.exe`'s `>` redirection, and bash's
`eval "$(naner --export-env)"` -- are unaffected.

**Known limitation**: the underlying PowerShell defect is not fixed by
this change and can't be fixed from naner's side at all -- PowerShell's
subexpression-capture and native `>` redirection remain unreliable for
any GUI-subsystem executable, naner included. `Invoke-Expression (naner
--export-env)` and `naner --export-env > file` in PowerShell will keep
silently doing nothing (no longer crashing, but no longer working
either); use the piped form or `cmd.exe` instead.

---

Ported two changes back from a live, in-use naner installation to the
default bundle:

A `scripts/` directory joins `bin/`, `config/`, `home/`, `icons/` as a
fifth top-level bundled/packed directory -- a place for a user's own
scripts that survives `naner pack`/re-extraction the same way the other
four do. Shipped empty (`.gitkeep`), same as `bin/`; naner doesn't
populate it or put it on `PATH` itself.

The default PowerShell profile (`home/.config/powershell/profile.ps1`)
picked up several general-purpose improvements noticed by diffing
against the same live installation, filtered down to what's actually
reusable -- machine-specific bits (a `C:\dev`/`C:\tools` shortcut, a
function tied to one unrelated project) were left out:

- Guarded imports for `posh-git`/`Terminal-Icons`/`z` -- loaded only if
  already present, since naner doesn't vendor any of the three.
- Persistent UTF-8 console output, needed for Nerd Font glyphs.
- `....` and `~` navigation shortcuts, and a `Get-EnvVars` helper
  (`env` alias).
- The hand-rolled `prompt` function is gone, replaced by a guarded
  `oh-my-posh init pwsh --config "jandedobbeleer" | Invoke-Expression`
  -- naner has vendored `oh-my-posh` since it was added as an optional
  vendor, but the shipped default profile never actually used it. Guarded
  on `Get-Command oh-my-posh` (unlike the live version) so a shell
  launched before `naner install ohmyposh` still gets a working, if
  plainer, prompt instead of an error on every startup.

Diffing also surfaced a real, unrelated bug in the profile as shipped: it
aliased `ll` to `Get-ChildItem` and *separately* defined a `function ll`
with nicer `Format-Table` output further down. PowerShell resolves an
alias before a same-named function, so the nicer `ll` has been dead code
since the file was first written -- fixed by renaming the alias to `l`.
`grep` is no longer aliased to `Select-String` either, for the same
shadowing reason: naner vendors a real `grep.exe` (Git for
Windows/MSYS2) on the same `PATH`, with unrelated flag syntax.

---

Follow-up to v0.9.23's `--allow-scripts=<package>` fix, which stopped
npm's install-script gate from leaving `@anthropic-ai/claude-code`'s
500-byte `bin/claude.exe` placeholder in place of the real ~330 MB native
binary -- but only for npm invocations naner itself makes
(`naner install`, `naner update-vendors`).

Reported live again: `claude update` (Claude Code's own self-updater)
broke the exact same way. It shells out to `npm install -g` directly,
with none of naner's CLI flags, so the earlier fix never reached it --
npm's log showed the identical "1 package had install scripts blocked
because they are not covered by allowScripts", and `claude --version`
failed again with Windows' generic "not a valid application for this OS
platform" (the loader trying to run the placeholder shell script as a PE
image).

`--allow-scripts` also reads from `.npmrc`, comma-separated
(`@npmcli/config/lib/parse-allow-scripts-list.js`), so every `Npm`-type
vendor install now also persists `allow-scripts=<package>` into
`home/.npmrc` -- npm's own userconfig location, since naner points
HOME/USERPROFILE at `home/` for every vendored tool. This covers any
future npm invocation for the package, not just the one naner is making
right now: a second vendor's entry appends to the existing list rather
than overwriting it, a package already listed is a no-op, and unrelated
lines already in `.npmrc` are left in place. Verified: a clean reinstall
writes `home/.npmrc` with `allow-scripts=@anthropic-ai/claude-code`, and
a subsequent `claude update` run outside naner no longer regresses
`bin/claude.exe` to the placeholder.

---

## v0.9.23 — 2026-08-24

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.22...v0.9.23).

Reported live: `claude --version` (after `naner install claudecode`)
failed with Windows' generic "This version of ... claude.exe is not
compatible with the version of Windows you're running" -- which is
Windows' loader trying to execute a shell script as if it were a PE
binary. `@anthropic-ai/claude-code`'s own `bin/claude.exe` is a tiny
500-byte placeholder script (`echo "Error: claude native binary not
installed."`) the package ships in place of the real ~330 MB
platform-native binary, meant to be overwritten by its own `postinstall`
script (`node install.cjs`, which links in the binary from a per-platform
optional dependency) during install.

npm's own log explained why that never happened: recent npm versions gate
install-time lifecycle scripts behind an `allowScripts` allowlist by
default, and `npm_install_command` never passed one -- `"1 package had
install scripts blocked because they are not covered by allowScripts"` --
silently leaving the placeholder script as the "installed" `claude.exe`.
Confirmed this is a real npm behavior change rather than a one-off: an
*earlier* install of the exact same package, captured in the same npm
cache's own logs before npm itself had been self-updated
(`npm install -g npm@latest`, also naner-driven), ran the postinstall
script fine.

Every `Npm`-type vendor install now passes `--allow-scripts=<package>` --
npm's own suggested remedy, scoped to exactly the package being installed
rather than a blanket allowlist for every install script naner might ever
run. Harmless no-op for a package with no lifecycle scripts to gate
(`codex`, unaffected by this bug in the first place, keeps working
identically).

Verified live: reinstalling `@anthropic-ai/claude-code` with the fix
replaced the 500-byte stub with the real 337,745,056-byte binary, and
`claude --version` now reports `2.1.241 (Claude Code)`.

## v0.9.22 — 2026-08-24

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.21...v0.9.22).

Follow-up to [#154](https://github.com/baileyrd/rusty_naner/issues/154):
even after v0.9.21 fixed `naner install MsvcBuildTools`'s two `msiexec`
bugs, a full install still could not produce a working compiler --
`cargo build`/`cargo check` failed linking every binary and build script
with `LNK1181: cannot open input file 'kernel32.lib'`.

Not a stale pin: fetched Microsoft's live `aka.ms/vs/17/release/channel`
manifest and confirmed the pinned `Windows SDK Desktop Libs x64` MSI's
SHA-256 matches exactly what's currently served. That package genuinely
never shipped `kernel32.lib`, `ntdll.lib`, `user32.lib`, `advapi32.lib`,
`ws2_32.lib`, or `userenv.lib`, confirmed by querying its own File table
(366 rows, zero matches) via the `WindowsInstaller.Installer` COM object
directly. Found the real owner the way the module's own doc comment
already describes: extracted `winsdksetup.exe` as a cabinet (its unnamed
first member is `BurnManifest.xml`), enumerated every `<MsiPackage>`, and
queried each fundamental-lib candidate's own File table -- all six live in
"Windows SDK for Windows Store Apps Libs" instead, despite the
Desktop-suggestive name on the package naner already fetches. New
`SDK_STORE_LIBS` component (msi + 6 external cabs) fetches it.

`msvcrt.lib` -- which rustc's MSVC linking always requests via
`/defaultlib:msvcrt` -- turned out to be missing for the exact same class
of reason one layer up: VC++ Tools' "Desktop" CRT package doesn't carry
it, only the parallel "Store" one does
(`Microsoft.VC.14.44.17.14.CRT.x64.Store.base.vsix`, added as a 5th
`VC_PACKAGES` entry, same plain-zip merge path every other VSIX already
uses).

Fixing the first gap exposed a real, separate correctness bug rather than
just a missing pin: `SDK_STORE_LIBS` and the pre-existing `SDK_LIBS` both
extract into the same `Lib\<ver>\um\x64` marker directory.
`extract_msi_component`'s "already there" check looked at that shared
`sdk_root` target *before* the current run's own fresh `scratch` output --
so by the time `SDK_STORE_LIBS` ran, `SDK_LIBS`'s prior merge (moments
earlier, same `install()` call) had already made the marker directory
exist, and the check read that as "nothing to do," silently skipping the
merge of `SDK_STORE_LIBS`'s own kernel32.lib/etc. entirely. Now checks
`scratch` first: it's wiped at the top of every call, so it only ever
reflects what *this* invocation's `msiexec` actually produced, regardless
of what a sibling component left in `sdk_root` beforehand.

Verified end-to-end, not just per-component: a full `cargo build --release
--workspace` and `cargo test --workspace` (231 tests, 0 failures) against
the fully-assembled toolchain both pass clean, `cargo clippy --workspace
--all-targets -- -D warnings` is silent, and the resulting `naner.exe`
runs and reports its own version correctly -- the first time this
environment has been able to build itself from source.

## v0.9.21 — 2026-08-24

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.20...v0.9.21).

Two installer vendors that had never actually completed a real install --
`Anaconda` and the newly-added `MsvcBuildTools` -- both failed on every
attempt, for unrelated reasons, both root-caused by reproducing the exact
failure against the real installer binaries rather than guessing from the
error text alone.

`naner install anaconda` failed every time with `Failed to extract
packages` (exit code 2). Anaconda's constructor-built installer hardens
`$INSTDIR` against CVE-2025-64343 by revoking write access for
Authenticated Users/BUILTIN Users immediately after creating it, then
compensates for a non-elevated run by granting `FullAccess` back to
`$USERDOMAIN\$USERNAME` -- read from the environment, not queried from
Windows. Whenever the process launching the installer never had those two
variables set, the compensating grant silently targets an empty
principal, and every subsequent package write fails. Confirmed by
launching the real installer directly with `USERDOMAIN`/`USERNAME`
explicitly set: a full, successful install, `Failed`-free. `naner`'s
`run_exe_installer` now sources both from `GetUserNameExW
(NameSamCompatible)` -- the actual token identity -- instead of trusting
whatever the parent process happened to hand it.

`naner install MsvcBuildTools` failed every attempt extracting the
Windows SDK with `msiexec extraction failed`
(`ERROR_INVALID_COMMAND_LINE`, 1639) before writing a single log line.
`KITSROOT`'s value always contains a space (`Windows Kits`), so
`Command::arg`'s automatic quoting wrapped the *entire* `KITSROOT=...`
token in one outer pair of quotes -- but msiexec's own command-line
parser, unlike most Windows programs, only accepts a quoted *value* half
(`PROPERTY="value"`) and rejects a quoted whole token outright. Fixed by
building that argument with `raw_arg`, quoting only the value. A second,
previously-masked bug sat right underneath: `fetch_msi_component`
downloaded each component's external `.cab` into an `Installers\`
subfolder beside the `.msi`, but `msiexec /a`'s admin install for this
package resolves the cab flat, directly beside the `.msi` -- confirmed via
`/lv` verbose logging (`Error 1311. Source file not found (cabinet)`).
Cabs are now downloaded flat alongside their `.msi`. Verified end-to-end
by hand against the real `Windows SDK Desktop Headers x64` MSI + cab:
`msiexec /a` now exits 0 and the SDK headers land under `Windows
Kits\10\Include\...\um` as expected.

## v0.9.20 — 2026-08-24

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.19...v0.9.20).

The v0.9.18 fix (self-update also reconciling `config/naner.json`/
`config/vendors/`, closing the exact gap that stopped `MsvcBuildTools` from
reaching an already-initialized tree) turned out not to reach anyone
*updating from* a pre-fix version -- confirmed live: updating straight
from v0.9.17 correctly landed the v0.9.19 binary, but `MsvcBuildTools`
still never showed up in `naner install --list`. Root cause:
`updater::update_from_release` only ever replaces the binary file on disk
-- the process performing that swap keeps executing its own,
now-superseded, in-memory code, because Windows has no way to hot-swap a
running exe's code section. A v0.9.17 process self-updating to v0.9.19
therefore ran the post-update reconciliation using v0.9.17's own
compiled-in vendor catalog (`SHIPPED_VENDORS_JSON`, an `include_str!`
baked in at compile time), not the one the update had just installed. The
fix landed in v0.9.18 was real and necessary, but by itself only helps
someone updating from v0.9.18 or later. `naner update` now re-invokes the
freshly-installed binary after the swap -- as `update-vendors
--sync-config-only`, a new undocumented flag that runs only the
config/vendor-defaults merge and never the full, slow vendor-reinstall
pass -- so the reconciliation always executes with the code that was
genuinely just shipped, regardless of how old the updating process was.

## v0.9.19 — 2026-08-24

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.18...v0.9.19).

Reported live: `naner update-vendors` reinstalled `RustyTerm` and `Rush` on
every run whether or not either was ever installed, and the freshly
force-installed `RustyTerm` then failed to launch with a GPU-related error.
Root cause: the hardcoded fallback vendor list
(`essential_vendor_definitions`, used both when `vendors.json` is entirely
broken and as `update-vendors`' "always keep current" set) carries six
entries, not four -- `RustyTerm`/`Rush` ride along as a safety net so a
broken config doesn't lose every optional terminal-adjacent tool at once --
but none of the six ever set `required`, so it silently defaulted to
`false` for all of them, including the four genuine bootstrap essentials.
`update-vendors`' essential-vendor selection treated "present in this
list" as "always keep current" rather than checking `required`, so
`RustyTerm` (`"enabled": true` like every other optional vendor, but
`"required": false` in its real shipped JSON) got force-installed on every
run for users who never asked for it. Fixed both ends: the four true
essentials (`SevenZip`/`PowerShell`/`WindowsTerminal`/`GitForWindows`) now
carry `required: true` in the hardcoded list, and vendor selection reads
`required` off the real, loaded config -- falling back to the hardcoded
value only for a vendor that config never mentions at all, so a totally
broken `vendors.json` still bootstraps correctly. The always-`false` flag
silently broke `naner repair`'s essential-vendor recovery too (it already
gated re-bootstrapping on this exact flag); fixed as the same change.

## v0.9.18 — 2026-08-24

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.17...v0.9.18).

Reported live: `MsvcBuildTools` (new in v0.9.17) didn't show up in `naner
install --list` after running `naner update`. Root cause: `naner update`/
`naner self-update` only ever swaps the `naner.exe` binary itself —
`updater::update_from_release` never touches `config/naner.json` or
`config/vendors/`. The reconciliation that brings a newly shipped vendor
definition into an already-initialized tree (`merge_config_defaults`,
originally added for #72 — "a bare naner.exe-swap upgrade never otherwise
touching either file") was wired into `naner update-vendors` only, not
into the actual bare-binary-swap command the doc comment describes. Now
called from both.

## v0.9.17 — 2026-08-24

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.16...v0.9.17).

New vendor: `MsvcBuildTools`, a portable MSVC compiler/linker (VC++ Tools
14.44.35207) and Windows SDK (10.0.26100.0) — the exact toolset
`x86_64-pc-windows-msvc` builds need, installed without the admin-only
`vs_buildtools.exe` bootstrapper. That installer needs admin no matter what
`--installPath` is given, since it registers itself with the VS Installer
service and writes MSI-based state machine-wide regardless of where the
toolset itself lands. Instead this fetches the individual payloads that
bootstrapper would fetch — VC++ Tools as plain-zip VSIX, Windows SDK
components as MSI + external CAB — and extracts them directly, the same
no-admin technique `mmozeiko/portable-msvc` and
`Data-Oriented-House/PortableBuildTools` use. VC++ Tools packages carry
their own published SHA-256 in the VS 17.14 channel manifest
(`aka.ms/vs/17/release/channel`); the Windows SDK's packages don't
(`Win11SDK_10.0.26100` publishes 229 anonymous content-hashed `.cab` files
with no names attached) — those pins came from extracting the Burn
manifest embedded inside `winsdksetup.exe` itself and matching its named
`MsiPackage` entries (`Windows SDK Desktop Headers x64`, ...) to the
hashed files the channel manifest actually serves. Dispatched by vendor
key in the installer, bypassing the generic single-artifact resolver
entirely — this vendor's shape (many payloads merging into one tree)
doesn't fit it — and not pinned by `naner.lock`, since there is no
upstream "latest" to compare a hardcoded pin table against.

Also: `naner update`'s config merge only ever added missing
`VendorPaths`/`Profiles` keys and appended `PathPrecedence` entries, never
new `Environment.EnvironmentVariables` keys. An already-initialized tree
never picked up a redirecting variable added after it was first created —
`USERPROFILE`/`TEMP`/`TMP`/`APPDATA`/`LOCALAPPDATA` (v0.9.14), the XDG
trio, `CLAUDE_CONFIG_DIR` — so dotfolders kept leaking into the real
Windows profile on those trees forever, on every bare `naner.exe` swap,
even after the shipped default had long since closed the leak for a
brand-new install. The merge now adds missing
`Environment.EnvironmentVariables` keys the same way it already does for
`VendorPaths`/`Profiles`.

## v0.9.16 — 2026-08-23

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.15...v0.9.16).

Reported live: `naner update`'s "Update now?" prompt was accepted (`Y`
echoed correctly, right where it was typed) and then did nothing — no
"Updating Naner" header, no error, no "Press any key to exit", just a
return to the calling shell's prompt. Reproduced the underlying shape
independently (a `CREATE_NEW_CONSOLE`-relaunched child alive and blocked,
invisible to whatever spawned it) via a hub-hosted pty. Every prior fix on
this bug (v0.9.6 through v0.9.13) touched handle association
(`refresh_std_handles`) or how input is read (`read_line_raw`) — never
window focus. A GUI-subsystem child only inherits the right to foreground
its own window for a short default grace period after `CreateProcess`
returns, and the version check's blocking GitHub API call, which runs
*before* the prompt, sits squarely inside that gap — the window keeps
rendering fine (rendering never needed focus, which is why the prompt text
and the echoed `Y` looked completely normal) while keystrokes go to
whatever window actually has focus. Added `console::force_foreground`
(`SetForegroundWindow`, called right before the interactive read) and
`console::allow_foreground` (`AllowSetForegroundWindow` from the parent
right after spawning the child, so the network-call delay can't cost it
eligibility). Verified the new code paths run without error through the
same hub repro; verifying the actual focus fix needs a live Windows
Terminal run.

The `OhMyPi` vendor installed the wrong package. npm's unscoped
`oh-my-pi` — by a different author, an extension for a different,
unrelated "pi" CLI — declares a bin named `oh-my-pi`, not `omp`; that's
what `naner install OhMyPi` had been installing all along. The actual
`omp` coding agent CLI is `@oh-my-pi/pi-coding-agent` (npm-provenance/SLSA
attested, homepage `omp.sh`), which only declares `engines.bun` — no
`engines.node` — so it can't run under naner's vendored Node at all.
`Npm`-type vendors now install through `bun add --global` instead of
`npm install -g` when their `dependencies` name `Bun` instead of `NodeJS`;
`OhMyPi.json` now points at the right package and depends on `Bun`.
Fixing that surfaced a second bug in the same shape as the `zed`
`pathPrecedence` gap from v0.9.15: `%NANER_ROOT%\home\.bun\bin`, where
`bun add --global` links its bins, was never on `PathPrecedence` — an
npm-via-bun vendor could install cleanly and still never resolve from a
naner-launched terminal. Verified end-to-end live: `bun add --global
@oh-my-pi/pi-coding-agent` installed `omp.exe`, and it runs
(`omp/18.0.3`).

Chasing that fix down surfaced a third, smaller bug: `naner-core`'s
`build.rs` only watched the `dist-assets/config/vendors/` directory for
`cargo:rerun-if-changed`, which Cargo does not reliably re-trigger on for
an in-place edit to a file already inside it — only entries being added or
removed are guaranteed to bump a directory's own mtime. Confirmed live:
editing `OhMyPi.json` and rebuilding kept shipping the stale embedded
catalog until the build script itself was touched by hand. `build.rs` now
also emits `cargo:rerun-if-changed` for every vendor file individually.

---

## v0.9.15 — 2026-08-23

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.14...v0.9.15).

`naner update-vendors` only ever refreshed the four hardcoded essential
vendors (7-Zip, PowerShell, Windows Terminal, Git for Windows) — every
optional vendor a user actually installed with `naner install` (Node.js,
Ruby, Go, Ruff, ...) was silently skipped no matter how many were on disk.
It now also updates every *installed, enabled* optional vendor, resolved
from `vendors.json` the same way `install --list` reports them; an
optional vendor that is merely available but never installed is still
left alone, so this doesn't turn into `install --all`.

Reported live: `naner install codex` failed with "response too big for
into_string". `fetch_npm` resolved npm-published vendors against
`GET /<package>` — the full packument, carrying every version ever
published with its own copy of the readme and dependency tree — which for
an actively-released package like `@openai/codex` blows past ureq's
`into_string` cap. It now resolves against `GET /<package>/latest`, the
same lightweight endpoint a bare `npm install` itself uses.

Fixing that surfaced a second, unrelated bug in the same path: once the
packument was small enough to fetch, `naner install codex` started failing
silently instead, with no error text at all. The shared HTTP client sent
`Accept: application/vnd.github+json` on every outbound request —
GitHub API calls, npm, PyPI, nodejs.org, go.dev, dotnet's channel
manifest, HTML scrapes, all of it. Every other resolver's server ignored
the unrecognized media type and served its normal JSON regardless;
`registry.npmjs.org`'s Fastly frontend does real content negotiation on
`Accept` and returned `406 Not Acceptable`, which `fetch_npm` read as "no
release found" and failed without logging why. That header is now sent
only to `api.github.com`; every other resolver gets a plain
`application/json` Accept.

New optional vendors: **Ruff** and **ty** (Astral's Python linter/formatter
and type checker), same GitHub-release-plus-`.sha256`-sidecar shape as the
existing Uv vendor.

Reported live: `naner install zed` failed checksum verification —
`Zed.json`'s pinned `checksum` didn't match the real, correctly-downloaded
`v1.16.1` artifact. Its `fallback.version` pin already matched latest, so it
wasn't drift `refresh-pins` would ever have caught: nothing refreshes a
static `checksum` on a GitHub-sourced vendor, ever, only the `fallback`
block. `refresh-pins` now also rewrites `checksum.value` when GitHub's
release API publishes a `digest` for the resolved asset (present on
immutable/attested releases) and it disagrees with the pinned one — an
operator's pin still wins at install time (`resolved_checksum`), refreshing
the pin file is that operator re-asserting it. A vendor with no `checksum`
object never gets one added. Same class of bug is latent in `ImageGlass`,
`Inkscape`, `Obsidian`, `Podman`, and `Zen`; `refresh-pins --dry-run` now
catches version-unchanged checksum staleness on all of them going forward.

Reported live: `naner install obsidian` failed with "no matching release
found upstream" despite a Windows build plainly existing.
`obsidianmd/obsidian-releases` interleaves desktop and mobile-only releases
in one repo, and GitHub's `/releases/latest` doesn't care which kind it
picks — it pointed at a mobile-only release carrying just an `.apk`, which
`assetPattern: "Obsidian-*.exe"` could never match. `fetch_github` now
falls back to scanning the full `/releases` list (skipping prereleases) for
the newest one that actually has a matching asset, when `/releases/latest`
doesn't. `ImageGlass` was failing the exact same way for the exact same
reason — fixed for both at once, not vendor-specific.

`naner install zed` reported success but there was no way to run `zed` from
a naner-launched terminal: `Zed.json` had no `pathPrecedence`, unlike
every other CLI-shaped vendor (`Uv`, `Bun`, `NodeJS`, ...), so
`merge_vendor_environment` had nothing of Zed's to add to the exported
`PATH`. `postInstallFunction: "Zed.PostInstall"`, present in the vendor
file, looked like it should have wired this up — it doesn't; that field
isn't read anywhere in the Rust port at all, dead config left over from
the original design. Added `pathPrecedence` pointing at
`vendor\zed\bin` (Zed's actual CLI-launcher directory, distinct from the
418 MB `Zed.exe` GUI binary at the vendor root) and a `provides: ["zed"]`
entry. Verified live: `zed --version` now resolves and runs from an
exported naner environment.

## v0.9.14 — 2026-08-21

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.13...v0.9.14).

Real-world validation of the vendor pipeline found naner's containment
guarantee had a hole big enough to drive Anaconda through: `naner install`
and `update-vendors` run a vendor's installer subprocess outside the
launcher path entirely, so a vendor whose `releaseSource` is a real
installer `.exe` (Anaconda, `rustup-init.exe`) never got naner.json's
`USERPROFILE`/`HOME`/`APPDATA`/`LOCALAPPDATA`/`TEMP`/`TMP` redirects the
way a launched terminal profile does. Anaconda's installer registers its
base environment into `~/.conda/environments.txt` as an unconditional last
step, so every `naner install anaconda` -- fresh install, reinstall, test
run -- wrote one more stale entry into the *real* Windows profile's
`.conda\environments.txt`, discovered live as six of them sitting there
from a single afternoon's testing. The same two installers also
self-register a Start Menu folder and an `HKCU` Add/Remove Programs entry
regardless of the install directory naner gave them, and because naner
installs into a `vendor/.staging/<name>` directory before renaming it into
place, Anaconda's own registry entry pointed at a path that no longer
existed the moment install finished.

`run_exe_installer` now applies the same home-tree redirect a launched
shell gets to every installer subprocess it spawns (extended to the
npm/pip package-manager install path too), and snapshots the Start Menu
folder and Add/Remove Programs key before running an installer, removing
whatever is new afterward -- diffed against the snapshot rather than
matched by name, since a versioned display name or an installer's own
folder name is exactly the kind of per-release detail that breaks on the
next version bump. Separately, `APPDATA`/`LOCALAPPDATA` -- the convention
most Windows dev tools actually use for their own config and cache, not
`USERPROFILE\.foo` -- were never redirected at all; both are now part of
the same home tree, with the directories guaranteed to exist on launch the
same way `TEMP` already was.

Verified against real, live installs on Windows, not fixtures: reinstalled
Anaconda before and after each fix and confirmed the real profile's
`environments.txt`, Start Menu, and registry stayed untouched, then cleared
the pollution the earlier, unfixed runs had already left behind.

## v0.9.13 — 2026-08-21

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.12...v0.9.13).

- Reported live, back on `naner update`'s long-running "Update now?" prompt
  issue (#130/#134): even after v0.9.9's broadened `refresh_std_handles`
  call, the prompt could still hang forever inside naner's own relaunched
  console -- but this time with neither of v0.9.9's diagnostics firing.
  No "could not refresh" warning, no "stdin read EOF/failed" warning, no
  exit at all -- just silence, with the prompt text sitting there exactly
  as it should. A screenshot of Task Manager settled what was actually
  happening: two `naner.exe` processes, the relaunched child genuinely
  alive and blocked, and no second window reachable by Alt+Tab -- so this
  was the one real, visible console, correctly rendering output, simply
  never receiving the keystrokes typed into it. That rules out both of
  v0.9.9's theories (a failed handle reopen, an immediate EOF) and the
  original "wrong console has focus" theory alike: the read itself was
  genuinely blocking, not failing.

  The fix looks sideways instead of deeper into the same mechanism.
  `console::wait_for_keypress` -- what naner-init's "Press any key to
  exit" pause uses, in this exact relaunched-console scenario -- has
  never shown this symptom since it shipped, and it reads differently
  from the Y/n prompt: raw `ReadConsoleInputW` against a freshly fetched
  handle, bypassing `std::io::stdin()`'s buffered line reader entirely.
  Added `console::read_line_raw`, that same raw-read primitive
  generalized from one keypress to a full line (echoing each character
  and handling Backspace/Enter by hand, since disabling
  `ENABLE_LINE_INPUT` hands that job to the caller). `prompt_yes` now
  tries it first whenever it's inside naner's own console, falling back
  to the original `stdin`-based path -- diagnostics and all -- only when
  `read_line_raw` reports `STD_INPUT_HANDLE` isn't a real console at all,
  which is exactly the piped/redirected case the existing EOF-is-no
  contract depends on and must keep working unchanged.

  This is a genuine fix attempt, not another diagnostics-only release --
  but it's still unconfirmed on real Windows, the same as every change in
  this area. If the prompt still hangs after this, the next lead is
  narrower still: something about how a `CREATE_NEW_CONSOLE`-relaunched,
  GUI-subsystem child's console interacts with keyboard input at all in
  this environment, since raw `ReadConsoleInputW` was the one mechanism
  with no prior report of failing here.

- Reported live: `naner install GitHub CLI`, typed straight out of `naner
  install --list`, failed with `Unknown vendor: GitHub` and `Unknown
  vendor: CLI`. Nothing was wrong with vendor resolution -- `naner install
  <name>` already accepts a vendor's JSON key (`GitHubCli`, no space) as
  well as its display name, exactly to give a space-free alternative for
  cases like this. The problem was the shell: an unquoted `GitHub CLI` on
  the command line is two arguments by the time naner ever sees it, one
  per word, and neither `GitHub` nor `CLI` alone matches anything. The
  list itself only ever showed the space-containing display name, so
  there was no way to know an unquoted alternative even existed short of
  reading the vendor JSON files directly. 11 of the shipped vendors have
  multi-word names (`GitHub CLI`, `Oh My Posh`, `Windows Terminal`, `.NET
  SDK`, and others), so this wasn't a one-vendor problem. `naner install
  --list` now prints the key in parentheses next to every name that
  contains a space -- `GitHub CLI (GitHubCli)` -- so the unquoted form is
  visible right where someone would otherwise copy the name that breaks.

- Asked, then decided against on reflection, then decided in favor of
  anyway: whether naner should redirect `USERPROFILE` the way it already
  redirects `HOME`. The concern raised first was real -- unlike `TEMP`
  (invisible plumbing nothing looks at directly), `USERPROFILE` is what
  Save/Open dialogs, browser downloads, and Explorer's quick-access list
  resolve against, so redirecting it changes where things land for any
  GUI app run from a naner shell. But naner's own vendor catalog *is*
  meant to be launched exactly that way (Zed, Zen Browser, Obsidian,
  Inkscape, and more all live on naner's PATH), and native Win32 dialogs
  mostly don't read the env var at all -- they resolve through the
  registry-backed Known Folder API, untouched either way. What *does*
  read `%USERPROFILE%`/`os.homedir()` directly is exactly the kind of
  thing naner exists to run: Node, Python, Go, git, and most CLI dev
  tooling. Redirecting it keeps that whole class contained inside naner's
  own tree instead of scattered across the real Windows profile, for the
  same reason `HOME` already is. `naner.json` now sets `USERPROFILE` to
  the same path as `HOME`.

  `TEMP`/`TMP` get the same treatment, redirected to
  `%NANER_ROOT%\home\.tmp`. Unlike the XDG cache/data directories already
  redirected here, no spec obligates a tool to create its own `%TEMP%`
  before writing to it -- a real Windows profile's temp directory is just
  always there -- so `setup_environment` now creates `home\.tmp`
  unconditionally at startup, the same way `home\` itself is guaranteed
  to exist.

  One side effect worth naming: this makes the `Advanced.IsolateEnvironment`
  + `USERPROFILE` fix from earlier this cycle moot going forward, in a
  good way. That fix was about *preserving* the host's `USERPROFILE`
  value through isolation; now `USERPROFILE` is unconditionally
  naner-owned, regardless of isolation state, so there's no host value
  left to lose in the first place.

- Immediate follow-up: redirecting `USERPROFILE` traded away exactly the
  thing GUI Save/Open dialogs and browser downloads use to find the real
  Documents/Downloads/Desktop -- the tradeoff named above, now bridged
  back rather than just accepted. `Advanced.HomeJunctions` creates
  directory junctions under `home\` linking specific real Windows
  locations back out from underneath the redirect: `Documents`,
  `Downloads`, and `Desktop` to their real counterparts by default, plus
  a personal `dev` to `C:\dev`.

  A junction rather than a symlink: `mklink /D` (and Rust's
  `std::os::windows::fs::symlink_dir`) needs `SeCreateSymbolicLinkPrivilege`
  -- admin, or Developer Mode enabled -- a real ask of anyone who just
  wants to double-click naner and go. `mklink /J` needs neither, and
  works fine for this: local, same-machine directories, no network paths
  involved. Targets can use the new `%HOST_USERPROFILE%` token --
  resolved from the real `USERPROFILE` value, captured at the very start
  of the launcher before naner's own redirect overwrites it for that
  process -- or a plain absolute path, as `dev` does.

  Created once, at first launch after init: a junction is a real
  filesystem entry, so every later launch's "does something already
  exist at this path" check is already true and skips it -- no repeated
  work, and nothing already there (a prior junction, or a real directory
  the user put there instead) ever gets overwritten. A target that
  doesn't exist yet (nobody's created `C:\dev` on this machine) is
  skipped the same way, not an error -- there's simply nothing to link
  to until it does.

## v0.9.12 — 2026-08-21

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.11...v0.9.12).

- Reported live: `naner install anaconda` failed every time with
  `Installer exited with code 2`, right after the download and checksum
  verification succeeded. Anaconda's own installer (constructor-based
  NSIS, invoked here as `/S /D=<target>`) refuses to proceed if its
  destination directory already exists -- interactively it shows a
  "directory already exists, continue?" prompt, and silent mode has no
  way to answer that, so it just aborts. Two layers of naner's own code
  were creating that directory ahead of time without realizing it: the
  shared staging step in `install_vendor` (`installer.rs`) unconditionally
  `create_dir_all`'d the staging target for every install type, and
  `run_exe_installer` (`archives.rs`) did the same again as its first
  line, in case it was ever called on its own. Between the two, an
  `.exe`-based installer could never find its own target directory
  missing. Neither pre-creation was actually needed for this path: every
  archive extractor (zip/tar.xz/msi) already creates its own destination
  internally, and an installer `.exe` is expected to create its own
  install directory the same way it would on a fresh machine -- only the
  `binary` install type (a plain file copy, no extraction or installer
  involved) genuinely needs the directory to exist first, so that's the
  only place still creating it.

## v0.9.11 — 2026-08-21

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.10...v0.9.11).

- Reported live right after v0.9.10 shipped: with `Advanced.IsolateEnvironment`
  on, double-clicking `naner.exe` threw `Could not access starting directory
  "C:\tools\naner\%USERPROFILE%"` instead of launching. `naner.json` sets
  every default profile's `StartingDirectory` to `%USERPROFILE%` and
  deliberately leaves that one variable for the host to expand rather than
  overriding it the way `HOME` is -- it's the one thing Windows tools that
  ignore `HOME` still resolve, and known-folder APIs depend on it. But
  `USERPROFILE` wasn't on `env_isolation::KEEP_ON_ISOLATE`, so isolation
  cleared it along with everything else; with it gone, `%USERPROFILE%` had
  nothing to expand into and stayed literal, and `wt.exe` -- launched with
  its working directory set to `naner_root` -- resolved that literal string
  as a path relative to `naner_root` instead of an unrecognized token.
  `USERPROFILE` joins the `ProgramFiles` family already on the keep list:
  a standard per-user OS variable every process expects to be set, not a
  signal of which dev tools are installed.

## v0.9.10 — 2026-08-20

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.9...v0.9.10).

- Chasing a live report of `naner install --list` showing `[OK] Rust` when
  Rust had never been installed turned up the actual bug behind it: an
  enabled vendor's PATH entries and environment variables were merged into
  the effective config the moment `enabled: true` was set, with no check
  for whether the vendor was actually present on disk. Without
  `Advanced.IsolateEnvironment` on, the reporter's own system-wide `rustup`
  was still first on PATH (naner's vendored Rust was never installed), so
  running it inherited naner's pre-set `CARGO_HOME`/`RUSTUP_HOME` and wrote
  its own state into naner's empty `vendor/rust` directory -- which then
  made `is_vendor_installed` report Rust as present, even though `naner
  install` had never touched it. `build_unified_path` already dropped
  nonexistent PATH directories from the final built PATH string, but the
  underlying `Environment.PathPrecedence` *data* still carried an
  uninstalled vendor's entry, and environment variables had no such filter
  at all. `merge_vendor_environment` now filters vendors through
  `is_vendor_installed` before contributing either PATH entries or
  variables -- `enabled` means "wanted", not "present", and a vendor
  contributes nothing until `naner install` has actually put something in
  its vendor directory.

## v0.9.9 — 2026-08-20

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.8...v0.9.9).

- `naner update`'s "Update now?" prompt is still reported stuck live, even
  after v0.9.7's CONIN$ refresh -- but new evidence changes the diagnosis. A
  screenshot showed that after the prompt appeared, typing a letter
  triggered the *calling shell's* own PSReadLine history-search popup, not
  naner reading it. That only happens once naner's own process has already
  exited and returned control of the pane to the parent shell -- meaning
  the process hit EOF on stdin (or errored) essentially immediately, rather
  than genuinely hanging waiting for input the whole time. This is not a
  confirmed fix -- it's targeted diagnostics plus a plausible tightening:
  `console::refresh_conin` is broadened to `refresh_std_handles`,
  reassociating stdin *and* stdout/stderr with fresh console handles
  together right before a prompt (previously only stdin was refreshed,
  which could leave a prompt's text and its actual input listening on two
  different console sessions). `prompt_yes` now also warns -- but only
  inside naner's own relaunched console, never for the deliberately-silent
  EOF-is-no path plain scripted/CI use depends on -- when the handle
  refresh fails or stdin reads EOF/errors immediately, so the next live
  report will say definitively which of those is actually happening
  instead of needing another screenshot to reconstruct it.

## v0.9.8 — 2026-08-20

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.7...v0.9.8).

- Reported live while testing `Advanced.IsolateEnvironment` (#128) on real
  Windows: a fresh isolated shell threw `x86: The term 'x86' is not
  recognized...`. Root cause: the isolation allowlist
  (`env_isolation::KEEP_ON_ISOLATE`) dropped `ProgramFiles(x86)` (and its
  siblings `ProgramFiles`, `CommonProgramFiles`, `CommonProgramFiles(x86)`,
  `ProgramW6432`, `CommonProgramW6432`) along with everything else under
  isolation -- but those are standard OS directory locations a lot of
  scripts read (PowerShell itself needs `${env:ProgramFiles(x86)}` syntax
  to reference the paren-containing name at all; an unset read apparently
  surfaced as a bare `x86` command elsewhere). None of them reveal which
  dev tools are installed, so they belong in the "always keep" list
  alongside `PROGRAMDATA`/`ALLUSERSPROFILE`, which are already there.
  Added.

## v0.9.7 — 2026-08-20

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.6...v0.9.7).

- Reported live right after v0.9.6 shipped: `naner update`'s "Update now?
  (Y/n):" prompt got stuck -- no keystroke did anything, even after
  clicking directly on the console window to make sure it had focus. Ruled
  out: no second console opened (this is a double-click launch, not the
  attached-shell case the #81 relaunch targets), and no error printed.
  The one structural difference from `naner init`'s first prompt (which is
  known to work on the same kind of launch): `naner update` makes a
  blocking network call (the release check) before its only prompt, while
  `naner init`'s first prompt has none before it. Added
  `console::refresh_conin`, called right before every interactive prompt
  read, which unconditionally re-opens `CONIN$` and re-installs it as
  `STD_INPUT_HANDLE` -- the same mechanism (already used, more narrowly, by
  `console::setup`) that fixed the structurally similar #81
  `CREATE_NEW_CONSOLE`-relaunch stdin issue. This targets the reported
  symptom on the best lead available; the underlying reason a blocking
  network call would affect the input handle at all is not confirmed, so
  this is worth re-testing live once it ships.

## v0.9.6 — 2026-08-20

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.5...v0.9.6).

- Requested for testing on a dev machine that already has Git/Node/PowerShell/
  Rust etc. installed system-wide: naner's `Advanced.InheritSystemPath`
  already isolated PATH resolution, but everything else inherited from the
  host (HOME-equivalents, and any `GIT_*`/`CARGO_HOME`/`RUSTUP_HOME`/
  `PYTHONHOME`/npm-config-style variable a prior install left set) still
  leaked through with no way to turn it off. Added `Advanced.IsolateEnvironment`
  (`naner.json`) / `NANER_ISOLATE_ENVIRONMENT` env override: when on, naner
  clears its own process environment down to a small OS-survival allowlist
  (`SystemRoot`, `ComSpec`, `TEMP`, etc. — nothing that reveals installed
  tools) before setting NANER_ROOT/HOME/configured variables/PATH, so a
  spawned terminal only ever sees naner's own environment. Since a profile
  picked directly from Windows Terminal's own list runs through
  `--export-env | Invoke-Expression` in an already-environed shell rather
  than through naner's own isolated process, `--export-env`'s output also
  emits removal statements (`Remove-Item Env:`/`unset`/`SET "NAME="` per
  shell) for the same variables, so that path is isolated too. Restoring
  after a test run needs no special handling: naner always isolates a
  freshly spawned process (or, for `--export-env`, only the shell it's
  piped into), never anything persistent — closing that window/tab is
  enough.

## v0.9.5 — 2026-08-19

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.4...v0.9.5).

- Reported live right after v0.9.4 shipped: at the "Press any key to
  exit..." pause (the one naner-init shows in a console of its own before
  closing), pressing a key did nothing — the screenshot showed keystrokes
  piling up as literal echoed characters on screen, and only Enter actually
  closed it. The message promised a single keypress; the implementation was
  `std::io::stdin().lock().read_line(...)`, a line-buffered read that
  needs Enter and echoes every character typed while it waits. Added
  `naner_core::console::wait_for_keypress`, which briefly clears
  `ENABLE_LINE_INPUT`/`ENABLE_ECHO_INPUT` on `CONIN$`, blocks on
  `ReadConsoleInputW` for the first key-down event, and restores the
  original console mode before returning — a real single-keypress wait,
  with the old line-read kept as a fallback if raw mode can't be
  established for some reason.

## v0.9.4 — 2026-08-19

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.3...v0.9.4).

- Reported live right after v0.9.3 shipped: a fresh `naner init` dumped the
  *entire* configuration-validation report — every `VendorPath` for a
  vendor that hasn't been installed yet, every profile icon that doesn't
  exist yet, every `PathPrecedence` entry with nothing there yet — three
  times in a row, right in the middle of "Installing Windows Terminal...".
  All expected warnings for a tree mid-bootstrap (nothing is installed yet,
  of course those paths don't exist), but printed three times because of a
  #83 regression: `WindowsTerminalConfigurator::create_settings` called
  `config::load` — which validates `naner.json` and logs every warning on
  every call — three separate times to write one `settings.json`. Loading
  it once and threading the result through cuts the noise back to the one
  copy it was always supposed to be.

## v0.9.3 — 2026-08-19

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.2...v0.9.3).

- The #81 keystroke-race relaunch (`reexec_in_own_console_if_racy`) had a
  gap the original fix never covered: it only handles the race once the
  relaunch into a console of naner's own actually *succeeds*. Reported live
  on v0.9.2, right after the #116 stdin fix and #121 both shipped: `naner
  update`'s prompt didn't respond to `Y`/Enter, reproduced in two different
  shells (a naner-launched profile and a plain `Windows PowerShell`
  window), with no second console ever appearing in either. That's the
  spawn itself failing — `Command::new(exe)
  .creation_flags(CREATE_NEW_CONSOLE)` erroring out — and the code's own
  fallback ("racy beats broken") silently ran the rest of the flow inline,
  landing right back in the pre-#81 race with no indication anything had
  gone wrong. The prompt printing and taking a cursor in the *same* window
  the banner appeared in, rather than a new one, was the tell. The fallback
  now logs the actual spawn error and the `Start-Process -Wait
  naner.exe -ArgumentList "..."` workaround before continuing, so the
  failure is visible instead of indistinguishable from the bug it exists to
  dodge. The underlying question — *why* the spawn fails on some machines —
  is still open; this makes it debuggable instead of silent.

## v0.9.2 — 2026-08-19

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.1...v0.9.2).

- Windows Terminal's four Naner profiles used to be hand-duplicated a
  second time in WT's own schema, in
  `dist-assets/home/.config/windows-terminal/settings.json` — a second copy
  of "how do you start the Unified profile" that nothing kept in sync with
  `naner.json`, and had already drifted from it in the shipped repo (#83).
  That file is gone. `settings/settings.json`'s profile list is now
  generated fresh from `naner.json`'s own `Profiles`, on every install and
  update, so there is exactly one place to edit a profile's shell, icon, or
  starting directory. GUIDs are unchanged and still fixed per profile,
  never derived from the config — the identity `naner`'s GUID-aware merge
  (#52) locates a profile by, so every already-installed `settings.json`
  reconciles the same way it always has, no resurrected deletions. One
  wrinkle worth knowing: `naner --profile X` sets up naner's environment on
  itself before it ever spawns `wt.exe`, so `naner.json`'s own
  `CustomShell.Arguments` for a PowerShell profile just sources
  `profile.ps1` directly — no bootstrapping needed. A profile picked
  straight from Windows Terminal's own list (double-click, pinned tile,
  WT's own "+" menu) starts cold, with none of that process state, so the
  *generated* `commandline` splices in the same `naner.exe --export-env
  --no-comments | Invoke-Expression` self-bootstrap the old template always
  carried for exactly that case.

- `rustup`/`cargo`/`rustc` were unreachable from every naner-launched shell,
  no matter how you launched one — reported live after enabling the Rust
  vendor. `Rust.json`'s `pathPrecedence` pointed at
  `vendor/rust/cargo/bin`/`vendor/rust/rustc/bin`, and its `CARGO_HOME`/
  `RUSTUP_HOME` env vars pointed at `home/.cargo`/`home/.rustup` — neither
  matches where `rustup-init` actually puts things. The installer (per
  `archives::run_exe_installer`, already documented in
  `MIGRATION_ANALYSIS.md` as "RUSTUP_HOME/CARGO_HOME pointed into the vendor
  dir") runs `rustup-init` with `CARGO_HOME`/`RUSTUP_HOME` set to the
  vendor's own `.cargo`/`.rustup` — rustup then drops every proxy binary
  (`rustup`, `cargo`, `rustc`, `rustfmt`, ...) into that single
  `$CARGO_HOME\bin`. `naner install` reported success, `naner doctor` and
  `naner suggest` both reported the vendor installed and "on PATH" — the
  binaries just never lived at the path the config pointed to. Fixed
  `Rust.json`'s `pathPrecedence`/`CARGO_HOME`/`RUSTUP_HOME` and
  `naner.json`'s `VendorPaths.Rustc`/`Cargo` to point at the real
  `vendor/rust/.cargo/bin`.

- Every optional vendor now defaults to `"enabled": true`. Until now, only
  the four essential tools (Git for Windows, PowerShell, 7-Zip, Windows
  Terminal) shipped enabled, and the other 26 — Rust, Go, NodeJS, Ruby,
  Claude Code, Codex, uv, and the rest — needed a manual `"enabled": true`
  edit in `vendors.json` before `naner install --all` would touch them.
  That opt-in model made sense when the vendor list was small, but it meant
  `install --all` quietly did less than its name implied as the list grew.
  Flipping the default makes `naner install --all` actually install
  everything shipped, matching what a first-time user expects from the
  name; anyone who wants a smaller footprint disables individual vendors
  the same way they always could.

## v0.9.1 — 2026-08-19

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.9.0...v0.9.1).

- The #81 keystroke race is still with us, one layer down: reported live on
  v0.9.0, `naner update`/`naner init` re-launch themselves into a console of
  their own to dodge racing the calling shell for input (the v0.8.1 fix) —
  but the relaunched child never actually wired stdin to that console.
  `AllocConsole` succeeds (it already owns the console `CreateProcess` just
  created for it), `console::setup`'s existing `reopen_conout` dance gets
  stdout/stderr working — visibly, since the version check and prompt text
  render fine — but nothing ever reopens `CONIN$` the same way, so
  `STD_INPUT_HANDLE` stays whatever the GUI-subsystem loader left it
  (unset). The `Y`/Enter prompt sits there forever: no race this time, no
  error either, just silence — every keystroke goes to a handle that was
  never opened. `console::setup` now reopens `CONIN$` right alongside
  `CONOUT$`/stderr, the same technique that already worked for output,
  applied to the one std handle it never covered. `Start-Process -Wait
  .\naner.exe -ArgumentList update` remains a working manual escape hatch
  either way — it never goes through this relaunch path at all.

## v0.9.0 — 2026-08-19

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.8.2...v0.9.0).

- Seven new vendors, and two new ways for a vendor to install. Most CLI
  tools these days are distributed through a language package manager
  rather than a standalone archive, so two release-source types join the
  existing six: `npm` (resolved against `registry.npmjs.org`'s `latest`
  dist-tag, installed with the vendored NodeJS's `npm install -g` into
  `home\.npm-global`) and `pip` (PyPI's JSON API, `python -m pip install
  --user` into `home\.local` via the vendored Anaconda). Both leave only a
  `.vendor-version` marker under `vendor\<name>\` — the real install lives
  in the package manager's own tree — and neither is pinned by
  `naner.lock`; the package manager verifies its own artifact. A third
  addition, `installType: "binary"`, covers a vendor that ships as one
  verified executable with nothing to extract: the download is placed
  directly under the vendor directory instead of through the archive
  extractor.

  New vendors: `GitHubCli` (`gh`, GitHub-sourced), `ClaudeCode`, `OhMyPi`,
  and `Codex` (all npm-sourced, all depending on the `NodeJS` vendor),
  `NotebookLmCli` (`notebooklm-py`, pip-sourced, depending on `Anaconda`),
  `OhMyPosh` (a `binary`-type static download, checksum-verified against
  the shared `checksums.txt` its CDN publishes alongside every release —
  the same scrape mechanism Anaconda's resolver already used, applied to a
  manifest instead of a directory listing), and `Antigravity` (a static
  `.exe`; Google publishes no digest for it, so it installs unverified,
  the same posture OneCommander already has). All seven ship disabled by
  default like every other optional vendor.

- Dotfolder leakage into the real `C:\Users\<name>\` profile is closed for
  the tools that can be redirected. naner has always pointed `HOME` into the
  portable tree, but Windows tools mostly resolve home via
  `USERPROFILE`/`os.homedir()` instead — so `.bun`, `.claude`, and every
  XDG-aware CLI landed outside the tree. The shipped environment now sets
  the XDG trio (`XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_CACHE_HOME` under
  `home\`), `CLAUDE_CONFIG_DIR` for the npm-installed Claude Code CLI, and
  `BUN_INSTALL` in Bun's own vendor file. `USERPROFILE` itself stays
  untouched on purpose: profiles start there, the prompt shortens against
  it, and known-folder APIs depend on it. The honest limit is stated in the
  config comment: a tool that reads only `USERPROFILE`, with no environment
  override, cannot be redirected this way — each new stray dotfolder means
  finding that tool's override and adding one line.

- First real `refresh-pins` pass over the shipped pins, from a sandbox whose
  proxy blocks `api.github.com`: the four resolvable-from-here vendors were
  refreshed and their new URLs verified live — Go `go1.21.6` -> `go1.26.6`,
  Node `v20.11.0` -> `v26.7.0`, .NET SDK `10.0.102` -> `10.0.400`, MSYS2
  `20240727` -> `20260611`. The 14 GitHub-sourced pins couldn't be checked
  from this environment and are unchanged; the four static-URL vendors are
  manual by design. The pass also caught a real installer bug the new
  command surfaced: `version_from_file_name` took the *first* digit run in a
  file name, so every MSYS2 install recorded `.vendor-version` as literally
  `"2"` (the digit in "msys2") and 7-Zip's scrape as `"7"`. It now picks the
  run carrying the most digits and trims a trailing dot the pattern could
  drag in from the extension.

- Vendor version-pin upkeep, in three connected pieces. `naner refresh-pins
  [dir] [--dry-run]` re-resolves what upstream currently calls latest for
  every dynamically-sourced vendor and rewrites the hardcoded `fallback`
  pins in `config/vendors/*.json` — the pins existed for installs whose
  resolution fails, but nothing ever refreshed them (Go's fallback said
  `go1.21.6`, Node's `v20.11.0`), so a degraded install silently got a
  years-old version. `naner outdated` answers the user-facing half: it
  compares each installed vendor's `.vendor-version` against live upstream
  and flags major-version jumps distinctly, exiting non-zero when updates
  exist. And `naner doctor` gains an offline nudge — an installed vendor
  older than its fallback pin prints an "updates are available" warning
  with no network touched, which stays honest precisely because
  `refresh-pins` keeps the pins recent. Resolution deliberately skips both
  `naner.lock` and the fallback cascade: checking a pin against the pin
  itself would always answer "current". Static-URL vendors (Anaconda,
  Inkscape, HiFile, OneCommander) are reported manual-only in both
  commands — their pinned version *is* the install.

- New vendor: uv, Astral's Python package and project manager. Ships as a
  `github`-sourced zip (`astral-sh/uv`, `uv-x86_64-pc-windows-msvc.zip`)
  verified against the `.sha256` sidecar uv publishes alongside every asset
  — the same mechanism rustup uses, so resolved and fallback downloads are
  both digest-checked. Disabled by default like the other optional tool
  vendors; `provides: ["uv", "uvx"]` wires it into `naner suggest`, and its
  cache, managed Pythons, and installed tools are pointed under
  `%NANER_ROOT%\home` (with `UV_TOOL_BIN_DIR` on the already-exported
  `home\.local\bin`) so nothing leaks outside the portable tree.

- Command-not-found suggestions (#103): typing `node` in a shell where naner
  could provide it now prints what to do instead of only the shell's generic
  error. `naner suggest <name> [--porcelain]` maps an executable name to a
  vendor — each vendor's new optional `provides` list first (shipped for the
  ten tool vendors), then names derived from `naner.json`'s `VendorPaths` —
  and prints the state-appropriate next step: install it, flip `"enabled":
  true` first for a disabled vendor (since `naner install` refuses those), or
  note the tool is only on PATH inside naner-launched shells. No match means
  no output and a non-zero exit, so a wrong guess never outshouts the shell's
  own error. `setup-shell` now writes the matching hooks
  (`CommandNotFoundAction` for PowerShell, `command_not_found_handle` for
  Bash) into its managed block, and the shipped `profile.ps1` carries the
  PowerShell hook — all guarded on `naner.exe` existing, offline, and
  error-swallowing so a missing or moved naner never breaks a shell.

- The first-run bootstrap offers PATH setup: after a successful install it
  asks whether to put `vendor\bin` on the user PATH (the `add-to-path`
  edit), so a fresh install ends with `naner` callable from any new shell
  without a second command. Opt-in, decline-safe, and EOF declines.


## v0.8.2 — 2026-08-18

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.8.1...v0.8.2).

One feature: `naner add-to-path` (#105) closes the last gap in calling
naner — nothing ever put `naner.exe` on the system PATH, so outside the
launched environment the bare name never resolved. The new command
appends `<NANER_ROOT>\vendor\bin` to the per-user PATH so `naner` works
from any newly opened shell, without importing the whole environment the
way `setup-shell` does; `--remove` undoes it, `--dry-run` previews the
exact value it would write.

The `HKCU\Environment` value is edited directly rather than through
`setx`, which silently truncates the stored PATH at 1024 characters. The
value's registry type and every other entry are preserved byte-for-byte,
a `WM_SETTINGCHANGE` broadcast makes newly started shells see the
change, and matching is case-insensitive and tolerant of trailing-slash
and quoted variants so re-running is a no-op. User hive only: no
elevation, and nothing outlives deleting the folder plus one `--remove`.

Also ships the post-0.8.1 documentation truth-up (#104): the README now
leads with bare invocation, since the #81 console fix — field-validated
on v0.8.1 — made the `Start-Process -Wait` / `start /wait` wrappers
unnecessary.

### Known limitations for this release

- `add-to-path` is untested on a real Windows box until this release's
  field check runs: `naner add-to-path`, open a new PowerShell window,
  `naner --version` from anywhere, then `--remove` to undo.


## v0.8.1 — 2026-08-18

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.8.0...v0.8.1).

One fix: the #81 keystroke race dies in code. Interactive flows
(bootstrap, `init`, `update`) run bare from a shell now open a console
of their own instead of racing the shell for keystrokes — the
`Start-Process -Wait` / `start /wait` wrappers keep working but are no
longer needed. Pipes, scripts, and CI keep the inline path.

### Known limitations for this release

- The re-exec is untested on a real Windows box until this release's
  three bare-invocation checks run: bare bootstrap in an empty folder,
  bare `naner init`, bare `naner update` — each should open its own
  console window with a working prompt.
- A tree on 0.8.0 must update to this release with a wrapped
  invocation one last time (0.8.0's prompt code predates the fix).

Third strike for the #81 keystroke race, and this time it dies in code. The
sequence: documented for `cmd.exe` in 0.6.x; discovered to bite PowerShell
identically during the v0.7.1 validation (the docs were corrected to demand
waiting wrappers); then hit again on day one of 0.8.0 field testing, because
nobody remembers a wrapper incantation while testing an installer. The root
cause is structural — a GUI-subsystem binary reading a prompt from a console
its parent shell is also reading loses keystrokes to the shell — and
`ConsoleState::Attached` identifies it precisely. Interactive flows
(bootstrap, `init`, `update`) now re-exec themselves into a console of their
own when attached, wait on the child, and mirror its exit code; the child
knows the window is its own and pauses before it closes. Redirected stdio
never re-execs, so the CI test that runs the real binary with a closed stdin
still exercises the inline path, and scripted use is untouched. If the
re-exec spawn itself fails, the flow runs inline as before — racy beats
broken.


## v0.8.0 — 2026-08-18

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.7.1...v0.8.0).

The single-binary release. `naner.exe` is launcher, installer, and updater
in one; `naner-init.exe` is retired as a separate program but every release
still publishes an asset by that name — a byte-copy — so deployed
0.6.x–0.7.x installs keep updating. Also ships the legacy-YAML convert
hint and the install-doc corrections from the v0.7.1 field validation.

### Known limitations for this release

The single-binary flows are CI-verified but not yet field-validated.
Three tests close that gap, in order (see docs/VALIDATION.md Step 6):

1. **Fresh install** — new folder, this release's `naner.exe`,
   `Unblock-File`, run, `Y` twice; pass = Windows Terminal opens.
2. **Update with leftovers** — same folder, set `.naner-version` to
   `v0.0.1`, `naner update`; pass = every copy refreshed, including
   `vendor\\bin\\naner-init.exe`.
3. **0.7.x compat** — in a 0.7.1 tree, run its old `naner-init update`;
   pass = it installs this release and the old name now holds the new
   binary.

naner is one binary now. The `naner`/`naner-init` split existed for a single
reason — a process cannot overwrite its own executable — and the v0.7.1
self-update validation proved in the field that it never needed to: Windows
will happily *rename* a running exe, and the rename-aside swap works against
a genuinely executing binary. With the one forcing function gone, the split
was pure overhead: two binaries to build, verify, ship, version-match, and
explain, and a whole class of stale-sibling bugs (the sync-to-embedded
downgrade trap) that only existed because two copies could disagree.

`naner.exe` is now launcher, installer, and updater. Run it in an empty
folder and it offers to install there — the same prompt, bundle-by-embedded-
tag download, verification, and essentials bootstrap the init binary owned.
`naner init`, `naner update`, and `naner check-update` carry the explicit
commands; `self-update` remains as an alias. `naner update` installs the
latest release into every copy of the binary the tree is known to carry: the
running one first (rename-aside, `.old` swept on the next launch), then the
canonical `vendor\bin\naner.exe`, then any pre-0.8.0 `naner-init.exe`
leftovers — refreshed not out of politeness but because a stale naner-init
would sync the tree back down to its own embedded version.

Compatibility is carried by the release, not the code: every release still
publishes a `naner-init.exe` asset, now a byte-copy of `naner.exe`. A 0.7.x
install's `naner-init update` requires that asset to exist and verifies it
against `SHA256SUMS`; what it installs is the new single binary, which
behaves correctly under the old name and refreshes the rest of the tree on
its first `update`. A 0.6.x install still updates the old way once (manual
download), as before.

One bug found by the merge itself: interactive prompts read EOF as an empty
line, and an empty line means yes. Fine when a human presses Enter; not fine
when the bare binary runs in an empty directory with a closed stdin — the
first CI run of the new bootstrap path silently consented to downloading a
full install. EOF is now a no, pinned by a test that runs the real binary
with stdin closed and asserts nothing downloads.

The last sharp edge of the v0.7.0 upgrade is filed down. Dropping YAML left
one genuinely unkind failure: a tree whose only config is `naner.yaml` got
"no configuration file found" — technically true, and a lie to anyone
looking at their config directory. The loader now checks for the pre-v0.7.0
files by name when nothing loadable exists and says the real thing: this
file is YAML, naner stopped reading it in v0.7.0, convert it to
`config/naner.json` and remove it. The first-run report gives the same
hint, so both the launcher path and the init path tell the truth. The plain
missing-config case is unchanged, and a test pins each behavior.

The v0.7.1 validation on a real Windows box confirmed the self-update
mechanics end to end — fresh install with the split config layout, then a
forced `naner-init update` that verified both binaries, swapped the running
init aside as `.old`, and swept it on the next launch. It also caught two
documentation lies, both now fixed. The README claimed PowerShell waits for
`naner-init.exe`; it does not — no shell waits for a GUI-subsystem process,
so the #81 keystroke race reproduces in PowerShell exactly as in `cmd.exe`,
and the install instructions now give both shells a waiting wrapper. And
nothing warned that SmartScreen silently blocks a freshly downloaded
unsigned exe — the "nothing happens at all" symptom — so `Unblock-File` is
now step one. `docs/VALIDATION.md` gains Step 6, recording the self-update
procedure that was actually used, so future releases re-prove the path
instead of trusting it.


## v0.7.1 — 2026-08-18

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.7.0...v0.7.1).

The self-update release. This is the first release whose `naner-init.exe`
can carry an installation forward on its own: install THIS release's
`naner-init.exe` by hand — the last manual step — and every release after
it is one `naner-init update` away.

### Known limitations for this release

- The rename-aside self-swap has not yet run against a genuinely executing
  binary — the tests stage a stand-in file. The first real `naner-init
  update` from this version is the validation, deliberately.
- A tree on v0.7.0 or earlier still updates the old way one last time:
  its installed init predates this code, so download this release's
  `naner-init.exe` manually.

The self-update mechanism worked; the discovery didn't. `naner-init`'s
"update" was sync-to-embedded — it installed the release matching its own
compiled-in version, and its update check compared two local values, so no
naner installation ever learned that a newer release existed. Updating
really meant: know somehow that a release happened, manually download the
new `naner-init.exe`, and let it drag `naner.exe` up to match. Coherent,
verifiable, and invisible.

`naner-init update` now asks GitHub for the latest release and installs it —
both binaries. Replacing itself uses the rename-aside trick (Windows will
rename a running exe but not overwrite one); the displaced file is parked as
`.old` and swept on the next launch. Two details carry the safety. Both
downloads are verified against the release's `SHA256SUMS` before either
file is touched — half a verified update is not an update. And the init is
swapped *first*: if the second swap fails, the tree holds a new init and an
old `naner.exe`, and the next run offers the update again — the other order
leaves a new `naner.exe` under a stale init whose sync check would offer to
"update" it back down. A release missing either binary installs nothing,
for the same reason, and a test pins each of these properties.

Plain launches stay offline — the fast local check is unchanged, and
`naner-init check-update` is the explicit "ask the network" command.

`naner.bat` is gone. It survived this long on the theory that it was the
one launcher with no network round-trip; reading the code killed that — the
launch check was always two local file reads, so the bat had no advantage
over `naner-init`'s pass-through launch from the same root directory. Its
other history this session was drifting twice (a trailing-backslash
`NANER_ROOT` and a PowerShell fallback that didn't exist), which is the
usual fate of a file nothing executes in CI.


## v0.7.0 — 2026-08-18

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.5...v0.7.0).

The configuration release. `config/` was carrying two dead designs and a
monolith: a plugin schema nothing read, a YAML twin of `naner.json` that had
silently drifted, and a 499-line `vendors.json` where the last two vendor bugs
both originated. All three are gone, and a vendor now lives in exactly one
file that says how to fetch it, where it unpacks, what it puts on PATH, at what
precedence, and which variables it needs.

Three breaking changes, which is why this is 0.7.0 rather than 0.6.6 — see the
entries below for each, and the known-limitations note at the end of this
section.

Two follow-ups to moving vendor environment data into the vendor files, both
closing gaps that move deliberately left open.

A vendor switched off now contributes nothing. Before the split every PATH
entry and variable lived in `naner.json` unconditionally, and what actually
kept an uninstalled vendor off PATH was `build_unified_path` discarding
directories that do not exist — a side effect doing a job nobody had assigned
it. The case it never covered is the one that matters: a vendor installed and
then disabled kept its directory on PATH and its variables set, which is
precisely what disabling it is for. `enabled` now gates the contribution
directly. On a fresh tree nothing changes, because those directories are not
there either way; the difference appears exactly where the old behavior was
wrong.

This did mean the test asserting the assembled PATH still matches the original
26-entry list had to stop reading the shipped `enabled` flags — eight of the
eleven vendors with PATH entries ship `enabled: false`, so reading them as
shipped would have left most of the `pathPriority` data unexercised. It now
switches every vendor on first, which is the right scope for it: that test is
about the ordering data being correct, not about which vendors are on.

And `DOTNET_CLI_TELEMETRY_OPTOUT` now has one source instead of two. It was
force-set in `apply_env_overrides` and also declared in `DotNetSDK.json`, and
because the overrides run before the merge — which only fills in keys still
missing — the code won every time and the vendor file's copy never did
anything. The vendor file is now the only source, which also means the
variable follows the vendor it belongs to: no .NET SDK enabled, no `dotnet`
CLI, nothing to opt out of. `POWERSHELL_TELEMETRY_OPTOUT` and
`AZURE_CORE_COLLECT_TELEMETRY` stay in code, having no vendor to belong to —
the Azure CLI is not something naner installs at all.

Splitting `vendors.json` into a file per vendor only did half the job, which
became obvious the moment anyone opened one: a vendor file said how to
download and unpack a tool and nothing about what it means once installed.
`GOROOT` was not in `Go.json`. `vendor\\go\\bin` was not in `Go.json`. Both
were in `naner.json`, in two global lists, along with everyone else's. The
numbers were lopsided — 17 of the 26 PATH entries and 19 of the 22
environment variables belonged to a specific vendor. Adding a vendor meant
edits in three files, and "work on one at a time" was still not true.

They live with their vendor now, as `pathPrecedence`, `environmentVariables`
and `pathPriority`. `naner.json` keeps what is genuinely naner's: `bin`,
`opt`, `vendor\\bin`, the `home\\` user-install trees, and `NANER_ROOT` /
`HOME` / `SSH_HOME`.

The interesting problem was ordering. Intra-vendor order is free — Git's
`cmd`, `mingw64\\bin`, `usr\\bin` stay an ordered array inside `GitForWindows.json`.
Inter-vendor order is not, and it decides real conflicts: Git for Windows and
MSYS2 both ship a `bash.exe`, and whichever directory comes first is the one
you get. Sorting by file name would have quietly reshuffled that. So order is
explicit data — `pathPriority`, numbered in tens so a new vendor slots in
without renumbering its neighbours, lower first, unranked vendors sorting
after ranked ones by key so the order is always total.

The vendor block also is not simply appended. It used to sit in the middle of
`naner.json`'s list, with `%NANER_ROOT%\\opt` after it — deliberately last, so
a user's own tools never shadow a vendor's. Appending would have inverted
that. `naner.json` now carries a `%VENDOR_PATHS%` marker at the exact position
the block belongs, and the merge substitutes it in place.

The merge itself happens inside `config::load` rather than at the call sites.
Six places read `config.environment` — the launcher, three in `main`, `diff`,
`bench` — and every one of them wants the merged view, so making it the only
view is what stops them drifting apart. `load_verbatim` deliberately skips it:
tooling that rewrites the user's config must not bake vendor entries into it.

One thing this deliberately does *not* change: a disabled vendor still
contributes. Eight of the eleven vendors carrying PATH entries ship
`enabled: false`, and all 26 entries used to sit in `naner.json`
unconditionally — what actually keeps an uninstalled vendor off PATH is
`build_unified_path` dropping directories that do not exist. Filtering on
`enabled` here is defensible and might even be better, but it is a behavior
change and this is a move. A test pins the current behavior and says why.
The load-bearing test asserts the assembled PATH is identical, entry for
entry, to the list `naner.json` used to carry — a priority typo fails it.

Fixed along the way: `vendors-schema.json` had been carrying a `$ref` to a
`definitions` block that the per-vendor split removed, so the schema resolved
to nothing and happily validated anything. Nothing in the workspace reads it —
it exists for editors — so only a person would ever have hit it. Restored,
extended with the new fields, and now guarded by a test for dangling `$ref`s
and another asserting every field the real vendor files use is actually
described.

Two things in `config/` were describing systems that do not exist, and both
are gone.

The plugin surface was dead twice over. `plugin-schema.json`, the
`PLUGINS` constant, and the `ALL` directory array had exactly zero readers
between them — `ALL` was never referenced at all, which is also what kept
`LOGS` and `VENDOR_BIN` alive. The C# plugin loader it descended from was an
`AssemblyLoadContext` over `plugins/*.dll` that the shipping entry point never
enabled, and MIGRATION_ANALYSIS marked it "do not port" for that reason. But
the schema did not describe that loader either: it described a manifest
bundling vendors, environment variables and PATH entries, with `.ps1` hooks —
a grouping layer over what `vendors.json` and `naner.json` already do, whose
vendor record was a strict subset of a real vendors entry. Two unbuilt designs
sharing one word, sitting next to the real thing. That is why reading the
config directory raised the question of whether plugins and vendors were
duplicating each other; they were not, because only one of them was ever real.

YAML went the same way, for the drift it had already produced. `naner.yaml`
was a field-for-field twin of `naner.json` — except it had stopped being one,
missing the `Naner` vendor path that points at naner's own executable. The
loader takes the first file that exists and never merges across formats, so
the two could disagree forever while only one was ever read, and which one
depended on a file's presence rather than anyone's intent. Keeping them in
sync was a chore with no upside: nothing needed two formats. So the twin, the
parser, the `naner.yaml`/`naner.yml` search entries, the merge path's YAML
branch, and the `serde_yaml_ng` dependency all went. One config format, one
shipped file, drift structurally impossible.

That last part is a real breaking change, and a quiet one by nature: a tree
whose only config is `naner.yaml` now finds no configuration at all rather
than loading it. The error names `naner.json` explicitly — it is built from
`CONFIG_FILE_NAMES`, so it corrected itself when the constant did — and a test
pins the behavior so a YAML-only tree fails loudly instead of half-working.

`config/vendors.json` was 499 lines describing 22 vendors, and the last two
vendor bugs both came from working in it: four vendors added in one batch all
shipped `installerArgs` without `%TARGETDIR%`, and a connection-reset fix
needed a change nowhere near the entry it affected. Each vendor now gets its
own file under `config/vendors/`, named after the key it declares.

The constraint that shaped the design is that the catalog has to be *compiled
into* the binary: `config_merge.rs` embeds it with `include_str!` so a bare
`naner.exe` swap -- which has no bundle to read from -- can still add newly
shipped vendors to an existing tree. `include_str!` takes exactly one file, so
a build script assembles the 22 authored files into one generated catalog in
`OUT_DIR` and the embed points there. Single source of truth, no generated
file checked in, no new runtime dependency. The build script also enforces the
authoring contract: one vendor per file, file name matching the declared key.

Two things got better rather than merely different. `merge_shipped_vendor_defaults`
used to parse the user's whole file, insert missing keys, and rewrite it; now
it writes a file for each missing vendor and never opens the others, so "a
vendor the user has customized is never overwritten" holds by construction
instead of by a key-by-key check -- and a single malformed entry no longer
blocks every other vendor from being added, because it is no longer in the
same document as them. The loader gained the matching property: one unparseable
file is reported and skipped, where a stray comma used to cost the user the
entire catalog at once.

The cutover is deliberate and hard: the pre-split file is not read at all. That
is a real edge, because the failure is quiet -- no vendors directory means the
loader falls back to four hardcoded essentials and the other eighteen simply
vanish from `install --list`. So a tree that still has a `vendors.json` gets
told exactly that, by name, instead of the generic "vendor configuration not
found" it would otherwise print while a perfectly good-looking file sits right
there.

Vendor listing order is now sorted by file name rather than authored order.
Installs were never affected -- those are dependency-driven by key, and
`naner-init`'s essential bootstrap runs off a hardcoded list -- but
`install --list` output changes once, and one test that indexed the loaded set
positionally now looks vendors up by key, which is what it meant anyway.

Dropped in passing: the file's inert `version`/`description`/`metadata` block.
Nothing read it, the schema already said most of it, and one of its notes
pointed at `Setup-NanerVendor.ps1` -- a PowerShell script this repo does not
have, the same species of stale reference as the `naner.bat` fallback.

Following on from the `naner.bat` fixes below: the branch that used to
advertise a dead PowerShell fallback now does something useful instead of
just failing politely. If `vendor\bin\naner.exe` is missing, the shim hands
the arguments to `naner-init.exe` — at the root, where a first-time user
drops it, or in `vendor\bin`, where an install that has updated itself keeps
it, matching the two locations `self_update::find_naner_init` already
searches. `naner-init` is the component that owns bootstrapping: it prompts
before downloading anything, installs `naner.exe`, and then launches it with
the arguments it was handed, so this recovers a half-installed tree rather
than starting a surprise download.

The one detail that makes it work rather than misfire is `start /wait`.
`naner-init.exe` is a GUI-subsystem binary, so `cmd.exe` does not wait for
it — invoked bare, cmd's own next prompt races `naner-init`'s `(Y/n)` for the
user's keystrokes and initialization can silently fail. That is issue #81,
already documented in the README for people typing `naner-init.exe` by hand;
a shim calling it from inside `cmd.exe` would have walked straight into the
same trap.

Two new assertions cover it, plus a third that is really about `cmd.exe`
rather than about naner: a `REM` containing a parenthesis inside an
`if exist (...)` block closes the block early and silently changes control
flow. Writing the `(Y/n)` explanation as a comment next to the code it
explains would have done exactly that. The test walks block depth and fails
on any parenthesis in a comment inside one — a mistake that is invisible in
a diff and produces no error when it happens.

`naner.bat` — the little shim that sits at the root of every bundle and is
the thing a lot of people actually type — had been carried over from the C#
repo untouched and had drifted out of sync with the tree it ships in. Two
problems. It set `NANER_ROOT` to `%~dp0` as-is, and `%~dp0` always ends with
a backslash; a value ending in `\` escapes the closing quote of any
`"%NANER_ROOT%"` a child process builds into a command line, which is the
classic way a perfectly correct-looking path turns into a parse error
somewhere far away. `naner.exe` itself was unaffected — `find_naner_root`
trims trailing separators, and re-exports the cleaned root — so the damage
was confined to anything reading the variable *before* naner.exe fixed it,
which is exactly the kind of bug that only shows up in someone else's
script. The shim now round-trips through a trailing dot to drop the
separator (leaving a drive root like `C:\` intact) and joins the exe path
with an explicit separator.

Second, its fallback branch still described a world that no longer exists:
if `naner.exe` was missing it announced a "PowerShell fallback", tried to
run `src\powershell\Invoke-Naner.ps1`, and told the user to build the C#
version with `cd src\csharp && .\build.ps1`. None of those paths are in this
repo — the PowerShell and C# implementations were left behind at the
migration. So the one moment the shim had something useful to say, it said
something that could not work. It now prints where it looked and points at
`naner-init.exe` and the releases page, which is how you actually get
`naner.exe`.

Both bugs were invisible to the test suite for the same reason: nothing in
the workspace reads `naner.bat`; it is consumed only by `cmd.exe` on a
user's machine. Added `shipped_bat_is_current.rs`, which reads the real
shipped file the way `wt_template_is_portable.rs` reads the real Windows
Terminal template — including a byte-level check that it still has CRLF
endings, since `.gitattributes` pins `*.bat` to CRLF precisely because
`cmd.exe` mis-parses an LF-only batch file.

Anaconda (~1 GB — by far the biggest thing naner ever downloads) failed
partway through with "response body closed before all bytes were read"
at 60%, no retry, install just failed. `Http::download` had no retry at
all: a single dropped connection — more likely the longer a download runs
and the bigger the file — failed the whole install outright, and
`static`-type vendors like Anaconda don't even have a fallback URL to
fall through to the way `github`-type ones do. It now retries up to 3
times with a short backoff before giving up, and a new local-server test
reproduces a truncated-then-complete connection deterministically and
offline, so this doesn't need a live 1 GB download to verify again.

A user installed Obsidian via `naner install` and it reported success —
but `vendor/obsidian/` held nothing but the version marker file. Root
cause: naner's `.exe`-installer path (`archives.rs`) only builds a
`/D=<target>`/`/S`-style silent-install-to-custom-directory command line
automatically when a vendor sets *no* `installerArgs` at all; the moment a
vendor supplies its own (as HiFile, Obsidian, Zed and Zen — all added in
the same batch — did, to get the right silent-install flag for their own
installer technology), that smart fallback is bypassed entirely and
naner-core trusts those args verbatim. None of the four referenced
`%TARGETDIR%`, so each ran its installer silently, successfully, and
somewhere naner never looks — Program Files or AppData, depending on the
app — while still recording it as installed. Fixed by adding the correct
target-directory switch for each installer's actual technology (Inno
Setup's `/DIR=`, NSIS's `/D=`, which per NSIS's own requirement must come
last and unquoted) and adding a test that loads the real shipped
`vendors.json` so this exact class of mistake fails CI next time instead
of shipping quietly.

A user installing the Claude Code CLI inside a naner environment hit its
"not on PATH" setup warning for `home\.local\bin` — the directory
`PYTHONUSERBASE` already designates for user-level tool installs (pip's
`--user` flag, and apparently Claude Code's own native installer, both
land things there). Turned out naner's own `Environment.PathPrecedence`
never included that directory, so anything installed there was invisible
even inside a shell naner itself launched, not just to tools checking the
real Windows PATH. Added `%NANER_ROOT%\home\.local\bin` and `\Scripts` to
the shipped `naner.json`/`naner.yaml` so this class of tool is picked up
automatically going forward. This doesn't touch the real Windows PATH —
that's deliberate; naner stays fully portable and admin-rights-free — so
a tool's installer will still warn if it also checks the persistent OS
PATH, but it will now actually run without hunting for it manually inside
any naner-launched terminal.

That fix alone only reaches a *fresh* install, though — the follow-up
question was "do I have to reinstall naner?" and the honest answer turned
out to be worse than a plain no: neither `naner self-update` (which only
swaps the binary) nor `naner update-vendors` (which does reconcile
`naner.json` against shipped defaults) would have delivered the new
`PathPrecedence` entries to an already-installed tree, because that
reconciliation only ever added whole-missing `VendorPaths`/`Profiles` keys
and a short hardcoded list of specific field migrations — a plain list
addition like this had no path in at all. `merge_shipped_naner_defaults`
now reconciles `Environment.PathPrecedence` the same principled way
`wt_config.rs` already reconciles Windows Terminal profiles: a shipped
entry missing from the user's list gets appended, unless a
`.naner-managed-path-precedence.json` marker says the user removed it on
purpose, in which case it's left alone. `naner update-vendors` on an
existing install now actually catches up.

The README explained the migration's phase history and listed every CLI
subcommand, but never actually said how to install or start using the
thing — someone landing on the repo had no path from "what is this" to
"running terminal." Added Installation (download `naner-init.exe`, what it
verifies and bootstraps on first run) and Usage (the common `naner`
commands, installing optional dev tools, self-update) sections right after
the intro, ahead of the migration-status writeup.

---

### Known limitations for this release

- A tree whose config is `naner.yaml` stops loading entirely and must be
  converted to `naner.json`. No converter ships; the error names `naner.json`
  but does not detect and call out an existing YAML file.
- A tree whose `config/vendors.json` predates the split is not read. The
  warning names the file and says to update the installation, but there is no
  in-place migration — the vendor set falls back to the four hardcoded
  essentials until a new bundle lands.
- A `naner.json` predating the `%VENDOR_PATHS%` marker gets vendor paths
  appended after `%NANER_ROOT%\\opt` rather than before it, with a warning.
  `merge_shipped_naner_defaults` should add the marker on update.
- `DOTNET_CLI_TELEMETRY_OPTOUT` is no longer set on a tree without the .NET
  SDK enabled, having moved to that vendor and stopped being force-set in
  code.

---

## v0.6.5 — 2026-08-17

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.4...v0.6.5).

Added eight new optional vendors to `config/vendors.json`: the file managers
HiFile and OneCommander, the container engine Podman, the image viewer
ImageGlass, the vector editor Inkscape, the note-taking app Obsidian, the
Zed code editor and the Zen browser. All ship disabled by default (`enabled:
false`), same as every other optional vendor — `naner install <Name>` turns
one on explicitly.

Releases now publish to this repo instead of cross-publishing to the
pre-rewrite `baileyrd/naner` repo. Through v0.6.4, tagging `rusty_naner`
built the exes but shipped them as a release on `baileyrd/naner`, so this
repo's own Releases page was always empty — by design, to keep pre-rewrite
installs' auto-updater (which checks `baileyrd/naner`) working. That
cross-publish is now removed: `release.yml` publishes to `rusty_naner`
itself, and `naner-init`'s update check (`constants::github::REPO`) now
points here too. The tradeoff is explicit and was asked for: any install
from before this change stops seeing new releases, since `baileyrd/naner`
no longer receives new tags. Bringing such an install forward requires
manually fetching a current `naner-init.exe` from `rusty_naner`.

This is also the first release actually being used to test the new
publish target end-to-end (real exe assets attached to a real `rusty_naner`
tag), so treat it as a workflow shakedown release rather than a
feature-complete milestone. That test caught a real bug before it shipped:
CI's `windows-latest` job failed on the first attempt at this PR, tracing
back to two `launcher` tests that mutate the process-global `PATH` env var
without synchronizing against each other. Under `cargo test`'s default
multi-threaded runner they raced, and the real system `bash.exe` (Git for
Windows ships one on `windows-latest`) leaked into a window one test
expected `PATH` to be empty. Both now hold a shared test-only mutex.

---

## v0.6.4 — 2026-08-17

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.3...v0.6.4).

Real-world validation of v0.6.3's vendor-set swap turned up seven more
issues — none of them CI-reachable, all of them the same shape as the
session that shipped v0.6.0: naner did one thing and reported another.

The load-bearing one: dropping a new `naner.exe` into an existing install
(the documented, supported upgrade path) never touched `config/naner.json`
or `config/vendors.json`. `update-vendors` already merged new Windows
Terminal profiles into an existing `settings.json`; it now does the same
for the launcher's own config — a pre-#64 tree's `Bash` profile and
`VendorPaths.GitBash` no longer point at MSYS2 forever, and new vendor
definitions (Git for Windows, Anaconda, Bun) reach installs that predate
them (#72).

Also fixed: a piped/logged `naner install` printed raw progress-percentage
noise with none of the status text that would explain it, because the
download progress bar was never gated by the same auto-quiet setting
everything else already was (#67). `naner doctor` always exited 0 no
matter what it found, so it was useless as a CI health gate (#68).
`naner install A B C` could report overall success with one of the
requested names silently dropped for being unknown or disabled (#69). The
missing-Bash install hint still pointed at `naner install msys2` after
#64 swapped the default provider (#70), and `--export-env` still set
`MSYSTEM`/`MSYS2_PATH_TYPE` unconditionally even though MSYS2 is disabled
by default now (#71). The release workflow gained a step that re-downloads
every asset it just published and re-verifies it against `SHA256SUMS`,
so a broken upload fails the job instead of shipping live and
undetected (#66).

---

## v0.6.3 — 2026-08-17

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.2...v0.6.3).

**The default vendor set changes shape.** Git for Windows replaces MSYS2 as
the vendor that's required and enabled out of the box — it's the same
self-extracting-archive install pattern already used elsewhere (a
`PortableGit-*-64-bit.7z.exe` run with `-y -o<dir>`, not a real archive
extraction), and it's what now backs the shipped `Bash` profile and
`VendorPaths.GitBash`. MSYS2 stays fully installable by name, just no
longer part of the default set. Anaconda replaces Miniconda as the optional
Python distribution — same `repo.anaconda.com` listing-scrape digest
verification, just pointed at `/archive/` (Miniconda's `/miniconda/` has a
stable `-latest-` alias; Anaconda's archive index doesn't, so its fallback
URL is a dated version that will need bumping occasionally). Bun joins as a
new optional, disabled-by-default vendor. The .NET SDK — enabled by default
since the C# migration needed it, not because naner itself does — is now
disabled by default too.

---

## v0.6.2 — 2026-08-17

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.1...v0.6.2).

Closes out the rest of the open backlog: #41, #52, #57.

**A missing vendor used to fail one process too late.** `naner` checked
that Windows Terminal existed, then handed it a shell path it had never
checked — `pwsh.exe`, `bash.exe`, whatever `VendorPaths` said or a bare
default guessed. On a tree with nothing installed yet (the ordinary state
before `naner install`, not an edge case), the terminal opened and
immediately showed an NT status code from itself, naming a path the user
never typed. `naner` now resolves and checks the shell before ever building
that argument string, and fails with "PowerShell is not installed — run
`naner install powershell`" instead (#41).

**A mistyped profile name used to just... work, wrong.** `naner -p
SomeTypo` silently launched the default profile instead — exit 0, a
terminal opens, and the only trace is a warning most callers never see.
Every call site resolved a profile the same way regardless of whether the
name came from an explicit `-p` or the configured default, so the failure
path this always claimed to have was dead code. An explicit, unresolvable
`-p` now fails loudly with the profile list and exit 1; not passing `-p` at
all is unaffected (#57).

**Windows Terminal profiles finally reach an existing install.** #50 made
`naner` stop overwriting a user's `settings.json` outright, which was the
right emergency fix and left a real gap: template changes never reached
anyone who had already installed. `update-vendors` now reconciles Naner's
own profiles into the file by GUID — refreshed if still present, added if
never offered before, left alone if the user removed one on purpose
(tracked in a small sidecar so "never added" and "deleted" are never
confused). The trade is the same one `naner migrate` already makes for
`naner.json`: the merge is a JSON round-trip, so comments do not survive
it, and every rewrite is backed up first (#52).

---

## v0.6.1 — 2026-08-16

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.0...v0.6.1).

Both found by actually launching v0.6.0 on a real Windows box and watching
the running application rather than trusting its exit codes and log lines —
the same discipline that produced the eleven `docs/VALIDATION.md` bugs.

**The Windows Terminal profiles naner installs were never portable.**
`naner install WindowsTerminal` writes four profiles into
`settings/settings.json` (`Naner (Unified)`, `Naner PowerShell`, `Naner Bash`,
`Naner CMD`), and `defaultProfile` points at the first. The template those
profiles come from has hardcoded `C:\tools\cmd_line\naner` — a path from
whichever machine generated it — since `v0.5.0-alpha.0`. `naner.exe` and
`naner.bat` themselves were never affected; they build their own `wt.exe`
command line at runtime from the real, correctly-resolved root. What was
broken is everything else: opening the portable `wt.exe` directly, or a
shortcut pinned to a specific `Naner *` profile, launched a shell that could
never find `naner.exe`, on every install except the one that happened to
share that exact path (#58).

**The bundled PowerShell profile fought its own launcher over the tab
title.** `naner.exe` sets a descriptive `--title` (`Naner (Unified)`,
`Naner PowerShell`) when it spawns Windows Terminal. `profile.ps1`'s custom
prompt then overwrote it on every single command with a generic
`pwsh in <folder>`, so the title was only ever correct for the fraction of a
second before the first prompt drew (#59).

---

## v0.6.0 — 2026-08-16

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.5.0...v0.6.0).

The output of a full repository audit. v0.5.0 completed the migration and shipped
a launcher that worked; this release is about the difference between working and
being trustworthy. Three themes:

**Things that reported success without doing anything.** Five commands
(`profile import`, `setup-shell`, `pack`, `self-update`, `checksum`) printed a
success message and acted on nothing. Vendor install reported success when it
could not place the tree. `enabled` in `vendors.json` was parsed and ignored.
`PreLaunch`'s exit code was discarded, so a hook that failed still launched the
terminal. Each of these is worse than an outright error, because the user has
no reason to look.

**Nothing was verified.** Downloaded vendor artifacts and self-update binaries
were trusted on the strength of TLS transport alone. Downloads now check against
the digest the distributor publishes where one exists, `naner.lock` pins version,
URL and SHA-256 for the vendors that publish none, and `naner-init` verifies
release assets against a `SHA256SUMS` manifest and fails closed without it.

**Two reachable injection paths.** Terminal arguments were interpolated
unescaped into a command line handed to `raw_arg`, reachable through
`naner -d '<value>'` — so any wrapper passing a caller-supplied directory was
exposed, not just someone editing their own config. Separately, environment
variable *names* went unescaped into `--export-env` output that is designed to
be piped into `Invoke-Expression` / `eval`.

### Fixed after the first Windows validation pass

Eleven bugs surfaced from working `docs/VALIDATION.md` against this release on
real Windows. Ten are fixed below; one is filed (#41). None were reachable from
CI, and the two worst were silent — correct-looking output, exit code 0, real
damage.

- **Fixed:** on a tree that is not initialized, `naner --export-env` printed the
  first-run notice to **stdout** and exited **0**. That stdout is documented for
  `| Invoke-Expression` and `eval "$(...)"`, so the calling shell was handed
  English prose to execute — `First: The term 'First' is not recognized...` — and
  the success code told any wrapper the export had worked. The first-run gate
  fires before the launcher arguments are parsed, which is why a diagnostic
  reached a machine-readable channel at all. The notice now goes to stderr in
  every invocation, and `--export-env` exits 1 when nothing was exported. The
  interactive first run still exits 0, which is deliberate C# parity and the
  double-click case (#38).
- **Fixed:** every non-ASCII character in console output is now ASCII —
  `logger::failure`'s marker (`[x]`, previously `[✗]`, and therefore every error
  message in both binaries), the first-run bullets, `diagnose`/`doctor`'s
  check marks, the copyright sign, and the em dashes in the dry-run notices. A
  Windows console on the default cp1252 code page renders UTF-8 as mojibake.
  Setting the console code page instead would have fixed the attached case and
  not the redirected one, since a pipe's encoding belongs to whatever reads it
  (#39).
- **Fixed:** `setup-shell` generated a block pointing at
  `<root>\bin\naner.exe`. `naner.exe` lives at `<root>\vendor\bin\naner.exe` —
  where the release workflow stages it, where `naner-init` installs and updates
  it, and what `naner.bat` calls. `bin/` is the user's own directory and ships
  empty. The generated block guards on `Test-Path`/`-f`, so the wrong path
  failed silently: the block was written, looked right in `--dry-run`, and
  never ran. `VendorPaths.Naner` in the shipped `naner.json` carried the same
  wrong path, which the config validator had been reporting as a warning all
  along — invisible on a GUI launch, where there is no console to read it
  (#42).
- **Fixed:** the ASCII sweep above missed five sites — an em dash each in
  `naner lock`, `self-update` and the vendor list, and an ellipsis in `lock`'s
  truncated digest display, which is the string a user reads to check a pin.
  They were missed because the search keyed on the print macro, and in a
  multi-line `format!` the macro sits on a different line from the string. A
  test now walks every source file and fails on the specific characters that
  render as mojibake, naming file and line. It deliberately does not forbid all
  non-ASCII: `paths.rs` tests accented and CJK path handling with real input.
- **Fixed:** a pinned install printed `Latest version: 26.02` directly beneath
  `Using pinned 7-Zip (26.02)`. Nothing had checked what was current -- that is
  the whole point of a pin -- so the line asserted a check that never ran, on
  the one screen where a user is deciding whether to trust what they are about
  to install. It now prints only on the resolving path, where it is true.
- **Fixed:** a failed install still printed "Restart your terminal to use the
  newly installed tools." A corrupted pin was correctly caught, the install
  correctly refused, the exit code correctly non-zero -- and then naner advised
  a restart for tools that were never placed. The advice is about a PATH that
  changed, so it now prints only when something was installed. A partial run
  still gets it; a run where everything failed does not.
- **Fixed:** `update-vendors` printed `Updating Windows Terminal
  (vv1.24.11911.0)...`, and the same for PowerShell, Rusty Term and Rush. The
  update line prefixed a `v` unconditionally onto a version that four of the six
  essential vendors already record with one. `github.rs` had the guard for this
  and the installer did not.
- **Fixed:** `update-vendors` destroyed the user's Windows Terminal settings on
  every run. Windows Terminal is deliberately the one vendor extracted over-top
  rather than deleted and reinstalled, precisely so `settings.json` survives —
  and then the post-install configurator rewrote it from the template anyway,
  unconditionally. Every colour scheme, key binding, font choice and custom
  profile, replaced, under a line reading `Preserving settings configuration`.
  An existing file is now left alone. The trade is that template changes no
  longer reach an existing install: a stale template is a missing feature, an
  overwrite is lost work. Merging Naner's profiles into the user's file is the
  right long-term answer and is tracked separately (#50).
- **Fixed:** the test named `windows_terminal_update_preserves_settings`
  asserted only that `settings.json` existed after an update, which was true
  whether it had been preserved or replaced. It reads the contents now. A test
  named for a property it does not check is worse than no test — it is the
  reason this looked covered.
- **Fixed:** none of the six built-in vendor definitions set `key`, so all six
  shared the empty-string entry in `naner.lock` -- each `update-vendors` install
  overwrote the previous one's pin, leaving a nameless row and no pin at all for
  four of them. The read side was the dangerous half: `load_all_vendors` falls
  back to that same keyless set when `vendors.json` is missing, empty or
  unparseable, and the pin lookup is by key, so every vendor resolved the one
  shared entry as its own. On such a tree, `naner install PowerShell` would
  fetch whichever artifact wrote that entry last, verify it **successfully** --
  the digest is genuine, just of the wrong file -- and install it under
  PowerShell's name. Integrity checking cannot catch that; nothing is corrupt.
  All six now carry the key `vendors.json` uses, with tests asserting they
  exist, are unique, and match the manifest (#53).
- **Fixed:** `naner install MSYS2` installed the oldest base MSYS2 publishes.
  The scrape resolver took the leftmost regex match, and a vendor's directory
  index is sorted ascending, so "first match" meant "first ever published" --
  `20240507` out of ten available, over two years stale, reported as
  "Fetching latest MSYS2". It now compares matches and takes the newest, using
  the pattern's version capture group numerically where there is one so that
  `1.10` sorts after `1.9`. The existing test could not catch this: its stub
  page held a single archive, so first and newest were the same document (#47).
- **Fixed:** `update-vendors` reinstalled vendors that `vendors.json` marks
  `"enabled": false` -- Rusty Term and Rush, both experimental, onto every tree
  on every run. It read a hardcoded set and never consulted the manifest, so
  the flag was honoured by `install` and ignored by the other command that
  installs things: switching a vendor off lasted until the next update. The
  definitions still come from the built-in set, which carries sources and
  fallbacks a user's manifest may be older than, but the manifest's `enabled`
  now filters it, and skipped vendors are named rather than silently dropped.
  A manifest that cannot be read disables nothing (#48).
- **Provenance:** every one of these came from working the checklist by hand on
  real Windows, and none of them is a v0.6.0 regression — most were ported
  verbatim from the C# and had behaved this way throughout. What changed is that
  someone ran the steps. Several of the affected paths had checklist entries
  that had never been executed, and one had a passing unit test asserting the
  wrong property.

### Breaking changes

- **`naner checksum` is removed.** It never computed or wrote anything. It now
  exits 2 with a pointer to what replaced it — automatic digest verification and
  `naner lock` — rather than vanishing and leaving a script with an unexplained
  "unknown command".
- **`ProfileConfig.WindowEffect` is removed.** It was parsed and never read, so
  the `Mica` / `Acrylic` / `Tabbed` backdrops the README advertised never
  existed. A config that still sets it is not rejected; the key is simply not
  part of the model, and the README no longer claims the feature.
- **An invalid environment variable name is now an error, not a warning.** A
  config using a name outside `[A-Za-z_][A-Za-z0-9_]*` previously loaded with a
  warning and will now fail validation. This is the fix for the `--export-env`
  injection, and configs travel via `naner pack` and `profile export`, so
  tolerating it was not defensible.
- **`enabled: false` in `vendors.json` is now honoured.** `install --all`
  previously installed the seven vendors the shipped config switches off. If you
  relied on that, those vendors will no longer install; set `enabled: true` or
  name them explicitly. Dependencies still install regardless, since they were
  not chosen from a menu.

### Known limitations

- **The Windows validation checklist is partly done.** Console modes, the
  config-writing commands and the vendor pipeline were worked through on real
  Windows and are what produced the nine fixes above. Two steps were not:
  **drop-in daily driving**, which is the one that catches what a checklist
  cannot anticipate, and the **golden parity harness**, which needs a C# naner
  to compare against. `docs/VALIDATION.md` has been rewritten against what this
  pass actually found — the previous revision told you to skip checksum
  verification, which stopped being true in this release.
- **One bug found during that pass is filed, not fixed.** The launcher does not
  verify the shell it hands to the terminal, so a missing vendor surfaces as an
  NT status code from Windows Terminal rather than "PowerShell is not installed
  — run `naner install powershell`" (#41).
- **Windows Terminal profiles are all-or-nothing.** Fixing the settings
  overwrite means naner now writes the whole file or none of it, so template
  changes never reach an existing install and there is no `--reset-settings` to
  ask for them. Merging Naner's profiles into a user's file is the growth path
  (#52).
- **A `naner.lock` written before the key fix keeps its malformed entry.** #53
  stops new nameless entries; it does not clean up an existing one, and
  `lock --refresh` takes a vendor name the entry does not have. Delete the file
  to reset.
- `naner schema vendors` is still hand-checked against `VendorJsonEntry`, which
  is private to `naner-core`, so it has no round-trip drift test the way
  `schema config` does.
- `naner migrate` still cannot preserve comments through a serde round-trip. It
  warns before overwriting a commented file and leaves a timestamped backup.
- Hooks still run under `-ExecutionPolicy Bypass`. That is deliberate — a hook
  is a script the config owner supplied on purpose — but it is a weakening, and
  it is now documented as one.
- `cargo-deny` treats `unmaintained` as a warning rather than an error, so an
  unmaintained dependency will not fail CI. Known vulnerabilities and yanked
  versions do.

### PR #36 — Cover command dispatch; pin the toolchain
**2026-08-16**

- **Changed:** the router's dispatch decision is now a pure `Verb::parse`,
  separate from running the command. `route` could not be tested directly —
  calling it would actually install vendors or hit the network — so the table
  had no coverage at all. Seven tests now cover it, including two that would
  have caught real classes of mistake: a name listed in `CONSOLE_COMMANDS`
  that does not route, and a verb that routes but is *missing* from that list,
  which would run with no console attached on a GUI launch.
- **Added:** `completions` shell-name parsing split out and tested, including
  a check that every advertised shell actually generates a non-empty script.
- **Added:** `rust-toolchain.toml` pinning the compiler. Left floating,
  `dtolnay/rust-toolchain@stable` moves under the repo, and a future stable
  that changes a lint turns `clippy -D warnings` red on a commit that touched
  nothing. CI now takes the version from that file alone rather than
  installing a second toolchain nothing uses.
- **Added:** `.editorconfig` matching what rustfmt already produces, with CRLF
  preserved for `.bat`/`.cmd`/`.ps1` so an editor does not undo
  `.gitattributes` on save.
- **Added:** a `cargo-deny` gate (`deny.toml` plus a CI job) covering
  advisories, licences and sources. It earns its place here specifically
  because the workspace carries a git dependency, `rusty_regx`, which no
  registry advisory feed watches on its own; `sources` also fails the build if
  a second, unvetted git dependency appears. The licence allow list is exactly
  what the current tree resolves to rather than a generous pre-approval, so a
  dependency under a new licence stops the build and gets a decision.

### PR #35 — Make the stub commands do what they claim
**2026-08-16**

Five commands reported success without acting. Four now act; one is retired.

- **Fixed:** `profile import` validated its input and then wrote nothing. It
  now merges the profile into `CustomProfiles` — deliberately not `Profiles`,
  so a built-in of the same name is never overwritten in place — with a
  timestamped backup, an atomic write, `--as <name>` and `--dry-run`. Like
  `migrate`, it reads the file verbatim so environment overrides are not
  baked in.
- **Fixed:** `setup-shell` never touched a startup file, and both branches of
  `--dry-run` were byte-identical. It now writes a marked, idempotent block to
  `$PROFILE` or `~/.bashrc`: re-running replaces the block rather than
  appending a second, an unchanged block is a no-op, and surrounding content
  survives. `cmd` still only prints, because it has no per-user startup file
  naner can edit — writing an AutoRun registry key behind someone's back is
  not a reasonable default, and that is now said out loud.
- **Fixed:** `pack` bundled only `config/` while claiming a "self-contained
  portable distribution", and ignored its documented `[dir]` argument. It now
  bundles `bin/`, `config/`, `home/`, `icons/` and `naner.bat`, honours
  `[dir]`, reports anything missing instead of quietly shipping a thinner
  archive, and skips transient files — `.downloads`, `.staging`, `.part`,
  `.tmp`, and the `.bak` files the config commands leave behind.
- **Fixed:** `self-update` printed "Self-update check completed" and replaced
  nothing. It now hands over to `naner-init`, which owns the update protocol,
  passing arguments through and returning its exit code. That is the honest
  design rather than a workaround: `naner.exe` cannot replace itself while
  running on Windows, which is why `naner-init` is a separate executable.
- **Removed:** `naner checksum`. It never computed or wrote anything, and what
  it was for is now covered properly — resolvers carry the distributor's
  digest, and `naner lock` records the exact version, URL and SHA-256 per
  vendor. It exits 2 with a pointer rather than silently disappearing.
- **Changed:** the back-up-then-atomically-replace discipline that `migrate`
  needed is now a shared helper, used by `profile import` and `setup-shell`
  too. Getting it wrong costs someone their configuration, so it lives in one
  place.

### PR #34 — Escape terminal arguments; validate env var names; make PreLaunch a gate
**2026-08-16**

- **Fixed:** values interpolated into the terminal's command line were not
  escaped, and the result goes to `Command::raw_arg`, so an embedded `"` ended
  the quoted section and injected further arguments. Reachable from
  `naner -d '<value>'`, so anything invoking naner with a caller-supplied
  directory was exposed — not only someone editing their own config. Quoting
  now follows the `CommandLineToArgvW` convention, including the trailing
  backslash run that would otherwise escape the closing quote. The existing
  byte-for-bit Windows Terminal argument test still passes, so ordinary
  inputs are unchanged.
- **Fixed:** environment variable *names* were interpolated raw into
  `--export-env` output while only values were escaped. That output is
  designed to be piped into `Invoke-Expression` / `eval`, so a crafted name
  became shell code in the consuming session. Names must now match
  `[A-Za-z_][A-Za-z0-9_]*`, as a validation **error** rather than a warning.
  Configs travel — `naner pack`, `naner profile export` — so "the user owns
  their own config" was not the whole story.
- **Fixed:** the `PreLaunch` hook's exit code was discarded, so a hook that
  failed still launched the terminal. A pre-launch gate that cannot stop the
  launch is not a gate. It now aborts with the reason. `PostLaunch` warns
  instead, since the terminal is already running by then and there is nothing
  left to prevent. A missing hook script is reported rather than silently
  succeeding.
- **Unchanged, deliberately:** hooks still run under `-ExecutionPolicy
  Bypass`. A hook is a script the config owner supplied on purpose and the
  default policy would refuse it. That is now documented as a weakening
  rather than left looking accidental.

### PR #33 — Stop `naner migrate` writing the environment into the config
**2026-08-16**

- **Fixed:** migrate serialized the *loaded* config, which `config::load`
  builds by folding in `NANER_ENV_*`, `NANER_DEFAULT_PROFILE` and the
  telemetry opt-out defaults, and by expanding `%NANER_ROOT%` to a concrete
  path. All correct for running naner; all wrong to write back to disk.
  `NANER_DEFAULT_PROFILE=Bash naner migrate` permanently rewrote the user's
  `DefaultProfile`. It now parses the file verbatim via a new
  `config::load_verbatim`.
- **Fixed:** `$schema`, `title` and `description` were dropped. Losing
  `$schema` breaks the IDE completion `naner schema` exists to provide. Any
  top-level key the model does not own is now carried across, ahead of the
  canonical body so an editor still finds it first.
- **Added:** a timestamped `.bak` beside the config, written before anything
  is overwritten, and refusal to proceed if the backup cannot be written.
- **Added:** `--dry-run`, which prints the result and writes nothing.
- **Changed:** the file is written to a temp path and renamed, so an
  interrupted run cannot truncate the config the launcher needs to start.
- **Changed:** `serde_json::to_string_pretty(&cfg).unwrap()` no longer panics
  on a serialization failure; it reports and exits non-zero.
- **Known limitation:** comments still cannot survive a serde round-trip. The
  command now warns before overwriting a commented file, and the backup makes
  it recoverable.

### PR #32 — Make `naner schema` describe the config that exists
**2026-08-16**

- **Fixed:** `naner schema config` advertised a `Services` block — "background
  sidecar daemons to run alongside terminal sessions" — that no field backs and
  no code implements, so a user following the schema got a silently ignored
  config section. Removed.
- **Fixed:** it omitted `WindowsTerminal`, `Advanced` and `CustomProfiles` at
  the top level and `PreLaunch` / `PostLaunch` on a profile, so roughly half
  the real surface had no autocompletion.
- **Fixed:** `naner schema vendors` had drifted the same way — no `installType`,
  `installerArgs` or `checksumSource`, and no enum on `releaseSource.type`,
  which now lists the six the loader accepts.
- **Changed:** both schemas moved out of the `execute` match into named
  functions, so the tests check the exact value the command prints rather than
  a copy.
- **Added:** two drift tests that make serde the source of truth — every key
  `NanerConfig` serializes must be described, and every described key must be
  one serde produces. The second is what catches an invented block; verified
  by re-introducing `Services` and watching it fail.
- **Known limitation:** `schema vendors` is still checked by eye against
  `VendorJsonEntry`, which is private to `naner-core`. It has no equivalent
  round-trip test.

### PR #31 — Honour the `enabled` flag; generate the vendor list
**2026-08-16**

- **Fixed:** `enabled` in `vendors.json` was parsed and then ignored, so
  `install --all` installed the seven vendors the shipped config switches off,
  and `install --list` advertised them. It is now honoured.
- **Changed:** listing deliberately still shows disabled vendors, marked
  `[--]` (`disabled` in `--porcelain`). Hiding them would make them
  undiscoverable — a user could never find the name to switch on. Installing
  one by name now says it is disabled rather than "unknown vendor", which
  would send them hunting for a typo.
- **Changed:** dependencies install regardless of `enabled`. They were not
  chosen from a menu; they are needed by something the user did choose, and
  failing the install instead would be the larger surprise.
- **Fixed:** omitting `enabled` means *enabled*. `#[serde(default)]` yields a
  bool `false`, so the very change that started reading the field would
  otherwise have switched off every entry that does not mention it — and the
  seven built-in essential definitions never set it, so a missing
  `vendors.json` would have installed nothing at all. Both defaults are now
  explicit and covered by tests.
- **Fixed:** `naner install` with no arguments generated its vendor list from
  the loaded definitions. The literal it replaced had drifted — it never
  gained `rustyterm` or `rush`, so the two newest vendors were undiscoverable.
- **Fixed:** `doctor --conflicts` announced a total and then silently printed
  five. It now says how many it is showing, and sorts first so the truncated
  view is the same list every run rather than an arbitrary five out of a
  `HashMap`.

### PR #30 — One owner for outbound HTTP; cache CI dependencies
**2026-08-16**

- **Fixed:** `GitHubReleasesClient` built its own agent and never read the
  proxy variables, so behind a corporate proxy vendor installs worked while
  `naner-init` bootstrap, `naner-init` update and `naner self-update` all
  failed with a bare "Failed to fetch release". Since `naner-init` is the entry
  point for a fresh install, a proxied user could not get started at all. Both
  clients now share `http::build_agent`, which is also what
  `ATLAS-BOUND-0001` asks for — one component owning the boundary.
- **Added:** `NO_PROXY=*` as a blanket opt-out, and an unusable proxy value is
  now reported and ignored rather than silently dropped.
- **Fixed:** `native_tls::TlsConnector::new().expect(...)` appeared in both
  constructors. `naner.exe` builds with `panic = "abort"` and
  `windows_subsystem = "windows"`, so a broken TLS stack aborted the process
  with no message at all on a GUI launch. It now warns and falls back to
  ureq's default TLS, which fails at request time with something readable.
- **Changed:** CI caches the Cargo registry and target directory
  (`Swatinem/rust-cache`), keyed per runner OS. Every run previously rebuilt
  the whole dependency tree; the Windows leg felt it worst, being the only one
  that does a release build under `lto = true` and `codegen-units = 1`.

### PR #29 — Stage downloads so an interrupted run cannot poison the cache
**2026-08-16**

- **Fixed:** transfers now stream into `<name>.part` and are published with a
  rename once complete. Writing straight to the final path meant a killed
  process — Ctrl-C, a crash, a lost machine — left a truncated file exactly
  where the next run looks for a finished one. Deleting on the error paths
  (shipped in #25) cannot cover that case, because nothing gets to run;
  staging makes it safe by construction.
- **Changed:** the cache decision moved from the transport to the installer,
  which is the only place that knows the expected digest. A cached asset is
  reused only if it is non-empty and, where a digest is known, matches it.
- **Fixed:** a stale cached asset is now discarded and re-fetched instead of
  being handed to the verifier, which would have failed the install rather
  than fixing it. This is a real case, not a hypothetical: names like
  `Miniconda3-latest-Windows-x86_64.exe` are stable while their contents move,
  and pinning (#27) makes the expected digest specific enough to notice.
- **Known limitation:** with no digest to check against, a complete cached file
  is still reused on name alone. That is the offline case the README
  advertises, and completeness is now guaranteed even though identity is not.
- 8 new tests (140 passed, 7 network-dependent `#[ignore]`d). Two of the new
  ones run a real transfer to confirm the artifact lands under its final name
  and no `.part` survives either success or failure.

### PR #28 — Stop reporting success when the vendor tree cannot be placed
**2026-08-16**

- **Fixed:** a failed staging swap was discarded (`let _ = fs_extra_copy(..)`),
  so `.vendor-version` was written and `Installed <vendor>` logged over a
  directory that never received the new tree. Because "installed" is judged by
  a non-empty directory, every later run then skipped the broken vendor. The
  placement result is now propagated: a failure logs the reason, returns false,
  and neither stamps a version nor pins the vendor.
- **Fixed:** the swap was not atomic on Windows. The target directory was
  re-created immediately before `fs::rename`, and `MoveFileExW` cannot replace
  an existing directory — so the rename failed every time on the one platform
  naner ships to, silently demoting every install to a recursive copy and
  losing symlinks with it. The previous tree is now moved aside instead, which
  lets the rename succeed; the copy path is left only for a genuine
  cross-device move, which two directories under `vendor/` will not hit.
  MSYS2, whose tree is full of symlinks and ~400 MB, benefits most.
- **Changed:** a failed install no longer destroys the install it was replacing
  — the previous tree is restored. The Windows Terminal merge still cannot be
  atomic, because preserving `settings/` rules out a swap; a part-way failure
  there leaves a mixed tree and is reported as a failure rather than papered
  over.
- **Fixed:** the recursive copy treated a missing source as success. That was
  the same silent-failure shape, one level down, and it is now an error.
- 6 new tests (158 passed). The end-to-end one was checked against the old
  behaviour first and fails on it, so it is a regression test rather than a
  restatement of what the code already does.

### PR #27 — Make the lockfile real; drop the inert window-effect field
**2026-08-16**

- **Added:** `naner.lock` now does something. A successful install pins the
  vendor's exact version, URL and SHA-256; later installs reproduce and verify
  that artifact instead of re-resolving to upstream latest. This is the only
  verification MSYS2 and the six GitHub-sourced vendors get — their distributors
  publish no digest, so #25's upstream-digest work could not reach them.
- **Added:** `naner lock [--refresh [vendor...]] [--porcelain]` to inspect pins
  and drop them. `--refresh` accepts a vendor's display name or key.
- **Changed:** `update-vendors` deliberately ignores the pin and rewrites it —
  honouring it there would make the command a permanent no-op on every pinned
  vendor. Reasoning in
  [ADR-0003](./docs/adr/0003-the-lockfile-pins-rather-than-records.md).
- **Removed:** `ProfileConfig::WindowEffect`. It was parsed and never read, and
  the README advertised backdrop effects (`Mica`, `Acrylic`, `Tabbed`) that
  nothing implemented. There is no delivery mechanism for it in the current
  design: the launch path passes CLI arguments, while a Windows Terminal backdrop
  is a `settings.json` profile property, and naner writes those settings only at
  install time via string substitution — deliberately not a JSON round-trip, so
  JSONC comments survive. README corrected rather than left overstating.
- **Known limitation:** the *first* install of a vendor without an upstream
  digest remains trust-on-first-use. The lock records what arrived; it cannot
  know whether that was the right thing. Pinning makes the second and later
  installs trustworthy, which is a real but weaker guarantee — said plainly in
  the module docs, the README and `naner lock`'s own output.
- 13 new tests (152 passed, 5 network-dependent tests `#[ignore]`d).

### PR #26 — Apply the standard governance file set
**2026-08-16**

- **Added:** `CONTRIBUTING`, `CODE_OF_CONDUCT`, `SECURITY`, `CHANGELOG`,
  `RELEASE_NOTES`, `ARCHITECTURE`, an ADR seed, PR and issue templates, and a
  `.gitattributes` that forces `eol=lf`. The line-ending one is not cosmetic here:
  this repo builds on both `windows-latest` and `ubuntu-latest`, so a
  Windows-authored `.sh` reaching the Linux runner with CRLF would die on its own
  shebang, far from the cause.
- **Added:** `ARCHITECTURE.md` documents the three-crate boundary table, each
  boundary's owner and failure contract, and cites the `ATLAS-001` requirements it
  answers to. It also records two known boundary violations rather than presenting
  a clean picture — split proxy ownership (#18) and vendor install reporting
  success on a failed swap (#14).
- **Deliberate scope cut:** no `ci-rust.yml` was added. The repo already has a
  `ci.yml` that does more (fmt + clippy + test across Linux and Windows, plus a
  Windows release build); a second workflow would duplicate every run. The
  governance audit's `ci-*.yml` glob does not match `ci.yml`, so it reports CI as
  missing — that is a false negative, not a gap.
- **Known limitation:** `CODE_OF_CONDUCT.md` and the review-approval rules in
  `CONTRIBUTING.md` are scaffold appropriate to a shared repo; this is currently a
  single-maintainer repo, so "at least one approval" is aspirational.

### PR #25 — Verify downloaded artifacts against upstream digests
**2026-08-16** · [#25](https://github.com/baileyrd/rusty_naner/pull/25)

- **Security:** nothing naner downloaded was verified beyond TLS transport trust.
  All 12 vendors omitted `checksum`, so the verifier took its "no checksum
  provided" branch every time — including for the artifacts that are *executed*
  (`rustup-init.exe`, the Miniconda installer, the 7-Zip MSI). `naner-init`
  replaced the installed `naner.exe` with whatever bytes arrived.
- **Added:** resolvers now carry the digest the distributor already publishes —
  Go and Node.js (SHA-256), the .NET SDK (SHA-512, via the channel manifest that
  also supplies the authoritative URL, replacing a hand-built one), plus a new
  optional `checksumSource` covering `rustup-init.exe` (`.sha256` sidecar) and
  Miniconda (repository listing). A `checksum` pinned in `vendors.json` still wins,
  so a compromised upstream manifest cannot overrule an operator's pin.
- **Added:** `naner-init` verifies `naner.exe` and `naner-bundle.zip` against a
  `SHA256SUMS` manifest now published by the release workflow, and fails closed.
  Safe rather than stranding, because the workflow enforces tag == embedded
  version — any release this binary installs from was built by the same workflow.
- **Fixed:** both download paths reject a short read and delete the partial file.
  Without the delete the size check is moot: the cache probe accepts any non-empty
  file, so the rejected partial would be reused next run (#15).
- **Fixed:** `vendors-schema.json` gains the `nodejs-api` and `dotnet-api` source
  types the loader has always supported. The shipped `vendors.json` did not
  validate against its own schema before this.
- **Known limitation:** MSYS2 publishes no digest sidecar (confirmed 404) and
  GitHub release assets expose none for the six vendors sourced that way. Those
  still install unverified unless pinned; closing that uniformly needs the
  lockfile in #20.
- 16 new tests (142 passed, 5 network-dependent tests `#[ignore]`d).

### PR #24 — Add the MIT license text
**2026-08-16** · [#24](https://github.com/baileyrd/rusty_naner/pull/24)

- **Fixed:** `Cargo.toml` had declared `license = "MIT"` since the first commit,
  inherited by all three crates, with no license text in the repo — an SPDX
  identifier and no actual grant.

### PR #10 — Unbreak the build
**2026-08-16** · [#10](https://github.com/baileyrd/rusty_naner/pull/10)

- **Fixed:** the workspace did not build from a clean checkout. `naner-core`
  declared five dependencies as paths escaping the repo (`../../../rusty_*`), which
  exist only on a machine with sibling checkouts — `cargo metadata` alone failed.
  `rusty_regx` is back on its pinned commit SHA; the other four were removed
  outright, being referenced nowhere. CI had been red on `main` for three weeks.
- **Fixed:** a panic in the case-insensitive matcher behind `%NANER_ROOT%` /
  `%{ARCH}` expansion. It took byte offsets from a lowercased *copy* of the input
  and applied them to the original, which desynchronizes whenever a character's
  lowercase form differs in UTF-8 length — `ẞ` → `ß` panicked on a mid-character
  slice, `İ` → `i̇` silently emitted a mangled path. Because `naner.exe` builds with
  `panic = "abort"` and `windows_subsystem = "windows"`, this made a GUI launch
  vanish with no message.
- **Fixed:** 56 rustfmt diffs and 7 clippy warnings; the previous feature commit
  had never been through either gate.

---

## v0.5.0 — 2026-07-09

The Phase 5 cutover: the Rust implementation became the published build.

- **Added:** the release workflow publishes Rust-built `naner.exe`,
  `naner-init.exe` and `naner-bundle.zip` to `baileyrd/naner` with identical tag
  and asset names, guarded by a tag == package-version check. Deployed
  `naner-init` installations look up the release matching their embedded version,
  so the two must never drift.
- **Fixed:** the post-parity bug wave B1–B6 — see
  [docs/post-parity-fix-wave.md](./docs/post-parity-fix-wave.md).
- All validation gates in [docs/VALIDATION.md](./docs/VALIDATION.md) signed off.
