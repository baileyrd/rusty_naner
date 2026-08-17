# Release Notes

What shipped in each release of `rusty_naner`, newest first, with the reasoning
behind each change rather than just the diff. Unreleased work is listed by merged
PR until it is tagged. Terse per-category entries live in
[CHANGELOG.md](./CHANGELOG.md); this file is the narrative one.

---

## Unreleased

Closes out the rest of the open backlog: #41, #52, #57.

**A missing vendor used to fail one process too late.** `naner` checked
that Windows Terminal existed, then handed it a shell path it had never
checked — `pwsh.exe`, `bash.exe`, whatever `VendorPaths` said or a bare
default guessed. On a tree with nothing installed yet (the ordinary state
before `naner install`, not an edge case), the terminal opened and
immediately showed an NT status code from itself, naming a path the user
never typed. `naner` now resolves and checks the shell before ever building
that argument string, and fails with "PowerShell is not installed — run
`naner install powershell`" instead (#41).

**A mistyped profile name used to just... work, wrong.** `naner -p
SomeTypo` silently launched the default profile instead — exit 0, a
terminal opens, and the only trace is a warning most callers never see.
Every call site resolved a profile the same way regardless of whether the
name came from an explicit `-p` or the configured default, so the failure
path this always claimed to have was dead code. An explicit, unresolvable
`-p` now fails loudly with the profile list and exit 1; not passing `-p` at
all is unaffected (#57).

**Windows Terminal profiles finally reach an existing install.** #50 made
`naner` stop overwriting a user's `settings.json` outright, which was the
right emergency fix and left a real gap: template changes never reached
anyone who had already installed. `update-vendors` now reconciles Naner's
own profiles into the file by GUID — refreshed if still present, added if
never offered before, left alone if the user removed one on purpose
(tracked in a small sidecar so "never added" and "deleted" are never
confused). The trade is the same one `naner migrate` already makes for
`naner.json`: the merge is a JSON round-trip, so comments do not survive
it, and every rewrite is backed up first (#52).

---

## v0.6.1 — 2026-08-16

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.0...v0.6.1).

Both found by actually launching v0.6.0 on a real Windows box and watching
the running application rather than trusting its exit codes and log lines —
the same discipline that produced the eleven `docs/VALIDATION.md` bugs.

**The Windows Terminal profiles naner installs were never portable.**
`naner install WindowsTerminal` writes four profiles into
`settings/settings.json` (`Naner (Unified)`, `Naner PowerShell`, `Naner Bash`,
`Naner CMD`), and `defaultProfile` points at the first. The template those
profiles come from has hardcoded `C:\tools\cmd_line\naner` — a path from
whichever machine generated it — since `v0.5.0-alpha.0`. `naner.exe` and
`naner.bat` themselves were never affected; they build their own `wt.exe`
command line at runtime from the real, correctly-resolved root. What was
broken is everything else: opening the portable `wt.exe` directly, or a
shortcut pinned to a specific `Naner *` profile, launched a shell that could
never find `naner.exe`, on every install except the one that happened to
share that exact path (#58).

**The bundled PowerShell profile fought its own launcher over the tab
title.** `naner.exe` sets a descriptive `--title` (`Naner (Unified)`,
`Naner PowerShell`) when it spawns Windows Terminal. `profile.ps1`'s custom
prompt then overwrote it on every single command with a generic
`pwsh in <folder>`, so the title was only ever correct for the fraction of a
second before the first prompt drew (#59).

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

### Fixed after the first Windows validation pass

Eleven bugs surfaced from working `docs/VALIDATION.md` against this release on
real Windows. Ten are fixed below; one is filed (#41). None were reachable from
CI, and the two worst were silent — correct-looking output, exit code 0, real
damage.

- **Fixed:** on a tree that is not initialized, `naner --export-env` printed the
  first-run notice to **stdout** and exited **0**. That stdout is documented for
  `| Invoke-Expression` and `eval "$(...)"`, so the calling shell was handed
  English prose to execute — `First: The term 'First' is not recognized...` — and
  the success code told any wrapper the export had worked. The first-run gate
  fires before the launcher arguments are parsed, which is why a diagnostic
  reached a machine-readable channel at all. The notice now goes to stderr in
  every invocation, and `--export-env` exits 1 when nothing was exported. The
  interactive first run still exits 0, which is deliberate C# parity and the
  double-click case (#38).
- **Fixed:** every non-ASCII character in console output is now ASCII —
  `logger::failure`'s marker (`[x]`, previously `[✗]`, and therefore every error
  message in both binaries), the first-run bullets, `diagnose`/`doctor`'s
  check marks, the copyright sign, and the em dashes in the dry-run notices. A
  Windows console on the default cp1252 code page renders UTF-8 as mojibake.
  Setting the console code page instead would have fixed the attached case and
  not the redirected one, since a pipe's encoding belongs to whatever reads it
  (#39).
- **Fixed:** `setup-shell` generated a block pointing at
  `<root>\bin\naner.exe`. `naner.exe` lives at `<root>\vendor\bin\naner.exe` —
  where the release workflow stages it, where `naner-init` installs and updates
  it, and what `naner.bat` calls. `bin/` is the user's own directory and ships
  empty. The generated block guards on `Test-Path`/`-f`, so the wrong path
  failed silently: the block was written, looked right in `--dry-run`, and
  never ran. `VendorPaths.Naner` in the shipped `naner.json` carried the same
  wrong path, which the config validator had been reporting as a warning all
  along — invisible on a GUI launch, where there is no console to read it
  (#42).
- **Fixed:** the ASCII sweep above missed five sites — an em dash each in
  `naner lock`, `self-update` and the vendor list, and an ellipsis in `lock`'s
  truncated digest display, which is the string a user reads to check a pin.
  They were missed because the search keyed on the print macro, and in a
  multi-line `format!` the macro sits on a different line from the string. A
  test now walks every source file and fails on the specific characters that
  render as mojibake, naming file and line. It deliberately does not forbid all
  non-ASCII: `paths.rs` tests accented and CJK path handling with real input.
- **Fixed:** a pinned install printed `Latest version: 26.02` directly beneath
  `Using pinned 7-Zip (26.02)`. Nothing had checked what was current -- that is
  the whole point of a pin -- so the line asserted a check that never ran, on
  the one screen where a user is deciding whether to trust what they are about
  to install. It now prints only on the resolving path, where it is true.
- **Fixed:** a failed install still printed "Restart your terminal to use the
  newly installed tools." A corrupted pin was correctly caught, the install
  correctly refused, the exit code correctly non-zero -- and then naner advised
  a restart for tools that were never placed. The advice is about a PATH that
  changed, so it now prints only when something was installed. A partial run
  still gets it; a run where everything failed does not.
- **Fixed:** `update-vendors` printed `Updating Windows Terminal
  (vv1.24.11911.0)...`, and the same for PowerShell, Rusty Term and Rush. The
  update line prefixed a `v` unconditionally onto a version that four of the six
  essential vendors already record with one. `github.rs` had the guard for this
  and the installer did not.
- **Fixed:** `update-vendors` destroyed the user's Windows Terminal settings on
  every run. Windows Terminal is deliberately the one vendor extracted over-top
  rather than deleted and reinstalled, precisely so `settings.json` survives —
  and then the post-install configurator rewrote it from the template anyway,
  unconditionally. Every colour scheme, key binding, font choice and custom
  profile, replaced, under a line reading `Preserving settings configuration`.
  An existing file is now left alone. The trade is that template changes no
  longer reach an existing install: a stale template is a missing feature, an
  overwrite is lost work. Merging Naner's profiles into the user's file is the
  right long-term answer and is tracked separately (#50).
- **Fixed:** the test named `windows_terminal_update_preserves_settings`
  asserted only that `settings.json` existed after an update, which was true
  whether it had been preserved or replaced. It reads the contents now. A test
  named for a property it does not check is worse than no test — it is the
  reason this looked covered.
- **Fixed:** none of the six built-in vendor definitions set `key`, so all six
  shared the empty-string entry in `naner.lock` -- each `update-vendors` install
  overwrote the previous one's pin, leaving a nameless row and no pin at all for
  four of them. The read side was the dangerous half: `load_all_vendors` falls
  back to that same keyless set when `vendors.json` is missing, empty or
  unparseable, and the pin lookup is by key, so every vendor resolved the one
  shared entry as its own. On such a tree, `naner install PowerShell` would
  fetch whichever artifact wrote that entry last, verify it **successfully** --
  the digest is genuine, just of the wrong file -- and install it under
  PowerShell's name. Integrity checking cannot catch that; nothing is corrupt.
  All six now carry the key `vendors.json` uses, with tests asserting they
  exist, are unique, and match the manifest (#53).
- **Fixed:** `naner install MSYS2` installed the oldest base MSYS2 publishes.
  The scrape resolver took the leftmost regex match, and a vendor's directory
  index is sorted ascending, so "first match" meant "first ever published" --
  `20240507` out of ten available, over two years stale, reported as
  "Fetching latest MSYS2". It now compares matches and takes the newest, using
  the pattern's version capture group numerically where there is one so that
  `1.10` sorts after `1.9`. The existing test could not catch this: its stub
  page held a single archive, so first and newest were the same document (#47).
- **Fixed:** `update-vendors` reinstalled vendors that `vendors.json` marks
  `"enabled": false` -- Rusty Term and Rush, both experimental, onto every tree
  on every run. It read a hardcoded set and never consulted the manifest, so
  the flag was honoured by `install` and ignored by the other command that
  installs things: switching a vendor off lasted until the next update. The
  definitions still come from the built-in set, which carries sources and
  fallbacks a user's manifest may be older than, but the manifest's `enabled`
  now filters it, and skipped vendors are named rather than silently dropped.
  A manifest that cannot be read disables nothing (#48).
- **Provenance:** every one of these came from working the checklist by hand on
  real Windows, and none of them is a v0.6.0 regression — most were ported
  verbatim from the C# and had behaved this way throughout. What changed is that
  someone ran the steps. Several of the affected paths had checklist entries
  that had never been executed, and one had a passing unit test asserting the
  wrong property.

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

- **The Windows validation checklist is partly done.** Console modes, the
  config-writing commands and the vendor pipeline were worked through on real
  Windows and are what produced the nine fixes above. Two steps were not:
  **drop-in daily driving**, which is the one that catches what a checklist
  cannot anticipate, and the **golden parity harness**, which needs a C# naner
  to compare against. `docs/VALIDATION.md` has been rewritten against what this
  pass actually found — the previous revision told you to skip checksum
  verification, which stopped being true in this release.
- **One bug found during that pass is filed, not fixed.** The launcher does not
  verify the shell it hands to the terminal, so a missing vendor surfaces as an
  NT status code from Windows Terminal rather than "PowerShell is not installed
  — run `naner install powershell`" (#41).
- **Windows Terminal profiles are all-or-nothing.** Fixing the settings
  overwrite means naner now writes the whole file or none of it, so template
  changes never reach an existing install and there is no `--reset-settings` to
  ask for them. Merging Naner's profiles into a user's file is the growth path
  (#52).
- **A `naner.lock` written before the key fix keeps its malformed entry.** #53
  stops new nameless entries; it does not clean up an existing one, and
  `lock --refresh` takes a vendor name the entry does not have. Delete the file
  to reset.
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
