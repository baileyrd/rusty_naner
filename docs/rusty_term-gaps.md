# rusty_term gaps — full definitions

Actionable expansion of [ECOSYSTEM.md §5.1](../ECOSYSTEM.md). Each gap below is
the launcher→terminal contract (§4.1a) turned into a spec: current behavior with
evidence, target, an implementation sketch, acceptance criteria, the integration
level it unblocks, size, and dependencies.

**Verified against** `baileyrd/rusty_term` @ `78a5a92` (2026-06-08, the same HEAD
ECOSYSTEM.md is pinned to — the repo has not moved). Gap IDs match §5.1's table.

## Current surface (baseline)

- **CLI, in full:** `--list-shells` (`src/main.rs:35`), `--gui` (`src/main.rs:70`),
  `--config <path>` (`src/config.rs:136`), `--gpu` (`src/gui/window.rs:641`, only
  under the `gui-gpu` feature). Hand-parsed with `args.iter().any(...)` — there is
  no `--cwd`, `--command`/`-e`, `--title`, `--maximized`, or `--fullscreen`.
- **Spawn contract:** `Backend::spawn_shell(cols, rows, shell: Option<&str>)`
  (`src/backend/mod.rs:5`) — **no cwd param, no args param**. Both are the root of
  G1/G2/G4.
- **Config fields:** `shell, scrollback, cols, rows, font*, font_size, ligatures,
  theme, cursor_style, cursor_blink, keys` (`src/config.rs:30–60`). No
  starting-directory, title, or launch-mode field.
- **Config injection already works:** `--config <path>` / `$RUSTY_TERM_CONFIG` —
  no post-install templating needed (contrast wt.exe's `.portable` marker).

Contract scorecard (what naner must hand any terminal, from §4.1a):

| Requirement | Status | Gap |
|---|---|---|
| Starting directory | missing | **G1** |
| Shell command + args | Windows via `CreateProcessW`; **broken on Unix** | **G2 / G4** |
| Initial title | missing (child OSC only) | **G3** |
| Launch mode (max/fullscreen) | missing | **G6** |
| Host-resize propagation | **implemented**, docs disagree | **G7** (verify + doc) |
| Detachable spawn | works | — |
| Config injectable | works (`--config` / env) | — |
| Vendorable artifact | nothing to vendor | **G8** |

---

## G1 — `--cwd <dir>` (starting directory)

**Current.** No path from CLI/config to the child's working directory. Unix: the
forked child never `chdir`s before `execvp` (`src/backend/unix.rs:66–86`). Windows:
`CreateProcessW`'s `lpCurrentDirectory` is `null` (`src/backend/windows.rs:147`), so
the child inherits rusty_term's process cwd.

**Target.** `--cwd <dir>` (alias `--starting-directory`) sets the child shell's
initial working directory. Maps naner's core profile `StartingDirectory`.

**Sketch.** Add `cwd: Option<&Path>` to `Backend::spawn_shell` (`backend/mod.rs:5`)
and thread it to both impls and all call sites (console runtime in
`src/runtime/tokio_rt.rs`, plus the gui reader-thread spawn). Unix: in the child
between `setsid`/`dup2` and `execvp`, `libc::chdir(cwd)` (async-signal-safe;
`_exit(1)` on failure). Windows: wide-encode `cwd` and pass as `lpCurrentDirectory`.

**Acceptance.** `rusty_term --gui --cwd C:\some\dir -- pwsh` starts pwsh with
`$PWD == C:\some\dir`; missing dir fails cleanly (nonzero, stderr), doesn't crash.

**Unblocks** Level 1 · **Size** S–M · **Deps** none.

## G2 — `-e/--command "<shell> <args>"` (command + args passthrough)

**Current.** `shell` is a single `Option<&str>`. Windows builds one command line
and lets `CreateProcessW` split it (`windows.rs:134`), so args work there. Unix
hardcodes `argv = [shell, NULL]` (`unix.rs:82`) — **no args reach the child at
all**.

**Target.** naner passes a profile's `CustomShell.ExecutablePath + Arguments`
without generating a per-launch config file. e.g. `--command` / trailing `-- <prog>
<args...>`.

**Sketch.** Extend the contract to carry args explicitly — `spawn_shell(cols, rows,
shell: Option<&str>, args: &[String], cwd: ...)` (do it in the same wave as G1).
Unix: build the `execvp` argv from `[program, args...]` instead of `[shell, NULL]`.
Windows: append quoted args to the command line (or keep the combined-string form).
Pairs with G4 for the Unix config path.

**Acceptance.** `rusty_term --gui -- bash -lc 'echo hi; exec bash'` runs the login
command on both platforms; `--list-shells` and normal launch unaffected.

**Unblocks** Level 1 · **Size** S–M · **Deps** shares the contract change with G1.

## G3 — `--title <t>` (initial window title)

**Current.** Window is created `.with_title("rusty_term")` (`src/gui/window.rs:1576`)
and retitled each frame from the child's OSC 0/2 (`window.rs:507`, default
`"rusty_term"`). No way to seed the title.

**Target.** `--title <t>` sets the initial window title; child OSC still wins once
it emits one. Maps naner profile `Name`.

**Sketch.** Parse `--title` (or a config `title` key), pass into `gui::run`, use as
the `with_title` seed and as the fallback in the per-frame `set_title` instead of the
hardcoded `"rusty_term"`. Mechanism (`set_title`) already exists — this is wiring a
value in.

**Acceptance.** `rusty_term --gui --title "naner: dev"` shows that title until the
shell emits its own OSC title.

**Unblocks** Level 1 · **Size** S · **Deps** none.

## G4 — Unix shell-arg splitting

**Current.** `unix.rs:82` execs the whole `shell` string as `argv[0]`, so config
`shell = "bash --login -i"` makes `execvp` look for a program literally named
`"bash --login -i"` and fail. Windows already honors args via `CreateProcessW`'s own
parsing — this is a **cross-platform inconsistency**, present even without G2.

**Target.** A `shell` string containing args behaves the same on Unix as on Windows.

**Sketch.** Split the `shell` string into program + argv on Unix (shell-words-style
tokenization; the repo's dependency-free style argues for a small hand-rolled
splitter). Once G2 lands, naner passes args pre-split and this covers only the
config-file path — still needed for parity.

**Acceptance.** Unix: `shell = "bash --login -i"` in the config launches bash with
those flags; a bare `shell = "zsh"` still works.

**Unblocks** correctness (Level 1 confidence) · **Size** S · **Deps** overlaps G2.

## G5 — new tabs/panes inherit cwd

**Current.** New tabs/panes spawn via the same `spawn_shell` and land in the process
cwd, not the focused pane's directory.

**Target.** A new tab/pane opens in the focused pane's cwd (via OSC 7 if the shell
reports it, else the `--cwd` value).

**Sketch.** Track per-pane cwd (OSC 7 `file://` reports, already parseable in the OSC
layer) and pass it as the G1 `cwd` when spawning a sibling; fall back to the launch
`--cwd`. Depends on G1's plumbing existing.

**Acceptance.** `cd` in a pane, open a new tab → new tab starts in that directory.

**Unblocks** Level 2 polish · **Size** M · **Deps** G1. Masked while naner uses the
§4.1 workaround (setting the spawned *process* cwd), so not on the L1 critical path.

## G6 — launch modes `--maximized` / `--fullscreen`

**Current.** No launch-mode flag or config field; only `cols/rows` (a window size,
ignored by the TUI — `main.rs:76`). winit's `set_maximized`/`is_maximized` are
already used for the custom title-bar button (`window.rs:782`), so the capability is
present.

**Target.** `--maximized` / `--fullscreen` map naner's `LaunchMode`
(default/maximized/fullscreen).

**Sketch.** Parse the flags (or a config `launch_mode`), and at window build
(`window.rs:1576` area) apply `WindowAttributes::with_maximized(true)` /
`with_fullscreen(Some(Fullscreen::Borderless(None)))`.

**Acceptance.** `rusty_term --gui --maximized` opens maximized; `--fullscreen` opens
borderless-fullscreen; neither set → normal.

**Unblocks** Level 2 · **Size** S · **Deps** none.

## G7 — host-resize: verify + reconcile docs (not a code gap)

**Finding.** Host-resize propagation **is implemented**, contrary to some docs:
- Console/TUI Windows path: a 150 ms timer polls console size and calls
  `set_winsize` on change (`src/runtime/tokio_rt.rs:553,588,603`).
- GUI path: the winit `Resized` event drives `set_winsize` (`tokio_rt.rs:380`).

Two stale claims contradict the code and must be fixed:
1. `src/backend/windows.rs:12` — module comment says "Host resize propagation is a
   known gap (no `SIGWINCH` equivalent is wired)". False; the poll wires it.
2. rusty_term's `README` / `docs/research/implementation-status.md` still list host
   resize as a gap, while `docs/FEATURES.md` marks it done.

**Action.** Verify `ResizePseudoConsole` on a real Windows 11 box (resize the host
console and the gui window; confirm the child reflows / gets `SIGWINCH`), then delete
the three stale "gap" statements. No feature code required.

**Unblocks** Level 1 daily-driver confidence · **Size** S (verify + doc) · **Deps**
a Windows box.

## G8 — distribution: license, CI, tagged win-x64 release

**Current.** No `LICENSE`/`COPYING` file, no `license` field in `Cargo.toml`, no
`.github/workflows/` (no CI), no tags/releases. **Nothing exists for naner to
vendor.**

**Target.** naner's `github` vendor source (`repo: baileyrd/rusty_term`,
`assetPattern: *win-x64.zip`, pinned fallback) consumes exactly a tagged release
carrying a prebuilt `win-x64` zip — mirror the existing PowerShell/WT vendor entries.

**Sketch.**
- Add a `LICENSE` file and `license = "..."` + `repository`/`description` to
  `Cargo.toml`.
- CI workflow: `fmt` + `clippy` + `test` on `ubuntu-latest` and `windows-latest`.
- Release workflow: on tag, build `--features gui` (and/or `gui-gpu`), zip the
  Windows binary as `rusty_term-<ver>-win-x64.zip`, attach to the GitHub release.

**Acceptance.** A `v0.1.x` tag produces a release with a `*win-x64.zip` asset that a
naner `vendors.json` `github` entry resolves, downloads, and extracts.

**Unblocks** Level 1–2 (the hard gate for both) · **Size** M · **Deps** none.

## G9 — `rusty_lsp` path dependency → git tag

**Current.** `Cargo.toml`: `rusty_lsp = { path = "../rusty_lsp", optional = true }`.
Any standalone build with the `l13` feature requires a sibling checkout, and it
blocks release CI (G8) for `l13` builds.

**Target.** A git dependency pinned to a tag (crates.io later).

**Sketch.** `rusty_lsp = { git = "https://github.com/baileyrd/rusty_lsp", tag =
"v0.1.0", optional = true }` once rusty_lsp cuts a tag (its own §5.3 gap).

**Acceptance.** `cargo build --features l13` succeeds from a clean checkout with no
sibling directory present.

**Unblocks** release hygiene (l13 builds in CI) · **Size** S · **Deps** blocked on
rusty_lsp tagging (ECOSYSTEM §5.3).

---

## Sequencing

The **default (non-gui) build needs none of these** — G1–G6 are gui-window items for
naner's launch path; G7–G9 are packaging/hygiene.

| Order | Gaps | Rationale |
|---|---|---|
| 1 | **G8** | Nothing is vendorable without it; gates Level 1 *and* 2. Independent, start anytime. |
| 2 | **G1 + G2 + G4** | One contract change (`spawn_shell` gains cwd + args); do together, splitting Unix argv covers G4. The Level 1 launch core. |
| 3 | **G3** | Small, independent title wiring. Completes the Level 1 flag set. |
| 4 | **G7** | Verify on Windows + delete the three stale docs. Level 1 confidence. |
| 5 | **G6** | Launch modes. First Level 2 item. |
| 6 | **G5** | Tab/pane cwd polish. Level 2; masked by the naner workaround until then. |
| — | **G9** | Whenever rusty_lsp tags; parallel to everything, `l13`-only. |

**Level 1** (manual daily-drive) = G1+G2+G3+G4+G7+G8. **Level 2** (vendored terminal
+ profile) additionally needs G6, G5, and the naner-side terminal abstraction
(ECOSYSTEM §4.1a). All of G1–G8 live in the **rusty_term** repo, not rusty_naner.
