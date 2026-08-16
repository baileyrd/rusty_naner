# Changelog

All notable changes to this repo are documented here.
Format: Added / Changed / Deprecated / Removed / Fixed / Security, newest first.
Reasoning and known limitations live in [RELEASE_NOTES.md](./RELEASE_NOTES.md).

## [Unreleased]
### Added
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
- `ProfileConfig::WindowEffect`, which was parsed and never read. The README's
  claim of `Mica`/`Acrylic`/`Tabbed` backdrop support has been corrected.

### Fixed
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

## [0.5.0] - 2026-07-09
### Added
- Phase 5 cutover: the release workflow publishes the Rust-built `naner.exe`,
  `naner-init.exe` and `naner-bundle.zip`, guarded by a tag == version check.

### Fixed
- Post-parity bug wave B1–B6 (see `docs/post-parity-fix-wave.md`).
