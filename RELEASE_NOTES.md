# Release Notes

What shipped in each release of `rusty_naner`, newest first, with the reasoning
behind each change rather than just the diff. Unreleased work is listed by merged
PR until it is tagged. Terse per-category entries live in
[CHANGELOG.md](./CHANGELOG.md); this file is the narrative one.

---

## Unreleased

Nothing merged since [v0.6.0](https://github.com/baileyrd/rusty_naner/releases/tag/v0.6.0).

---

## v0.6.0 — 2026-08-16

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.5.0...v0.6.0).

The output of a full repository audit. v0.5.0 completed the migration and shipped
a launcher that worked; this release is about the difference between working and
being trustworthy. Three themes:

**Things that reported success without doing anything.** Five commands
(`profile import`, `setup-shell`, `pack`, `self-update`, `checksum`) printed a
success message and acted on nothing. Vendor install reported success when it
could not place the tree. `enabled` in `vendors.json` was parsed and ignored.
`PreLaunch`'s exit code was discarded, so a hook that failed still launched the
terminal. Each of these is worse than an outright error, because the user has
no reason to look.

**Nothing was verified.** Downloaded vendor artifacts and self-update binaries
were trusted on the strength of TLS transport alone. Downloads now check against
the digest the distributor publishes where one exists, `naner.lock` pins version,
URL and SHA-256 for the vendors that publish none, and `naner-init` verifies
release assets against a `SHA256SUMS` manifest and fails closed without it.

**Two reachable injection paths.** Terminal arguments were interpolated
unescaped into a command line handed to `raw_arg`, reachable through
`naner -d '<value>'` — so any wrapper passing a caller-supplied directory was
exposed, not just someone editing their own config. Separately, environment
variable *names* went unescaped into `--export-env` output that is designed to
be piped into `Invoke-Expression` / `eval`.

### Breaking changes

- **`naner checksum` is removed.** It never computed or wrote anything. It now
  exits 2 with a pointer to what replaced it — automatic digest verification and
  `naner lock` — rather than vanishing and leaving a script with an unexplained
  "unknown command".
- **`ProfileConfig.WindowEffect` is removed.** It was parsed and never read, so
  the `Mica` / `Acrylic` / `Tabbed` backdrops the README advertised never
  existed. A config that still sets it is not rejected; the key is simply not
  part of the model, and the README no longer claims the feature.
- **An invalid environment variable name is now an error, not a warning.** A
  config using a name outside `[A-Za-z_][A-Za-z0-9_]*` previously loaded with a
  warning and will now fail validation. This is the fix for the `--export-env`
  injection, and configs travel via `naner pack` and `profile export`, so
  tolerating it was not defensible.
- **`enabled: false` in `vendors.json` is now honoured.** `install --all`
  previously installed the seven vendors the shipped config switches off. If you
  relied on that, those vendors will no longer install; set `enabled: true` or
  name them explicitly. Dependencies still install regardless, since they were
  not chosen from a menu.

### Known limitations

- **The Windows validation checklist has not been re-run for this release.**
  `docs/VALIDATION.md` was signed off for v0.5.0. This release changes real
  runtime behaviour on paths CI cannot reach — the staged-tree swap, the
  argument-escaping change on the spawned command line, the hook gate, and four
  commands that now write files they previously did not. Linux and Windows CI
  are green and the unit tests cover the logic, but no one has watched this
  build launch a terminal.
- `naner schema vendors` is still hand-checked against `VendorJsonEntry`, which
  is private to `naner-core`, so it has no round-trip drift test the way
  `schema config` does.
- `naner migrate` still cannot preserve comments through a serde round-trip. It
  warns before overwriting a commented file and leaves a timestamped backup.
- Hooks still run under `-ExecutionPolicy Bypass`. That is deliberate — a hook
  is a script the config owner supplied on purpose — but it is a weakening, and
  it is now documented as one.
- `cargo-deny` treats `unmaintained` as a warning rather than an error, so an
  unmaintained dependency will not fail CI. Known vulnerabilities and yanked
  versions do.

### PR #36 — Cover command dispatch; pin the toolchain
**2026-08-16**

- **Changed:** the router's dispatch decision is now a pure `Verb::parse`,
  separate from running the command. `route` could not be tested directly —
  calling it would actually install vendors or hit the network — so the table
  had no coverage at all. Seven tests now cover it, including two that would
  have caught real classes of mistake: a name listed in `CONSOLE_COMMANDS`
  that does not route, and a verb that routes but is *missing* from that list,
  which would run with no console attached on a GUI launch.
- **Added:** `completions` shell-name parsing split out and tested, including
  a check that every advertised shell actually generates a non-empty script.
- **Added:** `rust-toolchain.toml` pinning the compiler. Left floating,
  `dtolnay/rust-toolchain@stable` moves under the repo, and a future stable
  that changes a lint turns `clippy -D warnings` red on a commit that touched
  nothing. CI now takes the version from that file alone rather than
  installing a second toolchain nothing uses.
- **Added:** `.editorconfig` matching what rustfmt already produces, with CRLF
  preserved for `.bat`/`.cmd`/`.ps1` so an editor does not undo
  `.gitattributes` on save.
- **Added:** a `cargo-deny` gate (`deny.toml` plus a CI job) covering
  advisories, licences and sources. It earns its place here specifically
  because the workspace carries a git dependency, `rusty_regx`, which no
  registry advisory feed watches on its own; `sources` also fails the build if
  a second, unvetted git dependency appears. The licence allow list is exactly
  what the current tree resolves to rather than a generous pre-approval, so a
  dependency under a new licence stops the build and gets a decision.

### PR #35 — Make the stub commands do what they claim
**2026-08-16**

Five commands reported success without acting. Four now act; one is retired.

- **Fixed:** `profile import` validated its input and then wrote nothing. It
  now merges the profile into `CustomProfiles` — deliberately not `Profiles`,
  so a built-in of the same name is never overwritten in place — with a
  timestamped backup, an atomic write, `--as <name>` and `--dry-run`. Like
  `migrate`, it reads the file verbatim so environment overrides are not
  baked in.
- **Fixed:** `setup-shell` never touched a startup file, and both branches of
  `--dry-run` were byte-identical. It now writes a marked, idempotent block to
  `$PROFILE` or `~/.bashrc`: re-running replaces the block rather than
  appending a second, an unchanged block is a no-op, and surrounding content
  survives. `cmd` still only prints, because it has no per-user startup file
  naner can edit — writing an AutoRun registry key behind someone's back is
  not a reasonable default, and that is now said out loud.
- **Fixed:** `pack` bundled only `config/` while claiming a "self-contained
  portable distribution", and ignored its documented `[dir]` argument. It now
  bundles `bin/`, `config/`, `home/`, `icons/` and `naner.bat`, honours
  `[dir]`, reports anything missing instead of quietly shipping a thinner
  archive, and skips transient files — `.downloads`, `.staging`, `.part`,
  `.tmp`, and the `.bak` files the config commands leave behind.
- **Fixed:** `self-update` printed "Self-update check completed" and replaced
  nothing. It now hands over to `naner-init`, which owns the update protocol,
  passing arguments through and returning its exit code. That is the honest
  design rather than a workaround: `naner.exe` cannot replace itself while
  running on Windows, which is why `naner-init` is a separate executable.
- **Removed:** `naner checksum`. It never computed or wrote anything, and what
  it was for is now covered properly — resolvers carry the distributor's
  digest, and `naner lock` records the exact version, URL and SHA-256 per
  vendor. It exits 2 with a pointer rather than silently disappearing.
- **Changed:** the back-up-then-atomically-replace discipline that `migrate`
  needed is now a shared helper, used by `profile import` and `setup-shell`
  too. Getting it wrong costs someone their configuration, so it lives in one
  place.

### PR #34 — Escape terminal arguments; validate env var names; make PreLaunch a gate
**2026-08-16**

- **Fixed:** values interpolated into the terminal's command line were not
  escaped, and the result goes to `Command::raw_arg`, so an embedded `"` ended
  the quoted section and injected further arguments. Reachable from
  `naner -d '<value>'`, so anything invoking naner with a caller-supplied
  directory was exposed — not only someone editing their own config. Quoting
  now follows the `CommandLineToArgvW` convention, including the trailing
  backslash run that would otherwise escape the closing quote. The existing
  byte-for-bit Windows Terminal argument test still passes, so ordinary
  inputs are unchanged.
- **Fixed:** environment variable *names* were interpolated raw into
  `--export-env` output while only values were escaped. That output is
  designed to be piped into `Invoke-Expression` / `eval`, so a crafted name
  became shell code in the consuming session. Names must now match
  `[A-Za-z_][A-Za-z0-9_]*`, as a validation **error** rather than a warning.
  Configs travel — `naner pack`, `naner profile export` — so "the user owns
  their own config" was not the whole story.
- **Fixed:** the `PreLaunch` hook's exit code was discarded, so a hook that
  failed still launched the terminal. A pre-launch gate that cannot stop the
  launch is not a gate. It now aborts with the reason. `PostLaunch` warns
  instead, since the terminal is already running by then and there is nothing
  left to prevent. A missing hook script is reported rather than silently
  succeeding.
- **Unchanged, deliberately:** hooks still run under `-ExecutionPolicy
  Bypass`. A hook is a script the config owner supplied on purpose and the
  default policy would refuse it. That is now documented as a weakening
  rather than left looking accidental.

### PR #33 — Stop `naner migrate` writing the environment into the config
**2026-08-16**

- **Fixed:** migrate serialized the *loaded* config, which `config::load`
  builds by folding in `NANER_ENV_*`, `NANER_DEFAULT_PROFILE` and the
  telemetry opt-out defaults, and by expanding `%NANER_ROOT%` to a concrete
  path. All correct for running naner; all wrong to write back to disk.
  `NANER_DEFAULT_PROFILE=Bash naner migrate` permanently rewrote the user's
  `DefaultProfile`. It now parses the file verbatim via a new
  `config::load_verbatim`.
- **Fixed:** `$schema`, `title` and `description` were dropped. Losing
  `$schema` breaks the IDE completion `naner schema` exists to provide. Any
  top-level key the model does not own is now carried across, ahead of the
  canonical body so an editor still finds it first.
- **Added:** a timestamped `.bak` beside the config, written before anything
  is overwritten, and refusal to proceed if the backup cannot be written.
- **Added:** `--dry-run`, which prints the result and writes nothing.
- **Changed:** the file is written to a temp path and renamed, so an
  interrupted run cannot truncate the config the launcher needs to start.
- **Changed:** `serde_json::to_string_pretty(&cfg).unwrap()` no longer panics
  on a serialization failure; it reports and exits non-zero.
- **Known limitation:** comments still cannot survive a serde round-trip. The
  command now warns before overwriting a commented file, and the backup makes
  it recoverable.

### PR #32 — Make `naner schema` describe the config that exists
**2026-08-16**

- **Fixed:** `naner schema config` advertised a `Services` block — "background
  sidecar daemons to run alongside terminal sessions" — that no field backs and
  no code implements, so a user following the schema got a silently ignored
  config section. Removed.
- **Fixed:** it omitted `WindowsTerminal`, `Advanced` and `CustomProfiles` at
  the top level and `PreLaunch` / `PostLaunch` on a profile, so roughly half
  the real surface had no autocompletion.
- **Fixed:** `naner schema vendors` had drifted the same way — no `installType`,
  `installerArgs` or `checksumSource`, and no enum on `releaseSource.type`,
  which now lists the six the loader accepts.
- **Changed:** both schemas moved out of the `execute` match into named
  functions, so the tests check the exact value the command prints rather than
  a copy.
- **Added:** two drift tests that make serde the source of truth — every key
  `NanerConfig` serializes must be described, and every described key must be
  one serde produces. The second is what catches an invented block; verified
  by re-introducing `Services` and watching it fail.
- **Known limitation:** `schema vendors` is still checked by eye against
  `VendorJsonEntry`, which is private to `naner-core`. It has no equivalent
  round-trip test.

### PR #31 — Honour the `enabled` flag; generate the vendor list
**2026-08-16**

- **Fixed:** `enabled` in `vendors.json` was parsed and then ignored, so
  `install --all` installed the seven vendors the shipped config switches off,
  and `install --list` advertised them. It is now honoured.
- **Changed:** listing deliberately still shows disabled vendors, marked
  `[--]` (`disabled` in `--porcelain`). Hiding them would make them
  undiscoverable — a user could never find the name to switch on. Installing
  one by name now says it is disabled rather than "unknown vendor", which
  would send them hunting for a typo.
- **Changed:** dependencies install regardless of `enabled`. They were not
  chosen from a menu; they are needed by something the user did choose, and
  failing the install instead would be the larger surprise.
- **Fixed:** omitting `enabled` means *enabled*. `#[serde(default)]` yields a
  bool `false`, so the very change that started reading the field would
  otherwise have switched off every entry that does not mention it — and the
  seven built-in essential definitions never set it, so a missing
  `vendors.json` would have installed nothing at all. Both defaults are now
  explicit and covered by tests.
- **Fixed:** `naner install` with no arguments generated its vendor list from
  the loaded definitions. The literal it replaced had drifted — it never
  gained `rustyterm` or `rush`, so the two newest vendors were undiscoverable.
- **Fixed:** `doctor --conflicts` announced a total and then silently printed
  five. It now says how many it is showing, and sorts first so the truncated
  view is the same list every run rather than an arbitrary five out of a
  `HashMap`.

### PR #30 — One owner for outbound HTTP; cache CI dependencies
**2026-08-16**

- **Fixed:** `GitHubReleasesClient` built its own agent and never read the
  proxy variables, so behind a corporate proxy vendor installs worked while
  `naner-init` bootstrap, `naner-init` update and `naner self-update` all
  failed with a bare "Failed to fetch release". Since `naner-init` is the entry
  point for a fresh install, a proxied user could not get started at all. Both
  clients now share `http::build_agent`, which is also what
  `ATLAS-BOUND-0001` asks for — one component owning the boundary.
- **Added:** `NO_PROXY=*` as a blanket opt-out, and an unusable proxy value is
  now reported and ignored rather than silently dropped.
- **Fixed:** `native_tls::TlsConnector::new().expect(...)` appeared in both
  constructors. `naner.exe` builds with `panic = "abort"` and
  `windows_subsystem = "windows"`, so a broken TLS stack aborted the process
  with no message at all on a GUI launch. It now warns and falls back to
  ureq's default TLS, which fails at request time with something readable.
- **Changed:** CI caches the Cargo registry and target directory
  (`Swatinem/rust-cache`), keyed per runner OS. Every run previously rebuilt
  the whole dependency tree; the Windows leg felt it worst, being the only one
  that does a release build under `lto = true` and `codegen-units = 1`.

### PR #29 — Stage downloads so an interrupted run cannot poison the cache
**2026-08-16**

- **Fixed:** transfers now stream into `<name>.part` and are published with a
  rename once complete. Writing straight to the final path meant a killed
  process — Ctrl-C, a crash, a lost machine — left a truncated file exactly
  where the next run looks for a finished one. Deleting on the error paths
  (shipped in #25) cannot cover that case, because nothing gets to run;
  staging makes it safe by construction.
- **Changed:** the cache decision moved from the transport to the installer,
  which is the only place that knows the expected digest. A cached asset is
  reused only if it is non-empty and, where a digest is known, matches it.
- **Fixed:** a stale cached asset is now discarded and re-fetched instead of
  being handed to the verifier, which would have failed the install rather
  than fixing it. This is a real case, not a hypothetical: names like
  `Miniconda3-latest-Windows-x86_64.exe` are stable while their contents move,
  and pinning (#27) makes the expected digest specific enough to notice.
- **Known limitation:** with no digest to check against, a complete cached file
  is still reused on name alone. That is the offline case the README
  advertises, and completeness is now guaranteed even though identity is not.
- 8 new tests (140 passed, 7 network-dependent `#[ignore]`d). Two of the new
  ones run a real transfer to confirm the artifact lands under its final name
  and no `.part` survives either success or failure.

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
