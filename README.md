# rusty_naner

Rust migration of [naner](https://github.com/baileyrd/naner), the portable terminal
environment launcher for Windows.

## Status

Migration complete: `v0.5.0` is the Latest release on
[baileyrd/naner](https://github.com/baileyrd/naner), built from this repo and
published through the Phase 5 release workflow. Phases 0–5 done:

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

- [MIGRATION_ANALYSIS.md](MIGRATION_ANALYSIS.md) — detailed analysis of the existing C#
  codebase and the phased migration plan.
- [ECOSYSTEM.md](ECOSYSTEM.md) — assessment of the sibling Rust projects
  ([rusty_term](https://github.com/baileyrd/rusty_term),
  [rusty_lsp](https://github.com/baileyrd/rusty_lsp),
  [rush](https://github.com/baileyrd/rush)) and the roadmap for integrating them with
  naner into a full-Rust terminal environment.
