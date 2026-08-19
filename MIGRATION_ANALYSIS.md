# Naner → Rust Migration Analysis

**Source:** [`baileyrd/naner`](https://github.com/baileyrd/naner) v0.4.6 (C#/.NET 8, `win-x64`)
**Target:** this repository (`baileyrd/rusty_naner`)
**Date:** 2026-07-08

This document is a detailed analysis of the existing C# codebase and a concrete plan for
migrating it to Rust: what the system does, the exact behavioral contract a rewrite must
honor, the proposed Rust architecture and crate choices, known bugs/dead code and what to
do about them, and a phased migration plan with a parity-testing strategy.

---

## 1. What Naner is today

Naner is a **portable, self-contained terminal environment launcher for Windows**. It
lives in a directory tree (`bin/`, `vendor/`, `config/`, `home/`), downloads and manages
portable tools ("vendors": PowerShell 7, Windows Terminal, MSYS2, 7-Zip, optionally
Node/Go/Rust/Ruby/Miniconda/.NET SDK), constructs an isolated environment
(`PATH` + ~25 env vars, all rooted at `%NANER_ROOT%`), and launches Windows Terminal
with a configured profile.

### 1.1 Two-executable architecture

| Executable | Project | Role |
|---|---|---|
| `vendor/bin/naner-init.exe` | `Naner.Init` | Bootstrapper/updater. Downloads the `naner-bundle.zip` release asset from GitHub on first run, keeps `naner.exe` **in sync with its own embedded version**, downloads essential vendors, then execs `naner.exe`. |
| `vendor/bin/naner.exe` | `Naner.Launcher` | The launcher. Routes sub-commands (`--version`, `--help`, `--diagnose`, `install`, `update-vendors`), loads config, sets up environment/PATH, launches Windows Terminal (`wt.exe`) with a profile. |

Both are published as **single-file, self-contained, trimmed, `WinExe`** (GUI subsystem —
no console by default; they attach/allocate one on demand via kernel32 P/Invoke).
`naner.bat` at the repo root is a thin wrapper that calls `vendor\bin\naner.exe`.

### 1.2 C# project inventory (11,589 LOC total)

| Project | LOC | Purpose | Rust disposition |
|---|---|---|---|
| `Naner.Core` | 1,309 | Root discovery, path expansion, PATH builder, env exporter, logger, constants, event aggregator | Port → `core` modules (event aggregator: **drop**, unused) |
| `Naner.Vendors` | 1,876 | Vendor definitions, vendors.json loader, unified installer (6 release-source types), checksum verify, WT configurator | Port → `vendors` module |
| `Naner.Commands` | 1,543 | Command router, Version/Help/Diagnostics/Install/UpdateVendors commands, diagnostics services, plugin loader | Port (plugin loader: **drop/defer**, never engaged) |
| `Naner.Init` | 1,231 | GitHub releases client, version comparer, updater, essential vendor downloader | Port → `naner-init` binary |
| `Naner.Configuration` + `.Abstractions` | 1,183 | Config models, JSON/YAML/env providers, validator, source-gen JSON context | Port → `config` module (serde) |
| `Naner.Launcher` | 662 | Entry point, CLI parsing, terminal launcher, path resolver | Port → `naner` binary |
| `Naner.Archives` | 635 | Zip/7z/tar.xz/MSI/EXE-installer extractors + flattening | Port → `archives` module |
| `Naner.Setup` | 608 | First-run detector (live) + interactive setup wizard (**dead code**) | Port `FirstRunDetector` only |
| `Naner.Infrastructure` | 601 | HttpClient wrapper, download service (unused), console manager (P/Invoke) | Port console manager + one HTTP client |
| `Naner.DependencyInjection` | 297 | MS.DI service registration | **Drop** — the real entry point doesn't use it |
| `Naner.Tests` | 1,644 | xunit tests (Moq, FluentAssertions) | Re-express as Rust unit/integration tests |

NuGet dependencies are minimal: `CommandLineParser`, `YamlDotNet`,
`Microsoft.Extensions.DependencyInjection`, plus test packages. There is no registry
access, no COM, no WinForms/WPF — the Windows-specific surface is exactly:
kernel32 console APIs, `%VAR%` expansion, process creation, and known folders.

### 1.3 Key runtime facts

- **Root discovery** (`PathUtilities.FindNanerRoot`): `NANER_ROOT` env var wins if it
  points at a dir containing `bin/`+`vendor/`+`config/`; else walk up from the exe's
  directory up to 10 levels looking for those three markers; else fail with a verbose
  error (exit 1). *(The README's "6 fallback strategies" refers to
  `FirstRunDetector.EstablishNanerRoot`, which is dead code.)*
- **Config**: exactly one file loaded from `config/` in order `naner.json` →
  `naner.yaml` → `naner.yml` (no cross-file merging), PascalCase keys, case-insensitive,
  unknown fields ignored, JSON comments + trailing commas tolerated. Env-var overrides
  applied on top: `NANER_DEFAULT_PROFILE`, `NANER_INHERIT_SYSTEM_PATH`, `NANER_DEBUG`,
  `NANER_ENV_<NAME>` (adds env var), `NANER_PATH_<NAME>` (prepends PATH entry).
- **Placeholder expansion**, in order: `%NANER_ROOT%` (case-insensitive literal) →
  Windows `%VAR%` → PowerShell-style `$env:VAR` (regex `\$env:(\w+)`, unset left as-is).
- **PATH construction**: config `PathPrecedence` in order, **silently dropping entries
  whose directory doesn't exist**, joined with `;`, then the process PATH appended iff
  `Advanced.InheritSystemPath` (default true).
- **Terminal launch**: find `wt.exe` via `VendorPaths["WindowsTerminal"]` → `PATH` →
  `%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe`; build args
  (`--<launchMode>`, `--title "<name>"`, `--startingDirectory "<dir>"`,
  `-- "<shell>" <args>`); spawn **fire-and-forget** (exit 0 as soon as spawn succeeds).
- **Update model** (naner-init): *not* "install latest". naner-init fetches the GitHub
  release whose **tag matches its own embedded assembly version** and syncs
  `naner.exe`/`.naner-version` to it (string-inequality check — it will happily
  "downgrade"). `GetLatestReleaseAsync` exists but is unused by this flow. Releases live
  at `github.com/baileyrd/naner`; assets: `naner-bundle.zip` (init) and `naner.exe`
  (update). `GITHUB_TOKEN` is honored as a Bearer token if set.
- **Vendor pipeline**: resolve URL per source type (`github`, `web-scrape`, `static`,
  `nodejs-api`, `golang-api`, `dotnet-api`) with a two-level fallback (fallback URL on
  resolution failure *and* a second download attempt on download failure) → download to
  `vendor/.downloads/` (10-min timeout, 8 KB buffer, `\r  Progress: N%` every 10%, no
  retries) → optional checksum (never populated in practice; uppercase-hex,
  punctuation-stripped compare) → extract to `vendor/<extractDir>/` → flatten a single
  top-level subdirectory (rename-based) → post-install (Windows Terminal only: write
  `.portable` marker + `settings/settings.json`, its four Naner profiles generated
  fresh from `config/naner.json`'s own `Profiles` on every call — the single source of
  truth per #83, not a second hand-maintained WT-schema template) → write
  `vendor/<extractDir>/.vendor-version` → delete `vendor/.downloads`.
  "Installed" = extract dir exists and is non-empty. Updates delete-and-reinstall,
  **except Windows Terminal**, which is extracted over-top to preserve settings.
- **Archive handling**: `.zip` natively (System.IO.Compression); `.7z` and `.tar.xz` by
  shelling out to `vendor/7zip/7z.exe` (`x "<src>" -o"<dst>" -y`; tar.xz is two-stage
  via an intermediate `.tar`); `.msi` via `msiexec /a "<msi>" /qn TARGETDIR="<dst>"`
  (with a hardcoded `Files/7-Zip` flatten specific to the 7-Zip MSI); `.exe` installers
  run directly with per-vendor args (`%TARGETDIR%`/`$TARGETDIR` substitution; rustup
  gets `RUSTUP_HOME`/`CARGO_HOME` pointed into the vendor dir).
- **Console management**: as `WinExe`, on startup decide `NeedsConsole` from `args[0]`
  against a per-exe command list; if stdout is already redirected
  (`GetStdHandle`+`GetFileType`), **do not attach** (keeps `naner --export-env |
  Invoke-Expression` working); else `AttachConsole(-1)` (print one leading newline to
  clear the prompt) or `AllocConsole()`; reopen std streams with CRLF + autoflush.
  naner-init's "Press any key to exit" pause fires **only** when a console was
  allocated (double-click case).
- **Logging contract**: `[*]` cyan status, `[OK]` green, `[✗]` red, gray 4-space-indent
  info, `[DEBUG]` yellow (only when debug mode), header + `=` underline — all stdout;
  `[!]` yellow **warnings are the only thing on stderr**. `--export-env` writes the
  trimmed script to stdout with no log prefixes.
- **Exit codes**: `0` success everywhere, `1` for every failure (parse errors, missing
  root, launch failure, any vendor failure); internal `-1` is only a router sentinel
  ("no sub-command → run launcher") and never escapes to the OS.

### 1.4 CLI surface (must be preserved verbatim)

`naner.exe` — router commands (case-insensitive on `args[0]`):
`--version|-v`, `--help|-h|/?`, `--diagnose`, `update-vendors`,
`install [--list | --all | <vendor>...]` (vendors: `nodejs`, `miniconda`, `go`, `rust`,
`ruby`, `dotnetsdk`). Everything else parses as launch options:
`-p|--profile`, `-e|--environment` (default `default`), `-d|--directory`, `-c|--config`,
`--debug`, `--setup-only`, `--export-env`, `-f|--format` (`powershell|ps|ps1`,
`bash|sh|zsh`, `cmd|bat|batch`), `--no-comments`.

`naner-init.exe` — `init`, `update`, `check-update`, `--version|-v`, `--help|-h`;
any other args pass through verbatim to `naner.exe`. Interactive prompts accept
empty/`y`/`yes` (trimmed, case-insensitive) as yes.

### 1.5 On-disk contract

| Path (relative to root) | Meaning |
|---|---|
| `.naner-initialized` | Init marker (3-line comment header incl. version) |
| `vendor/bin/naner.exe`, `naner-init.exe` | The executables |
| `vendor/bin/.naner-version` | Installed version; written both as `0.4.6` (build) and `v0.4.6` (GitHub tag) — comparisons must normalize the `v` |
| `vendor/<extractDir>/` | One dir per vendor (`7zip`, `powershell`, `terminal`, `msys64`, …) |
| `vendor/<extractDir>/.vendor-version` | Per-vendor installed version (plain text) |
| `vendor/.downloads/` | Transient download staging (deleted after batch) |
| `config/naner.json` (`.yaml`, `.yml`) | Main config |
| `config/vendors.json` | Vendor definitions |
| `home/` | Portable `$HOME` (`HOME`/`NANER_HOME` set iff it exists) |
| `bin/`, `opt/`, `plugins/`, `logs/` | User binaries / user tools / (dead) plugins / logs |

Root markers are `bin`+`vendor`+`config` (note: *without* `home`); first-run's
"essential" check additionally requires `home`. This asymmetry is intentional behavior.

---

## 2. Proposed Rust architecture

### 2.1 Workspace layout

```
rusty_naner/
├── Cargo.toml                 # [workspace] members = ["crates/*"]
├── crates/
│   ├── naner-core/            # library: everything shared
│   │   └── src/
│   │       ├── constants.rs   # NanerConstants port (names, dirs, URLs, timeouts)
│   │       ├── paths.rs       # find_naner_root, expand_naner_path, path builder
│   │       ├── config/        # models (serde), json/yaml load, env overrides, validator
│   │       ├── env_export.rs  # EnvironmentExporter (powershell/bash/cmd)
│   │       ├── logger.rs      # [*]/[OK]/[✗]/[!] console logger, stdout/stderr split
│   │       ├── console.rs     # Win32 attach/alloc console (windows-sys), cfg(windows)
│   │       ├── http.rs        # one blocking HTTP client (UA, timeout, GITHUB_TOKEN)
│   │       ├── github.rs      # releases-by-tag/latest client + asset download
│   │       ├── version.rs     # VersionComparer port (exact semantics)
│   │       ├── archives/      # zip / tar.xz / 7z / msi / exe-installer + flatten
│   │       └── vendors/       # definitions, vendors.json loader, installer, WT config
│   ├── naner/                 # binary: the launcher (Naner.Launcher + Commands + Setup)
│   │   └── src/
│   │       ├── main.rs        # console decision → route → launch
│   │       ├── cli.rs         # clap definitions (router verbs + launch options)
│   │       ├── commands/      # version, help, diagnose, install, update_vendors
│   │       ├── launcher.rs    # TerminalLauncher port
│   │       └── first_run.rs   # FirstRunDetector port
│   └── naner-init/            # binary: bootstrapper (Naner.Init)
│       └── src/
│           ├── main.rs
│           └── updater.rs     # NanerUpdater port (init/update/check-update/launch)
└── .github/workflows/         # ci.yml (test+clippy+fmt), release.yml (tag → assets)
```

Two binaries, one shared library crate. This mirrors the C# split while collapsing the
9 library projects — the layering they encoded (interfaces + DI) exists to enable
mocking and doesn't pay its way in Rust; module boundaries + a few traits where tests
need seams (HTTP, filesystem-ish operations) are enough. The actual C# entry point
already bypasses the DI container and `new`s everything directly, so nothing of
architectural value is lost.

### 2.2 Concept mapping

| C# concept | Rust replacement |
|---|---|
| DI container (`Microsoft.Extensions.DependencyInjection`) | Plain construction; a small `Context { root: PathBuf, logger, http }` threaded by `&ref` |
| Interfaces (`ILogger`, `IHttpClientWrapper`, `ICommand`, …) | Concrete types; a trait only where tests must substitute (e.g. `trait Http`) |
| `async/await` everywhere | **Synchronous/blocking.** The C# async is incidental — downloads are sequential, every caller blocks (`GetAwaiter().GetResult()` in commands). Skipping tokio cuts compile time and binary size; parallel downloads, if ever wanted, are a `std::thread` fan-out |
| Source-generated JSON (`NanerJsonContext`) | `serde` derive (the AOT/trimming problem serde was solving doesn't exist in Rust) |
| `CommandLineParser` 2.9.1 | `clap` v4 (derive), with router verbs checked first to preserve two-layer dispatch |
| Exceptions → catch → exit 1 | `anyhow::Result` in `main`, mapped to exit codes `0`/`1` |
| `EventAggregator` (pub/sub) | **Drop.** Registered in DI but no live publish/subscribe wiring exists |
| Static `Logger` facade | Small `logger` module (free functions + `AtomicBool` debug flag), same prefixes/colors/stream split |
| Plugin loader (`AssemblyLoadContext` over `plugins/*.dll`) | **Drop/defer.** The shipping `naner.exe` constructs `CommandRouter` without plugins, so this is inert today. If revived later: subprocess protocol or WASM, not dylibs |

### 2.3 Crate selection

| Concern | C# today | Recommended crate | Notes |
|---|---|---|---|
| CLI | CommandLineParser | `clap` (derive) | Keep exact flag names/defaults; `-e` default `"default"`, `-f` default `"powershell"` |
| JSON config | System.Text.Json (comments+trailing commas on, case-insensitive, unknown-skip) | `serde` + `serde_json`; parse `naner.json`/`vendors.json` through a comment/trailing-comma-tolerant front-end (`json5`, or strip with `json-comments`-style reader) | `#[serde(rename_all = "PascalCase")]`, aliases for case-insensitivity where needed, **no** `deny_unknown_fields`; must tolerate root `$schema`/`title`/`description` keys |
| YAML config | YamlDotNet (PascalCase, ignore-unmatched) | `serde_yaml_ng` (or `serde-saphyr`) | `serde_yaml` is archived; usage is a fallback path (JSON wins the search order) so this is low-risk. Alternative: drop YAML and document it — needs a product decision |
| HTTP | HttpClient | `reqwest` (blocking feature, `native-tls`) | schannel = Windows cert store + system proxy behavior closest to .NET. `ureq` is a leaner option if binary size matters more than proxy parity |
| Zip | System.IO.Compression | `zip` | Covers `naner-bundle.zip`, PowerShell, WT, Node, Go, .NET SDK |
| tar.xz | shell-out to `7z.exe` (two-stage) | `tar` + `xz2` (static liblzma) | Native extraction removes the 7-Zip bootstrap dependency for MSYS2 (~400 MB archive). Keep the 7z.exe shell-out as fallback; see risks §4.3 |
| 7z | shell-out to `7z.exe` | keep shell-out (`std::process::Command`) | Only Ruby (optional vendor) ships `.7z`. `sevenz-rust2` exists if we ever want it native |
| MSI | `msiexec /a` | keep shell-out | Administrative-install extraction of the 7-Zip MSI; no sane native option. Preserve the `Files/7-Zip` flatten |
| Checksums | System.Security.Cryptography | `sha2`, `sha1`, `md5`, `hex` | Preserve normalize-then-compare (strip ` `, `-`, `:`, uppercase) |
| Version compare | hand-rolled `VersionComparer` | **port the ~60 lines as-is** | Its quirks are the update protocol: `v`-strip, prerelease-drop, major/minor/patch only, string-equality sync check. Don't substitute the `semver` crate semantics |
| Win32 console | kernel32 P/Invoke | `windows-sys` (`Win32_System_Console`) | `AttachConsole`/`AllocConsole`/`GetStdHandle`/`GetFileType`; binaries built with `#![windows_subsystem = "windows"]` |
| Colors | `Console.ForegroundColor` | ANSI + enable `ENABLE_VIRTUAL_TERMINAL_PROCESSING` (or `crossterm`) | Windows 10+ consoles all support VT once enabled |
| Env expansion | `Environment.ExpandEnvironmentVariables` + regex | hand-rolled (~40 lines) | Must implement all three passes: `%NANER_ROOT%` → `%VAR%` → `$env:VAR` |
| Glob (asset patterns) | `string.Contains` (buggy) | `globset` | Only if fixing bug B1 (§3) |
| Process spawn | `ProcessStartInfo` | `std::process::Command` (+ `CommandExt::creation_flags(CREATE_NO_WINDOW)` for the naner-init → naner spawn) | Preserve fire-and-forget vs `wait()` per call site; preserve env inheritance |
| Errors | exceptions | `anyhow` (+ `thiserror` in `naner-core` where callers branch) | |
| Tests | xunit + Moq + FluentAssertions | `#[test]`, `assert_cmd`, `tempfile`, `httpmock` | See §5 |

Expected binary footprint with `opt-level = "s"`, `lto = true`, `strip = true`,
`panic = "abort"`: roughly **1.5–4 MB per exe** vs today's ~11 MB trimmed .NET
launcher — plus materially faster cold start (no runtime/single-file extraction).

### 2.4 Design principles: applying the Unix philosophy

Assessed against the classic tenets (McIlroy's "do one thing well," Raymond's rules):
the philosophy is largely applicable, partly already present in the C# app, and the
migration is the cheapest possible moment to adopt the rest — with one tenet
(one-program-per-tool) deliberately compromised for compatibility.

**Already aligned (preserve through the port):**

- *Text streams as the interface* — `--export-env` emits an eval-able script on pure
  stdout (`naner --export-env | Invoke-Expression`), warnings alone on stderr,
  trailing newline trimmed for pipeline safety.
- *Pipeline awareness* — the `IsStdoutCaptured` check already detects redirection and
  refuses to attach a console over a pipe.
- *Representation / mechanism-not-policy* — `naner.json` and `vendors.json` hold the
  policy (profiles, PATH order, vendor sources); the launcher and installer are dumb
  engines executing data.
- *Separation of interfaces from engines* — the two-executable split
  (bootstrapper vs launcher).

**Current violations the rewrite should address:**

- *Do one thing* — `naner.exe` is four tools in one: launcher, env exporter, vendor
  package manager, diagnostics.
- *Silence is golden* — `[*]`/`[OK]` status lines narrate every operation
  unconditionally.
- *Repair (fail loudly, fail fast)* — the vendor fallback cascade degrades **silently**;
  bug B1 went unnoticed precisely because nothing reports when a fallback URL is used.
- *Composability gaps* — `install --list` output is human-decorated only; there is no
  scriptable primitive for "where is NANER_ROOT?".

**Adoption plan, in three tiers:**

1. **Internal design (free, now)** — modularity, clarity, transparency,
   representation. Already embodied in the workspace layout: each subcommand is an
   isolated module in `naner-core`/`naner` with args-in → text-out → exit-code
   semantics, independently testable.
2. **Additive surface (parity-safe, during Phases 2–3)** — new commands/flags that
   cannot perturb golden-parity diffs:
   - `naner root` — print the discovered NANER_ROOT and exit (composable primitive:
     `cd $(naner root)`).
   - `--quiet`/`-q` on launcher and vendor commands — suppress `[*]` chatter.
   - Machine-readable vendor listing (e.g. `install --list --porcelain`:
     one `name<TAB>installed<TAB>version` line per vendor).
3. **Default-output changes (post-parity wave, with B1–B6)** — these are the *right*
   behavior but would diverge from the C# golden outputs, so they land as deliberate,
   changelog-visible follow-ups after cutover:
   - Auto-silence status chatter when stdout is not a TTY (the detection mechanism
     already exists); chatty on console, silent in pipelines.
   - Fail loudly: warn on stderr whenever a fallback URL is taken or a configured
     PATH entry is dropped for not existing (today both vanish without a word) —
     this is the same instinct as fixing B1.

**The deliberate compromise:** a literal binary-per-tool split (`naner-launch.exe`,
`naner-env.exe`, `naner-vendor.exe`, …) collides with the release/update contract
(§4.2 — deployed inits fetch an asset named exactly `naner.exe`), `naner.bat`, and
user muscle memory. The **multi-call binary** pattern (git/cargo/busybox) captures
~90% of the value: one `naner.exe`, subcommands designed and tested as independent
small tools. That is how the `naner` crate is organized regardless. A true
multi-binary split stays available as a post-cutover decision, with `naner.exe` as a
thin dispatcher, if ever wanted.

**Where not to force it:** spawning a GUI terminal is inherently not a pipeline
stage, and `naner-init`'s interactive Y/n bootstrap is deliberately a human
interface. Apply text-stream composability to the surfaces where composition is real
(`root`, `--export-env`, vendor listing, diagnostics), not dogmatically to the
launcher's core act.

### 2.5 Windows-only vs cross-platform

Recommendation: **target `x86_64-pc-windows-msvc` only, with clean seams.** The product
is intrinsically Windows-shaped (Windows Terminal, MSYS2, `%VAR%` expansion, `;` PATH,
drive-letter → `/c/` bash conversion). Attempting cross-platform now would multiply the
test surface for no user benefit. Keep `console.rs` and the launcher behind
`#[cfg(windows)]` and keep path/expansion logic in pure functions so a future Linux/macOS
story (or just running unit tests on Linux CI) stays cheap. Note: the pure-logic core
(config, expansion, PATH assembly, version compare, export formatting) is fully testable
on Linux; only console/launch/archive-tool integration tests need Windows runners.

---

## 3. Bugs, dead code, and drift found during analysis

The rewrite must take an explicit position on each of these — silently porting them is a
choice too. **Recommended default: preserve observable behavior in Phases 1–4, then fix
the flagged items as deliberate follow-ups** (each fix is user-visible and should be a
changelog entry, not a migration side-effect).

### Bugs (behavior differs from evident intent)

- **B1 — GitHub asset patterns never match for JSON-defined vendors.** `vendors.json`
  uses glob-style `assetPattern` (`*win-x64.zip`), but the code matches with
  `string.Contains` (literal `*`) and never populates `AssetPatternEnd` from JSON — so
  every GitHub-sourced vendor silently falls through to its pinned fallback URL. The
  "dynamic latest-version fetch" is fiction for JSON-loaded vendors today. *Fix with
  `globset` after parity is proven; until then Rust should reproduce the
  fallback-always outcome or we'll ship a behavior change disguised as a port.*
- **B2 — checksum verification is unreachable.** The verifier machinery is complete,
  but the vendors.json → model conversion never sets a checksum (no JSON field defined),
  so verification always skips. *Worth fixing for real: add `checksum` to the vendor
  schema and wire it through — it's a security posture improvement.*
- **B3 — `dependencies` is parsed but ignored.** No dependency-ordering exists;
  installation happens in map iteration order. It works today because
  `VendorDefinitionFactory` hardcodes 7-Zip first for the essential set. *Implement
  real topological ordering in Rust (cheap) or document the field as inert.*
- **B4 — MSYS2 `packages` array is inert.** vendors.json lists `git`, `gcc`, etc., and
  the installer prints "will be installed on first terminal launch", but nothing
  implements pacman package installation. Users get MSYS2 base without git unless
  something else installs it. *Product decision needed: implement (`pacman -S
  --noconfirm` shell-out post-install) or remove the field and message.*
- **B5 — `VersionComparer.Normalize` edge:** `"1.2"` vs `"1.2.0"` string-mismatch →
  spurious "update available" (self-heals by re-syncing). Tags have always been
  3-part, so latent. *Preserve, then fix by comparing parsed triples.*
- **B6 — `.naner-version` has two writers with different formats** (build writes
  `0.4.6`, updater writes tag `v0.4.6`). Works only because comparisons normalize.
  *Preserve normalization forever; pick one canonical written form in Rust (tag form).*

### Dead code (do not port)

- `Naner.Setup.SetupManager` interactive wizard + `EstablishNanerRoot` (6-strategy
  root discovery): referenced only by itself/tests. The live first-run path is
  "print 'run naner-init'". Only `FirstRunDetector.IsFirstRun`/`GetFirstRunInfo` are live.
- `Naner.Launcher/Resources/*.ps1|psm1`: orphaned legacy PowerShell implementation —
  not embedded, not copied, not referenced. (Keep `ErrorCodes.psm1`'s `NANER-XXXX`
  taxonomy in mind only if we ever want structured error codes; the C# exe never used it.)
- `EventAggregator` + all `NanerEvents`: registered, never meaningfully wired.
- `Naner.DependencyInjection`: the launcher entry point news everything directly.
- `HttpDownloadService`: complete, unused (vendor pipeline uses the plain wrapper).
- `IGitHubClient`: interface with zero implementations (the real client is concrete).
- Plugin loader: supported by `CommandRouter`, never enabled by the shipping entry point.
- `tests/unit/*.Tests.ps1` (Pester): target PowerShell modules deleted from the repo.
- Config fields `UnifiedPath`, `PreservePath`, `VerboseLogging` (and `DebugMode` for
  logging): parsed, never read. **Keep in the serde model for schema compatibility**,
  mark inert.

### Drift to reconcile

- `vendors-schema.json` omits the `nodejs-api`/`dotnet-api` source types the code
  accepts; unknown types silently parse as `static`.
- `HelpTextProvider` prints a placeholder doc URL (`github.com/yourusername/naner`).
- ARCHITECTURE.md / ERROR-CODES.md partially describe the deleted PowerShell era.
- `NanerConstants` says MSYS2's display name is `"MSYS2 (Git/Bash)"`; vendors.json says
  `"MSYS2"`. Install-order code matches on the constants' names — keep exact strings.

---

## 4. Risks and hard parts

### 4.1 Console subsystem semantics (highest fidelity risk)

The `WinExe` + attach/alloc dance is the most subtle behavior in the codebase and the
easiest to get wrong in Rust:

- `#![windows_subsystem = "windows"]` on both binaries.
- Re-implement `IsStdoutCaptured` (`GetStdHandle(STD_OUTPUT_HANDLE)` validity /
  `GetFileType` pipe check) **before** any attach attempt — this is what keeps
  `naner --export-env | iex` and file redirection working.
- `AttachConsole(ATTACH_PARENT_PROCESS)` → success requires printing one leading `\n`
  (clears the shell prompt line); fall back to `AllocConsole()`.
- After attach, stdout/stderr must actually work: in Rust, `println!` to an attached
  console requires reopening `CONOUT$`/getting valid handles — verify against all four
  launch modes: from a shell, double-click, piped, redirected to file.
- `--export-env` output: trimmed, prefix-free, stdout-pure (warnings alone on stderr).

*Mitigation: build a tiny throwaway spike exe for this in Phase 0 and test the four
modes on a real Windows box before porting anything else onto it.*

### 4.2 Parity of the update protocol

Existing `naner-init.exe` installations in the wild fetch **release-by-tag from
`baileyrd/naner`** with asset names `naner-bundle.zip`/`naner.exe`. Whatever repo the
Rust binaries are developed in, **releases must continue to appear on `baileyrd/naner`
with identical tag format (`vX.Y.Z`) and asset names**, or old inits strand. The
embedded-version→tag lookup means the Rust `naner-init` must bake its version at compile
time (`env!("CARGO_PKG_VERSION")`) and the release workflow must guarantee
tag == package version (CI check, as the C# build script does by rewriting
`Directory.Build.props`).

### 4.3 MSYS2 extraction

The 400 MB `msys2-base` tar.xz contains thousands of files and POSIX-y entries
(symlinks, hardlinks). Today 7z.exe absorbs that. If we go native (`tar`+`xz2`), test
specifically: symlink entries on Windows without developer mode (expect fallback to
copy or skip-with-warning), long paths (`\\?\` prefixes / `longPathAware`), and
extraction time vs 7z. Keep the 7z.exe code path as a fallback flag until proven.

### 4.4 Lower-grade risks

- **YAML crate churn** (`serde_yaml` archived): low blast radius — YAML is a fallback
  config format behind JSON in search order.
- **GitHub API rate limits**: unauthenticated 60 req/hr; behavior today is
  fail→fallback URL. Preserve `GITHUB_TOKEN` support.
- **`%VAR%` expansion fidelity**: hand-rolled expander must match
  `Environment.ExpandEnvironmentVariables` (notably: unknown `%VAR%` stays literal,
  `%%` handling) — unit-test against captured .NET outputs.
- **Windows Terminal settings templating**: string-level `%NANER_ROOT%` replacement
  with doubled backslashes inside JSON — port as string ops, don't "improve" to a JSON
  round-trip (WT settings are JSONC; round-tripping loses comments).
- **Trimmed-.NET → Rust behavior diffs in `Path` semantics**: `GetFullPath`,
  trailing-separator trimming, case-insensitive compares — centralize in `paths.rs` and
  test on Windows.

---

## 5. Testing and parity strategy

1. **Port the 19 xunit test intents** (constants, PathBuilder, validators, router,
   version/diagnostics commands, WT configurator, HTTP wrapper) into
   `naner-core` unit tests. Most are pure-logic and run on Linux CI.
2. **Golden-output parity harness** (the migration's safety net): a script that runs
   `naner.exe` (C#) and `naner` (Rust) side-by-side in a fixture tree for:
   `--version`, `--help`, `--diagnose`, `--export-env -f powershell|bash|cmd
   [--no-comments]`, `install --list`, bad-args, missing-root — diffing stdout, stderr,
   and exit codes. (Timestamps in export headers need masking.) Run on a Windows CI
   runner on every PR.
3. **Integration tests** with `tempfile` fixture trees for: root discovery (env var,
   upward walk, failure message), first-run detection matrix, config search order,
   env-override precedence, PATH existence-filtering, `.naner-version` normalization.
4. **HTTP tests** with `httpmock`: release-by-tag flow, asset selection, fallback
   cascade (API error → fallback URL; download error → fallback download), token header,
   octet-stream Accept swap for `api.github.com` asset URLs.
5. **Manual smoke matrix on Windows** (once per phase): shell launch, double-click,
   piped export, first run end-to-end (real GitHub), `install nodejs`, `update-vendors`,
   WT settings preserved across `update`.

---

## 6. Phased migration plan

Phases are ordered to produce a usable artifact as early as possible and to leave the
riskiest-but-most-isolated work (init/update protocol) until the contract tests exist.

### Phase 0 — Scaffolding and the console spike (small)
Workspace + two stub binaries; `windows_subsystem` + attach/alloc/pipe-detection spike
validated in all four launch modes; CI (fmt, clippy, test on ubuntu + windows); release
workflow skeleton producing correctly named assets + `.naner-version`; golden-parity
harness running against stub binaries.

### Phase 1 — `naner-core` foundations (medium)
`constants`, `paths` (root discovery, expansion), `config` (serde models incl. inert
fields, JSON/YAML providers, env overrides, validator, search order), PATH builder,
`env_export`, `logger`, `version`. Bulk of the unit-test porting happens here.
**Exit criteria:** all pure-logic parity tests green on Linux and Windows.

### Phase 2 — `naner` launcher MVP (medium)
clap CLI with two-layer routing; `--version`/`--help`/`--diagnose` (directory verifier,
config verifier, environment reporter); `--export-env`/`--setup-only`; first-run
detection/message; `TerminalLauncher` (WT discovery chain, arg building, fire-and-forget
spawn). Additive Unix-philosophy surface from §2.4 tier 2: `naner root` and `--quiet`
(new command/flag — parity-safe). **Exit criteria:** Rust `naner.exe` is a
daily-drivable drop-in inside an existing initialized tree (vendors still managed by
C# exe); golden harness green.

### Phase 3 — vendors + archives (large)
`vendors.json` loader (lenient JSON), six source-type resolvers with fallback cascade,
downloader (progress format preserved), checksum verifier, extractors (zip native;
tar.xz native + 7z fallback; 7z/msi/exe shell-outs; flattening), WT portable-mode
configurator, `install` and `update-vendors` commands (incl. the parity-safe `--porcelain`
machine-readable listing from §2.4), WT-preserving update semantics.
**Exit criteria:** clean-tree essential-vendor install and per-vendor update produce a
layout byte-comparable (modulo timestamps) to the C# pipeline; optional-vendor spot
checks (`nodejs`, `rust` installer path).

### Phase 4 — `naner-init` (medium)
GitHub client (by-tag, token, octet-stream assets), `NanerUpdater` port (init from
bundle, update naner.exe, marker/version files, sync-to-embedded-version semantics,
prompts, console-gated pause), essential-vendor bootstrap, arg pass-through.
**Exit criteria:** end-to-end first run from an empty directory against a real staged
release; update/downgrade sync verified.

### Phase 5 — cutover and cleanup (small)
Release workflow on `baileyrd/naner` publishes Rust-built `naner.exe`,
`naner-init.exe`, `naner-bundle.zip` (same tags/asset names — see §4.2); `naner.bat`
unchanged; freeze `src/csharp` (delete in a later major); retire stale docs/Pester
tests; then schedule the deliberate bug-fix wave (B1–B6, dependency ordering, checksums,
MSYS2 packages) as post-parity releases, together with the §2.4 tier-3 output changes
(auto-quiet when stdout is not a TTY; loud stderr warnings on fallback URLs and dropped
PATH entries).

### Rough effort

| Phase | New Rust LOC (est.) | Effort |
|---|---|---|
| 0 — scaffolding + console spike | ~400 | 1–2 days |
| 1 — core | ~1,800 | 3–5 days |
| 2 — launcher | ~1,200 | 2–4 days |
| 3 — vendors/archives | ~2,500 | 5–8 days |
| 4 — init | ~1,000 | 2–4 days |
| 5 — cutover | ~200 + CI | 1–2 days |
| **Total** | **~7,000 + tests** | **~3–5 focused weeks** |

(11.6k C# LOC compresses: DI/interface/GlobalUsings boilerplate disappears, dead code
isn't ported, but Rust error handling and Win32 FFI add some back.)

---

## 7. Recommended decisions (summary)

1. **One workspace, two binaries (`naner`, `naner-init`), one shared `naner-core` lib.** No DI framework, no async runtime — synchronous code with a threaded `Context`.
2. **Windows-only target with clean seams**; pure logic testable on Linux CI.
3. **Bug-for-bug parity first, deliberate fixes after** — especially B1 (asset globs), which currently makes every JSON-defined GitHub vendor use its pinned fallback.
4. **Preserve the release contract exactly** (repo `baileyrd/naner`, `vX.Y.Z` tags, `naner-bundle.zip` / `naner.exe` asset names, `.naner-version` normalization) so existing installations keep updating.
5. **Native zip + tar.xz extraction, shell-out for msi/7z/exe installers**, keeping 7z.exe fallback for tar.xz until the MSYS2 extraction is proven on real trees.
6. **Do not port**: setup wizard, plugin loader, EventAggregator, DI layer, orphaned PowerShell resources, unused HTTP service, Pester tests.
7. **Invest early in the console spike and the golden-parity harness** — they de-risk everything else.
8. **Adopt the Unix philosophy via the multi-call binary pattern** (§2.4): subcommands designed as independent small tools inside one `naner.exe`; parity-safe composability additions (`naner root`, `--quiet`, `--porcelain`) during Phases 2–3; default-output changes (auto-quiet in pipelines, loud fallback warnings) deferred to the post-parity wave. No literal binary-per-tool split — it breaks the release/update contract.
