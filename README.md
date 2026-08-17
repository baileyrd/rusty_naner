# rusty_naner

Rust migration of [naner](https://github.com/baileyrd/naner), the portable terminal
environment launcher for Windows.

## Installation

`rusty_naner` ships as a single self-bootstrapping executable — there is no
separate installer.

1. Download `naner-init.exe` from the
   [latest release](https://github.com/baileyrd/rusty_naner/releases/latest).
2. Put it in an empty folder. That folder becomes `NANER_ROOT` — everything
   naner owns (its own config, vendor tools, and binary) lives under it, so
   the whole install is self-contained and can be removed by deleting the
   folder.
3. Run `naner-init.exe` — double-clicking it in Explorer or running it from
   an existing PowerShell or Windows Terminal window both work. It's a
   console-less GUI-subsystem binary, so on first run it opens its own
   console window if you double-clicked it, or attaches to the one you're
   already in if you ran it from a shell; either way you'll see the same
   prompts. (Double-clicked, it also pauses with "press any key to exit"
   at the end so the window doesn't disappear before you can read it.)

   **From `cmd.exe` specifically**, run it as `start /wait naner-init.exe`
   rather than typing its name directly — `cmd.exe` does not wait for a
   GUI-subsystem process the way PowerShell does, so its own next-command
   prompt races `naner-init.exe`'s `(Y/n)` prompt for your keystrokes, and
   the plain-`naner-init.exe` form can silently fail to initialize
   ([#81](https://github.com/baileyrd/rusty_naner/issues/81)). `start /wait`
   makes `cmd.exe` actually wait, which avoids the race.

   On first run it:
   - downloads `naner-bundle.zip` matching its own version and verifies it
     against the release's published `SHA256SUMS` manifest before touching
     disk — it refuses to install on a mismatch or a missing manifest;
   - extracts the bundle into `NANER_ROOT` in place;
   - prompts to bootstrap the four required tools, installed in a fixed
     order: 7-Zip, PowerShell, Windows Terminal, then Git for Windows;
   - offers to launch naner immediately once that's done.

No admin rights are required, and nothing is written outside `NANER_ROOT`.

## Usage

Once initialized, `naner` (or `naner-init`, which passes unrecognized
arguments straight through to it) launches your default terminal profile:

```sh
naner                          # launch the default profile
naner --profile PowerShell     # launch a specific profile (Unified, PowerShell, Bash, CMD)
naner -p Bash -d C:\projects   # launch Bash starting in a given directory
naner --diagnose               # check installation health
naner --export-env             # print env vars for sourcing into an existing shell
```

Optional developer tools — Node.js, Go, Rust, Ruby, Bun, Anaconda, .NET SDK,
Podman, and more (see
[`dist-assets/config/vendors.json`](dist-assets/config/vendors.json) for the
full, current list) — install on demand:

```sh
naner install --list           # see what's available and what's already enabled
naner install nodejs ruby      # install specific tools
naner install --all            # install everything
naner update-vendors           # update installed tools to their latest versions
```

Keep naner itself current with `naner self-update` (equivalent to running
`naner-init update` directly).

`naner --help` lists every subcommand — `doctor`, `schema`, `completions`,
`setup-shell`, `repair`, `profile`, `diff`, `bench`, `migrate`, `pack`,
`lock`, and more — each with its own `--help` text; the full reference is
also in [Core CLI Subcommands](#core-cli-subcommands) below.

## Status

Migration complete: `v0.6.5` is the Latest release on this repo
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
  comment/trailing-comma-tolerant JSON, YAML fallback, env-var overrides,
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
  (`bin/`, `config/`, `home/`, `icons/`, `naner.bat`) is vendored from the
  C# repo in `dist-assets/`. All validation gates
  ([docs/VALIDATION.md](docs/VALIDATION.md)) passed and `v0.5.0` shipped as
  a full (non-prerelease) release, including the post-parity bug-fix wave
  (B1–B6, [docs/post-parity-fix-wave.md](docs/post-parity-fix-wave.md)) and
  tier-3 output changes.

## Features & Capabilities

`rusty_naner` includes comprehensive terminal environment launcher features, developer inspection tools, self-healing, profile export/import, atomic extraction, and ecosystem integrations:

### Core CLI Subcommands
- **`naner doctor [--porcelain] [--conflicts]`**: Health checks `%NANER_ROOT%`, vendor directories, config health, and reports `PATH` binary collisions.
- **`naner schema [config|vendors]`**: Generates official JSON Schema definitions for `naner.json` and `vendors.json` for instant IDE autocompletion.
- **`naner completions <shell>`**: Generates tab-completion scripts for PowerShell, Bash, Zsh, and Fish.
- **`naner shell-integration <shell>`**: Emits OSC 133 prompt-marking and command lifecycle hooks for **rusty_term** / `l13` / MCP protocols.
- **`naner setup-shell [pwsh|bash|cmd] [--dry-run]`**: Adds the naner environment export to the shell's startup file, idempotently and with a backup. `cmd` has no startup file to edit, so it prints the line and says why.
- **`naner repair`**: Cleans broken staging directories and re-bootstraps missing essential vendor tools.
- **`naner profile [list|export|import]`**: Lists profiles, exports one to JSON, and imports one back. `import` writes into `CustomProfiles` (so a built-in of the same name is never overwritten in place), keeps a timestamped backup, and supports `--as <name>` and `--dry-run`.
- **`naner diff [profile]`**: Compares host environment variables against target profile environment definitions.
- **`naner bench [profile]`**: Startup latency profiler measuring execution timings for root discovery, config loading, profile resolution, and PATH assembly in milliseconds.
- **`naner migrate [--dry-run]`**: Rewrites the configuration file in canonical JSON form. Keeps a timestamped backup, preserves top-level keys the model does not own (`$schema` among them), and writes via a temp file so an interrupted run cannot truncate the config. Comments cannot survive the round-trip and it says so before proceeding.
- **`naner pack [dir] --out bundle.zip`**: Bundles a naner installation (`bin/`, `config/`, `home/`, `icons/`, `naner.bat`) into a portable zip, skipping transient files. Defaults to the discovered root; `[dir]` overrides it.
- **`naner self-update`**: Hands over to `naner-init`, which performs the update. It is a separate executable because `naner.exe` cannot replace itself while running.
- **`naner lock [--refresh [vendor...]] [--porcelain]`**: Inspects `naner.lock`, the pin of exactly which vendor artifacts this environment installs, and drops pins so the next install re-resolves.

### Infrastructure & Subsystem Enhancements
- **Download Integrity Verification**: every vendor download is checked against a digest published by the distributor itself where one exists — Go and Node.js (SHA-256), the .NET SDK (SHA-512, via the channel manifest that also supplies the authoritative URL), `rustup-init.exe` (`.sha256` sidecar) and Anaconda (repository listing). A vendor may also pin a digest via `checksum` in `vendors.json`, which takes precedence. A mismatch against an upstream digest blocks installation. Sources that publish no digest (MSYS2, GitHub release assets) install unverified unless pinned.
- **Reproducible Environments (`naner.lock`)**: a successful install pins the vendor's exact version, URL and SHA-256. Later installs reproduce that artifact instead of re-resolving to upstream latest, and verify it — which is the only verification MSYS2 and the GitHub-sourced vendors get, since their distributors publish no digest. `update-vendors` deliberately ignores the pin and rewrites it. The first install of an unpinned vendor is still trust-on-first-use.
- **Verified Self-Update**: `naner-init` verifies `naner.exe` and `naner-bundle.zip` against the `SHA256SUMS` manifest published with each release before replacing anything on disk, and refuses to install if the manifest is missing or does not match.
- **Corporate Proxy & CA Support**: Auto-detects and respects `HTTP_PROXY` / `HTTPS_PROXY` / `http_proxy` / `https_proxy`, with `NO_PROXY=*` as a blanket opt-out. Applied to every outbound request — vendor downloads, `naner-init` bootstrap and update alike.
- **Privacy Telemetry Opt-Out Enforcer**: Injects default telemetry opt-out variables (`DOTNET_CLI_TELEMETRY_OPTOUT=1`, `POWERSHELL_TELEMETRY_OPTOUT=1`, `AZURE_CORE_COLLECT_TELEMETRY=0`).
- **Dynamic Architecture Resolution (`%{ARCH}`)**: Dynamically expands `%{ARCH}` into `arm64` or `x64` based on host target compilation.
- **Atomic Staged Extraction**: Extracts archives to `vendor/.staging/<name>`, then swaps the tree into place with a single rename. The previous install is moved aside rather than deleted, so a failed placement restores it instead of leaving a half-populated directory. Windows Terminal is the exception — it merges over its existing install so `settings/` survives an update, which cannot be atomic.
- **Download Asset Caching**: Reuses an already-downloaded vendor asset instead of re-fetching it, for offline/air-gapped environments. Transfers stage to `<name>.part` and are published with a rename, so an interrupted run leaves no file under the name the cache trusts. When a digest is known the cached file must match it — a stale entry is discarded and re-fetched rather than failing the install.
- **Job Object Breakaway Isolation**: Spawns terminal targets with `CREATE_BREAKAWAY_FROM_JOB` (0x01000000) on Windows.
- **Event Hooks**: Supports `PreLaunch` / `PostLaunch` script hooks. A failing `PreLaunch` hook aborts the launch — it is a gate, not a notification; `PostLaunch` only warns, since the terminal is already up. Both run under `-ExecutionPolicy Bypass`, deliberately, because the default policy would refuse a script the config owner supplied on purpose.

---

- [MIGRATION_ANALYSIS.md](MIGRATION_ANALYSIS.md) — detailed analysis of the existing C# codebase and the phased migration plan.
- [ECOSYSTEM.md](ECOSYSTEM.md) — assessment of the sibling Rust projects ([rusty_term](https://github.com/baileyrd/rusty_term), [rusty_lsp](https://github.com/baileyrd/rusty_lsp), [rush](https://github.com/baileyrd/rush)) and the roadmap for integrating them into a full-Rust terminal environment.
