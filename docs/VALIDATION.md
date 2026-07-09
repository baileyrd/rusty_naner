# Windows Validation Checklist

The pre-cutover validation for the Rust port (MIGRATION_ANALYSIS §4.1, §5,
§6 exit criteria). Everything here runs on a real Windows 10/11 box — these
are exactly the behaviors that cannot be proven from Linux CI or the
cross-compile check.

Work through the steps in order; each builds on the previous. Record any
divergence you find: either it's a bug to fix here, or it goes on the
deliberate-divergence list at the bottom.

**Prerequisites**

- Windows 10/11 with [rustup](https://rustup.rs) (MSVC toolchain) and git
- PowerShell (for `scripts/parity.ps1`)
- Your existing, working C# naner installation (for parity and the
  drop-in test) — **back it up first**

---

## Step 0 — Build and unit-test natively

```powershell
git clone https://github.com/baileyrd/rusty_naner
cd rusty_naner
cargo test --workspace          # runs the cfg(windows) code natively
cargo build --release           # target\release\naner.exe, naner-init.exe
```

Expect: all tests green (CI already runs this leg, but confirm locally),
two exes of roughly 2–4 MB each.

## Step 1 — Console spike: the four launch modes (§4.1)

The subtlest ported behavior. Both binaries are GUI-subsystem apps that
decide at startup whether to attach, allocate, or leave the console alone.
Compare each mode against the C# exe doing the same thing.

| # | Mode | How | Expected |
|---|------|-----|----------|
| 1 | Shell | `target\release\naner.exe --version` from PowerShell and cmd | Output in the *same* console (attached), one leading blank line clearing the prompt, ANSI colors render, `$LASTEXITCODE`/`%ERRORLEVEL%` correct |
| 2 | Double-click | Explorer double-click `naner-init.exe` in an **empty folder** | A *new* console window appears (allocated), first-run prompt shows; on decline/error the window stays open on "Press any key to exit..." |
| 3 | Piped | `naner.exe --export-env --no-comments \| Invoke-Expression` then check `$env:NANER_ROOT` | Works. No console attach, no `[*]` chatter or ANSI garbage in the pipe, warnings (if any) on stderr only |
| 4 | Redirected | `naner.exe --version > out.txt 2> err.txt` | File gets the output, no console window flash, correct exit code |

Run 1–4 for `naner.exe` and 1, 2, 4 for `naner-init.exe`. Mode 3 is the
load-bearing one — it is what keeps `--export-env` composable.

## Step 2 — Golden parity harness (§5.2)

Runs the C# and Rust launchers side-by-side over the fixed command matrix
and diffs stdout/stderr/exit codes.

```powershell
# Inside your existing initialized naner tree:
.\scripts\parity.ps1 `
    -CSharpExe C:\<naner>\vendor\bin\naner.exe `
    -RustExe   .\target\release\naner.exe `
    -WorkingDirectory C:\<naner> `
    -AllowFailures
```

Also run the missing-root case: repeat from an empty temp directory with
`NANER_ROOT` cleared (`$env:NANER_ROOT = $null`).

Inspect every `DIFF` in `parity-out\` (`.csharp.*` vs `.rust.*` pairs).
**Expected (deliberate) diffs — everything else is a bug:**

- Version numbers (`0.4.6` vs the Rust version) anywhere they print
- The phase line: "Pure C# Implementation" vs "Pure Rust Implementation"
- `--help`: the Rust side additionally lists `root`, `--porcelain`, `--quiet`
- Bad-args stderr text (CommandLineParser vs clap error phrasing; exit
  code 1 must match)

Re-run without `-AllowFailures` once the only diffs are the list above.

## Step 3 — Drop-in daily driving (Phase 2 exit criterion)

In your existing tree (after backing up `vendor\bin\naner.exe`):

```powershell
copy C:\<naner>\vendor\bin\naner.exe C:\<naner>\vendor\bin\naner-cs.exe.bak
copy .\target\release\naner.exe C:\<naner>\vendor\bin\naner.exe
```

Then exercise, comparing feel and results with muscle memory:

- `naner.bat` → Windows Terminal opens with the Unified profile; inside it
  check `$env:NANER_ROOT`, `$env:PATH` ordering, `where git`
- `naner -p Bash`, `naner -p CMD`, `naner -d C:\`, `naner --debug`
- `naner --diagnose`, `naner root`, `cd $(naner root)`
- `naner --export-env -f powershell|bash|cmd`, `--no-comments`
- Wrong profile name → failure message + available list, exit 1

Daily-drive it for a while before moving on. Vendors are still managed by
whichever exe you point at — both operate on the same tree layout.

## Step 4 — Vendor pipeline (Phase 3 exit criteria)

Use a **scratch copy** of the tree, not your real one.

1. `naner install --list` and `naner install --list --porcelain`
2. `naner install nodejs` → then `vendor\nodejs\node.exe --version` works
3. **The MSYS2 trial (§4.3, the big one):** delete `vendor\msys64`, run
   `naner update-vendors` (or install fresh). Watch the native tar.xz
   extraction on the ~400 MB archive:
   - It completes without error (a "Native .tar.xz extraction failed …
     trying 7-Zip fallback" warning means the native path degraded — note
     why); rough timing vs the C# run is worth recording
   - `vendor\msys64\usr\bin\bash.exe --version` runs
   - Spot-check symlink-heavy paths under `usr\bin` (entries that fail
     unpack are skipped with a debug note — verify nothing load-bearing
     is missing)
4. **Windows Terminal preserve semantics:** after WT installs, confirm
   `vendor\terminal\.portable` and `settings\settings.json` (with
   `%NANER_ROOT%` expanded, backslashes doubled). Hand-edit
   settings.json, run `naner update-vendors` again, confirm the edit
   survives (WT extracts over-top; everything else delete-reinstalls)
5. **7-Zip MSI path:** `vendor\7zip\7z.exe` exists post-install (msiexec
   administrative extract + `Files\7-Zip` hoist)

Known-by-design (don't chase these): GitHub-sourced vendors from
vendors.json always use their pinned fallback URL (bug B1, preserved);
checksums never verify (B2); MSYS2 packages (git/gcc) are not actually
installed despite the message (B4).

## Step 5 — naner-init end-to-end against a staged release (Phase 4 / §4.2)

Do NOT publish to `baileyrd/naner` yet. Stage on this repo first:

1. **Point the test build at the staging repo:** temporarily change
   `constants::github::{OWNER, REPO}` to `baileyrd` / `rusty_naner` and
   rebuild `naner-init.exe`. (Revert before any real release — deployed
   inits must keep fetching from `baileyrd/naner`.)
2. **Fix the bundle contents first:** the release workflow's bundle
   staging is still a skeleton — it packs only `vendor\bin\*` and
   `.naner-version`. A real first run also needs `config\naner.json`,
   `config\vendors.json`, `home\` (incl. the WT settings template),
   `bin\`, `naner.bat`, and `icons\` from the C# repo. Populate
   `dist\bundle\` accordingly in `.github/workflows/release.yml` (this is
   a tracked Phase 5 item).
3. Tag exactly the Cargo version (`git tag v0.5.0-alpha.0 && git push --tags`)
   — the workflow refuses a mismatch — then publish the draft release it
   creates.
4. **First run:** empty folder, copy the staged `naner-init.exe` in,
   double-click → accept prompts → verify: bundle downloads and extracts,
   `.naner-initialized` + `vendor\bin\.naner-version` written (tag form),
   essential vendors bootstrap (7-Zip first), launch prompt works.
5. **Update/downgrade sync:** overwrite `vendor\bin\.naner-version` with
   `v0.0.1`, run `naner-init check-update` (reports the embedded version),
   `naner-init update` (Y/n prompt, exe swap, version file rewritten).
   Then write a *higher* version and confirm it still "updates" —
   sync-to-embedded means downgrade is correct behavior.
6. Pass-through: `naner-init -p Bash` launches naner with args.

## Step 6 — Record and decide

- Anything that diverged and shouldn't: file it on `rusty_naner` and fix
  before cutover.
- Anything deliberate: add it to the list in Step 2 / the README.
- When Steps 1–5 are clean: Phase 5 is a go — move release publishing to
  `baileyrd/naner` (same tags, same asset names: `naner.exe`,
  `naner-bundle.zip`), un-draft the workflow, freeze `src/csharp`, and
  schedule the post-parity B1–B6 fix wave.
