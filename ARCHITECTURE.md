# Architecture

## Overview

`rusty_naner` is the Rust implementation of [naner](https://github.com/baileyrd/naner),
a portable terminal environment launcher for Windows. It assembles a `PATH` and
environment from declarative config, downloads and installs third-party toolchains
("vendors") into a self-contained tree, and launches a terminal into that
environment.

It is a **modular monolith**: three crates in one Cargo workspace, shipping as two
executables. It is not a package manager, not a shell, and not a service — it runs,
does one job, and exits.

Non-goals are listed at the bottom; the migration's own scope and phasing are in
[MIGRATION_ANALYSIS.md](./MIGRATION_ANALYSIS.md), and the relationship to the sibling
Rust projects in [ECOSYSTEM.md](./ECOSYSTEM.md).

## Structure

| Crate | Kind | Responsibility |
| --- | --- | --- |
| `naner-core` | library | All logic: config, paths, vendors, archives, HTTP, digests, updater, logging |
| `naner` | binary | The single binary — launcher, installer, and self-updater in one |

The binary depends on `naner-core` through its public module surface rather
than reaching into implementation detail (`ATLAS-LAYER-0001`); `naner-core`'s
boundaries are documented rather than left implicit (`ATLAS-MOD-0010`).

The modular-monolith default is the standing one for this codebase
(`ATLAS-001` Ch. 23): splitting a component out requires a concrete forcing
function — independent scaling, a team or language boundary, or hard fault
isolation — not speculative future need. Nothing here has crossed that line.
There used to be a second binary, `naner-init.exe`, on the theory that a
process cannot replace its own executable; 0.8.0 collapsed the split once the
rename-aside swap (Windows renames a running exe, it just won't overwrite
one) was proven in the field. Releases still publish a `naner-init.exe` asset
— a byte-copy of `naner.exe` — because deployed 0.6.x–0.7.x updaters require
it to exist and will happily run the new binary under the old name.

> **Standards note.** `ATLAS-100` (Architecture) and `ATLAS-300` (Rust Workspace
> and Cargo) are both `Seed` status — no requirements published — so no specific
> workspace or layering requirement governs this repo yet. The citations above are
> to `ATLAS-001` (Foundation), which is published and normative. Absence of a
> requirement in the Seed volumes means *not yet specified*, not *anything goes*.
> `Rusty-Mill/rusty_foundation_akb` governs the Rusty Mill platform's seven-layer
> model; `rusty_naner` is a standalone personal repo and not a component of that
> platform, so that model is not binding here.

## Boundaries

A boundary is where naner's own consistent state meets something it doesn't
control. Each has exactly one component responsible for translating across it
(`ATLAS-BOUND-0001`), and each declares what it does on failure
(`ATLAS-BOUND-0010`).

| Boundary | Owner | Substitutable via | Failure contract |
| --- | --- | --- | --- |
| HTTP (vendor downloads) | `core::http::UreqHttp` | `Http` trait | `get_text` → `Err` on transport failure, `Ok(status, _)` for non-2xx; `download` stages to `<name>.part` and publishes by rename, so a failed or interrupted transfer leaves nothing under the final name |
| Download cache | `installer::reuse_cached` | — | A cached asset is reused only if it is non-empty and, when a digest is known, matches it; a stale entry is deleted and re-fetched rather than handed to the verifier |
| HTTP (GitHub releases) | `core::github::GitHubReleasesClient` (agent from `core::http`) | `ReleasesApi` trait | Returns `None` on any non-2xx or parse failure; `download_asset` → `false` |
| Archive extraction | `core::archives` | — (dispatch on extension) | `false` on unsupported format or extraction error; staging dir removed |
| Vendor tree placement | `installer::swap_into_place` / `merge_over` | — | Swap is atomic via rename, and restores the previous tree if placement fails; the Windows Terminal merge cannot be atomic (it must preserve `settings/`), so a part-way failure leaves a mixed tree and is reported as a failed install |
| Artifact integrity | `core::checksum` + resolver-supplied digests | `ChecksumInfo` | An upstream digest mismatch blocks installation; a missing digest logs and proceeds |
| Environment pinning | `core::lockfile::NanerLockfile` | `naner.lock` | Unreadable, malformed or future-versioned lock reads as unlocked, with the reason reported; a failed write warns but does not fail the install |
| Config file | `core::config::loader` | JSON provider | `ConfigError`; validation errors block the load, warnings are logged |
| Filesystem layout | `core::paths` | — | `RootNotFound` carries the full search diagnostic |
| Process spawn | `naner::launcher` | `TerminalKind` | Non-zero exit with a message; spawn is fire-and-forget once it succeeds |
| Console attach | `core::console` | `cfg(windows)` | No-op off Windows |

The two `trait` seams (`Http`, `ReleasesApi`) exist so the vendor pipeline and the
updater can be tested against stubs — they define their inputs, outputs and failure
modes explicitly (`ATLAS-COMP-0001`) and expose only what those two callers need
(`ATLAS-COMP-0010`). They are the only traits introduced for *substitution*; the
codebase has one other (`core::digest::Digest`), which exists for algorithm
polymorphism across SHA-256/384/512, SHA-1 and MD5 rather than as a seam.
Everything else is concrete — traits are added where tests need substitution, not
by default.

Outbound HTTP has a single owner: `core::http::build_agent` configures TLS,
timeout, user agent and proxy for both the vendor pipeline and the releases
client, so the two cannot drift apart (`ATLAS-BOUND-0001`).

## Data flow

**Launch** (`naner.exe`, the common path):

```
args → console decision → command router ─── matched? ──→ run command, exit
                                │
                                └── no match ──→ first-run gate
                                                     │
                          find NANER_ROOT (env var, else walk up for bin/ vendor/ config/)
                                                     │
                          load config (naner.json)
                                                     │
                          apply env overrides → expand %NANER_ROOT% / %VAR% / $env:VAR
                                                     │
                          validate (errors block, warnings log)
                                                     │
                          set process env + assemble unified PATH
                                                     │
                    --export-env? → print eval-able script, exit
                    --setup-only? → exit
                                                     │
                          resolve profile → find terminal → spawn (inherits env), exit 0
```

**Vendor install** (`naner install <name>`):

```
vendors.json → resolve per source type (github | web-scrape | static | *-api)
                    │                              │
                    │                        fetch upstream digest where published
                    ↓
              download to vendor/.downloads/  ──→ verify digest ──→ mismatch: abort
                    ↓
              extract to vendor/.staging/<name>  →  swap into vendor/<name>
                    ↓
              write .vendor-version, clean up .downloads
```

Resolution has a two-level fallback: a `fallback` URL is used both when dynamic
resolution finds nothing and when the primary download fails. Fallback use is
logged loudly — a silently pinned old version is how the original bug went
unnoticed for years.

**Bootstrap** (`naner init`, or a bare launch in an empty folder): fetch the
GitHub release whose tag matches this binary's own compile-time version →
download the bundle → verify against the release's `SHA256SUMS` → extract.
Sync-to-embedded on purpose: a fresh install can never strand, because the
release a binary bootstraps from always exists — the release workflow
enforces tag == package version. **Update** (`naner update`): fetch the
*latest* release, verify, then install into every copy of the binary the
tree carries — the running one first, via rename-aside — so an interrupted
update can only offer the update again, never a downgrade.

## Key decisions

See [docs/adr/](./docs/adr/) for the record of individual decisions and their
tradeoffs. Pre-dating the ADR log, the migration's dependency and porting decisions
are argued in [MIGRATION_ANALYSIS.md](./MIGRATION_ANALYSIS.md) §2.2–§2.5.

## Non-goals

- **Cross-platform runtime.** The launcher targets Windows. `naner-core`'s pure
  logic builds and tests on Linux so CI can run there, and archive handling is
  pure-Rust so the Windows cross-check stays C-free — but launching is
  Windows-only by design.
- **Package management.** naner installs a fixed, declared set of vendor
  toolchains. It does not resolve dependency graphs, and it does not manage
  packages *within* a vendor (MSYS2's pacman included).
- **Running as a service.** No daemon, no background process, no persistent state
  beyond files on disk.
- **Reproducible installs by default.** Dynamic resolvers track upstream latest
  until a vendor is pinned. `naner.lock` makes an environment reproducible once
  written, but naner does not ship a pre-populated lock — the first install of
  each vendor still resolves.
