# rush gaps — full definitions

Actionable expansion of [ECOSYSTEM.md §5.2](../ECOSYSTEM.md). rush is integrated
differently from rusty_lsp/rusty_term: it is **never a crate dependency** — naner
*vendors the binary and spawns it as a shell*, purely at runtime. So "incorporating
rush" means Level 3 (a vendored, experimental shell profile), gated on the tier-1
gaps below. Today naner's config validator actively rejects it
(`crates/naner-core/src/config/validator.rs:290`: *"'rush' is not a standard shell
type"*).

**Verified against** `baileyrd/rush` @ `fc13153` (2026-06-16, the same HEAD
ECOSYSTEM.md is pinned to — the repo has not moved). Gap IDs match §5.2's tiers.

## Current surface (baseline)

- **What works** (README feature table is accurate): a rustyline REPL with
  persistent `$HOME/.rush_history`, multi-line input, pipelines, redirections
  (`>`/`>>`/`<`/`2>&1`), globs, `$VAR`/`~`/`$(...)`/arithmetic expansion, control
  operators (`&&`/`||`/`;`/`&`), `if`/`while`/`for`, functions, and in-process
  builtins: `cd, pwd, echo, export, unset, test, break, continue, return, true, :,
  false, exit` (`src/builtins.rs:13`). Non-interactive `rush -c "…"` and
  `rush FILE args…` (`src/main.rs:47`).
- **Unix job control** (`fg`/`bg`/`jobs`, Ctrl-Z) behind `#[cfg(unix)]`
  (`src/job.rs`, gated at `src/main.rs:18`). **Windows is foreground-only.**
- **CI exists** — build + test + clippy on ubuntu + windows (`.github/workflows/ci.yml`).
  No release job, no tags.
- **Architecture note:** redirections are modeled with `std::process::Stdio`, not
  real fd `dup2`, and subshells/compounds are *approximated without forking*
  (`src/exec.rs:170` — "a known limitation of not forking"). This is the root of G10.

---

# Tier 1 — "vendorable as an experimental shell" (unblocks Level 3)

The minimum for naner to pull rush via its `github` vendor source and offer it as an
opt-in, clearly-experimental profile.

## G1 — LICENSE file + Cargo license metadata

**Current.** No `LICENSE`/`COPYING` file; `Cargo.toml` has no `license` field
(`name`, `version = 0.1.0`, `edition = 2024` only). Same blocker as rusty_term G8.

**Target.** A `LICENSE` file + `license = "…"` (and `repository`/`description`) in
`Cargo.toml`, so the vendored artifact is legally redistributable and naner can label
provenance.

**Acceptance.** `cargo publish --dry-run` stops complaining about missing license; the
release zip carries the license text.

**Unblocks** Level 3 · **Size** S · **Deps** none.

## G2 — Tagged release + CI release job with a Windows artifact

**Current.** `ci.yml` runs build/test/clippy but has **no release job**, no tag
trigger, and produces no downloadable artifact. Inventory: no tags/releases.

**Target.** naner's `github` vendor source (`repo: baileyrd/rush`, `assetPattern:
*win-x64.zip`, pinned fallback — same shape as the PowerShell/WT entries) needs a
tagged release carrying a prebuilt Windows zip.

**Sketch.** Add a tag-triggered workflow: build `--release` on `windows-latest`, zip
the binary as `rush-<ver>-win-x64.zip`, attach to the GitHub release. The build matrix
already proves Windows compiles.

**Acceptance.** A `v0.1.x` tag yields a release with a `*win-x64.zip` asset a naner
`vendors.json` entry resolves, downloads, and extracts.

**Unblocks** Level 3 (the hard gate) · **Size** S–M · **Deps** G1 (license in the zip).

## G3 — Honest status block in README

**Current.** **Largely already done** — README carries a feature status table with
✅/❌ markers and states Windows is foreground-only (`README.md:29,88–91`).

**Target.** Confirm the block explicitly says what naner's profile label will echo:
"experimental," and that Windows lacks job control. This is a *review-and-top-up*, not
a from-scratch write.

**Acceptance.** README's status section names the experimental status and the
Windows foreground-only limitation in plain terms; naner's profile label matches it.

**Unblocks** Level 3 (truth-in-labeling) · **Size** S · **Deps** none.

## G4 — `cd -` (previous directory)

**Current.** Explicitly unsupported — `src/builtins.rs:63`: "`cd -` is not yet
supported." No `OLDPWD` tracking.

**Target.** `cd -` returns to the prior directory and prints it (POSIX), maintaining
`OLDPWD`.

**Sketch.** In `cd()` (`builtins.rs:62`): before `set_current_dir`, save the current
dir to `OLDPWD`; when `argv[1] == "-"`, target `OLDPWD` and echo it. Small, high
interactive value.

**Acceptance.** `cd /a; cd /b; cd -` lands back in `/a` and prints `/a`.

**Unblocks** Level 3 polish (disproportionately annoying absence) · **Size** S · **Deps** none.

---

# Tier 2 — "daily driver" (gates any default-shell thought; rush's own roadmap)

Not on naner's critical path — sequenced by rush, not by the migration. naner never
needs these to *vendor* rush; they gate ever making it a *default*.

## G5 — Tab completion

**Current.** The REPL uses `rustyline::DefaultEditor` (`src/main.rs:86`) — no
`Completer` wired, so Tab does nothing.

**Target.** File, command (PATH), and builtin completion.

**Sketch.** Replace `DefaultEditor` with a custom `Editor` + a `Helper` implementing
rustyline's `Completer` (files from cwd, executables from PATH, the builtin table from
`builtins.rs`).

**Acceptance.** Tab completes a partial path, a PATH command, and a builtin name.

**Size** M–L · **Deps** none.

## G6 — Startup files (`~/.rushrc`)

**Current.** `interactive()` loads history but **sources no rc file** (`src/main.rs:85`).

**Target.** Source `~/.rushrc` (or similar) at interactive startup. **Also the
prerequisite for rusty_term OSC 133 shell integration** (ECOSYSTEM §4.1b) — the
integration scripts are sourced from an rc file, so without this, rush-inside-rusty_term
stays a "basic child process."

**Sketch.** Before the REPL loop, if the rc file exists, `run_source(&contents)`.

**Acceptance.** A `~/.rushrc` setting a var / defining a function / an alias (G8) takes
effect in a new interactive session.

**Size** M · **Deps** none (but pairs with G8 to be useful; unblocks the §4.1b path).

## G7 — Prompt customization (PS1 or equivalent)

**Current.** Prompt is hardcoded `format!("{cwd} $ ")` (`src/main.rs:35–41`).

**Target.** A configurable prompt (PS1-style, or a `PROMPT` var), so users/rc files
can theme it.

**Sketch.** Read a `PS1`/`PROMPT` var in `prompt()`, expanding a small escape set
(cwd, user, exit status); fall back to the current default.

**Acceptance.** Setting the prompt var (or in `~/.rushrc`) changes the interactive
prompt.

**Size** S–M · **Deps** G6 to set it persistently.

## G8 — Aliases; `set -e`; `trap`

**Current.** No `alias`, `set`, or `trap` builtin exists (`src/builtins.rs:13`
dispatch has none).

**Target.** `alias`/`unalias`; `set -e` (errexit) and friends; `trap` (at least
`EXIT`/`INT`).

**Sketch.** Add builtins + an alias table consulted during command resolution; an
errexit flag checked after each simple command in `exec::run_list`; a trap table fired
on the relevant events.

**Acceptance.** `alias ll='ls -l'` then `ll` expands; `set -e; false; echo nope`
stops before `nope`; `trap 'echo bye' EXIT` fires on exit.

**Size** M–L · **Deps** none (aliases most valuable with G6).

## G9 — Test coverage for `exec.rs` and `job.rs`

**Current.** **Zero `#[test]` in either file** (verified: 63 unit tests total —
parser 19, lexer 17, expand 14, glob 6, arith 3, builtins 3, vars 1 — none in
`exec.rs`/`job.rs`, the runtime core). The most behavior-critical, least-tested code.

**Target.** Cover pipeline wiring, redirection routing, exit-status propagation, and
(Unix) job-control transitions.

**Acceptance.** Meaningful `#[test]`s exist for `exec.rs`; Unix job-control paths in
`job.rs` have coverage.

**Size** M · **Deps** stabilize alongside G10 (tests will encode the fixed semantics).

## G10 — Real fd semantics: `2>&1 |`, forked subshells/compounds

**Current.** Redirects use `std::process::Stdio` rather than real fd `dup2`, and
subshells/compounds are approximated without forking (`src/exec.rs:170,77`). So
`2>&1` combined with a pipe, and `cd`/`exit`/var-scope inside `( … )`, don't match
POSIX (a subshell `cd`/`exit` leaks to the parent).

**Target.** Correct `2>&1 |` fd routing; genuinely isolated subshells and compound
commands in pipelines (fork on Unix).

**Sketch.** On Unix, fork subshell/compound pipeline stages and apply real `dup2`
before exec; decide the Windows approximation explicitly (ties into G11/§8.4).

**Acceptance.** `echo x 2>&1 | cat` pipes stderr through; `(cd /tmp); pwd` stays in the
original dir; `(exit 3); echo $?` prints 3 without exiting the shell.

**Size** L · **Deps** informs G9's tests.

## G11 — Validate the MSYS2 (`cfg(unix)`) build; decide the Windows strategy

**Current.** `job.rs` is `#[cfg(unix)]`; under MSYS2 on Windows the unix job-control
path *would* compile but is **untested**. The native-Windows target is foreground-only
by design.

**Target.** Confirm the MSYS2 build works (job control included) and record the
deliberate Windows strategy (native = foreground-only; MSYS2 = full) — ECOSYSTEM §8.4.

**Acceptance.** A documented MSYS2 build/run result; an explicit statement of the
Windows job-control stance.

**Size** M · **Deps** none (decision + validation, minimal code).

---

## Sequencing

**Level 3 (vendor rush as an experimental profile)** needs only **Tier 1: G1 + G2 +
G3 + G4** — plus the naner-side change to stop the validator rejecting `rush`
(`validator.rs:290`) and a `vendors.json` `Rush` entry. G1→G2 first (packaging is the
gate; G1 feeds G2's zip); G3 is a quick review; G4 is a small quality-of-life win.

**Tier 2 (G5–G11)** gates ever making rush a *default* shell (Level 4 territory) and is
driven by rush's own roadmap. Within it, **G6** is the highest-leverage for the
ecosystem — it unblocks rusty_term OSC 133 shell integration (§4.1b) and makes G7/G8
persistent. G9+G10 are the correctness pair (test the runtime, then fix fd/fork
semantics). G11 is a decision + validation.

All of G1–G11 live in the **rush** repo. The only naner-side work is the one-line
validator change + the `vendors.json` entry, once Tier 1 lands.
