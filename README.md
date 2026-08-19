# rusty_naner

Rust migration of [naner](https://github.com/baileyrd/naner), the portable terminal
environment launcher for Windows.

## Installation

`rusty_naner` ships as a single self-bootstrapping executable — there is no
separate installer.

1. Download `naner.exe` from the
   [latest release](https://github.com/baileyrd/rusty_naner/releases/latest).
   It is launcher, installer, and updater in one binary (the separate
   `naner-init.exe` retired in 0.8.0; the release still carries an asset by
   that name — a byte-copy — so older installs can update).
2. Put it in an empty folder. That folder becomes `NANER_ROOT` — everything
   naner owns (its own config, vendor tools, and binary) lives under it, so
   the whole install is self-contained and can be removed by deleting the
   folder.
3. **Unblock the download first.** Windows stamps files fetched by a browser
   with the Mark of the Web, and SmartScreen then blocks an unsigned exe with
   an unfamiliar hash — silently, since a GUI-subsystem binary has no console
   to print an error into. "Nothing happens" on a freshly downloaded
   `naner.exe` is almost always this. In PowerShell:

   ```powershell
   Unblock-File .\naner.exe
   ```

   (or right-click → Properties → tick **Unblock**.)

4. Run `naner.exe` — bare, however you like:

   - **Double-click in Explorer**: it opens its own console window.
   - **From a shell** (PowerShell or `cmd.exe`): just type `.\naner.exe`.
     For interactive commands started from a shared console, it re-launches
     itself into its own console window, so its prompts never compete with
     the shell for your keystrokes.
   - Either way, it pauses with "press any key to exit" at the end so the
     window doesn't disappear before you can read it.

   (Why the extra window: neither PowerShell nor `cmd.exe` waits for a
   GUI-subsystem process, so in a shared console the shell's next prompt
   races naner's `(Y/n)` prompt for input and initialization could
   silently fail —
   [#81](https://github.com/baileyrd/rusty_naner/issues/81). Versions
   before 0.8.1 need a waiting wrapper as a workaround —
   `Start-Process -Wait .\naner.exe` in PowerShell, `start /wait naner.exe`
   in `cmd.exe`. The wrappers still work on current versions; they're just
   no longer necessary.)

   On first run (an uninitialized folder) it:
   - downloads `naner-bundle.zip` matching its own version and verifies it
     against the release's published `SHA256SUMS` manifest before touching
     disk — it refuses to install on a mismatch or a missing manifest;
   - extracts the bundle into `NANER_ROOT` in place;
   - prompts to bootstrap the four required tools, installed in a fixed
     order: 7-Zip, PowerShell, Windows Terminal, then Git for Windows;
   - offers to put naner on your user PATH so `naner` works from any new
     shell (same as running `naner add-to-path` later; `--remove` undoes it);
   - offers to launch naner immediately once that's done.

No admin rights are required, and nothing is written outside `NANER_ROOT` —
except the user-PATH entry above, and only if you say yes to it.

## Usage

Once initialized, `naner` launches your default terminal profile:

```sh
naner                          # launch the default profile
naner --profile PowerShell     # launch a specific profile (Unified, PowerShell, Bash, CMD)
naner -p Bash -d C:\projects   # launch Bash starting in a given directory
naner --diagnose               # check installation health
naner --export-env             # print env vars for sourcing into an existing shell
naner add-to-path              # make `naner` callable from any shell (undo: --remove)
```

Optional developer tools — Node.js, Go, Rust, Ruby, Bun, Anaconda, .NET SDK,
Podman, and more (see
[`dist-assets/config/vendors/`](dist-assets/config/vendors) for the
full, current list) — install on demand:

```sh
naner install --list           # see what's available and what's already enabled
naner install nodejs ruby      # install specific tools
naner install --all            # install everything
naner update-vendors           # update installed tools to their latest versions
```

Keep naner itself current with `naner update` (`naner self-update` is the
same command under its pre-0.8.0 name). It fetches the latest release,
verifies it against the release's `SHA256SUMS`, and replaces every copy of
the binary in the tree — including itself, by renaming the running exe
aside.

`naner --help` lists every subcommand — `doctor`, `schema`, `completions`,
`setup-shell`, `suggest`, `outdated`, `refresh-pins`, `repair`, `profile`,
`diff`, `bench`, `migrate`, `pack`, `lock`, and more — each with its own `--help` text; the full reference is
also in [Core CLI Subcommands](#core-cli-subcommands) below.

## Status

Migration complete: `v0.9.0` is the Latest release on this repo
([baileyrd/rusty_naner](https://github.com/baileyrd/rusty_naner)), published
through the Phase 5 release workflow. Releases through `v0.6.4` were
published to [baileyrd/naner](https://github.com/baileyrd/naner) instead;
that cross-publish has been removed. Phases 0–5 done:

- **Phase 0**: Cargo workspace (`naner-core` / `naner` / `naner-init`), the
  Windows console attach/alloc/pipe-detection spike, CI (fmt + clippy + test on
  Linux and Windows), a draft release workflow with the tag == version guard,
  and the golden-parity harness (`scripts/parity.ps1`). Outstanding exit
  criterion: manual validation of the four console launch modes on a real
  Windows box.
- **Phase 1**: `constants`, `version` (exact `VersionComparer` semantics),
  `paths` (root discovery, .NET-faithful `%VAR%`/`$env:VAR` expansion, unified
  PATH assembly), `config` (serde models with C# defaults and inert fields,
  comment/trailing-comma-tolerant JSON, env-var overrides,
  validator, search-order loader), `logger` (exact prefix/stream contract),
  and `env_export` (powershell/bash/cmd formats) — 50 unit tests, all
  pure-logic and green on Linux and the Windows cross-check.

- **Phase 2**: the launcher MVP — two-layer CLI dispatch (router verbs, then
  clap launch options), `--version`/`--help`/`--diagnose` with verbatim C#
  output, first-run detection and message, `--export-env`/`--setup-only`
  end-to-end, the `TerminalLauncher` port (wt.exe discovery chain,
  string-identical argument building, fire-and-forget spawn), and the additive
  `naner root` composable primitive. 60 tests; the full flow smoke-tested
  against a fixture tree on Linux.

- **Phase 3**: the vendor pipeline — vendors.json loader (lenient JSON,
  default-essential fallback), all six release-source resolvers with the
  two-level fallback cascade (bugs B1/B2 preserved bug-for-bug), blocking
  HTTP with the exact download-progress format, checksum verifier, archive
  extractors (native zip + native tar.xz with 7z.exe fallback; 7z/msi/exe
  shell-outs; rename-based flattening), Windows Terminal portable-mode
  configurator with settings-preserving updates, and the real
  `install`/`update-vendors` commands with dependency-first installs.
  Additive Unix-philosophy surface: `install --list --porcelain` and
  `--quiet`. 85 tests; the happy path verified live (Node.js resolved,
  downloaded, extracted, flattened, version-stamped; re-run skips).

- **Phase 4**: `naner-init` — the GitHub releases client (release-by-tag with
  the `v`-prefix rule, latest-with-prerelease fallback, octet-stream asset
  downloads, `GITHUB_TOKEN` bearer support), the `NanerUpdater` port with the
  exact sync-to-embedded-version semantics (normalized string inequality —
  happily "downgrades"; version file written in tag form), bundle
  initialization (no-flatten extraction, marker + version files), essential
  vendor bootstrap in the fixed 7-Zip-first order, Y/n prompts
  (empty/y/yes = yes), the allocated-console-only exit pause, and argument
  pass-through to naner.exe. 90 tests.

- **Phase 5 (cutover, done)**: the release workflow publishes Rust-built
  `naner.exe` / `naner-init.exe` / `naner-bundle.zip` to `baileyrd/naner`
  with identical tags and asset names (§4.2). The static bundle content
  (`bin/`, `config/`, `home/`, `icons/`) is vendored from the
  C# repo in `dist-assets/`. All validation gates
  ([docs/VALIDATION.md](docs/VALIDATION.md)) passed and `v0.5.0` shipped as
  a full (non-prerelease) release, including the post-parity bug-fix wave
  (B1–B6, [docs/post-parity-fix-wave.md](docs/post-parity-fix-wave.md)) and
  tier-3 output changes.

## Features & Capabilities

`rusty_naner` includes comprehensive terminal environment launcher features, developer inspection tools, self-healing, profile export/import, atomic extraction, and ecosystem integrations:

### Core CLI Subcommands
- **`naner doctor [--porcelain] [--conflicts]`**: Health checks `%NANER_ROOT%`, vendor directories, config health, and reports `PATH` binary collisions.
- **`naner schema [config|vendors]`**: Generates official JSON Schema definitions for `naner.json` and for a single vendor definition file for instant IDE autocompletion.
- **`naner completions <shell>`**: Generates tab-completion scripts for PowerShell, Bash, Zsh, and Fish.
- **`naner shell-integration <shell>`**: Emits OSC 133 prompt-marking and command lifecycle hooks for **rusty_term** / `l13` / MCP protocols.
- **`naner setup-shell [pwsh|bash|cmd] [--dry-run]`**: Adds the naner environment export to the shell's startup file, idempotently and with a backup — plus a command-not-found hook (`CommandNotFoundAction` for PowerShell, `command_not_found_handle` for Bash) that calls `naner suggest` so a missing command prints how to get it. `cmd` has no startup file to edit, so it prints the line and says why.
- **`naner suggest <name> [--porcelain]`**: Maps an executable name the shell failed to find to the vendor that provides it — from each vendor's optional `provides` list, falling back to names derived from `naner.json`'s `VendorPaths` — and prints the right next step: `naner install <vendor>`, the `"enabled": true` edit for a disabled vendor, or a reminder that the tool is only on PATH inside naner-launched shells. Offline, fast, and silent (exit 1) when nothing matches, so the shell hooks can call it on every miss.
- **`naner add-to-path [--remove] [--dry-run]`**: Puts `%NANER_ROOT%\vendor\bin` on the *user* PATH (`HKCU\Environment`, no admin), so `naner` itself resolves from any shell without importing the whole environment the way `setup-shell` does. Edits the registry value directly — `setx` truncates at 1024 characters — preserving its type and the rest of its contents, then broadcasts the change so new shells see it. `--remove` undoes it.
- **`naner outdated [--porcelain]`**: Compares each *installed* vendor's recorded version (`.vendor-version`) against what its release source currently calls latest, flagging major-version jumps distinctly (`outdated (major)`). Exits non-zero when updates exist, so scripts can gate on it; `naner update-vendors` remains the tool that actually updates. Static-URL vendors are reported `unchecked` — their pinned version *is* the install.
- **`naner refresh-pins [dir] [--dry-run] [--porcelain]`**: Re-resolves upstream latest for every vendor with a dynamic release source and rewrites the hardcoded `fallback` pin (`version`/`url`/`fileName`) in `config/vendors/<Key>.json` — the maintenance pass that stops fallback pins rotting. `[dir]` points at a vendor-definitions directory explicitly (e.g. this repo's `dist-assets/config/vendors` from a checkout). Static-URL vendors are reported manual-only, never rewritten. `naner doctor` complements both offline: an installed vendor older than its (now-refreshable) fallback pin gets an "updates available" nudge with no network involved.
- **`naner repair`**: Cleans broken staging directories and re-bootstraps missing essential vendor tools.
- **`naner profile [list|export|import]`**: Lists profiles, exports one to JSON, and imports one back. `import` writes into `CustomProfiles` (so a built-in of the same name is never overwritten in place), keeps a timestamped backup, and supports `--as <name>` and `--dry-run`.
- **`naner diff [profile]`**: Compares host environment variables against target profile environment definitions.
- **`naner bench [profile]`**: Startup latency profiler measuring execution timings for root discovery, config loading, profile resolution, and PATH assembly in milliseconds.
- **`naner migrate [--dry-run]`**: Rewrites the configuration file in canonical JSON form. Keeps a timestamped backup, preserves top-level keys the model does not own (`$schema` among them), and writes via a temp file so an interrupted run cannot truncate the config. Comments cannot survive the round-trip and it says so before proceeding.
- **`naner pack [dir] --out bundle.zip`**: Bundles a naner installation (`bin/`, `config/`, `home/`, `icons/`) into a portable zip, skipping transient files. Defaults to the discovered root; `[dir]` overrides it.
- **`naner update` / `naner self-update`**: Updates naner itself to the latest release in place. `naner.exe` cannot be *overwritten* while running, but Windows will rename a running exe — so the update renames the live binary aside, installs the new one under its name, and sweeps the `.old` leftover on the next launch.
- **`naner lock [--refresh [vendor...]] [--porcelain]`**: Inspects `naner.lock`, the pin of exactly which vendor artifacts this environment installs, and drops pins so the next install re-resolves.

### Infrastructure & Subsystem Enhancements
- **Download Integrity Verification**: every vendor download is checked against a digest published by the distributor itself where one exists — Go and Node.js (SHA-256), the .NET SDK (SHA-512, via the channel manifest that also supplies the authoritative URL), `rustup-init.exe` (`.sha256` sidecar) and Anaconda (repository listing). A vendor may also pin a digest via `checksum` in its own definition file, which takes precedence. A mismatch against an upstream digest blocks installation. Sources that publish no digest (MSYS2, GitHub release assets) install unverified unless pinned.
- **Reproducible Environments (`naner.lock`)**: a successful install pins the vendor's exact version, URL and SHA-256. Later installs reproduce that artifact instead of re-resolving to upstream latest, and verify it — which is the only verification MSYS2 and the GitHub-sourced vendors get, since their distributors publish no digest. `update-vendors` deliberately ignores the pin and rewrites it. The first install of an unpinned vendor is still trust-on-first-use.
- **Verified Self-Update**: every binary and bundle download is verified against the `SHA256SUMS` manifest published with each release before anything on disk is replaced; a missing or mismatched manifest refuses the install.
- **Corporate Proxy & CA Support**: Auto-detects and respects `HTTP_PROXY` / `HTTPS_PROXY` / `http_proxy` / `https_proxy`, with `NO_PROXY=*` as a blanket opt-out. Applied to every outbound request — vendor downloads, bootstrap and self-update alike.
- **Privacy Telemetry Opt-Out Enforcer**: Injects default telemetry opt-out variables (`DOTNET_CLI_TELEMETRY_OPTOUT=1`, `POWERSHELL_TELEMETRY_OPTOUT=1`, `AZURE_CORE_COLLECT_TELEMETRY=0`).
- **Dynamic Architecture Resolution (`%{ARCH}`)**: Dynamically expands `%{ARCH}` into `arm64` or `x64` based on host target compilation.
- **Atomic Staged Extraction**: Extracts archives to `vendor/.staging/<name>`, then swaps the tree into place with a single rename. The previous install is moved aside rather than deleted, so a failed placement restores it instead of leaving a half-populated directory. Windows Terminal is the exception — it merges over its existing install so `settings/` survives an update, which cannot be atomic.
- **Download Asset Caching**: Reuses an already-downloaded vendor asset instead of re-fetching it, for offline/air-gapped environments. Transfers stage to `<name>.part` and are published with a rename, so an interrupted run leaves no file under the name the cache trusts. When a digest is known the cached file must match it — a stale entry is discarded and re-fetched rather than failing the install.
- **Job Object Breakaway Isolation**: Spawns terminal targets with `CREATE_BREAKAWAY_FROM_JOB` (0x01000000) on Windows.
- **Event Hooks**: Supports `PreLaunch` / `PostLaunch` script hooks. A failing `PreLaunch` hook aborts the launch — it is a gate, not a notification; `PostLaunch` only warns, since the terminal is already up. Both run under `-ExecutionPolicy Bypass`, deliberately, because the default policy would refuse a script the config owner supplied on purpose.
- **`npm`/`pip`-Published Vendors**: alongside archive/installer vendors, a vendor's `releaseSource.type` can be `npm` (resolved against the npm registry, installed with the vendored NodeJS's `npm install -g`) or `pip` (resolved against PyPI, installed with the vendored Anaconda's `python -m pip install --user`) — both into `home\` so the result stays portable, and both leave only a `.vendor-version` marker under `vendor\<name>\` since the real install lives in the package manager's own tree. `installType: "binary"` covers the third shape: a single verified executable placed as-is, no archive to extract. New vendors of this kind: `GitHubCli` (`gh`), `ClaudeCode`, `OhMyPi`, `Codex` (npm), `NotebookLmCli` (pip), `OhMyPosh` (binary, checksum-verified via a shared `checksums.txt`), and `Antigravity` (static `.exe`, installs unverified — Google publishes no digest for it, same posture as OneCommander).

---

- [MIGRATION_ANALYSIS.md](MIGRATION_ANALYSIS.md) — detailed analysis of the existing C# codebase and the phased migration plan.
- [ECOSYSTEM.md](ECOSYSTEM.md) — assessment of the sibling Rust projects ([rusty_term](https://github.com/baileyrd/rusty_term), [rusty_lsp](https://github.com/baileyrd/rusty_lsp), [rush](https://github.com/baileyrd/rush)) and the roadmap for integrating them into a full-Rust terminal environment.
