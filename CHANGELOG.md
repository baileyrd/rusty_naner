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

### Fixed
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
