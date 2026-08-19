## [Unreleased]

Nothing merged since v0.9.0.

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
