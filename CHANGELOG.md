## [Unreleased]

## [0.9.27] - 2026-09-03
### Fixed
- Follow-up to 0.9.26's `naner update`/`naner init` console-flash fix,
  reported live again: even without the extra console window, naner's
  final status line (e.g. "[OK] Naner is already up to date!") still
  landed spliced into the parent PowerShell tab's next prompt row
  instead of appearing above it. Confirmed via before/after screenshot
  comparison this was never caused by 0.9.26's change -- reexec-vs-not
  made no observable difference. Root cause: a GUI-subsystem process
  (`#![windows_subsystem = "windows"]`) that `AttachConsole`s to its
  parent shell's console is never *waited for* by that shell --
  `cmd.exe`/PowerShell dispatch it and move straight on to draw their
  own next prompt, so naner's console writes and the shell's prompt
  redraw are two independent writers on the same shared console with
  no ordering guarantee between them. Every exit path in `naner.exe`
  now calls the new `console::detach()` (`FreeConsole`) immediately
  before terminating -- after every byte naner will ever write has
  already landed, so there is nothing left for the shell's own
  prompt-draw to race against. Applies to every console-attached
  command, not just `update`/`init`.

## [0.9.26] - 2026-09-03
### Fixed
- `naner update` and `naner init` flashed a second, real console window
  open on every run when attached to a parent shell (PowerShell/cmd),
  even when nothing interactive was about to happen -- the `#81`
  keystroke-race workaround (re-exec into a `CREATE_NEW_CONSOLE` window)
  fired unconditionally at the top of both commands, before either knew
  whether it would ever call `prompt_yes`. Reported live: `naner
  update`'s common "Naner is already up to date!" path opened that
  second console, printed into it, and closed it instantly -- no
  "press any key" pause on that path -- racing the parent PowerShell
  tab's own prompt redraw and visibly corrupting the terminal output.
  `reexec_in_own_console_if_racy` now runs immediately before each
  function's first `prompt_yes` call instead of unconditionally at
  entry, so a console of naner's own only opens when a prompt is
  actually imminent; `naner check-update`, which never prompts, was
  already unaffected.

## [0.9.25] - 2026-08-28
### Added
- New command: `naner reclaim [--dry-run]` -- sweeps `.claude/`,
  `.claude.json` (+ its `.backup.*` siblings), `.codex/`, and `.gemini/`
  out of the real Windows profile into `%NANER_ROOT%\home`, for the three
  cases (see docs/VALIDATION.md's Known limitations) where the shipped
  `Environment.EnvironmentVariables` redirects cannot reach them at all.
  Bridges the original location back to the moved copy afterward: a
  directory junction (`mklink /J`, via the same mechanism
  `Advanced.HomeJunctions` already uses, so no admin/Developer Mode
  needed) for `.codex/`/`.gemini/`, a real symlink for the single-file
  `.claude.json` (NTFS reparse points only redirect directories, so this
  one does need `SeCreateSymbolicLinkPrivilege` -- Developer Mode or
  Administrator; a failure there is reported, not fatal, since the file
  is already safely moved either way). Never overwrites: if naner's home
  already has its own copy of something that also leaked, the leaked
  copy is preserved under a timestamped name instead of being discarded
  or clobbering what's there. Resolves the real profile directory via
  `SHGetKnownFolderPath(FOLDERID_Profile)` rather than trusting
  `USERPROFILE`, which may already be naner's own redirected value if
  invoked from inside an already-launched naner shell.
- New vendor: `Gemini` (`@google/gemini-cli`, npm) — Google's agentic
  coding CLI, alongside the existing `ClaudeCode`/`Codex` vendors. Not
  naner-managed before, so its `npm install -g` (and every subsequent
  invocation) ran wherever the ambient shell's `npm`/`gemini` resolved,
  outside naner's controlled `home\.npm-global` and outside the
  redirected environment entirely when installed globally from a
  non-naner shell.

### Fixed
- Reported live: Claude Code, Codex CLI, and Gemini CLI dotfolders all
  leaking into the real Windows profile from naner-launched shells.
  Codex is a native Rust binary (unlike the npm-installed Claude/Gemini)
  that resolves its home directory through the OS known-folder API, not
  by reading `USERPROFILE` -- the existing `USERPROFILE` redirect that
  covers every Node/Python/Go tool never reached it. Added `CODEX_HOME`
  (Codex's own documented override, same pattern as `CLAUDE_CONFIG_DIR`)
  to the shipped `Environment.EnvironmentVariables`. Gemini CLI has no
  config-dir override of its own upstream
  (`google-gemini/gemini-cli#2815`, unresolved) and reads `os.homedir()`
  like Node tools generally do, so the existing `USERPROFILE` redirect
  already covers it once it runs through a naner-managed install --
  closed by adding the `Gemini` vendor above rather than a new
  environment variable.

## [0.9.24] - 2026-08-27
### Added
- `dist-assets/scripts/` -- a fifth bundled/packed directory (alongside
  `bin/`, `config/`, `home/`, `icons/`) for user-owned scripts, ported back
  from a live installation's own convention. `naner pack`'s `BUNDLED` list
  now includes it.
- Default PowerShell profile (`home/.config/powershell/profile.ps1`)
  improvements ported back from the same live installation: guarded
  `posh-git`/`Terminal-Icons`/`z` module imports (only loaded if already
  present -- none of the three are naner-vendored), persistent UTF-8
  console output for Nerd Font glyphs, `....` and `~` navigation
  shortcuts, a `Get-EnvVars` helper (`env` alias), and a guarded
  `oh-my-posh init` prompt (only runs if the optional `OhMyPosh` vendor is
  installed) replacing the hand-rolled `prompt` function -- naner already
  vendors oh-my-posh but the shipped default profile never used it.

### Fixed
- `naner --export-env` crashed with a visible Rust panic (`failed printing
  to stdout: The pipe is being closed. (os error 232)`) when invoked as
  `Invoke-Expression (naner.exe --export-env)` -- the exact form naner's own
  `--help` documented. Root cause is on PowerShell's side and confirmed
  against `PowerShell/PowerShell#25875`: for a `/SUBSYSTEM:WINDOWS` process
  (naner.exe, deliberately, to avoid a console flash), PowerShell's
  subexpression-capture and native `>` redirection do not reliably wait for
  the process before tearing down the handle they gave it, racing naner's own
  write. That race can't be won from naner's side, but naner doesn't need to
  crash losing it: `handle_export_env` (and `naner root`, the other
  documented pipeline-composable primitive) now write stdout via a new
  `console::write_stdout_best_effort`, which swallows a write failure instead
  of letting `print!`'s panic-on-error propagate -- matching Unix's default
  `SIGPIPE` disposition, which already makes this a non-issue there. The
  reliable forms (`naner --export-env | Invoke-Expression`, what
  `setup_shell.rs` actually installs, and bash's `eval "$(naner --export-env)"`)
  were never affected; `--help`'s PowerShell example now shows the
  pipe form instead of the broken subexpression one.
- The shipped default PowerShell profile aliased `ll` to `Get-ChildItem`
  (line ~85) and then defined a `function ll` with fancier formatting
  (line ~147) further down -- PowerShell resolves an alias before a
  function of the same name, so the fancier `ll` was silently dead code
  since the profile was first written. Renamed the alias to `l`, freeing
  `ll` to reach the function. Also stopped aliasing `grep` to
  `Select-String`: naner vendors a real `grep.exe` (Git for
  Windows/MSYS2) on the same PATH, with entirely different flag syntax --
  the alias silently shadowed it for anyone who typed `grep` expecting
  the real thing.
- Follow-up to 0.9.23's `--allow-scripts=<package>` fix: that flag only
  reaches npm invocations naner itself makes (`naner install`,
  `update-vendors`). Reported live again -- `claude update` (Claude Code's
  own self-updater, which shells out to `npm install -g` directly with
  none of naner's CLI flags) hit the identical gate and put the 500-byte
  `bin/claude.exe` placeholder back in place, breaking `claude` with the
  same "not a valid application for this OS platform" symptom. Every
  `Npm`-type vendor install now also persists `allow-scripts=<package>`
  into `home/.npmrc` (npm's own userconfig, since naner points
  HOME/USERPROFILE at `home/`) -- npm parses it as a comma-separated list
  (`@npmcli/config/lib/parse-allow-scripts-list.js`), so a second vendor's
  entry appends rather than clobbers, an already-listed package is a
  no-op, and every other line in an existing `.npmrc` is preserved
  untouched. Covers any future npm invocation for the package, including
  ones naner never mediates.

## [0.9.23] - 2026-08-24
### Fixed
- Reported live: `claude --version` failed with Windows' generic "This
  version of ... claude.exe is not compatible with the version of Windows
  you're running" -- the loader trying to run a shell script as a PE
  image. `@anthropic-ai/claude-code`'s own `bin/claude.exe` is a tiny
  placeholder (`echo "Error: claude native binary not installed."`, 500
  bytes) that ships in place of the real ~330 MB native binary until its
  `postinstall` script (`node install.cjs`) links the actual one in from
  its per-platform optional dependency. npm's own log showed why it never
  ran: recent npm versions gate install-time lifecycle scripts behind an
  `allowScripts` allowlist by default, and `npm_install_command` never
  passed one, so npm silently blocked it -- "1 package had install
  scripts blocked because they are not covered by allowScripts" -- and
  left the placeholder in place as the "installed" binary. An *earlier*
  install of the same package, before npm itself had been self-updated,
  ran the postinstall fine, confirming this is a real npm behavior
  change, not a one-off fluke. Every `Npm`-type vendor install now passes
  `--allow-scripts=<package>`, npm's own suggested remedy, scoped to
  exactly the package this call installs -- inert for a package with no
  lifecycle scripts to gate. Verified live: reinstalling replaced the
  500-byte stub with the real 337,745,056-byte binary and `claude
  --version` now reports `2.1.241 (Claude Code)`.

## [0.9.22] - 2026-08-24
### Fixed
- Follow-up to #154: even after v0.9.21 fixed `naner install
  MsvcBuildTools`'s `msiexec` bugs, a full install still could not
  produce a working compiler -- `cargo build` failed linking with `LNK1181:
  cannot open input file 'kernel32.lib'`. Confirmed against Microsoft's
  live channel manifest that the pinned `Windows SDK Desktop Libs x64`
  package is current, not stale -- it genuinely never shipped
  kernel32.lib, ntdll.lib, user32.lib, advapi32.lib, ws2_32.lib, or
  userenv.lib, despite the name. Found the real owner by extracting
  `winsdksetup.exe` as a cabinet (its unnamed first member is
  `BurnManifest.xml`) and querying each candidate MSI's own File table via
  `WindowsInstaller.Installer`: all six live in "Windows SDK for Windows
  Store Apps Libs" instead. New `SDK_STORE_LIBS` component fetches it.
  `msvcrt.lib` (which rustc's MSVC linking always requests via
  `/defaultlib:msvcrt`) was missing for the same reason one level up: the
  VC++ Tools "Desktop" CRT package doesn't carry it, only the "Store" one
  does (`Microsoft.VC.14.44.17.14.CRT.x64.Store.base.vsix`, added as a 5th
  `VC_PACKAGES` entry). Fixing the first gap exposed a real, separate bug:
  `SDK_STORE_LIBS` and the pre-existing `SDK_LIBS` both extract into the
  same `Lib\<ver>\um\x64` marker directory, and `extract_msi_component`'s
  "already there" check looked at the shared target directory before the
  current run's own fresh output -- so by the time `SDK_STORE_LIBS` ran,
  `SDK_LIBS`'s prior merge had already made the marker directory exist,
  and `SDK_STORE_LIBS`'s own kernel32.lib/etc. were silently never merged
  in. Now checks the current run's `scratch` output first. Verified
  end-to-end: a full `cargo build --release --workspace` and `cargo test
  --workspace` (231 tests) against the assembled toolchain both pass
  clean, and the resulting `naner.exe` runs.

## [0.9.21] - 2026-08-24
### Fixed
- Reported live: `naner install anaconda` failed every attempt with
  `Failed to extract packages` (installer exit code 2), `install.log`
  showing `CreateDirectory: can't create "$INSTDIR\tmp" (err=5)` --
  ACCESS_DENIED -- immediately after `$INSTDIR` was created, before a
  single file was written. Anaconda's constructor-built installer hardens
  against CVE-2025-64343 by revoking write access on `$INSTDIR` for
  Authenticated Users/BUILTIN Users right after creating it, then
  compensates for a non-elevated run by granting `FullAccess` back to
  `$USERDOMAIN\$USERNAME` -- read via `ReadEnvStr`, not queried from
  Windows. A launching process whose environment never had those two
  variables set ends up compensating an empty principal, and every write
  under `$INSTDIR` fails from that point on. `run_exe_installer` now also
  sets `USERDOMAIN`/`USERNAME` from `GetUserNameExW(NameSamCompatible)` --
  the real token identity, regardless of what the parent process handed
  it -- so the compensating grant always targets a resolvable principal.
- Reported live: `naner install MsvcBuildTools` failed every attempt
  extracting the Windows SDK with `msiexec extraction failed`
  (`ERROR_INVALID_COMMAND_LINE`, 1639), before writing any log. `KITSROOT`'s
  value always contains a space (`Windows Kits`), which forced
  `Command::arg`'s automatic Windows quoting to wrap the *entire*
  `KITSROOT=...` token in one outer pair of quotes -- but unlike most
  Windows programs, msiexec's own command-line parser only accepts a
  quoted *value* half (`PROPERTY="value"`), rejecting a quoted whole
  token outright. `extract_msi_component` now builds that argument with
  `raw_arg`, quoting only the value. Underneath that: `fetch_msi_component`
  downloaded each component's external `.cab` into an `Installers\`
  subfolder next to the `.msi`; `msiexec /a`'s admin install actually
  resolves it flat, directly beside the `.msi` (confirmed via `/lv`
  verbose logging: `Error 1311. Source file not found (cabinet)`) -- cabs
  are now downloaded flat alongside their `.msi`.

## [0.9.20] - 2026-08-24
### Fixed
- The v0.9.18 fix (self-update also reconciling `config/naner.json`/
  `config/vendors/`) didn't actually reach anyone updating *from* a
  pre-fix version: `updater::update_from_release` only replaces the
  binary file on disk -- the process performing that swap keeps executing
  its own, now-superseded, in-memory code (Windows has no way to hot-swap
  a running exe's code section), so a v0.9.17 process self-updating to
  v0.9.19 ran the reconciliation using v0.9.17's own compiled-in vendor
  catalog, not the one actually just installed. Confirmed live: updating
  straight from v0.9.17 landed the v0.9.19 binary correctly but still did
  not add `MsvcBuildTools` to `config/vendors/`. `naner update` now
  re-invokes the freshly-installed binary (as `update-vendors
  --sync-config-only`, a new undocumented flag that runs only the
  config/vendor-defaults merge, never the full vendor-reinstall pass) so
  the reconciliation always runs with the code that was actually just
  shipped.

## [0.9.19] - 2026-08-24
### Fixed
- Reported live: `naner update-vendors` reinstalled `RustyTerm` (an
  "Experimental Rust-based terminal emulator") and `Rush` on every run,
  regardless of whether either was ever installed -- and once installed
  that way, launching `RustyTerm` failed with a GPU-related error. Root
  cause: the hardcoded fallback vendor list
  (`essential_vendor_definitions`) bundles `RustyTerm`/`Rush` alongside the
  four true bootstrap essentials (`SevenZip`/`PowerShell`/
  `WindowsTerminal`/`GitForWindows`) as a safety net for a broken
  `vendors.json`, but none of the six had `required` set (all silently
  defaulted to `false`) -- and `update-vendors`' essential-vendor selection
  treated "is in this list at all" as "always keep current", not "is
  actually required". `RustyTerm`/`Rush` ship `"enabled": true` (like
  every optional vendor) but `"required": false`, so this force-installed
  them regardless of whether the user ever asked for either. The four true
  essentials now carry `required: true` in the hardcoded list too, and
  vendor selection reads `required` off the real, loaded config (falling
  back to the hardcoded value only when a vendor is entirely absent from
  it) instead of blind list membership. Same root cause silently broke
  `naner repair`'s essential-vendor recovery, which already checked this
  same (always-`false`) flag and could never actually re-bootstrap a
  missing essential vendor -- now fixed too.

## [0.9.18] - 2026-08-24
### Fixed
- `naner update`/`naner self-update` (the binary swap) never called the
  `config/naner.json`/`config/vendors/` reconciliation that `naner
  update-vendors` already ran — so a newly shipped vendor (`MsvcBuildTools`
  in v0.9.17) never reached an already-initialized tree's `config/vendors/`
  after self-updating; it only appeared once the separate,
  easy-to-forget `update-vendors` command was also run. The self-update
  path now reconciles both, same as `update-vendors` already did.

## [0.9.17] - 2026-08-24
### Added
- New vendor: `MsvcBuildTools` — a portable MSVC compiler/linker (VC++
  Tools 14.44.35207) and Windows SDK (10.0.26100.0), for the
  `x86_64-pc-windows-msvc` target `naner` itself ships against. The
  standard `vs_buildtools.exe` bootstrapper needs admin no matter what
  `--installPath` is given — it registers with the VS Installer service
  and writes MSI-based state machine-wide regardless of where the toolset
  lands — so instead this fetches the individual VSIX/MSI payloads that
  bootstrapper would fetch and extracts them directly (the same technique
  `mmozeiko/portable-msvc` and `Data-Oriented-House/PortableBuildTools`
  use). VC++ Tools packages carry their own SHA-256 in the VS 17.14
  channel manifest; the Windows SDK's packages don't (`Win11SDK_10.0.26100`
  is 229 anonymous hashed `.cab`s) — those pins came from extracting the
  Burn manifest embedded in `winsdksetup.exe` itself, matching named MSI
  packages (`Windows SDK Desktop Headers x64`, ...) to the hashed files the
  channel manifest actually publishes. Dispatched by vendor key, bypassing
  the generic single-artifact resolver/installer entirely (many payloads
  merge into one tree); not pinned by `naner.lock` since there is no
  upstream "latest" to compare a hardcoded pin table against. `enabled:
  true` like every other optional vendor.

### Fixed
- A tree that first ran before a new `Environment.EnvironmentVariables` key
  shipped (`USERPROFILE`/`TEMP`/`TMP`/`APPDATA`/`LOCALAPPDATA` in v0.9.14,
  the XDG trio, `CLAUDE_CONFIG_DIR`, ...) never picked it up: `naner
  update`'s config merge only ever added missing `VendorPaths`/`Profiles`
  keys and appended `PathPrecedence` entries, never new
  `Environment.EnvironmentVariables` keys — so an already-initialized tree
  kept leaking `.codex`/`.gemini`/etc. into the real Windows profile
  forever on a bare `naner.exe` swap, even after the shipped default
  closed the leak for a brand-new install. The merge now adds missing
  `Environment.EnvironmentVariables` keys the same way it already does for
  `VendorPaths`/`Profiles`.

## [0.9.16] - 2026-08-23
### Fixed
- `naner update`'s "Update now?" prompt could be accepted (`Y` echoed
  correctly) and then silently do nothing -- no error, no install, no
  "Press any key to exit" -- instead of the previously-reported hang.
  Reproduced live: the `CREATE_NEW_CONSOLE`-relaunched, GUI-subsystem child
  only inherits the right to foreground its own window for a short default
  grace period after `CreateProcess` returns, and the version check's
  blocking GitHub API call, which runs before the prompt, sits squarely
  inside that gap -- every prior fix touched handle association or how
  input is read, never window focus. Added `console::force_foreground`
  (`SetForegroundWindow` right before the interactive read) and
  `console::allow_foreground` (`AllowSetForegroundWindow` from the parent
  right after spawning the child, widening its eligibility window instead
  of leaving it to the default).
- The `OhMyPi` vendor installed the wrong package: npm's unscoped
  `oh-my-pi` (an unrelated small extension for a different "pi" CLI,
  provides `oh-my-pi`) instead of the actual `omp` coding agent CLI
  (`@oh-my-pi/pi-coding-agent`, npm-provenance-attested, homepage
  `omp.sh`). The real package only declares `engines.bun`, no
  `engines.node`, so it can't run under naner's vendored Node at all --
  `Npm`-type vendors now install through `bun add --global` instead of
  `npm install -g` when their `dependencies` name `Bun` instead of
  `NodeJS`. `OhMyPi.json` now points at `@oh-my-pi/pi-coding-agent` and
  depends on `Bun`.
- `%NANER_ROOT%\home\.bun\bin` (where `bun add --global` links its bins)
  was never on `PathPrecedence` -- same class of bug as the `zed`
  `pathPrecedence` fix in v0.9.15: an npm-via-bun vendor could install
  cleanly and still not resolve from a naner-launched terminal. Added.
- `naner-core`'s `build.rs` only emitted `cargo:rerun-if-changed` on the
  `dist-assets/config/vendors/` directory itself, which Cargo does not
  reliably re-trigger on for an in-place edit to a file already inside it
  (only add/remove is guaranteed) -- confirmed live: an edited vendor file
  built against a stale embedded catalog until forced. Now also watches
  every vendor file individually.

## [0.9.15] - 2026-08-23
### Fixed
- `naner update-vendors` only ever refreshed the four hardcoded essential
  vendors, silently skipping every optional vendor (`nodejs`, `ruby`, `go`,
  ...) the user had actually installed with `naner install`. It now also
  updates every installed, enabled optional vendor; an available-but-not-
  installed vendor is still left alone.
- `naner install <npm-vendor>` (e.g. `codex`) could fail with "response too
  big for into_string": `fetch_npm` fetched the full npm packument instead
  of the `latest`-tag manifest. Now resolves against `GET
  /<package>/latest`.
- The shared HTTP client sent a GitHub-specific `Accept` header on every
  request, including npm/PyPI/other registries. `registry.npmjs.org`
  rejects it with `406`, which `fetch_npm` silently read as "no release
  found." That header is now scoped to `api.github.com` only.

### Added
- New optional vendors: `Ruff` and `Ty` (Astral's Python linter/formatter
  and type checker), GitHub-release-sourced with `.sha256` sidecar
  verification, same shape as `Uv`.

### Fixed
- `Zed.json`'s pinned `checksum` had gone stale (didn't match the real
  `v1.16.1` asset) with nothing to catch it: `refresh-pins` only ever
  rewrote `fallback`, never a static `checksum`. It now also rewrites
  `checksum.value` when GitHub's release API publishes a `digest` for the
  resolved asset and it disagrees with the pin. Never adds a `checksum`
  object that wasn't already there.
- `naner install obsidian`/`imageglass` failed with "no matching release
  found upstream": both repos' `/releases/latest` sometimes points at a
  release with no asset matching `assetPattern` (Obsidian interleaves
  mobile-only releases with desktop ones in the same repo).
  `fetch_github` now falls back to the newest non-prerelease release in
  the full `/releases` list that actually has a matching asset.
- `naner install zed` reported success but `zed` was never runnable from a
  naner-launched terminal: `Zed.json` had no `pathPrecedence`, unlike every
  other CLI-shaped vendor. `postInstallFunction`, present on this vendor,
  looks like it should wire that up but isn't read anywhere in the Rust
  port at all. Added `pathPrecedence` pointing at `vendor\zed\bin` (the
  actual CLI-launcher directory) and `provides: ["zed"]`.

## [0.9.14] - 2026-08-21
### Fixed
- `naner install`/`update-vendors` never applied naner.json's home
  isolation to the installer subprocess it spawns for a vendor whose
  `releaseSource` runs a real installer `.exe` (Anaconda, `rustup-init.exe`)
  -- that code path never runs `run_launcher`'s `setup_environment`, so the
  installer inherited the host's raw environment. Anaconda's installer
  registers its base env into `~/.conda/environments.txt` as its last step
  regardless of user action, so every `naner install anaconda` /
  `update-vendors` wrote a stale entry into the *real* Windows profile's
  `.conda/environments.txt`, one per install, forever. `run_exe_installer`
  now sets `USERPROFILE`/`HOME`/`APPDATA`/`LOCALAPPDATA`/`TEMP`/`TMP` on the
  spawned subprocess, pointed into naner's own home tree; the same set is
  now also applied ahead of the npm/pip package-manager install path's own
  `NPM_CONFIG_*`/`PYTHONUSERBASE` vars.
- The same two installers additionally self-register a Start Menu folder
  and an Add/Remove Programs registry entry under `HKCU`, independent of
  the `/D=`/`-o`-style install directory naner gave them -- unlike every
  archive-extracted vendor, whose whole footprint is its own `target_dir`.
  Worse, because naner installs into `vendor/.staging/<name>` and only
  renames into place afterward, Anaconda's registry entry pointed at the
  now-deleted staging path from the moment install finished. `naner`
  snapshots both before running an installer `.exe` and removes whatever
  is new afterward (diffed, not name-matched, since a versioned Add/Remove
  Programs display name or an installer's own Start Menu folder name is
  per-release knowledge that breaks on the next version bump).
- `APPDATA`/`LOCALAPPDATA` were never redirected at all -- the convention
  most Windows dev tools (npm, pip, Docker Desktop, VS Code, NuGet, Git
  Credential Manager, `go env -w`) actually use for their own config/cache,
  not `USERPROFILE\.foo`. Added to `naner.json`'s `EnvironmentVariables`,
  mirroring a real profile's `Roaming`/`Local` layout; `setup_environment`
  creates both directories unconditionally on launch, same guarantee it
  already gave `TEMP`.

## [0.9.13] - 2026-08-21
### Fixed
- Reported live, still unconfirmed on real Windows: `naner update`'s
  "Update now?" prompt could hang forever inside naner's own relaunched
  console even after v0.9.9's `refresh_std_handles` -- no warning, no
  exit, nothing, while Task Manager confirmed the process was genuinely
  alive and blocked and the prompt text had rendered correctly.
  `std::io::stdin()`'s buffered line read never saw the keystrokes even
  though `console::wait_for_keypress` (used for naner-init's "Press any
  key to exit", the identical relaunched-console scenario) reads raw via
  `ReadConsoleInputW` against a freshly fetched handle and has never
  shown this symptom. Added `console::read_line_raw`, that same
  primitive generalized to a full line; `prompt_yes` now tries it first
  inside naner's own console, falling back to the old `stdin` path only
  when it reports no real console to read from (piped/redirected stdin,
  where EOF-is-no must stay exactly as it was).
- Reported live: `naner install GitHub CLI`, typed straight out of
  `naner install --list`, failed with `Unknown vendor: GitHub` /
  `Unknown vendor: CLI` -- the shell splits the unquoted space into two
  arguments. `naner install <name>` already resolved a vendor's
  space-free JSON key too, but the list never showed it, so a
  multi-word display name was the only thing to type and could never be
  typed unquoted. `naner install --list` now hints the key in
  parentheses for every name containing a space (11 of the shipped
  vendors), e.g. `GitHub CLI (GitHubCli)`.

### Added
- `USERPROFILE`/`TEMP`/`TMP` now redirect into naner's own home tree
  (`naner.json`), same as `HOME` already did: `USERPROFILE` points at
  `%NANER_ROOT%\home` (a tool reading only `USERPROFILE` -- `os.homedir()`
  on Windows, `os.path.expanduser`, many Node/Electron/Go tools -- now
  lands there too, same as everything else); `TEMP`/`TMP` point at
  `%NANER_ROOT%\home\.tmp`, which `setup_environment` now creates at
  startup since (unlike the XDG cache/data dirs already redirected here)
  no spec obligates a tool to create its own TEMP directory before using
  it. Scoped to processes launched from within a naner shell -- their own
  children inherit it, the host is untouched -- but that does include any
  GUI app run from that shell (naner's own vendored ones included), whose
  own Save/Open dialogs and downloads may now default into naner's home
  instead of the real Windows profile. Also makes the `Advanced.IsolateEnvironment`
  + `USERPROFILE` fix from earlier moot going forward: `USERPROFILE` is
  now always naner-owned, isolated or not.
- `Advanced.HomeJunctions` (`naner.json`): directory junctions
  (`mklink /J` -- no admin or Developer Mode needed, unlike a real
  symlink) created under `home\` on first launch after init, bridging
  specific real Windows locations back out from underneath the
  `USERPROFILE` redirect above. Shipped default links `Documents`,
  `Downloads`, and `Desktop` to their real counterparts via the new
  `%HOST_USERPROFILE%` token (the real profile directory, captured
  before naner's own redirect overwrites it for this process) plus a
  personal `dev` to `C:\dev`. Skipped, not an error, when a target
  doesn't exist yet or something's already at the link path -- never
  overwrites.

## [0.9.12] - 2026-08-21
### Fixed
- Reported live: `naner install anaconda` failed every attempt with
  `Installer exited with code 2`. Anaconda's constructor-based silent
  installer (`/S /D=<target>`) aborts if its target directory already
  exists, even empty. `install_vendor` pre-created that directory for
  every install type before running the extractor, and `run_exe_installer`
  did the same again, so an `.exe` installer always found its own target
  already there. Both pre-creations are removed for the exe-installer
  path -- an installer creates its own destination; archive extractors
  (zip/tar/msi) already create theirs internally, so only the `binary`
  install type (a plain file copy) still needs it done ahead of time.

## [0.9.11] - 2026-08-21
### Fixed
- Reported live: with `Advanced.IsolateEnvironment` on, double-clicking
  `naner.exe` threw `Could not access starting directory
  "C:\tools\naner\%USERPROFILE%"`. `USERPROFILE` was missing from
  `env_isolation::KEEP_ON_ISOLATE`, so isolation cleared it -- but
  `naner.json` deliberately leaves `%USERPROFILE%` unexpanded by naner
  itself (profiles start there; it's the one thing tools that ignore `HOME`
  still resolve), so it stayed literal and `wt.exe` resolved it relative to
  its own working directory (`naner_root`) instead. Added `USERPROFILE` to
  the keep list -- same category as the `ProgramFiles` family: a standard
  per-user OS variable, not a tool-install indicator.

## [0.9.10] - 2026-08-20
### Fixed
- Reported live while chasing a false `[OK] Rust` in `naner install --list`:
  an enabled vendor's PATH entries and environment variables were merged
  into the effective config regardless of whether that vendor was actually
  installed. With `Advanced.IsolateEnvironment` off, this let a host
  `rustup` (installed independently of naner, found first on PATH since
  naner's own Rust was never installed) inherit naner's pre-set
  `CARGO_HOME`/`RUSTUP_HOME` and write into naner's empty vendor directory
  -- making an uninstalled vendor look installed. `merge_vendor_environment`
  now filters vendors through `is_vendor_installed` before contributing
  either PATH entries or variables: `enabled` means "wanted", not
  "present".

## [0.9.9] - 2026-08-20
### Changed
- Diagnostics only, no confirmed fix: `naner update`'s "Update now?" prompt
  is still reported stuck live -- a screenshot showed the prompt text
  staying visible while the process had already exited before a key was
  pressed (a stray keystroke landed in the calling shell's own
  PSReadLine history search instead), meaning stdin hit EOF immediately
  rather than genuinely hanging. `console::refresh_conin` (v0.9.7) is
  broadened to `refresh_std_handles`, refreshing stdin *and* stdout/stderr
  together right before a prompt, closing the possibility that they end up
  associated with different console sessions. `prompt_yes` also now warns,
  only inside naner's own relaunched console (never for the by-design
  silent EOF-is-no path scripted/CI use relies on), when the reopen fails
  or stdin reads EOF/errors immediately -- so the next live report carries
  hard data on which of those actually happens, instead of another
  screenshot to interpret.

## [0.9.8] - 2026-08-20
### Fixed
- Reported live while testing `Advanced.IsolateEnvironment` (#128): the
  `ProgramFiles`/`ProgramFiles(x86)`/`CommonProgramFiles`/`ProgramW6432`
  family wasn't on the isolation allowlist, so clearing them under
  isolation broke a script that referenced `ProgramFiles(x86)` (surfaced as
  a bare `x86` command not being recognized). Added the whole family to
  `env_isolation::KEEP_ON_ISOLATE` -- they're standard OS directory
  locations, not tool-install indicators, same category as the
  `PROGRAMDATA`/`ALLUSERSPROFILE` entries already kept.

## [0.9.7] - 2026-08-20
### Fixed
- Reported live: `naner update`'s "Update now? (Y/n):" prompt (and
  potentially other interactive prompts following a blocking network call,
  on a freshly allocated double-click console) stopped responding to
  keystrokes -- confirmed not a window-focus issue. Added
  `console::refresh_conin`, which unconditionally re-associates
  `STD_INPUT_HANDLE` with a fresh `CONIN$` handle right before every
  interactive prompt read, reusing the same mechanism that already fixed
  the analogous #81 `CREATE_NEW_CONSOLE`-relaunch stdin issue. Root cause of
  *why* the handle stops delivering input after a blocking network call is
  still unconfirmed; this addresses the reported symptom.

## [0.9.6] - 2026-08-20
### Added
- `Advanced.IsolateEnvironment` (`naner.json`) / `NANER_ISOLATE_ENVIRONMENT`: a
  testing/dev switch, off by default. When on, every process environment
  variable outside a small OS-survival allowlist (`env_isolation`) is cleared
  before naner sets NANER_ROOT/NANER_ENVIRONMENT/HOME/PATH/configured
  variables, so tools already installed system- or user-wide on the host
  can't leak into a test run. `--export-env`'s emitted script also unsets
  those names in the calling shell, so a profile launched directly from
  Windows Terminal's own list is isolated too, not just a normal `naner`
  launch.

## [0.9.5] - 2026-08-19
### Fixed
- The "Press any key to exit..." pause at the end of `naner init` (and other
  exit paths that hold a console of naner's own open) said "any key" but
  actually did a line-buffered `stdin` read: it required Enter and echoed
  every character typed beforehand onto the screen. Added
  `naner_core::console::wait_for_keypress`, a real single-keypress read via
  `ReadConsoleInputW` with `ENABLE_LINE_INPUT`/`ENABLE_ECHO_INPUT`
  temporarily cleared (mode restored after), falling back to the old
  line-read if raw mode can't be set up.

## [0.9.4] - 2026-08-19
### Fixed
- Windows Terminal installation during first-run bootstrap printed the
  entire configuration-validation warning report — every not-yet-installed
  vendor's missing directory, every icon that doesn't exist yet, the whole
  `naner.json` — three times in a row. A #83 regression:
  `WindowsTerminalConfigurator::create_settings` called `config::load`
  (which validates and logs on every call) three separate times for a
  single settings.json write. `naner.json` is now loaded once per
  `create_settings`/`update_settings` call and threaded through instead of
  reloaded.

## [0.9.3] - 2026-08-19
### Fixed
- `reexec_in_own_console_if_racy` (the #81 relaunch that dodges the
  keystroke race by spawning a console of naner's own) silently fell back
  to running inline whenever the spawn itself failed — reproduced live on a
  real machine where `Command::new(exe).creation_flags(CREATE_NEW_CONSOLE)`
  errors, in both a naner-launched shell and a plain `Windows PowerShell`
  window. No second console ever appeared; the interactive prompt printed
  and accepted a cursor in the *original* window instead, indistinguishable
  from the pre-#81 race itself, with no indication anything had gone wrong.
  The fallback now logs the spawn error and the `Start-Process -Wait`
  workaround before continuing, instead of failing silently.

## [0.9.2] - 2026-08-19
### Changed
- Windows Terminal's four Naner profiles in `settings/settings.json` are now
  generated fresh from `config/naner.json`'s own `Profiles`, on every
  install/update, instead of being hand-duplicated a second time in WT's
  own schema in `dist-assets/home/.config/windows-terminal/settings.json`
  (now deleted). GUIDs are unchanged and still fixed, never derived, so
  every already-installed `settings.json` reconciles normally. A profile
  launched directly from Windows Terminal (not via `naner --profile X`)
  gets a `naner.exe --export-env --no-comments | Invoke-Expression`
  self-bootstrap spliced into its `-Command`, matching what the old
  template always did for that case; `naner --profile X` itself needs no
  such thing, since it sets the environment before spawning `wt.exe`. (#83)

### Fixed
- The `Rust` vendor's `pathPrecedence`/`CARGO_HOME`/`RUSTUP_HOME` pointed at
  `vendor/rust/cargo/bin` and `vendor/rust/rustc/bin` — folders `rustup-init`
  never creates. rustup actually installs every proxy binary (`rustup`,
  `cargo`, `rustc`, `rustfmt`, ...) into a single `$CARGO_HOME/bin`, and the
  installer already points `CARGO_HOME`/`RUSTUP_HOME` at `vendor/rust/.cargo`
  and `vendor/rust/.rustup` (the vendor's own dir, per the documented
  install-time redirect) — the vendor config just never matched. `rustup`
  was "installed" and reported "on PATH" by every diagnostic, yet not found
  in any naner-launched shell. Fixed `Rust.json` and `naner.json`'s
  `VendorPaths` to point at the real `.cargo/bin`.

### Changed
- All optional vendors now ship `"enabled": true` by default — `naner
  install --all` installs the full vendor set out of the box. Anyone who
  wants a leaner tree can flip individual vendors back to `"enabled": false`
  in `vendors.json`.

## [0.9.1] - 2026-08-19
### Fixed
- `naner init`/`naner update`, when re-launched into a console of their own
  (the #81 keystroke-race fix), never wired up stdin — `AllocConsole`
  claimed the console `CreateProcess` had just created for the child and
  output ended up bound correctly, but `STD_INPUT_HANDLE` was never
  reopened, so the `Y`/Enter prompt sat there taking every keystroke and
  doing nothing, silently, in the relaunched window. `console::setup` now
  reopens `CONIN$` the same way it has always reopened `CONOUT$`.

## [0.9.0] - 2026-08-19
### Added
- `naner suggest <name> [--porcelain]`: maps a command the shell failed to
  find to the vendor providing it (vendor `provides` lists first, then
  `VendorPaths`-derived names) and prints the install/enable/"not a naner
  shell" hint; silent with exit 1 on no match. `setup-shell` writes matching
  command-not-found hooks for PowerShell and Bash, the shipped `profile.ps1`
  gains the PowerShell hook, and vendor definitions accept an optional
  `provides` array (shipped for NodeJS, Bun, Go, Rust, Ruby, Anaconda,
  DotNetSDK, GitForWindows, PowerShell, SevenZip). (#103)
- New vendor `Uv` (uv, Astral's Python package/project manager): GitHub
  release source with `.sha256` sidecar verification, disabled by default,
  `provides: ["uv", "uvx"]`, cache/python/tool dirs redirected under
  `%NANER_ROOT%\home`, and a `VendorPaths` entry for `uv.exe`.
- `naner refresh-pins [dir] [--dry-run] [--porcelain]`: re-resolves upstream
  latest for every dynamically-sourced vendor and rewrites the `fallback`
  pins in `config/vendors/*.json`; static-URL vendors reported manual-only.
- `naner outdated [--porcelain]`: compares installed vendors'
  `.vendor-version` against live upstream, flags `outdated (major)` for
  first-segment jumps, exits non-zero when updates exist.
- `naner doctor` now prints an offline "updates are available" nudge when an
  installed vendor is older than its shipped fallback pin (porcelain:
  `stale_installed`), plus a lenient vendor-version comparator in
  `naner-core::version` (`vendor_compare`/`vendor_major_differs`) that
  handles `go1.21.6` / `bun-v1.3.14` style strings the C#-quirk comparator
  mangles.
- The first-run bootstrap (and `naner init`) now offers to put naner on the
  user PATH after a successful install — the same edit `naner add-to-path`
  makes, as an opt-in `(Y/n)` prompt. Declining just prints how to do it
  later, a registry failure does not fail the bootstrap, and non-interactive
  runs (EOF) decline automatically.
- Two new vendor `releaseSource.type`s: `npm` (registry.npmjs.org latest
  dist-tag, installed via the vendored NodeJS's `npm install -g` into
  `home\.npm-global`) and `pip` (pypi.org JSON API, installed via the
  vendored Anaconda's `pip install --user` into `home\.local`). Neither is
  pinned by `naner.lock`; both leave a `.vendor-version` marker only.
- `installType: "binary"`: a verified download placed as-is under
  `binaryName`, for vendors that ship a single executable with nothing to
  extract.
- Seven new vendors: `GitHubCli`, `ClaudeCode`, `OhMyPi`, `Codex` (npm),
  `NotebookLmCli` (pip), `OhMyPosh` (binary, checksum-verified via a
  shared `checksums.txt`), `Antigravity` (static, unverified — no
  published digest). All disabled by default.

### Changed
- Fallback pins refreshed (URLs verified live): Go `go1.26.6`, NodeJS
  `v26.7.0`, DotNetSDK `10.0.400`, MSYS2 `20260611`. GitHub-sourced pins
  unchanged — unreachable from the refreshing environment (`api.github.com`
  blocked); static-URL vendors remain manual.

### Fixed
- `version_from_file_name` picked the first digit run in a file name, so
  scrape-resolved installs recorded junk versions (`.vendor-version` = `"2"`
  for MSYS2, `"7"` for 7-Zip). It now picks the run with the most digits and
  trims trailing dots. Surfaced by the first real `refresh-pins` pass.
- Dotfolders no longer leak into the real Windows profile for redirectable
  tools: shipped environment gains `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/
  `XDG_CACHE_HOME` (under `%NANER_ROOT%\home`) and `CLAUDE_CONFIG_DIR`;
  Bun's vendor file gains `BUN_INSTALL`. `USERPROFILE` deliberately stays
  untouched; tools reading only it cannot be redirected by environment.

## [0.8.2] - 2026-08-18
### Added
- `naner add-to-path [--remove] [--dry-run]`: puts `<NANER_ROOT>\vendor\bin`
  on the per-user PATH (`HKCU\Environment`, no admin) so `naner` resolves
  from any shell without importing the whole environment the way
  `setup-shell` does. The registry value is edited directly rather than via
  `setx` (which truncates at 1024 characters), its type and other entries
  are preserved, and a `WM_SETTINGCHANGE` broadcast makes new shells pick
  the change up. `--remove` undoes it; matching is case-insensitive and
  tolerant of trailing-slash/quoted variants.

## [0.8.1] - 2026-08-18
### Fixed
- The #81 keystroke race is closed in code instead of documentation. Neither
  `cmd.exe` nor PowerShell waits for a GUI-subsystem process, so an
  interactive prompt read from the parent shell's console competed with the
  shell's own next prompt for keystrokes — bootstrap and `update` looked
  hung when run bare from a shell. Interactive flows now detect the attached
  state, relaunch themselves in a console of their own (where nothing
  competes), wait, and mirror the exit code; the child pauses before its
  window closes so the outcome is readable. Piped/redirected stdio never
  re-execs, so scripts and CI are unaffected. The `Start-Process -Wait` /
  `start /wait` wrappers still work but are no longer required.

## [0.8.0] - 2026-08-18
### Changed
- **Breaking (packaging): naner is one binary.** `naner-init.exe` is retired;
  `naner.exe` is launcher, installer, and updater in one. A bare launch on an
  uninitialized tree runs the interactive bootstrap the init binary used to
  own (prompt, bundle-by-embedded-tag, essentials, optional launch); new
  `init`, `update`, and `check-update` subcommands carry the explicit
  commands, and `self-update` stays as an alias of `update`. `naner update`
  installs the latest release into every copy of the binary the tree carries
  — the running one first, via the field-proven rename-aside swap — and
  refreshes pre-0.8.0 `naner-init.exe` leftovers, since a stale one is a
  standing downgrade hazard. Releases still publish a `naner-init.exe` asset
  (a byte-copy of `naner.exe`): deployed 0.6.x–0.7.x updaters require the
  asset to exist, and the new binary behaves correctly under the old name.
- Interactive prompts treat EOF as *no*. Previously a closed stdin read as an
  empty line, which counted as yes — so any non-interactive spawn of the bare
  binary in an empty directory silently consented to downloading and
  installing a full tree. Caught by CI the first time it could happen.

### Fixed
- A tree whose only configuration is a pre-v0.7.0 `naner.yaml`/`naner.yml`
  is now told exactly that, by file name, with the fix — convert it to
  `config/naner.json` — instead of a generic "no configuration file found"
  while a good-looking config sits right there. New `ConfigError::LegacyYaml`
  variant in the loader; the first-run report gains the same hint.
- README's install instructions no longer claim PowerShell waits for
  `naner-init.exe` — it does not (no shell waits for a GUI-subsystem
  process), and the #81 keystroke race reproduces there too, observed live
  during the v0.7.1 validation. Both shells now get a waiting-wrapper
  command (`Start-Process -Wait` / `start /wait`). Also added the
  `Unblock-File` step: SmartScreen blocks a freshly downloaded unsigned exe
  silently, which presents as "nothing happens".

### Added
- `docs/VALIDATION.md` Step 6 — the self-update validation procedure
  (forced-update variant via a doctored `.naner-version`, real-upgrade
  variant, and the offline fail-closed spot-check), as validated for real
  on v0.7.1.

## [0.7.1] - 2026-08-18
### Changed
- `naner-init update` (and `naner self-update`, which hands over to it) now
  updates to the **latest published release** and replaces both binaries —
  `naner-init.exe` itself included, via rename-aside, since Windows will
  rename a running exe but not overwrite one. Previously the update synced
  `naner.exe` to the init's own embedded version, so nothing ever surfaced
  that a newer release existed; updating meant knowing to manually download
  a new `naner-init.exe` first. Both downloads are verified against the
  release's `SHA256SUMS` before either file is touched, and the init is
  swapped first so an interrupted update leaves a tree that offers the
  update again rather than one whose stale init would offer a downgrade.
  `naner-init check-update` now also compares against the latest release.
  Plain launches stay offline: the launch-time check is unchanged.

### Removed
- `naner.bat`. Nothing in the workspace called it, and its one remaining
  justification — launching without a network round-trip — turned out to be
  no justification at all: the launch-time check was always two local file
  reads. `naner-init` covers the double-click and pass-through cases from
  the same root directory, with bootstrap on top.

## [0.7.0] - 2026-08-18
### Fixed
- `Http::download` now retries up to 3 times on a dropped connection
  instead of failing the install outright — observed for real on Anaconda
  (~1 GB, by far the largest artifact naner downloads): a connection reset
  partway through failed the whole install, and `static`-type vendors like
  it have no fallback URL to fall through to the way `github`-type ones
  do. Added a local-server regression test that deterministically
  reproduces a truncated-then-complete connection without touching the
  real network.
- Four vendors added in the same batch as HiFile/OneCommander/etc. (HiFile,
  Obsidian, Zed, Zen) shipped `installerArgs` that never referenced
  `%TARGETDIR%`, so `naner install` ran their installer silently and
  successfully but let it fall through to its own default location
  (Program Files/AppData) instead of the vendor directory — naner still
  reported success and pinned a version over an empty `vendor/<name>/`
  folder. Added the missing target-directory switch for each installer
  technology (`/DIR=` for the two Inno Setup installers, `/D=` — last,
  unquoted — for the two NSIS ones), and a regression test that loads the
  real shipped `vendors.json` and fails if any future `.exe`-installer
  vendor's `installerArgs` omits the placeholder.
- `Environment.PathPrecedence` in the shipped `naner.json`/`naner.yaml` now
  includes `%NANER_ROOT%\home\.local\bin` and `\Scripts` — `PYTHONUSERBASE`
  already pointed at `home\.local` (for `pip install --user` and, as it
  turns out, the Claude Code CLI's native installer), but nothing on that
  path was ever added to naner's own PATH, so tools placed there were
  invisible even inside naner-launched shells.
- `merge_shipped_naner_defaults` now reconciles `Environment.PathPrecedence`
  into an existing `naner.json`/`.yaml`, not just `VendorPaths`/`Profiles`
  keys and the hardcoded field-migration list — the entries added above
  (and any future `PathPrecedence` addition) now reach an already-installed
  tree via `naner update-vendors`, not only a fresh init. Uses the same
  `.naner-managed-*.json` marker technique `wt_config.rs` already uses for
  Windows Terminal profiles, so an entry the user deliberately removed is
  never silently added back.
- `dist-assets/naner.bat`, the shim that ships at the root of every bundle,
  set `NANER_ROOT` from `%~dp0` verbatim — which always ends in a backslash,
  so the exported value escaped the closing quote of any `"%NANER_ROOT%"` a
  child process built into a command line. It now strips the separator and
  joins paths explicitly. The same file still advertised a PowerShell
  fallback at `src\powershell\Invoke-Naner.ps1` and a `src\csharp` build
  tree, neither of which exists in this repo, so its not-found path printed
  instructions that could not work; it now names `naner-init.exe` and the
  releases page instead. Added a regression test that reads the real shipped
  file, since nothing else in the workspace does.

- `naner.bat` now hands over to `naner-init.exe` when `vendor\bin\naner.exe`
  is missing, instead of only printing an error — it looks at the root (where
  a first-time user drops it) then `vendor\bin` (where an install that has
  updated itself keeps it), and launches it via `start /wait`, since
  `naner-init` is a GUI-subsystem binary that `cmd.exe` does not wait for
  (the same race as #81). `naner-init` prompts before downloading anything
  and then launches naner with the original arguments, so the shim recovers
  a half-installed tree without doing anything silently. With neither binary
  present it still fails loudly, listing every path it checked.

- **Breaking:** `config/vendors.json` is replaced by `config/vendors/`, one
  JSON file per vendor named after the key it declares. The pre-split file is
  no longer read; a tree that still has one gets an explicit warning saying so
  rather than the generic "not found", because the silent consequence is
  falling back to four hardcoded essentials and losing the other eighteen.
  `build.rs` assembles the files into the single catalog `config_merge.rs`
  embeds, so `include_str!` still has one file to point at and a bare
  `naner.exe` swap still carries the catalog. `merge_shipped_vendor_defaults`
  now writes a file per missing vendor instead of editing the user's JSON,
  which makes "never overwrite a customized entry" structural rather than a
  key-by-key check, and stops one malformed entry blocking every other vendor
  from being added. `vendors-schema.json` and `naner schema vendors` describe
  a single-vendor file. Vendor listing order is now sorted by file name.

### Removed
- The plugin surface: `dist-assets/config/plugin-schema.json` and the
  `directory_names::PLUGINS`/`ALL` constants. Nothing read any of it — `ALL`
  had zero call sites, and the C# plugin loader it descended from was marked
  "do not port" (MIGRATION_ANALYSIS §202) because the shipping entry point
  never enabled it. The schema described a *different* unbuilt design again —
  a manifest bundling vendors, env vars and PATH entries, with `.ps1` hooks —
  whose vendor record was a strict subset of a `vendors.json` entry. Two dead
  designs for one word is the reason reading the config directory was
  confusing.
- **Breaking:** YAML configuration. `config/naner.yaml` had silently drifted
  out of sync with `naner.json` (it was missing the `Naner` vendor path), and
  since the loader takes the first file that exists and never merges, the two
  could disagree indefinitely with only one of them ever read. Gone with it:
  `config/yaml.rs`, `load_yaml`, the `naner.yaml`/`naner.yml` entries in
  `CONFIG_FILE_NAMES`, the YAML branch of `merge_shipped_naner_defaults`, and
  the `serde_yaml_ng` dependency. A tree whose only config is `naner.yaml` now
  reports no configuration found rather than loading a format nothing else
  supports.

- A vendor's PATH entries and environment variables now live in the vendor's
  own file (`pathPrecedence`, `environmentVariables`, `pathPriority`) instead
  of `naner.json`'s global `Environment` block — 17 of 26 PATH entries and 19
  of 22 variables were vendor-owned, so adding a vendor took edits in three
  places and no single file described one completely. `naner.json` keeps its
  own 9 PATH entries and 3 variables, plus a `%VENDOR_PATHS%` marker saying
  where the vendor block lands; the marker matters because `%NANER_ROOT%\opt`
  sits after it and must stay lowest-precedence. Inter-vendor order comes from
  `pathPriority` (lower first, so it wins conflicts — Git for Windows and
  MSYS2 both ship a `bash.exe`), with unranked vendors sorting after ranked
  ones by key. The merge happens inside `config::load`, so all six readers of
  `config.environment` get the merged view and none can drift. A test asserts
  the assembled PATH is identical, entry for entry, to the list `naner.json`
  used to carry.

### Fixed
- `vendors-schema.json` had a `$ref` pointing at a `definitions` block that
  the per-vendor split removed, so the schema resolved to nothing and
  validated anything. Restored, extended with the three new fields, and
  guarded by tests for dangling `$ref`s and for every field the shipped
  vendor files actually use.

### Changed
- **Breaking:** a vendor with `enabled: false` no longer contributes its
  `pathPrecedence` entries or `environmentVariables`. Previously every entry
  sat in `naner.json` unconditionally and only `build_unified_path` dropping
  nonexistent directories kept an uninstalled vendor off PATH — so a vendor
  installed and then switched off kept its directory on PATH and its variables
  set, which is what switching it off should have stopped. On a fresh tree the
  effect is nil (the directories do not exist yet); the difference shows on a
  tree where a vendor was installed and later disabled.
- `DOTNET_CLI_TELEMETRY_OPTOUT` is no longer force-set by
  `apply_env_overrides`; `config/vendors/DotNetSDK.json` is its only source.
  The overrides ran first and the merge only fills in missing keys, so the
  code always won and the vendor file's copy was dead. Combined with the
  change above, the variable now follows its vendor: with no .NET SDK enabled
  it is not set, there being no `dotnet` CLI to opt out of.
  `POWERSHELL_TELEMETRY_OPTOUT` and `AZURE_CORE_COLLECT_TELEMETRY` stay in
  code — neither belongs to a vendor naner installs.

### Added
- README now has real "Installation" and "Usage" sections — how to get
  `naner-init.exe`, what it does on first run, and the common `naner`
  commands — instead of only the migration-phase history and CLI reference
  it had before.

## [0.6.5] - 2026-08-17
### Added
- Eight new optional vendor definitions in `config/vendors.json`: HiFile,
  OneCommander, Podman, ImageGlass, Inkscape, Obsidian, Zed and Zen. Like
  every other optional vendor they ship `enabled: false` and install only
  when explicitly requested.

### Changed
- **Breaking for pre-v0.6.5 installs.** `.github/workflows/release.yml` now
  publishes tagged releases to this repo (`baileyrd/rusty_naner`) instead of
  cross-publishing to the pre-rewrite `baileyrd/naner` repo, and
  `constants::github::REPO` now points `naner-init`'s update check at
  `rusty_naner` to match. Installs from before this change still check
  `baileyrd/naner`, which no longer receives new tags, so they will not see
  this release or any later one — those installs need a fresh
  `naner-init.exe` from `rusty_naner` to resume getting updates.

### Fixed
- Two `launcher` tests (`resolve_shell_falls_back_to_path_like_the_terminal_does`,
  `rusty_term_discovery_prefers_vendor_path_then_path_env`) each mutated the
  process-global `PATH` env var without synchronizing against each other,
  racing under `cargo test`'s default multi-threaded runner. Caught on a
  real Windows CI run: the race let the real system `bash.exe` (Git for
  Windows ships one on `windows-latest` runners) leak into a window where
  the test expected `PATH` to be empty. Both now hold a shared test-only
  mutex for their full set/act/restore sequence.

## [0.6.4] - 2026-08-17
### Added
- `update-vendors` now reconciles `config/naner.json` (or `.yaml`/`.yml`) and
  `config/vendors.json` against the defaults this binary ships, not just
  `settings/settings.json` — a bare `naner.exe` swap (the documented update
  path) previously never touched either file, so a vendor-set change like
  #64 never reached an already-installed tree. New `VendorPaths`/`Profiles`/
  vendor keys are always added; a handful of specific fields changed by #64
  (`VendorPaths.GitBash`, `Profiles.Bash.Description`,
  `Profiles.Bash.CustomShell.ExecutablePath`) refresh only when the current
  value still matches exactly what naner last shipped there — a hand-edited
  value is never touched (#72).
- `.github/workflows/release.yml` re-downloads every published asset from
  its real public URL after publishing and re-hashes it against
  `SHA256SUMS`, failing the job if they do not match — closes the gap that
  let a broken upload ship live and undetected (#66).

### Fixed
- `naner install`/`update-vendors` no longer print the raw
  `\r    Progress: N%` download bar when stdout is not a terminal — the
  existing "Tier-3 auto-quiet in pipelines" behavior already suppressed
  every other status line in that case; the progress bar was the one thing
  left over, noise with none of the substantive text to explain it (#67).
- `naner doctor` now returns a non-zero exit code when a required vendor is
  missing or the configuration file cannot be loaded, instead of always
  reporting success regardless of what it found (#68).
- `naner install A B C` now reflects an unknown or disabled name in its
  final exit code even when another requested vendor installs
  successfully, instead of reporting overall success with part of the
  request silently dropped (#69).
- `resolve_shell`'s missing-Bash install hint now says
  `naner install GitForWindows`, not the pre-#64 `naner install msys2` (#70).
- `--export-env` no longer sets `MSYSTEM`/`MSYS2_PATH_TYPE` unconditionally
  on a fresh install, now that MSYS2 is disabled by default (#71).

## [0.6.3] - 2026-08-17
### Changed
- Default vendor set: **Git for Windows** replaces MSYS2 as the required,
  enabled-by-default provider of Bash/Git (portable `PortableGit-*.7z.exe`,
  installed like the other self-extracting `.exe` vendors). MSYS2 remains
  installable by name but is no longer enabled or required. **Anaconda**
  replaces Miniconda as the optional Python distribution (same
  `repo.anaconda.com` digest-scrape verification, pointed at `/archive/`
  instead of `/miniconda/`). **.NET SDK** is now disabled by default.
- Added **Bun** as an optional vendor (disabled by default, GitHub-sourced,
  no upstream digest — installs trust-on-first-use like the other
  GitHub-sourced vendors).
- `VendorPaths.GitBash` and the shipped `Bash` profile now point at
  `vendor\git\bin\bash.exe`; `PathPrecedence` gains `vendor\bun` and the
  Git for Windows subdirectories ahead of MSYS2's (still listed, for a
  tree where it has been re-enabled).

## [0.6.2] - 2026-08-17
### Added
- `update-vendors`/`install WindowsTerminal` now reconciles Naner's own
  profiles into an existing `settings/settings.json` by GUID instead of
  leaving it untouched: a profile the user still has is refreshed to match
  the current template, one they never had is added, and one they removed
  on purpose stays gone (tracked via `.naner-managed-profiles.json`) (#52).

### Fixed
- `naner -p <name>` now fails loudly (`Profile not found`, the available
  list, exit 1) on a mistyped or removed profile instead of silently
  falling back to the default profile with exit 0. Only the explicit `-p`
  case is stricter; not passing `-p` at all keeps today's behavior (#57).
- Launching a profile whose shell vendor is not installed now fails with
  `<Shell> is not installed - run \`naner install <vendor>\`` instead of
  handing Windows Terminal a path it cannot find, which surfaced as a raw
  NT status code (`0x80070002`) from a different program naming a path the
  user never typed (#41).

## [0.6.1] - 2026-08-16
### Fixed
- The Windows Terminal profiles `naner install WindowsTerminal` writes
  (`Naner (Unified)`, `Naner PowerShell`, `Naner Bash`, `Naner CMD`) no longer
  hardcode a dev machine's path. The shipped template had baked in
  `C:\tools\cmd_line\naner` since `v0.5.0-alpha.0` instead of the
  `%NANER_ROOT%` placeholder `create_settings` actually substitutes, so on
  every other install `defaultProfile` (pinned to "Naner (Unified)") failed
  to find `naner.exe` the moment Windows Terminal was opened directly rather
  than through `naner.exe`/`naner.bat` (#58).
- `profile.ps1`'s custom `prompt` no longer overwrites the Windows Terminal
  tab title on every command. It was unconditionally resetting it to a
  generic `pwsh in <folder>`, clobbering the descriptive `--title` naner sets
  at launch (#59).

## [0.6.0] - 2026-08-16
### Added
- `rust-toolchain.toml` pinning the compiler version, and `.editorconfig`.
- `cargo-deny` supply-chain gate (`deny.toml` + CI job) over advisories,
  licences and sources, covering the `rusty_regx` git dependency that registry
  advisory feeds do not.
- Standard governance file set: `CONTRIBUTING`, `CODE_OF_CONDUCT`, `SECURITY`,
  `CHANGELOG`, `RELEASE_NOTES`, `ARCHITECTURE`, ADR seed, PR/issue templates.
- `.gitattributes` forcing `eol=lf`, with CRLF retained for `.bat`/`.cmd`/`.ps1`.
- `LICENSE` (MIT), matching the long-standing `license = "MIT"` declaration.
- Vendor downloads verified against upstream-published digests (Go, Node.js,
  .NET SDK, `rustup-init.exe`, Miniconda), with an optional `checksumSource`
  in `vendors.json` and pinned `checksum` entries taking precedence.
- `naner-init` verifies release assets against a `SHA256SUMS` manifest, now
  published by the release workflow, and fails closed.
- `naner.lock`: a successful install pins the vendor's exact version, URL and
  SHA-256; later installs reproduce and verify it. Covers MSYS2 and the
  GitHub-sourced vendors, which publish no upstream digest.
- `naner lock [--refresh [vendor...]] [--porcelain]` to inspect and drop pins.

### Changed
- CI caches Cargo registry and build artifacts per runner OS.
- `update-vendors` ignores any existing pin and rewrites it, so updating is not a
  no-op on pinned vendors.

### Removed
- `naner checksum`, which never computed or wrote anything; superseded by
  automatic digest verification and `naner lock`.
- `ProfileConfig::WindowEffect`, which was parsed and never read. The README's
  claim of `Mica`/`Acrylic`/`Tabbed` backdrop support has been corrected.

### Fixed
- `naner --export-env` no longer prints the first-run notice to stdout, where the
  calling shell would try to execute it; the notice moves to stderr and the
  command exits non-zero when nothing was exported (#38).
- Console output is ASCII, so a Windows console on the default code page no
  longer renders `[x]`, bullets and check marks as mojibake (#39).
- `setup-shell` writes a block pointing at `vendor\bin\naner.exe` rather than
  `bin\naner.exe`, so the shell integration actually runs; `VendorPaths.Naner`
  in the shipped config had the same wrong path (#42).
- Five further non-ASCII characters missed by the first sweep, in `lock`,
  `self-update` and the vendor list, with a test that now enforces it (#39).
- A pinned install no longer prints `Latest version:`, which claimed a currency
  check that a pin by definition does not perform.
- "Restart your terminal to use the newly installed tools." is no longer printed
  when every install failed and nothing was placed.
- Vendor update lines no longer print a doubled `v` (`vv1.24.11911.0`) when the
  recorded version already carries the prefix.
- `update-vendors` no longer overwrites an existing Windows Terminal
  `settings.json` from the template, which destroyed every colour scheme, key
  binding and custom profile on each run while reporting that it preserved them
  (#50).
- `install MSYS2` fetches the newest archive on the index rather than the first,
  which on an ascending directory listing was the oldest -- a base two years
  stale, under a line reading "Fetching latest" (#47).
- `update-vendors` honours `enabled` in `vendors.json`, and says which vendors it
  skipped. It previously reinstalled disabled vendors on every run (#48).
- Built-in vendor definitions now carry a `key`. Without one they shared a
  single `naner.lock` entry, losing pins and -- on a tree with no readable
  `vendors.json` -- resolving one vendor's artifact as another's pin (#53).
- `naner profile import` writes the profile instead of only validating it;
  `setup-shell` writes an idempotent block to the shell startup file; `pack`
  bundles the whole distribution and honours its `[dir]` argument;
  `self-update` delegates to `naner-init` instead of doing nothing.
- A failing `PreLaunch` hook now aborts the launch instead of being ignored.
- `naner migrate` no longer writes environment overrides, telemetry defaults
  or expanded paths into the config file, no longer drops `$schema`/`title`/
  `description`, keeps a timestamped backup, writes atomically, and gained
  `--dry-run`.
- `naner schema config` no longer describes a non-existent `Services` block,
  and now covers `WindowsTerminal`, `Advanced`, `CustomProfiles`, `PreLaunch`
  and `PostLaunch`; `schema vendors` gains `installType`, `installerArgs`,
  `checksumSource` and a `releaseSource.type` enum.
- `enabled` in `vendors.json` is honoured; disabled vendors are still listed
  (marked) so they stay discoverable, but are not installed.
- `naner install` generates its vendor list instead of using a stale literal.
- `doctor --conflicts` reports how many collisions it is showing and sorts
  them, so the truncated view is deterministic.
- Proxy settings are honoured on every outbound request, not just vendor
  downloads; `naner-init` bootstrap and update previously ignored them.
- A broken TLS stack no longer aborts the process; it warns and falls back.
- Downloads stage to `<name>.part` and are published by rename, so an
  interrupted run leaves nothing that the next run mistakes for a complete
  cached asset.
- A cached asset that does not match the expected digest is discarded and
  re-fetched instead of failing the install.
- Vendor install no longer reports success when the staged tree cannot be
  placed; the failure is logged and no version marker or lock pin is written.
- The staged-tree swap is atomic again on Windows: the target is moved aside
  rather than re-created before the rename, which previously guaranteed the
  rename failed and demoted every install to a recursive copy.
- A failed placement restores the previous install instead of leaving a
  half-populated directory.
- Workspace did not build from a clean checkout — four unused path dependencies
  removed and `rusty_regx` repinned to its commit SHA.
- Panic and silent path corruption in the case-insensitive `%NANER_ROOT%` /
  `%{ARCH}` matcher on non-ASCII input; same defect in `.tar.xz` suffix handling.
- Truncated downloads are rejected instead of being treated as complete, and the
  partial file is deleted so it is not reused as a cache hit.
- `vendors-schema.json` accepts the `nodejs-api` and `dotnet-api` source types the
  loader has always supported, and documents `checksum` / `checksumSource`.
- rustfmt and clippy gates restored to green.

### Security
- Vendor artifacts and self-update binaries are integrity-checked before use;
  previously nothing was verified beyond TLS transport trust.
- Terminal arguments are escaped, so a caller-supplied `--directory` cannot
  inject flags into the spawned terminal's command line.
- Environment variable names must be valid identifiers, so a crafted name
  cannot become shell code in `--export-env` output.

## [0.5.0] - 2026-07-09
### Added
- Phase 5 cutover: the release workflow publishes the Rust-built `naner.exe`,
  `naner-init.exe` and `naner-bundle.zip`, guarded by a tag == version check.

### Fixed
- Post-parity bug wave B1–B6 (see `docs/post-parity-fix-wave.md`).
