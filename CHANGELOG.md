## [Unreleased]
### Added
- Eight new optional vendor definitions in `config/vendors.json`: HiFile,
  OneCommander, Podman, ImageGlass, Inkscape, Obsidian, Zed and Zen. Like
  every other optional vendor they ship `enabled: false` and install only
  when explicitly requested.

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
