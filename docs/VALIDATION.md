# Windows Validation Checklist

The behaviours that cannot be proven from CI. Everything here runs on a real
Windows 10/11 box.

CI compiles and unit-tests on Windows, so this is not about whether the code
builds or the logic is right. It is about the things a test harness never sees:
which stream output lands on, whether a window stays open, whether a guard
silently skips, whether a file a user owns survives an update.

That distinction is not theoretical. Working this checklist against v0.6.0
surfaced eleven bugs, none of which CI could reach, and the two worst were
silent — correct-looking output, exit code 0, real damage. Assume the same
again: a step that "looks fine" has not passed until you have checked the thing
it is actually asking about.

Work through the steps in order. Record any divergence: either it is a bug to
file, or it belongs on the deliberate-divergence list at the bottom.

**Prerequisites**

- Windows 10/11 with [rustup](https://rustup.rs) (MSVC toolchain) and git
- PowerShell 7 (`pwsh`) and `cmd` — you need both, see Step 2
- A C# naner installation, **only** for the optional parity appendix

---

## Step 0 — Build and unit-test natively

```powershell
git clone https://github.com/baileyrd/rusty_naner
cd rusty_naner
cargo test --workspace
cargo build --release
```

Expect all tests green and two exes in `target\release\` of roughly 2–4 MB.

The toolchain is pinned in `rust-toolchain.toml`; rustup installs it on the
first cargo call. Do not install a toolchain by hand.

If you want the supply-chain gate locally: `cargo install --locked cargo-deny`,
then `cargo deny check`.

## Step 1 — Build a scratch tree

Most of this checklist needs a naner root, and you should not point it at a real
installation — Step 4 deletes and reinstalls vendors.

```powershell
$root = "C:\naner-test"
Copy-Item <repo>\dist-assets $root -Recurse
New-Item -ItemType Directory -Force "$root\vendor\bin" | Out-Null
Copy-Item <repo>\target\release\naner.exe      "$root\vendor\bin\"
Copy-Item <repo>\target\release\naner.exe "$root\vendor\bin\naner-init.exe"  # legacy-name copy
New-Item -ItemType File -Force "$root\.naner-initialized" | Out-Null
```

That satisfies every check in `first_run::is_first_run_at`: the root markers
(`bin`, `vendor`, `config`), the essential four (plus `home`), the init marker,
and a config file.

Root discovery (`paths.rs:30`) is: `NANER_ROOT` if it is set *and* the directory
has the markers, otherwise walk up from the exe's own directory, ten levels max.
Running the exe from `$root\vendor\bin\` finds `$root` on its own.

**Rebuild and re-copy the exe whenever you pull.** Several rounds of this
checklist have been run against a stale binary.

## Step 2 — Console modes

The subtlest ported behaviour. Both binaries are GUI-subsystem apps that decide
at startup whether to attach a console, allocate one, or leave the stream alone.

Run all four from `C:\naner-test`.

| # | Mode | How | Expected |
|---|------|-----|----------|
| 1 | Attached | `.\vendor\bin\naner.exe --version` from pwsh and from cmd | Output in the *same* console, one leading blank line clearing the prompt, colours render (no raw `←[96m`), `$LASTEXITCODE` / `%ERRORLEVEL%` correct |
| 2 | Allocated | Explorer double-click `naner.exe` in an **empty** folder | A *new* console window appears; on decline it stays open on "Press any key to exit..." |
| 3 | Piped | `.\vendor\bin\naner.exe --export-env --no-comments \| Invoke-Expression` then `$env:NANER_ROOT` | The variable is set. Nothing else in the pipe — no prose, no `[*]` chatter, no ANSI |
| 4 | Redirected | See the trap below | File gets the output, no console flash, correct exit code |

Run 1–4 for `naner.exe` (single binary since 0.8.0 — the bootstrap prompts
now live in it).

**Mode 3 is the load-bearing one.** `--export-env` writes a shell program to
stdout, meant for `Invoke-Expression` or `eval`. Anything else on that stream is
handed to the calling shell to execute. Test it twice: once in the tree above,
and once from a directory that is *not* a naner root, where the first-run notice
fires. The second case is what #38 was.

**Mode 4 trap: PowerShell does not wait for GUI-subsystem executables.**
`naner.exe --version > out.txt` in pwsh produces an **empty file** and leaves
`$LASTEXITCODE` stale from the previous command. That is PowerShell, not naner.
Test redirection from cmd:

```
cmd /c ".\vendor\bin\naner.exe --version > out.txt 2>&1"
type out.txt
```

The same behaviour makes two naner commands run concurrently if you paste them
together in pwsh. Run one at a time and wait for the prompt.

**Mode 2 is what catches a panic.** `panic = "abort"` plus
`windows_subsystem = "windows"` means a panic makes the window vanish with no
message at all. A window that flashes and disappears is a crash, not a quick
exit.

## Step 3 — Commands that write files the user owns

Five commands rewrite files a user may have hand-edited. Each must back up
first, write atomically, and honour `--dry-run`.

```powershell
cd C:\naner-test
.\vendor\bin\naner.exe migrate --dry-run
.\vendor\bin\naner.exe migrate
Get-ChildItem config\*.bak
```

Expect: dry-run prints and writes nothing; the real run reports a backup path; a
timestamped `.bak` exists; `$schema`, `title` and `description` survive and lead
the file; `%NANER_ROOT%` is **not** expanded.

That last one has its own check, because it was a real bug (#13) — a transient
environment variable being written permanently into the config:

```powershell
$env:NANER_DEFAULT_PROFILE = "Bash"
.\vendor\bin\naner.exe migrate --dry-run | Select-String '"DefaultProfile"'
$env:NANER_DEFAULT_PROFILE = $null
```

Must still read `"Unified"`.

```powershell
cmd /c ".\vendor\bin\naner.exe profile export Unified > u.json"
.\vendor\bin\naner.exe profile import u.json --as Test 2>$null
(Get-Content config\naner.json -Raw | ConvertFrom-Json).CustomProfiles.PSObject.Properties.Name
```

Expect `Test` under **`CustomProfiles`**, not `Profiles` — so a built-in of the
same name can never be overwritten in place.

```powershell
.\vendor\bin\naner.exe setup-shell pwsh --dry-run 2>$null
```

Expect a marked, idempotent block, nothing written, and the path inside it
pointing at `vendor\bin\naner.exe`. The block is guarded by `Test-Path`, so a
wrong path there fails silently and forever (#42).

```powershell
.\vendor\bin\naner.exe pack
Expand-Archive naner-bundle.zip -DestinationPath zipcheck -Force
Get-ChildItem zipcheck -Recurse -File
```

Expect `bin/`, `config/`, `home/`, `icons/` and `scripts/`, and **no `.bak` or
`.tmp`** — you have a `.bak` in `config\` from the migrate step, so this is a
live test of the exclusion.

`2>$null` on these suppresses the validation warnings a tree with no vendors
installed produces. They are expected; they also bury the output you are reading.

## Step 4 — Vendor pipeline

Use the scratch tree. This deletes and reinstalls things.

### 4a — Resolve, verify, pin

```powershell
.\vendor\bin\naner.exe install --list
.\vendor\bin\naner.exe install SevenZip
.\vendor\bin\naner.exe lock
```

`--list` marks disabled vendors `[--]`; installing one by name must say
*disabled*, not *unknown vendor*. Note the keys are `SevenZip`, `NodeJS`,
`WindowsTerminal` — lookup is case-insensitive on key **and** display name, so
`7zip` matches neither.

After the install, `naner lock` must show the vendor with a version, and every
row must have a name. A blank name means definitions are sharing a lockfile
entry (#53).

### 4b — The pin is honoured and verified

```powershell
Remove-Item vendor\7zip -Recurse -Force
.\vendor\bin\naner.exe install SevenZip
```

Expect `Using pinned 7-Zip (...)` with **no** "Fetching latest" line, then
`Verifying SHA256 checksum...` against the recorded digest. If it re-resolves,
the pin is not doing its job.

### 4c — Upstream digests

Enable `NodeJS` in `config\vendors\NodeJS.json`, then:

```powershell
.\vendor\bin\naner.exe install NodeJS
```

This is the separate path: a digest fetched from the distributor's own
`SHA256SUMS`, not one naner recorded. Go, Node.js, .NET SDK, rustup and
Anaconda publish them; GitHub-sourced vendors and MSYS2 do not, and rely on the
lock instead.

### 4d — Verification fails closed

Accepting a good file proves nothing on its own. Corrupt a pin and confirm the
install refuses:

```powershell
(Get-Content naner.lock -Raw) -replace '<first 16 hex of the NodeJS digest>', 'deadbeefdeadbeef' | Set-Content naner.lock
Remove-Item vendor\nodejs -Recurse -Force
.\vendor\bin\naner.exe install NodeJS
Test-Path vendor\nodejs
```

Expect: download succeeds, `Checksum verification failed!` with both digests,
install aborts, `vendor\nodejs` **not** created, exit non-zero, and no
"Restart your terminal" advice.

Recover with `naner lock --refresh NodeJS` then reinstall.

### 4e — MSYS2 native extraction

```powershell
.\vendor\bin\naner.exe install MSYS2
.\vendor\msys64\usr\bin\bash.exe --version
```

~400 MB. `Native .tar.xz extraction failed ... trying 7-Zip fallback` means the
native path degraded — record why. Spot-check symlink-heavy paths under
`usr\bin`; entries that fail to unpack are skipped with only a debug note.

Check which archive it fetched. The scrape takes the first match on the
directory index, which is sorted oldest-first (#47) — it should not be picking a
years-old base.

### 4f — Windows Terminal settings survive an update

Windows Terminal is the only vendor extracted over-top rather than deleted and
reinstalled, specifically so your settings survive.

```powershell
.\vendor\bin\naner.exe install WindowsTerminal
(Get-Content vendor\terminal\settings\settings.json -Raw) -replace '^\{', '{"zzJunkKey": "survived",' | Set-Content vendor\terminal\settings\settings.json
.\vendor\bin\naner.exe update-vendors
Select-String zzJunkKey vendor\terminal\settings\settings.json
```

`zzJunkKey` must still be there. This is #50 — the file was rewritten from the
template on every update, destroying every colour scheme and key binding, while
the run printed "Preserving settings configuration". **Read the contents. A
present file is not a preserved file.**

Also confirm `update-vendors` does not install vendors that their definition
marks `"enabled": false` (#48).

## Step 5 — Drop-in daily driving

The step no checklist substitutes for. Back up `vendor\bin\naner.exe` in your
real tree, drop the new one in, and use it.

```powershell
naner                           # Windows Terminal, Unified profile
naner -p Bash                   # and -p CMD
naner -d "C:\Some Path With Spaces"
naner -d 'C:\has"quote'
naner --debug
naner --diagnose
naner root
naner --export-env -f powershell|bash|cmd
naner --export-env --no-comments
naner <wrong profile name>      # failure message + list, exit 1
```

The two `-d` cases exercise the argument escaping from #21 — a caller-supplied
directory used to be able to inject flags into the spawned terminal's command
line.

Inside the launched terminal, check `$env:NANER_ROOT`, `$env:PATH` ordering, and
`where git`.

Then live in it. Every structured step above was designed against a known
behaviour; the bugs that matter most are the ones nobody thought to write a step
for.

## Step 6 — Self-update

Validated for real on v0.7.1 (fresh install + forced update, 2026-08-18); this
records the procedure so every release after it re-proves the path.

The update mechanics live in `naner update` (alias: `self-update`): fetch the
**latest** release, verify the binary against its `SHA256SUMS` before touching
anything, replace the *running* copy first via rename-aside (the live exe
moves to `naner.exe.old`; Windows allows renaming a running binary but not
overwriting it), then the canonical `vendor\bin\naner.exe`, then any
pre-0.8.0 `naner-init.exe` leftovers — a stale one is a standing downgrade
hazard — and finally write `.naner-version`. The `.old` leftover is swept on
the next launch.

**Forced-update variant** (no newer release needed — this is the everyday
check):

```powershell
# In a current tree, pretend to be ancient:
Set-Content vendor\bin\.naner-version -Value 'v0.0.1' -NoNewline
.\naner.exe update
```

Run bare — no `Start-Process -Wait` wrapper. Part of what this validates is
the 0.8.1 console fix for
[#81](https://github.com/baileyrd/rusty_naner/issues/81): launched from a
shared console, `update` must re-exec into its own console window, and the
output below appears there (with a "press any key to exit" pause at the
end). On 0.8.0 and older the wrapper is still required.

Expect, in order: "Current version: v0.0.1", the real latest tag, a `(Y/n)`
prompt, `Verified naner.exe`, an install line per refreshed copy, and the
version file restored to the latest tag. Then:

- `naner.exe.old` exists beside the copy you ran
- the next `naner` run deletes it silently
- `naner --version` reports the latest version

**Real-upgrade variant** (when a newer release actually exists): run the same
`update` in a tree on the older version — no version-file edit needed. On a
pre-0.7.0 tree, additionally expect the two config-migration warnings
(`vendors.json` no longer read; vendor paths appended pending the
`%VENDOR_PATHS%` marker).

**Fail-closed spot-check**: disconnect the network and run `update` — it must
fail without modifying either binary or the version file.

## Recording what you find

- Diverged and should not have: file it, fix before tagging.
- Diverged deliberately: add it below.
- A step that cannot run (missing vendor, no C# exe): say so explicitly rather
  than marking it passed.

---

## Appendix — Golden parity harness (optional)

Only if you still have a C# naner to compare against.

```powershell
.\scripts\parity.ps1 `
    -CSharpExe C:\<naner>\vendor\bin\naner.exe `
    -RustExe   .\target\release\naner.exe `
    -WorkingDirectory C:\<naner> `
    -AllowFailures
```

Also run it from an empty temp directory with `$env:NANER_ROOT = $null` for the
missing-root case. Inspect every `DIFF` in `parity-out\`.

Expected differences: version numbers; the phase line ("Pure C# Implementation"
vs "Pure Rust Implementation"); `--help` listing `root`, `--porcelain` and
`--quiet` on the Rust side; bad-args stderr wording (CommandLineParser vs clap —
the exit code 1 must still match).

---

## Deliberate divergences from the C#

These are choices, not bugs. Do not file them.

- **`naner checksum` is removed.** Exits 2 with a pointer to automatic digest
  verification and `naner lock`.
- **`ProfileConfig.WindowEffect` is removed.** It was parsed and never read; the
  `Mica`/`Acrylic`/`Tabbed` backdrops never existed.
- **An invalid environment variable name is an error, not a warning.** Names
  must match `[A-Za-z_][A-Za-z0-9_]*`, because `--export-env` output is
  evaluated by the calling shell.
- **`enabled: false` in a vendor definition is honoured** by `install`. Disabled
  vendors are still *listed*, marked `[--]`, so they stay discoverable.
- **Hooks run under `-ExecutionPolicy Bypass`.** A hook is a script the config
  owner supplied on purpose and the default policy would refuse it. A deliberate
  weakening, recorded as one.
- **`PreLaunch` can abort the launch; `PostLaunch` only warns.** By then the
  terminal is running and there is nothing left to prevent.

## Known limitations

Real gaps, already understood. Confirm they have not got worse; do not re-file.

- **MSYS2 `packages` (git, gcc, make…) are not installed.** The array in
  `vendors.json` is inert. The C#'s false "will be installed on first launch"
  message is gone, so nothing claims otherwise — but `pacman` is never run.
- **`naner migrate` cannot preserve comments.** It warns and leaves a backup.
- **`naner schema vendors` has no round-trip drift test**, unlike
  `schema config`, because `VendorJsonEntry` is private to `naner-core`.
- **`cargo-deny` treats `unmaintained` as a warning**, not an error. Known
  vulnerabilities and yanked versions do fail.
- **`~/.claude.json` still lands in the real Windows profile, not
  `%NANER_ROOT%\home`.** `CLAUDE_CONFIG_DIR` only redirects paths *under*
  `~/.claude/` (Anthropic's own docs: "every `~/.claude` path... lives under
  that instead") — `.claude.json` is a sibling *file* of `.claude/`, not a
  path under it, and Claude Code ships no override for it
  (anthropics/claude-code#24479, #33857, both open). The general
  `USERPROFILE` redirect that covers every other `os.homedir()`-reading tool
  doesn't reach it either: Claude Code's CLI is a Bun-compiled binary, and
  Bun's own environment-variable handling on Windows is known to diverge
  from Node's in other ways too (e.g. it ignores `HTTP_PROXY`/`HTTPS_PROXY`
  the way Node does) — confirmed live, `.claude.json` keeps receiving writes
  in the real profile from a naner-launched shell. Nothing to redirect
  without an override Claude itself doesn't provide.
- **Google Antigravity's bundled Gemini agent is not redirected at all.**
  `Antigravity` is a closed-source GUI IDE (a static `.exe`, no `provides`,
  no `environmentVariables`) with an embedded Gemini-powered agent that
  writes its own dotfolder outside naner's tree. Unlike the standalone
  `Gemini` vendor (`@google/gemini-cli`, which *is* redirected — it's a
  plain Node CLI covered by the existing `USERPROFILE` redirect once
  installed through naner), Antigravity's bundled copy is opaque: no
  published environment-variable override, and — being a GUI app most users
  launch from a shortcut rather than typing into a naner-launched shell
  every time — no guarantee it ever inherits naner's redirected environment
  in the first place, even before asking whether its runtime honors
  `USERPROFILE` the way Node does.

## History

B1–B6, the post-parity bug wave documented in
[post-parity-fix-wave.md](./post-parity-fix-wave.md), are all resolved. Earlier
revisions of this checklist told you not to chase them — in particular that
"checksums never verify", which stopped being true in v0.6.0 and would have you
skip the single most important thing on this page. If an instruction here tells
you to ignore something, check that it is still true.
