# rusty_naner

Rust migration of [naner](https://github.com/baileyrd/naner), the portable terminal
environment launcher for Windows.

## Status

Phase 1 (naner-core foundations) in progress. Done so far:

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

Next: Phase 4 (`naner-init`: GitHub release client, updater, bootstrap).
Still outstanding on a real Windows box: console-spike launch modes,
`scripts/parity.ps1` against the C# exe, and an MSYS2-scale tar.xz
extraction trial (symlinks/long paths, §4.3).

- [MIGRATION_ANALYSIS.md](MIGRATION_ANALYSIS.md) — detailed analysis of the existing C#
  codebase and the phased migration plan.
- [ECOSYSTEM.md](ECOSYSTEM.md) — assessment of the sibling Rust projects
  ([rusty_term](https://github.com/baileyrd/rusty_term),
  [rusty_lsp](https://github.com/baileyrd/rusty_lsp),
  [rush](https://github.com/baileyrd/rush)) and the roadmap for integrating them with
  naner into a full-Rust terminal environment.
