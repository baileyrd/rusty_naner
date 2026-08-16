# Release Notes

What shipped in each release of `rusty_naner`, newest first, with the reasoning
behind each change rather than just the diff. Unreleased work is listed by merged
PR until it is tagged. Terse per-category entries live in
[CHANGELOG.md](./CHANGELOG.md); this file is the narrative one.

---

## Unreleased

Merged against `main` since [v0.5.0](https://github.com/baileyrd/rusty_naner/releases/tag/v0.5.0).
[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.5.0...main).

### PR #28 — Stop reporting success when the vendor tree cannot be placed
**2026-08-16**

- **Fixed:** a failed staging swap was discarded (`let _ = fs_extra_copy(..)`),
  so `.vendor-version` was written and `Installed <vendor>` logged over a
  directory that never received the new tree. Because "installed" is judged by
  a non-empty directory, every later run then skipped the broken vendor. The
  placement result is now propagated: a failure logs the reason, returns false,
  and neither stamps a version nor pins the vendor.
- **Fixed:** the swap was not atomic on Windows. The target directory was
  re-created immediately before `fs::rename`, and `MoveFileExW` cannot replace
  an existing directory — so the rename failed every time on the one platform
  naner ships to, silently demoting every install to a recursive copy and
  losing symlinks with it. The previous tree is now moved aside instead, which
  lets the rename succeed; the copy path is left only for a genuine
  cross-device move, which two directories under `vendor/` will not hit.
  MSYS2, whose tree is full of symlinks and ~400 MB, benefits most.
- **Changed:** a failed install no longer destroys the install it was replacing
  — the previous tree is restored. The Windows Terminal merge still cannot be
  atomic, because preserving `settings/` rules out a swap; a part-way failure
  there leaves a mixed tree and is reported as a failure rather than papered
  over.
- **Fixed:** the recursive copy treated a missing source as success. That was
  the same silent-failure shape, one level down, and it is now an error.
- 6 new tests (158 passed). The end-to-end one was checked against the old
  behaviour first and fails on it, so it is a regression test rather than a
  restatement of what the code already does.

### PR #27 — Make the lockfile real; drop the inert window-effect field
**2026-08-16**

- **Added:** `naner.lock` now does something. A successful install pins the
  vendor's exact version, URL and SHA-256; later installs reproduce and verify
  that artifact instead of re-resolving to upstream latest. This is the only
  verification MSYS2 and the six GitHub-sourced vendors get — their distributors
  publish no digest, so #25's upstream-digest work could not reach them.
- **Added:** `naner lock [--refresh [vendor...]] [--porcelain]` to inspect pins
  and drop them. `--refresh` accepts a vendor's display name or key.
- **Changed:** `update-vendors` deliberately ignores the pin and rewrites it —
  honouring it there would make the command a permanent no-op on every pinned
  vendor. Reasoning in
  [ADR-0003](./docs/adr/0003-the-lockfile-pins-rather-than-records.md).
- **Removed:** `ProfileConfig::WindowEffect`. It was parsed and never read, and
  the README advertised backdrop effects (`Mica`, `Acrylic`, `Tabbed`) that
  nothing implemented. There is no delivery mechanism for it in the current
  design: the launch path passes CLI arguments, while a Windows Terminal backdrop
  is a `settings.json` profile property, and naner writes those settings only at
  install time via string substitution — deliberately not a JSON round-trip, so
  JSONC comments survive. README corrected rather than left overstating.
- **Known limitation:** the *first* install of a vendor without an upstream
  digest remains trust-on-first-use. The lock records what arrived; it cannot
  know whether that was the right thing. Pinning makes the second and later
  installs trustworthy, which is a real but weaker guarantee — said plainly in
  the module docs, the README and `naner lock`'s own output.
- 13 new tests (152 passed, 5 network-dependent tests `#[ignore]`d).

### PR #26 — Apply the standard governance file set
**2026-08-16**

- **Added:** `CONTRIBUTING`, `CODE_OF_CONDUCT`, `SECURITY`, `CHANGELOG`,
  `RELEASE_NOTES`, `ARCHITECTURE`, an ADR seed, PR and issue templates, and a
  `.gitattributes` that forces `eol=lf`. The line-ending one is not cosmetic here:
  this repo builds on both `windows-latest` and `ubuntu-latest`, so a
  Windows-authored `.sh` reaching the Linux runner with CRLF would die on its own
  shebang, far from the cause.
- **Added:** `ARCHITECTURE.md` documents the three-crate boundary table, each
  boundary's owner and failure contract, and cites the `ATLAS-001` requirements it
  answers to. It also records two known boundary violations rather than presenting
  a clean picture — split proxy ownership (#18) and vendor install reporting
  success on a failed swap (#14).
- **Deliberate scope cut:** no `ci-rust.yml` was added. The repo already has a
  `ci.yml` that does more (fmt + clippy + test across Linux and Windows, plus a
  Windows release build); a second workflow would duplicate every run. The
  governance audit's `ci-*.yml` glob does not match `ci.yml`, so it reports CI as
  missing — that is a false negative, not a gap.
- **Known limitation:** `CODE_OF_CONDUCT.md` and the review-approval rules in
  `CONTRIBUTING.md` are scaffold appropriate to a shared repo; this is currently a
  single-maintainer repo, so "at least one approval" is aspirational.

### PR #25 — Verify downloaded artifacts against upstream digests
**2026-08-16** · [#25](https://github.com/baileyrd/rusty_naner/pull/25)

- **Security:** nothing naner downloaded was verified beyond TLS transport trust.
  All 12 vendors omitted `checksum`, so the verifier took its "no checksum
  provided" branch every time — including for the artifacts that are *executed*
  (`rustup-init.exe`, the Miniconda installer, the 7-Zip MSI). `naner-init`
  replaced the installed `naner.exe` with whatever bytes arrived.
- **Added:** resolvers now carry the digest the distributor already publishes —
  Go and Node.js (SHA-256), the .NET SDK (SHA-512, via the channel manifest that
  also supplies the authoritative URL, replacing a hand-built one), plus a new
  optional `checksumSource` covering `rustup-init.exe` (`.sha256` sidecar) and
  Miniconda (repository listing). A `checksum` pinned in `vendors.json` still wins,
  so a compromised upstream manifest cannot overrule an operator's pin.
- **Added:** `naner-init` verifies `naner.exe` and `naner-bundle.zip` against a
  `SHA256SUMS` manifest now published by the release workflow, and fails closed.
  Safe rather than stranding, because the workflow enforces tag == embedded
  version — any release this binary installs from was built by the same workflow.
- **Fixed:** both download paths reject a short read and delete the partial file.
  Without the delete the size check is moot: the cache probe accepts any non-empty
  file, so the rejected partial would be reused next run (#15).
- **Fixed:** `vendors-schema.json` gains the `nodejs-api` and `dotnet-api` source
  types the loader has always supported. The shipped `vendors.json` did not
  validate against its own schema before this.
- **Known limitation:** MSYS2 publishes no digest sidecar (confirmed 404) and
  GitHub release assets expose none for the six vendors sourced that way. Those
  still install unverified unless pinned; closing that uniformly needs the
  lockfile in #20.
- 16 new tests (142 passed, 5 network-dependent tests `#[ignore]`d).

### PR #24 — Add the MIT license text
**2026-08-16** · [#24](https://github.com/baileyrd/rusty_naner/pull/24)

- **Fixed:** `Cargo.toml` had declared `license = "MIT"` since the first commit,
  inherited by all three crates, with no license text in the repo — an SPDX
  identifier and no actual grant.

### PR #10 — Unbreak the build
**2026-08-16** · [#10](https://github.com/baileyrd/rusty_naner/pull/10)

- **Fixed:** the workspace did not build from a clean checkout. `naner-core`
  declared five dependencies as paths escaping the repo (`../../../rusty_*`), which
  exist only on a machine with sibling checkouts — `cargo metadata` alone failed.
  `rusty_regx` is back on its pinned commit SHA; the other four were removed
  outright, being referenced nowhere. CI had been red on `main` for three weeks.
- **Fixed:** a panic in the case-insensitive matcher behind `%NANER_ROOT%` /
  `%{ARCH}` expansion. It took byte offsets from a lowercased *copy* of the input
  and applied them to the original, which desynchronizes whenever a character's
  lowercase form differs in UTF-8 length — `ẞ` → `ß` panicked on a mid-character
  slice, `İ` → `i̇` silently emitted a mangled path. Because `naner.exe` builds with
  `panic = "abort"` and `windows_subsystem = "windows"`, this made a GUI launch
  vanish with no message.
- **Fixed:** 56 rustfmt diffs and 7 clippy warnings; the previous feature commit
  had never been through either gate.

---

## v0.5.0 — 2026-07-09

The Phase 5 cutover: the Rust implementation became the published build.

- **Added:** the release workflow publishes Rust-built `naner.exe`,
  `naner-init.exe` and `naner-bundle.zip` to `baileyrd/naner` with identical tag
  and asset names, guarded by a tag == package-version check. Deployed
  `naner-init` installations look up the release matching their embedded version,
  so the two must never drift.
- **Fixed:** the post-parity bug wave B1–B6 — see
  [docs/post-parity-fix-wave.md](./docs/post-parity-fix-wave.md).
- All validation gates in [docs/VALIDATION.md](./docs/VALIDATION.md) signed off.
