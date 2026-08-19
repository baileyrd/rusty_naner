# Release Notes

What shipped in each release of `rusty_naner`, newest first, with the reasoning
behind each change rather than just the diff. Unreleased work is listed by merged
PR until it is tagged. Terse per-category entries live in
[CHANGELOG.md](./CHANGELOG.md); this file is the narrative one.

---

## Unreleased

- Seven new vendors, and two new ways for a vendor to install. Most CLI
  tools these days are distributed through a language package manager
  rather than a standalone archive, so two release-source types join the
  existing six: `npm` (resolved against `registry.npmjs.org`'s `latest`
  dist-tag, installed with the vendored NodeJS's `npm install -g` into
  `home\.npm-global`) and `pip` (PyPI's JSON API, `python -m pip install
  --user` into `home\.local` via the vendored Anaconda). Both leave only a
  `.vendor-version` marker under `vendor\<name>\` — the real install lives
  in the package manager's own tree — and neither is pinned by
  `naner.lock`; the package manager verifies its own artifact. A third
  addition, `installType: "binary"`, covers a vendor that ships as one
  verified executable with nothing to extract: the download is placed
  directly under the vendor directory instead of through the archive
  extractor.

  New vendors: `GitHubCli` (`gh`, GitHub-sourced), `ClaudeCode`, `OhMyPi`,
  and `Codex` (all npm-sourced, all depending on the `NodeJS` vendor),
  `NotebookLmCli` (`notebooklm-py`, pip-sourced, depending on `Anaconda`),
  `OhMyPosh` (a `binary`-type static download, checksum-verified against
  the shared `checksums.txt` its CDN publishes alongside every release —
  the same scrape mechanism Anaconda's resolver already used, applied to a
  manifest instead of a directory listing), and `Antigravity` (a static
  `.exe`; Google publishes no digest for it, so it installs unverified,
  the same posture OneCommander already has). All seven ship disabled by
  default like every other optional vendor.

- First real `refresh-pins` pass over the shipped pins, from a sandbox whose
  proxy blocks `api.github.com`: the four resolvable-from-here vendors were
  refreshed and their new URLs verified live — Go `go1.21.6` -> `go1.26.6`,
  Node `v20.11.0` -> `v26.7.0`, .NET SDK `10.0.102` -> `10.0.400`, MSYS2
  `20240727` -> `20260611`. The 14 GitHub-sourced pins couldn't be checked
  from this environment and are unchanged; the four static-URL vendors are
  manual by design. The pass also caught a real installer bug the new
  command surfaced: `version_from_file_name` took the *first* digit run in a
  file name, so every MSYS2 install recorded `.vendor-version` as literally
  `"2"` (the digit in "msys2") and 7-Zip's scrape as `"7"`. It now picks the
  run carrying the most digits and trims a trailing dot the pattern could
  drag in from the extension.

- Vendor version-pin upkeep, in three connected pieces. `naner refresh-pins
  [dir] [--dry-run]` re-resolves what upstream currently calls latest for
  every dynamically-sourced vendor and rewrites the hardcoded `fallback`
  pins in `config/vendors/*.json` — the pins existed for installs whose
  resolution fails, but nothing ever refreshed them (Go's fallback said
  `go1.21.6`, Node's `v20.11.0`), so a degraded install silently got a
  years-old version. `naner outdated` answers the user-facing half: it
  compares each installed vendor's `.vendor-version` against live upstream
  and flags major-version jumps distinctly, exiting non-zero when updates
  exist. And `naner doctor` gains an offline nudge — an installed vendor
  older than its fallback pin prints an "updates are available" warning
  with no network touched, which stays honest precisely because
  `refresh-pins` keeps the pins recent. Resolution deliberately skips both
  `naner.lock` and the fallback cascade: checking a pin against the pin
  itself would always answer "current". Static-URL vendors (Anaconda,
  Inkscape, HiFile, OneCommander) are reported manual-only in both
  commands — their pinned version *is* the install.

- New vendor: uv, Astral's Python package and project manager. Ships as a
  `github`-sourced zip (`astral-sh/uv`, `uv-x86_64-pc-windows-msvc.zip`)
  verified against the `.sha256` sidecar uv publishes alongside every asset
  — the same mechanism rustup uses, so resolved and fallback downloads are
  both digest-checked. Disabled by default like the other optional tool
  vendors; `provides: ["uv", "uvx"]` wires it into `naner suggest`, and its
  cache, managed Pythons, and installed tools are pointed under
  `%NANER_ROOT%\home` (with `UV_TOOL_BIN_DIR` on the already-exported
  `home\.local\bin`) so nothing leaks outside the portable tree.

- Command-not-found suggestions (#103): typing `node` in a shell where naner
  could provide it now prints what to do instead of only the shell's generic
  error. `naner suggest <name> [--porcelain]` maps an executable name to a
  vendor — each vendor's new optional `provides` list first (shipped for the
  ten tool vendors), then names derived from `naner.json`'s `VendorPaths` —
  and prints the state-appropriate next step: install it, flip `"enabled":
  true` first for a disabled vendor (since `naner install` refuses those), or
  note the tool is only on PATH inside naner-launched shells. No match means
  no output and a non-zero exit, so a wrong guess never outshouts the shell's
  own error. `setup-shell` now writes the matching hooks
  (`CommandNotFoundAction` for PowerShell, `command_not_found_handle` for
  Bash) into its managed block, and the shipped `profile.ps1` carries the
  PowerShell hook — all guarded on `naner.exe` existing, offline, and
  error-swallowing so a missing or moved naner never breaks a shell.

- The first-run bootstrap offers PATH setup: after a successful install it
  asks whether to put `vendor\bin` on the user PATH (the `add-to-path`
  edit), so a fresh install ends with `naner` callable from any new shell
  without a second command. Opt-in, decline-safe, and EOF declines.


## v0.8.2 — 2026-08-18

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.8.1...v0.8.2).

One feature: `naner add-to-path` (#105) closes the last gap in calling
naner — nothing ever put `naner.exe` on the system PATH, so outside the
launched environment the bare name never resolved. The new command
appends `<NANER_ROOT>\vendor\bin` to the per-user PATH so `naner` works
from any newly opened shell, without importing the whole environment the
way `setup-shell` does; `--remove` undoes it, `--dry-run` previews the
exact value it would write.

The `HKCU\Environment` value is edited directly rather than through
`setx`, which silently truncates the stored PATH at 1024 characters. The
value's registry type and every other entry are preserved byte-for-byte,
a `WM_SETTINGCHANGE` broadcast makes newly started shells see the
change, and matching is case-insensitive and tolerant of trailing-slash
and quoted variants so re-running is a no-op. User hive only: no
elevation, and nothing outlives deleting the folder plus one `--remove`.

Also ships the post-0.8.1 documentation truth-up (#104): the README now
leads with bare invocation, since the #81 console fix — field-validated
on v0.8.1 — made the `Start-Process -Wait` / `start /wait` wrappers
unnecessary.

### Known limitations for this release

- `add-to-path` is untested on a real Windows box until this release's
  field check runs: `naner add-to-path`, open a new PowerShell window,
  `naner --version` from anywhere, then `--remove` to undo.


## v0.8.1 — 2026-08-18

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.8.0...v0.8.1).

One fix: the #81 keystroke race dies in code. Interactive flows
(bootstrap, `init`, `update`) run bare from a shell now open a console
of their own instead of racing the shell for keystrokes — the
`Start-Process -Wait` / `start /wait` wrappers keep working but are no
longer needed. Pipes, scripts, and CI keep the inline path.

### Known limitations for this release

- The re-exec is untested on a real Windows box until this release's
  three bare-invocation checks run: bare bootstrap in an empty folder,
  bare `naner init`, bare `naner update` — each should open its own
  console window with a working prompt.
- A tree on 0.8.0 must update to this release with a wrapped
  invocation one last time (0.8.0's prompt code predates the fix).

Third strike for the #81 keystroke race, and this time it dies in code. The
sequence: documented for `cmd.exe` in 0.6.x; discovered to bite PowerShell
identically during the v0.7.1 validation (the docs were corrected to demand
waiting wrappers); then hit again on day one of 0.8.0 field testing, because
nobody remembers a wrapper incantation while testing an installer. The root
cause is structural — a GUI-subsystem binary reading a prompt from a console
its parent shell is also reading loses keystrokes to the shell — and
`ConsoleState::Attached` identifies it precisely. Interactive flows
(bootstrap, `init`, `update`) now re-exec themselves into a console of their
own when attached, wait on the child, and mirror its exit code; the child
knows the window is its own and pauses before it closes. Redirected stdio
never re-execs, so the CI test that runs the real binary with a closed stdin
still exercises the inline path, and scripted use is untouched. If the
re-exec spawn itself fails, the flow runs inline as before — racy beats
broken.


## v0.8.0 — 2026-08-18

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.7.1...v0.8.0).

The single-binary release. `naner.exe` is launcher, installer, and updater
in one; `naner-init.exe` is retired as a separate program but every release
still publishes an asset by that name — a byte-copy — so deployed
0.6.x–0.7.x installs keep updating. Also ships the legacy-YAML convert
hint and the install-doc corrections from the v0.7.1 field validation.

### Known limitations for this release

The single-binary flows are CI-verified but not yet field-validated.
Three tests close that gap, in order (see docs/VALIDATION.md Step 6):

1. **Fresh install** — new folder, this release's `naner.exe`,
   `Unblock-File`, run, `Y` twice; pass = Windows Terminal opens.
2. **Update with leftovers** — same folder, set `.naner-version` to
   `v0.0.1`, `naner update`; pass = every copy refreshed, including
   `vendor\\bin\\naner-init.exe`.
3. **0.7.x compat** — in a 0.7.1 tree, run its old `naner-init update`;
   pass = it installs this release and the old name now holds the new
   binary.

naner is one binary now. The `naner`/`naner-init` split existed for a single
reason — a process cannot overwrite its own executable — and the v0.7.1
self-update validation proved in the field that it never needed to: Windows
will happily *rename* a running exe, and the rename-aside swap works against
a genuinely executing binary. With the one forcing function gone, the split
was pure overhead: two binaries to build, verify, ship, version-match, and
explain, and a whole class of stale-sibling bugs (the sync-to-embedded
downgrade trap) that only existed because two copies could disagree.

`naner.exe` is now launcher, installer, and updater. Run it in an empty
folder and it offers to install there — the same prompt, bundle-by-embedded-
tag download, verification, and essentials bootstrap the init binary owned.
`naner init`, `naner update`, and `naner check-update` carry the explicit
commands; `self-update` remains as an alias. `naner update` installs the
latest release into every copy of the binary the tree is known to carry: the
running one first (rename-aside, `.old` swept on the next launch), then the
canonical `vendor\bin\naner.exe`, then any pre-0.8.0 `naner-init.exe`
leftovers — refreshed not out of politeness but because a stale naner-init
would sync the tree back down to its own embedded version.

Compatibility is carried by the release, not the code: every release still
publishes a `naner-init.exe` asset, now a byte-copy of `naner.exe`. A 0.7.x
install's `naner-init update` requires that asset to exist and verifies it
against `SHA256SUMS`; what it installs is the new single binary, which
behaves correctly under the old name and refreshes the rest of the tree on
its first `update`. A 0.6.x install still updates the old way once (manual
download), as before.

One bug found by the merge itself: interactive prompts read EOF as an empty
line, and an empty line means yes. Fine when a human presses Enter; not fine
when the bare binary runs in an empty directory with a closed stdin — the
first CI run of the new bootstrap path silently consented to downloading a
full install. EOF is now a no, pinned by a test that runs the real binary
with stdin closed and asserts nothing downloads.

The last sharp edge of the v0.7.0 upgrade is filed down. Dropping YAML left
one genuinely unkind failure: a tree whose only config is `naner.yaml` got
"no configuration file found" — technically true, and a lie to anyone
looking at their config directory. The loader now checks for the pre-v0.7.0
files by name when nothing loadable exists and says the real thing: this
file is YAML, naner stopped reading it in v0.7.0, convert it to
`config/naner.json` and remove it. The first-run report gives the same
hint, so both the launcher path and the init path tell the truth. The plain
missing-config case is unchanged, and a test pins each behavior.

The v0.7.1 validation on a real Windows box confirmed the self-update
mechanics end to end — fresh install with the split config layout, then a
forced `naner-init update` that verified both binaries, swapped the running
init aside as `.old`, and swept it on the next launch. It also caught two
documentation lies, both now fixed. The README claimed PowerShell waits for
`naner-init.exe`; it does not — no shell waits for a GUI-subsystem process,
so the #81 keystroke race reproduces in PowerShell exactly as in `cmd.exe`,
and the install instructions now give both shells a waiting wrapper. And
nothing warned that SmartScreen silently blocks a freshly downloaded
unsigned exe — the "nothing happens at all" symptom — so `Unblock-File` is
now step one. `docs/VALIDATION.md` gains Step 6, recording the self-update
procedure that was actually used, so future releases re-prove the path
instead of trusting it.


## v0.7.1 — 2026-08-18

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.7.0...v0.7.1).

The self-update release. This is the first release whose `naner-init.exe`
can carry an installation forward on its own: install THIS release's
`naner-init.exe` by hand — the last manual step — and every release after
it is one `naner-init update` away.

### Known limitations for this release

- The rename-aside self-swap has not yet run against a genuinely executing
  binary — the tests stage a stand-in file. The first real `naner-init
  update` from this version is the validation, deliberately.
- A tree on v0.7.0 or earlier still updates the old way one last time:
  its installed init predates this code, so download this release's
  `naner-init.exe` manually.

The self-update mechanism worked; the discovery didn't. `naner-init`'s
"update" was sync-to-embedded — it installed the release matching its own
compiled-in version, and its update check compared two local values, so no
naner installation ever learned that a newer release existed. Updating
really meant: know somehow that a release happened, manually download the
new `naner-init.exe`, and let it drag `naner.exe` up to match. Coherent,
verifiable, and invisible.

`naner-init update` now asks GitHub for the latest release and installs it —
both binaries. Replacing itself uses the rename-aside trick (Windows will
rename a running exe but not overwrite one); the displaced file is parked as
`.old` and swept on the next launch. Two details carry the safety. Both
downloads are verified against the release's `SHA256SUMS` before either
file is touched — half a verified update is not an update. And the init is
swapped *first*: if the second swap fails, the tree holds a new init and an
old `naner.exe`, and the next run offers the update again — the other order
leaves a new `naner.exe` under a stale init whose sync check would offer to
"update" it back down. A release missing either binary installs nothing,
for the same reason, and a test pins each of these properties.

Plain launches stay offline — the fast local check is unchanged, and
`naner-init check-update` is the explicit "ask the network" command.

`naner.bat` is gone. It survived this long on the theory that it was the
one launcher with no network round-trip; reading the code killed that — the
launch check was always two local file reads, so the bat had no advantage
over `naner-init`'s pass-through launch from the same root directory. Its
other history this session was drifting twice (a trailing-backslash
`NANER_ROOT` and a PowerShell fallback that didn't exist), which is the
usual fate of a file nothing executes in CI.


## v0.7.0 — 2026-08-18

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.5...v0.7.0).

The configuration release. `config/` was carrying two dead designs and a
monolith: a plugin schema nothing read, a YAML twin of `naner.json` that had
silently drifted, and a 499-line `vendors.json` where the last two vendor bugs
both originated. All three are gone, and a vendor now lives in exactly one
file that says how to fetch it, where it unpacks, what it puts on PATH, at what
precedence, and which variables it needs.

Three breaking changes, which is why this is 0.7.0 rather than 0.6.6 — see the
entries below for each, and the known-limitations note at the end of this
section.

Two follow-ups to moving vendor environment data into the vendor files, both
closing gaps that move deliberately left open.

A vendor switched off now contributes nothing. Before the split every PATH
entry and variable lived in `naner.json` unconditionally, and what actually
kept an uninstalled vendor off PATH was `build_unified_path` discarding
directories that do not exist — a side effect doing a job nobody had assigned
it. The case it never covered is the one that matters: a vendor installed and
then disabled kept its directory on PATH and its variables set, which is
precisely what disabling it is for. `enabled` now gates the contribution
directly. On a fresh tree nothing changes, because those directories are not
there either way; the difference appears exactly where the old behavior was
wrong.

This did mean the test asserting the assembled PATH still matches the original
26-entry list had to stop reading the shipped `enabled` flags — eight of the
eleven vendors with PATH entries ship `enabled: false`, so reading them as
shipped would have left most of the `pathPriority` data unexercised. It now
switches every vendor on first, which is the right scope for it: that test is
about the ordering data being correct, not about which vendors are on.

And `DOTNET_CLI_TELEMETRY_OPTOUT` now has one source instead of two. It was
force-set in `apply_env_overrides` and also declared in `DotNetSDK.json`, and
because the overrides run before the merge — which only fills in keys still
missing — the code won every time and the vendor file's copy never did
anything. The vendor file is now the only source, which also means the
variable follows the vendor it belongs to: no .NET SDK enabled, no `dotnet`
CLI, nothing to opt out of. `POWERSHELL_TELEMETRY_OPTOUT` and
`AZURE_CORE_COLLECT_TELEMETRY` stay in code, having no vendor to belong to —
the Azure CLI is not something naner installs at all.

Splitting `vendors.json` into a file per vendor only did half the job, which
became obvious the moment anyone opened one: a vendor file said how to
download and unpack a tool and nothing about what it means once installed.
`GOROOT` was not in `Go.json`. `vendor\\go\\bin` was not in `Go.json`. Both
were in `naner.json`, in two global lists, along with everyone else's. The
numbers were lopsided — 17 of the 26 PATH entries and 19 of the 22
environment variables belonged to a specific vendor. Adding a vendor meant
edits in three files, and "work on one at a time" was still not true.

They live with their vendor now, as `pathPrecedence`, `environmentVariables`
and `pathPriority`. `naner.json` keeps what is genuinely naner's: `bin`,
`opt`, `vendor\\bin`, the `home\\` user-install trees, and `NANER_ROOT` /
`HOME` / `SSH_HOME`.

The interesting problem was ordering. Intra-vendor order is free — Git's
`cmd`, `mingw64\\bin`, `usr\\bin` stay an ordered array inside `GitForWindows.json`.
Inter-vendor order is not, and it decides real conflicts: Git for Windows and
MSYS2 both ship a `bash.exe`, and whichever directory comes first is the one
you get. Sorting by file name would have quietly reshuffled that. So order is
explicit data — `pathPriority`, numbered in tens so a new vendor slots in
without renumbering its neighbours, lower first, unranked vendors sorting
after ranked ones by key so the order is always total.

The vendor block also is not simply appended. It used to sit in the middle of
`naner.json`'s list, with `%NANER_ROOT%\\opt` after it — deliberately last, so
a user's own tools never shadow a vendor's. Appending would have inverted
that. `naner.json` now carries a `%VENDOR_PATHS%` marker at the exact position
the block belongs, and the merge substitutes it in place.

The merge itself happens inside `config::load` rather than at the call sites.
Six places read `config.environment` — the launcher, three in `main`, `diff`,
`bench` — and every one of them wants the merged view, so making it the only
view is what stops them drifting apart. `load_verbatim` deliberately skips it:
tooling that rewrites the user's config must not bake vendor entries into it.

One thing this deliberately does *not* change: a disabled vendor still
contributes. Eight of the eleven vendors carrying PATH entries ship
`enabled: false`, and all 26 entries used to sit in `naner.json`
unconditionally — what actually keeps an uninstalled vendor off PATH is
`build_unified_path` dropping directories that do not exist. Filtering on
`enabled` here is defensible and might even be better, but it is a behavior
change and this is a move. A test pins the current behavior and says why.
The load-bearing test asserts the assembled PATH is identical, entry for
entry, to the list `naner.json` used to carry — a priority typo fails it.

Fixed along the way: `vendors-schema.json` had been carrying a `$ref` to a
`definitions` block that the per-vendor split removed, so the schema resolved
to nothing and happily validated anything. Nothing in the workspace reads it —
it exists for editors — so only a person would ever have hit it. Restored,
extended with the new fields, and now guarded by a test for dangling `$ref`s
and another asserting every field the real vendor files use is actually
described.

Two things in `config/` were describing systems that do not exist, and both
are gone.

The plugin surface was dead twice over. `plugin-schema.json`, the
`PLUGINS` constant, and the `ALL` directory array had exactly zero readers
between them — `ALL` was never referenced at all, which is also what kept
`LOGS` and `VENDOR_BIN` alive. The C# plugin loader it descended from was an
`AssemblyLoadContext` over `plugins/*.dll` that the shipping entry point never
enabled, and MIGRATION_ANALYSIS marked it "do not port" for that reason. But
the schema did not describe that loader either: it described a manifest
bundling vendors, environment variables and PATH entries, with `.ps1` hooks —
a grouping layer over what `vendors.json` and `naner.json` already do, whose
vendor record was a strict subset of a real vendors entry. Two unbuilt designs
sharing one word, sitting next to the real thing. That is why reading the
config directory raised the question of whether plugins and vendors were
duplicating each other; they were not, because only one of them was ever real.

YAML went the same way, for the drift it had already produced. `naner.yaml`
was a field-for-field twin of `naner.json` — except it had stopped being one,
missing the `Naner` vendor path that points at naner's own executable. The
loader takes the first file that exists and never merges across formats, so
the two could disagree forever while only one was ever read, and which one
depended on a file's presence rather than anyone's intent. Keeping them in
sync was a chore with no upside: nothing needed two formats. So the twin, the
parser, the `naner.yaml`/`naner.yml` search entries, the merge path's YAML
branch, and the `serde_yaml_ng` dependency all went. One config format, one
shipped file, drift structurally impossible.

That last part is a real breaking change, and a quiet one by nature: a tree
whose only config is `naner.yaml` now finds no configuration at all rather
than loading it. The error names `naner.json` explicitly — it is built from
`CONFIG_FILE_NAMES`, so it corrected itself when the constant did — and a test
pins the behavior so a YAML-only tree fails loudly instead of half-working.

`config/vendors.json` was 499 lines describing 22 vendors, and the last two
vendor bugs both came from working in it: four vendors added in one batch all
shipped `installerArgs` without `%TARGETDIR%`, and a connection-reset fix
needed a change nowhere near the entry it affected. Each vendor now gets its
own file under `config/vendors/`, named after the key it declares.

The constraint that shaped the design is that the catalog has to be *compiled
into* the binary: `config_merge.rs` embeds it with `include_str!` so a bare
`naner.exe` swap -- which has no bundle to read from -- can still add newly
shipped vendors to an existing tree. `include_str!` takes exactly one file, so
a build script assembles the 22 authored files into one generated catalog in
`OUT_DIR` and the embed points there. Single source of truth, no generated
file checked in, no new runtime dependency. The build script also enforces the
authoring contract: one vendor per file, file name matching the declared key.

Two things got better rather than merely different. `merge_shipped_vendor_defaults`
used to parse the user's whole file, insert missing keys, and rewrite it; now
it writes a file for each missing vendor and never opens the others, so "a
vendor the user has customized is never overwritten" holds by construction
instead of by a key-by-key check -- and a single malformed entry no longer
blocks every other vendor from being added, because it is no longer in the
same document as them. The loader gained the matching property: one unparseable
file is reported and skipped, where a stray comma used to cost the user the
entire catalog at once.

The cutover is deliberate and hard: the pre-split file is not read at all. That
is a real edge, because the failure is quiet -- no vendors directory means the
loader falls back to four hardcoded essentials and the other eighteen simply
vanish from `install --list`. So a tree that still has a `vendors.json` gets
told exactly that, by name, instead of the generic "vendor configuration not
found" it would otherwise print while a perfectly good-looking file sits right
there.

Vendor listing order is now sorted by file name rather than authored order.
Installs were never affected -- those are dependency-driven by key, and
`naner-init`'s essential bootstrap runs off a hardcoded list -- but
`install --list` output changes once, and one test that indexed the loaded set
positionally now looks vendors up by key, which is what it meant anyway.

Dropped in passing: the file's inert `version`/`description`/`metadata` block.
Nothing read it, the schema already said most of it, and one of its notes
pointed at `Setup-NanerVendor.ps1` -- a PowerShell script this repo does not
have, the same species of stale reference as the `naner.bat` fallback.

Following on from the `naner.bat` fixes below: the branch that used to
advertise a dead PowerShell fallback now does something useful instead of
just failing politely. If `vendor\bin\naner.exe` is missing, the shim hands
the arguments to `naner-init.exe` — at the root, where a first-time user
drops it, or in `vendor\bin`, where an install that has updated itself keeps
it, matching the two locations `self_update::find_naner_init` already
searches. `naner-init` is the component that owns bootstrapping: it prompts
before downloading anything, installs `naner.exe`, and then launches it with
the arguments it was handed, so this recovers a half-installed tree rather
than starting a surprise download.

The one detail that makes it work rather than misfire is `start /wait`.
`naner-init.exe` is a GUI-subsystem binary, so `cmd.exe` does not wait for
it — invoked bare, cmd's own next prompt races `naner-init`'s `(Y/n)` for the
user's keystrokes and initialization can silently fail. That is issue #81,
already documented in the README for people typing `naner-init.exe` by hand;
a shim calling it from inside `cmd.exe` would have walked straight into the
same trap.

Two new assertions cover it, plus a third that is really about `cmd.exe`
rather than about naner: a `REM` containing a parenthesis inside an
`if exist (...)` block closes the block early and silently changes control
flow. Writing the `(Y/n)` explanation as a comment next to the code it
explains would have done exactly that. The test walks block depth and fails
on any parenthesis in a comment inside one — a mistake that is invisible in
a diff and produces no error when it happens.

`naner.bat` — the little shim that sits at the root of every bundle and is
the thing a lot of people actually type — had been carried over from the C#
repo untouched and had drifted out of sync with the tree it ships in. Two
problems. It set `NANER_ROOT` to `%~dp0` as-is, and `%~dp0` always ends with
a backslash; a value ending in `\` escapes the closing quote of any
`"%NANER_ROOT%"` a child process builds into a command line, which is the
classic way a perfectly correct-looking path turns into a parse error
somewhere far away. `naner.exe` itself was unaffected — `find_naner_root`
trims trailing separators, and re-exports the cleaned root — so the damage
was confined to anything reading the variable *before* naner.exe fixed it,
which is exactly the kind of bug that only shows up in someone else's
script. The shim now round-trips through a trailing dot to drop the
separator (leaving a drive root like `C:\` intact) and joins the exe path
with an explicit separator.

Second, its fallback branch still described a world that no longer exists:
if `naner.exe` was missing it announced a "PowerShell fallback", tried to
run `src\powershell\Invoke-Naner.ps1`, and told the user to build the C#
version with `cd src\csharp && .\build.ps1`. None of those paths are in this
repo — the PowerShell and C# implementations were left behind at the
migration. So the one moment the shim had something useful to say, it said
something that could not work. It now prints where it looked and points at
`naner-init.exe` and the releases page, which is how you actually get
`naner.exe`.

Both bugs were invisible to the test suite for the same reason: nothing in
the workspace reads `naner.bat`; it is consumed only by `cmd.exe` on a
user's machine. Added `shipped_bat_is_current.rs`, which reads the real
shipped file the way `wt_template_is_portable.rs` reads the real Windows
Terminal template — including a byte-level check that it still has CRLF
endings, since `.gitattributes` pins `*.bat` to CRLF precisely because
`cmd.exe` mis-parses an LF-only batch file.

Anaconda (~1 GB — by far the biggest thing naner ever downloads) failed
partway through with "response body closed before all bytes were read"
at 60%, no retry, install just failed. `Http::download` had no retry at
all: a single dropped connection — more likely the longer a download runs
and the bigger the file — failed the whole install outright, and
`static`-type vendors like Anaconda don't even have a fallback URL to
fall through to the way `github`-type ones do. It now retries up to 3
times with a short backoff before giving up, and a new local-server test
reproduces a truncated-then-complete connection deterministically and
offline, so this doesn't need a live 1 GB download to verify again.

A user installed Obsidian via `naner install` and it reported success —
but `vendor/obsidian/` held nothing but the version marker file. Root
cause: naner's `.exe`-installer path (`archives.rs`) only builds a
`/D=<target>`/`/S`-style silent-install-to-custom-directory command line
automatically when a vendor sets *no* `installerArgs` at all; the moment a
vendor supplies its own (as HiFile, Obsidian, Zed and Zen — all added in
the same batch — did, to get the right silent-install flag for their own
installer technology), that smart fallback is bypassed entirely and
naner-core trusts those args verbatim. None of the four referenced
`%TARGETDIR%`, so each ran its installer silently, successfully, and
somewhere naner never looks — Program Files or AppData, depending on the
app — while still recording it as installed. Fixed by adding the correct
target-directory switch for each installer's actual technology (Inno
Setup's `/DIR=`, NSIS's `/D=`, which per NSIS's own requirement must come
last and unquoted) and adding a test that loads the real shipped
`vendors.json` so this exact class of mistake fails CI next time instead
of shipping quietly.

A user installing the Claude Code CLI inside a naner environment hit its
"not on PATH" setup warning for `home\.local\bin` — the directory
`PYTHONUSERBASE` already designates for user-level tool installs (pip's
`--user` flag, and apparently Claude Code's own native installer, both
land things there). Turned out naner's own `Environment.PathPrecedence`
never included that directory, so anything installed there was invisible
even inside a shell naner itself launched, not just to tools checking the
real Windows PATH. Added `%NANER_ROOT%\home\.local\bin` and `\Scripts` to
the shipped `naner.json`/`naner.yaml` so this class of tool is picked up
automatically going forward. This doesn't touch the real Windows PATH —
that's deliberate; naner stays fully portable and admin-rights-free — so
a tool's installer will still warn if it also checks the persistent OS
PATH, but it will now actually run without hunting for it manually inside
any naner-launched terminal.

That fix alone only reaches a *fresh* install, though — the follow-up
question was "do I have to reinstall naner?" and the honest answer turned
out to be worse than a plain no: neither `naner self-update` (which only
swaps the binary) nor `naner update-vendors` (which does reconcile
`naner.json` against shipped defaults) would have delivered the new
`PathPrecedence` entries to an already-installed tree, because that
reconciliation only ever added whole-missing `VendorPaths`/`Profiles` keys
and a short hardcoded list of specific field migrations — a plain list
addition like this had no path in at all. `merge_shipped_naner_defaults`
now reconciles `Environment.PathPrecedence` the same principled way
`wt_config.rs` already reconciles Windows Terminal profiles: a shipped
entry missing from the user's list gets appended, unless a
`.naner-managed-path-precedence.json` marker says the user removed it on
purpose, in which case it's left alone. `naner update-vendors` on an
existing install now actually catches up.

The README explained the migration's phase history and listed every CLI
subcommand, but never actually said how to install or start using the
thing — someone landing on the repo had no path from "what is this" to
"running terminal." Added Installation (download `naner-init.exe`, what it
verifies and bootstraps on first run) and Usage (the common `naner`
commands, installing optional dev tools, self-update) sections right after
the intro, ahead of the migration-status writeup.

---

### Known limitations for this release

- A tree whose config is `naner.yaml` stops loading entirely and must be
  converted to `naner.json`. No converter ships; the error names `naner.json`
  but does not detect and call out an existing YAML file.
- A tree whose `config/vendors.json` predates the split is not read. The
  warning names the file and says to update the installation, but there is no
  in-place migration — the vendor set falls back to the four hardcoded
  essentials until a new bundle lands.
- A `naner.json` predating the `%VENDOR_PATHS%` marker gets vendor paths
  appended after `%NANER_ROOT%\\opt` rather than before it, with a warning.
  `merge_shipped_naner_defaults` should add the marker on update.
- `DOTNET_CLI_TELEMETRY_OPTOUT` is no longer set on a tree without the .NET
  SDK enabled, having moved to that vendor and stopped being force-set in
  code.

---

## v0.6.5 — 2026-08-17

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.4...v0.6.5).

Added eight new optional vendors to `config/vendors.json`: the file managers
HiFile and OneCommander, the container engine Podman, the image viewer
ImageGlass, the vector editor Inkscape, the note-taking app Obsidian, the
Zed code editor and the Zen browser. All ship disabled by default (`enabled:
false`), same as every other optional vendor — `naner install <Name>` turns
one on explicitly.

Releases now publish to this repo instead of cross-publishing to the
pre-rewrite `baileyrd/naner` repo. Through v0.6.4, tagging `rusty_naner`
built the exes but shipped them as a release on `baileyrd/naner`, so this
repo's own Releases page was always empty — by design, to keep pre-rewrite
installs' auto-updater (which checks `baileyrd/naner`) working. That
cross-publish is now removed: `release.yml` publishes to `rusty_naner`
itself, and `naner-init`'s update check (`constants::github::REPO`) now
points here too. The tradeoff is explicit and was asked for: any install
from before this change stops seeing new releases, since `baileyrd/naner`
no longer receives new tags. Bringing such an install forward requires
manually fetching a current `naner-init.exe` from `rusty_naner`.

This is also the first release actually being used to test the new
publish target end-to-end (real exe assets attached to a real `rusty_naner`
tag), so treat it as a workflow shakedown release rather than a
feature-complete milestone. That test caught a real bug before it shipped:
CI's `windows-latest` job failed on the first attempt at this PR, tracing
back to two `launcher` tests that mutate the process-global `PATH` env var
without synchronizing against each other. Under `cargo test`'s default
multi-threaded runner they raced, and the real system `bash.exe` (Git for
Windows ships one on `windows-latest`) leaked into a window one test
expected `PATH` to be empty. Both now hold a shared test-only mutex.

---

## v0.6.4 — 2026-08-17

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.3...v0.6.4).

Real-world validation of v0.6.3's vendor-set swap turned up seven more
issues — none of them CI-reachable, all of them the same shape as the
session that shipped v0.6.0: naner did one thing and reported another.

The load-bearing one: dropping a new `naner.exe` into an existing install
(the documented, supported upgrade path) never touched `config/naner.json`
or `config/vendors.json`. `update-vendors` already merged new Windows
Terminal profiles into an existing `settings.json`; it now does the same
for the launcher's own config — a pre-#64 tree's `Bash` profile and
`VendorPaths.GitBash` no longer point at MSYS2 forever, and new vendor
definitions (Git for Windows, Anaconda, Bun) reach installs that predate
them (#72).

Also fixed: a piped/logged `naner install` printed raw progress-percentage
noise with none of the status text that would explain it, because the
download progress bar was never gated by the same auto-quiet setting
everything else already was (#67). `naner doctor` always exited 0 no
matter what it found, so it was useless as a CI health gate (#68).
`naner install A B C` could report overall success with one of the
requested names silently dropped for being unknown or disabled (#69). The
missing-Bash install hint still pointed at `naner install msys2` after
#64 swapped the default provider (#70), and `--export-env` still set
`MSYSTEM`/`MSYS2_PATH_TYPE` unconditionally even though MSYS2 is disabled
by default now (#71). The release workflow gained a step that re-downloads
every asset it just published and re-verifies it against `SHA256SUMS`,
so a broken upload fails the job instead of shipping live and
undetected (#66).

---

## v0.6.3 — 2026-08-17

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.2...v0.6.3).

**The default vendor set changes shape.** Git for Windows replaces MSYS2 as
the vendor that's required and enabled out of the box — it's the same
self-extracting-archive install pattern already used elsewhere (a
`PortableGit-*-64-bit.7z.exe` run with `-y -o<dir>`, not a real archive
extraction), and it's what now backs the shipped `Bash` profile and
`VendorPaths.GitBash`. MSYS2 stays fully installable by name, just no
longer part of the default set. Anaconda replaces Miniconda as the optional
Python distribution — same `repo.anaconda.com` listing-scrape digest
verification, just pointed at `/archive/` (Miniconda's `/miniconda/` has a
stable `-latest-` alias; Anaconda's archive index doesn't, so its fallback
URL is a dated version that will need bumping occasionally). Bun joins as a
new optional, disabled-by-default vendor. The .NET SDK — enabled by default
since the C# migration needed it, not because naner itself does — is now
disabled by default too.

---

## v0.6.2 — 2026-08-17

[Compare](https://github.com/baileyrd/rusty_naner/compare/v0.6.1...v0.6.2).

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
