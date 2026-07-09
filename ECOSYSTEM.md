# The Naner Rust Ecosystem: rusty_term, rusty_lsp, rush

**Companion to:** [MIGRATION_ANALYSIS.md](MIGRATION_ANALYSIS.md)
**Repos:** [`baileyrd/rusty_term`](https://github.com/baileyrd/rusty_term),
[`baileyrd/rusty_lsp`](https://github.com/baileyrd/rusty_lsp),
[`baileyrd/rush`](https://github.com/baileyrd/rush) — assessed against
[`baileyrd/naner`](https://github.com/baileyrd/naner) and the migration plan in this repo.
**Date:** 2026-07-09

MIGRATION_ANALYSIS.md owns the C#→Rust migration of naner itself: what to port, in what
order, with what parity guarantees. This document owns everything around it: an assessment
of the three sibling Rust projects — a terminal emulator, an LSP framework, and a shell —
and a roadmap for how they compose with naner into a coherent, Unix-philosophy Rust stack.
**Nothing in this document changes the migration's phases, scope, or timeline** (§4.3 makes
that rule explicit).

---

## 1. Inventory

Snapshot at the time of writing. Every maturity claim later in this document traces back to
this table.

| Repo | What it is | Version | LOC (src) | Commits | Tests | CI | Tags/releases | HEAD |
|---|---|---|---|---|---|---|---|---|
| `naner` | C#/.NET 8 launcher (migration source) | 0.4.6 | 11,589 | — | 19 xunit intents | yes | `vX.Y.Z` releases | `4f7c623` |
| `rusty_naner` | Migration planning repo (this repo) | — | docs only | 4 | — | no | none | `59e63e1` |
| `rusty_term` | Terminal emulator (TUI relay + native window) | 0.1.0 | 18,984 | 70 | 405 `#[test]` | **no** | **none** | `78a5a92` |
| `rusty_lsp` | Async LSP server framework (library) | 0.1.0 | 2,365 (+414 integ. tests, +267 example) | 1 | 27 (16 unit + 11 integration) | **no** | **none** | `2b0a528` |
| `rush` | POSIX-ish shell (REPL + interpreter) | 0.1.0 | 4,403 | 26 | 63 unit | yes (ubuntu + windows) | **none** | `fc13153` |

Common facts worth stating once: all three Rust projects are Rust edition 2024, live under
`baileyrd/*` on GitHub, have **zero tags and zero releases**, and **none ships a LICENSE
file** (rusty_lsp declares `MIT OR Apache-2.0` in Cargo.toml but its `repository` field is
the placeholder `github.com/example/rusty_lsp`; rusty_term and rush declare no license at
all). All three have zero TODO/FIXME markers in source — unusually clean trees.

---

## 2. The vision, and the reality

The four projects compose into an all-Rust portable terminal environment:

```
┌─────────────────────────────────────────────────────────────┐
│  naner (Rust)          launcher: env, PATH, vendors, root   │
│    │ spawns with cwd/title/shell args                       │
│    ▼                                                        │
│  rusty_term --gui      terminal: window, tabs, panes,       │
│    │ ConPTY/pty spawn  images, ligatures                    │
│    │   ├── l13 side channel (OSC 5379 JSON-RPC / MCP)       │
│    │   │     └── rusty_lsp::{jsonrpc, transport, lsp}       │
│    ▼                                                        │
│  rush                  shell: POSIX-ish REPL + scripts      │
│                        (alongside pwsh, bash, cmd)          │
└─────────────────────────────────────────────────────────────┘
```

The reality today: the working stack is `naner (C#) → wt.exe → PowerShell/MSYS2 bash`, and
the only funded, planned work is the parity-first migration of naner itself
(MIGRATION_ANALYSIS §6). rusty_term is far along but has no release artifacts and lacks the
launch flags a launcher needs; rush is a promising three-week-old prototype; rusty_lsp is a
polished but single-commit library. The roadmap in §4 is therefore deliberately a
**parallel and post-cutover track**: each project closes its own gaps in its own repo, and
integration happens at explicit, criteria-gated levels — never on the migration's critical
path.

---

## 3. Project profiles

Each profile uses the same shape — what it is, maturity, Unix-philosophy alignment (scored
in the vocabulary of MIGRATION_ANALYSIS §2.4), ecosystem role — and stays descriptive.
Everything prescriptive lives in §5 (gaps) and §6 (recommendations).

### 3.1 rusty_term — the terminal

**What it is.** A from-scratch VT/ANSI terminal emulator with a deliberately tiny dependency
surface (its PNG/JPEG/base64/inflate decoders are hand-rolled; the config parser is a
dependency-free TOML subset). It runs in two modes: a default **TUI/passthrough mode** that
parses the child's output into an internal grid and re-emits ANSI into the host terminal
(tmux-like relay), and a **native window mode** behind the `gui`/`gui-gpu` Cargo features
(winit + softbuffer CPU renderer, optional wgpu GPU renderer), selected with `--gui`. The
window mode is the one relevant to naner: a standalone window that could stand in for
Windows Terminal. Platform seams are clean: a `Backend` trait with `UnixBackend`
(openpty/fork) and `WindowsBackend` (ConPTY) implementations, and a single tokio reactor on
every platform (`src/runtime/`).

**Maturity.** The strongest of the three by a wide margin: ~19k LOC, 70 commits, 405 tests,
candid docs (`docs/FEATURES.md`, `docs/repo-analysis.md`,
`docs/research/implementation-status.md`) that label gaps as gaps. **Windows support is
real, not aspirational**: the ConPTY backend is documented as run and verified on Windows 11,
and the window backend (CPU and GPU) has been exercised on Windows 11 hardware. The window
mode has tabs, split panes, GSUB ligature shaping (CPU renderer), pixel-perfect
Sixel/Kitty/iTerm2 inline images (CPU renderer; GPU and TUI fall back to half-blocks),
system clipboard + OSC 52, scrollback search, IME, OSC 8 hyperlinks, desktop notifications,
15 theme presets, rebindable keys, live config reload, and an in-app settings page. Known
soft spots: the GPU renderer lacks ligatures and pixel images (the full-featured renderer
is the CPU one), and the docs disagree with themselves about Windows host-resize
propagation — `docs/FEATURES.md` marks it done (a 150 ms poll driving
`ResizePseudoConsole`, and the code path exists in `src/runtime/tokio_rt.rs`), while the
README and `implementation-status.md` still list it as a gap. Needs verification on a real
Windows box; treat the "gap" wording as probably stale.

**The l13 side channel.** Behind an off-by-default `l13` feature, rusty_term implements a
full-duplex JSON-RPC 2.0 transport over a private OSC (`OSC 5379`), in-band on the child's
PTY (`src/core/channel.rs`). On it it hosts: a version-negotiation protocol, a **complete
MCP server** exposing the terminal to the child process as tools and resources
(`get_screen`, `get_scrollback`, `get_cwd`, `get_title`, `get_dimensions`, `get_cursor`;
`terminal://…` resources with subscribe/push wired to the OSC 133 command lifecycle,
including a typed `command_finished { exit }` notification), a terminal-owned status-line
overlay, and stub `lsp`/`acp` handshakes. This is where rusty_lsp enters: the `l13` feature
pulls `rusty_lsp = { path = "../rusty_lsp" }` for its JSON-RPC message model and LSP types
only. It also ships composability assets in `extra/`: a self-describing terminfo entry and
OSC 133 shell-integration scripts for bash/zsh/fish/pwsh.

**Unix-philosophy alignment: strong.**
- *One thing well* — it is only a terminal; shell detection is a probe, not a bundled shell.
- *Text streams* — TUI mode's entire output **is** an ANSI stream into the host terminal;
  diagnostics and config warnings go to stderr; `--list-shells` prints to stdout and exits.
- *Mechanism, not policy* — a flat, dependency-free config file holds policy; a malformed
  config warns and never blocks startup; 15 themes are data, not code.
- *Composability hooks* — terminfo entry, shell-integration scripts, `RUSTY_TERM_CONFIG`
  env override, and the l13/MCP channel (a machine interface to terminal state).
- Noted deviation: none structural. The gaps in §5.1 are missing mechanism (flags), not
  philosophy violations.

**Ecosystem role:** the eventual launched terminal — naner's replacement for `wt.exe`.
Gaps for that role in §5.1.

### 3.2 rusty_lsp — the protocol substrate

**What it is.** A library-only async LSP **server framework**: "own the protocol plumbing;
implement one trait for your language." A consumer implements the `LanguageServer` trait
(`src/service.rs`) — only `initialize` is required; every other handler has a sane default —
and runs `Server::stdio()` or `Server::new(reader, writer)` over any
`AsyncRead`/`AsyncWrite` pair. The layering is clean and acyclic: `error` ← `jsonrpc` (a
standalone JSON-RPC 2.0 codec with **zero LSP coupling**) ← `transport` (Content-Length
framing over any async byte stream, 256 MiB frame guard) — then, separately, typed `lsp/`
data structures with escape hatches for everything unmodeled, and `server`/`client`/
`service` on top (lifecycle enforcement, in-order notifications, spawned requests,
exactly-once cancellation semantics). Dependencies are exactly tokio + serde + serde_json;
there is no platform-specific code at all — it is fully Windows-compatible.

**Maturity.** One commit, but a disciplined one: 16 unit tests, 11 end-to-end integration
tests over in-memory duplex pipes, a complete 267-line runnable example server, and
first-rate rustdoc/README. It is not on crates.io and is consumed only as a path/git
dependency; its Cargo.toml `repository` URL is a placeholder. It provides no LSP *client*
(its `Client` type is the server's outbound handle) and no TCP convenience constructor —
both are non-gaps for its current ecosystem roles.

**Unix-philosophy alignment: strong.** Small (one trait to implement), does one thing
(protocol plumbing, explicitly not a language server), and — the key property — exposes its
lower layers for independent reuse: `jsonrpc` and `transport` have no dependency on `lsp`,
which is exactly how rusty_term consumes it. Library-as-toolbox rather than framework
lock-in.

**Ecosystem role:** shared protocol substrate. Today: the JSON-RPC model under rusty_term's
l13 channel. Tomorrow: the base for any LSP/agent-protocol work in the ecosystem (e.g. a
config-file language server, or the l13 `lsp`/`acp` backends when they grow past
handshakes). Gaps in §5.3.

### 3.3 rush — the shell

**What it is.** A bash-compatible POSIX-ish shell in ~4.4k LOC with only two dependencies
(rustyline for line editing; libc, Unix-only, for job control). It is both an interactive
REPL (history in `~/.rush_history`, multi-line continuation, Ctrl-C/D/Z) and a script
interpreter (`rush file.sh`, `rush -c "…"`). The language surface is genuinely broad for
its age: functions with recursion and positional params, hand-rolled globs, `$((…))`
arithmetic, the full common expansion set (`${VAR:-…}` and friends, `"$@"`, tilde, command
substitution, word splitting), `if`/`while`/`until`/`for`/`case`, redirection including
`2>&1`, fd duplication and here-docs, and real Unix job control (process groups,
`tcsetpgrp`, fg/bg/jobs/kill, Ctrl-Z) behind `#[cfg(unix)]`. The architecture is a clean
linear pipeline (lexer → parser → expand → exec) documented with diagrams in
`docs/ARCHITECTURE.md`.

**Platform support: Unix-first, Windows-degraded by design.** CI builds and tests on both
ubuntu and windows. On native Windows it runs as a foreground-only shell: the job-control
module is compiled out, background `&` is rejected at runtime with a clear message, and
`fg`/`bg`/`jobs`/`kill` are absent. Under MSYS2 (a `cfg(unix)` target), the full job-control
path would compile — but that configuration is untested. Tilde expansion falls back to
`%USERPROFILE%` and `\r` is stripped for Windows line endings — small deliberate
Windows-aware touches.

**Maturity: young and honest.** 26 commits over ~3 weeks, v0.1.0, 63 unit tests
concentrated in the parser/lexer/expander — and **zero tests on the runtime**
(`exec.rs`, `job.rs`). Its own CHANGELOG candidly documents the sharp edges: subshells
save/restore state rather than fork (so `exit` in a subshell exits the whole shell),
`cmd 2>&1 | next` leaks stderr to the terminal, compound commands can't appear inside
pipelines or command substitution, a builtin mid-pipeline is punted, no `cd -`. Missing for
daily-driver status: tab completion (rustyline's completer is not wired), aliases,
rc/profile sourcing, prompt customization, `set -e`/`trap`, arrays, `getopts`.

**Unix-philosophy alignment: good instincts, incomplete mechanics.** Does one thing; tiny
dependency footprint; disciplined exit codes (parse errors exit 2, exec errors 1, `$?`
threaded properly, `&&`/`||` short-circuit on status); honest self-documentation of
limitations. The fork-less subshell and stderr-pipe leak are real composability violations —
known, documented, and on its own roadmap.

**Ecosystem role:** a vendored, opt-in shell payload — a profile alongside PowerShell,
bash, and cmd, not a replacement for them. Gaps in §5.2.

---

## 4. Integration architecture

### 4.1 The three contracts

Integration between these projects reduces to three seams. Naming them precisely is most of
the design work; each is intentionally an args-and-env seam, not a shared-config or
shared-code seam.

**(a) Launcher → terminal.** What naner needs from *any* terminal it launches, derived from
the existing invocation (MIGRATION_ANALYSIS §1.3):
`wt.exe --<launchMode> --title "<name>" --startingDirectory "<dir>" -- "<shell>" <args>`,
spawned fire-and-forget. As an abstract contract:

| Requirement | wt.exe today | rusty_term today |
|---|---|---|
| Settable starting directory | `--startingDirectory` | **missing** — child inherits rusty_term's process cwd; ConPTY spawn passes `lpCurrentDirectory = NULL` (`src/backend/windows.rs`). Workaround: naner sets the *spawned process's* cwd — viable but leaks into new tabs (see §5.1) |
| Settable title | `--title` | **missing** — title comes only from the child's OSC 0/2, default `"rusty_term"` |
| Shell command + args passthrough | `-- "<shell>" <args>` | **partial** — config `shell = "…"` string; trailing args honored on Windows (passed to `CreateProcessW` verbatim) but **not split on Unix** (whole string becomes the execvp program name) |
| Launch mode (default/maximized/fullscreen) | `--<launchMode>` | **missing** — `[window] cols/rows` in config only |
| Detachable fire-and-forget spawn | yes | yes (a normal GUI process) |
| Config injectable by launcher | settings.json template, `.portable` marker | **better**: `--config <path>` / `$RUSTY_TERM_CONFIG` — naner can generate a config and point at it, no post-install templating needed |

rusty_term's actual CLI is `--gui`, `--gpu`, `--config <path>`, `--list-shells`
(`src/main.rs`) — the delta against this contract is the top of §5.1's backlog. Note the
last row: the WT-specific post-install machinery naner carries (the `.portable` marker and
`settings.json` templating with `%NANER_ROOT%` substitution, plus the special
update-preserves-settings path) **has no rusty_term equivalent and needs none**. A
generated TOML file in `config/` plus one env var replaces all of it — a genuine
simplification, and an argument for the flags-not-profiles stance in §6.

There is one naner-side item hiding in this contract: today's `TerminalLauncher` knows only
`wt.exe` (discovery chain `VendorPaths["WindowsTerminal"]` → PATH → WindowsApps). Launching
anything else requires a terminal abstraction — an additive config surface (e.g. a
per-profile `Terminal` field defaulting to `WindowsTerminal`) plus per-terminal argument
mapping. Because it is additive and absent-by-default, it is parity-safe under
MIGRATION_ANALYSIS §2.4 tier 2 rules — but it should still wait until the Phase 2 launcher
port is green rather than complicate it (§4.3).

**(b) Terminal → shell.** rusty_term selects its child via config `shell` / `$SHELL` /
`%COMSPEC%` with a probe-based default (`src/shells.rs`; `--list-shells` to inspect). For
rush to be that child: on Windows it works today as a foreground-only shell; under MSYS2
the full-featured build applies (untested). The richer integration — OSC 133 command
lifecycle, which lights up rusty_term's prompt-aware features and the MCP command events —
runs through rusty_term's shell-integration scripts (`extra/shell-integration/`), which are
sourced from a shell's rc file. **rush cannot source rc files yet**, so
rush-inside-rusty_term stays "basic child process" until rush grows startup-file support
(§5.2). Nothing blocks pwsh/bash/zsh/fish integration today — the scripts already exist.

**(c) Protocol substrate.** rusty_lsp's `jsonrpc` + `transport` layers are the reusable
piece — zero LSP coupling, generic over any async byte stream — and rusty_term's l13
channel already consumes them. The dependency is currently a **path dep on a sibling
checkout** (`rusty_term/Cargo.toml`: `rusty_lsp = { path = "../rusty_lsp" }`), which breaks
any standalone build of rusty_term with `l13` enabled and blocks release CI. Fixing the
form of this dependency (git tag now, crates.io later) is the one cross-repo mechanical
change this document asks for (§5.3, §6).

### 4.2 The integration ladder

Integration proceeds through criteria-gated levels. **No dates** — three of the four repos
have no release process yet, so a dated roadmap would be fiction. Each level names its
entry criteria; hitting them *is* the schedule.

**Level 0 — today.** `naner (C#) → wt.exe → pwsh/bash/cmd`. The migration
(MIGRATION_ANALYSIS §6) proceeds independently. Ecosystem repos close gaps on their own
schedules.

**Level 1 — rusty_term is manually daily-drivable inside a naner environment.**
A user runs `naner --export-env | iex` (or launches from a naner shell) and starts
`rusty_term --gui` by hand; env and PATH flow through naturally since rusty_term passes its
environment to the child untouched.
*Entry criteria:* launch-contract flags implemented in rusty_term (`--cwd`, `--title`,
command/args — §5.1 items 1–3); a tagged rusty_term release with a prebuilt `win-x64` zip;
license file. *Requires nothing from naner.*

**Level 2 — rusty_term is a vendored terminal and an opt-in profile.**
`vendors.json` gains a `RustyTerm` entry using the existing `github` source type
(`repo: baileyrd/rusty_term`, `assetPattern: *win-x64.zip`, pinned fallback — the exact
shape of the existing PowerShell/WT entries); naner.json profiles gain a terminal-selection
field; `wt.exe` remains the default.
*Entry criteria:* Level 1 criteria + naner migration Phase 3 complete (Rust vendor pipeline
proven) + the naner-side terminal abstraction from §4.1(a). *This is the first level that
touches naner, and it is additive config only.*

**Level 3 — rush is a vendored, experimental shell profile.**
`vendors.json` gains a `Rush` entry (same `github` mechanics); naner.json gains an
opt-in profile (`Shell: rush`, clearly labeled experimental). naner's multi-profile design
makes this near-free — it exercises the vendor pipeline without betting the default shell
on a three-week-old codebase.
*Entry criteria:* rush "vendorable" tier complete (§5.2 tier 1: license, tagged release
with Windows artifact, honest status block) + naner Phase 3 complete. Independent of
Levels 1–2.

**Level 4 — the full-Rust stack is the default, and l13/MCP diagnostics get explored.**
Default profile becomes rusty_term-hosted; wt.exe stays available as a profile
indefinitely (it is just a vendor + profile, not a hard dependency). The speculative
synergies in §6 (MCP-driven `naner doctor`, parity harness) become worth spiking only here.
*Entry criteria:* migration Phase 5 cutover done; Level 2 running without regressions long
enough to trust; rusty_term Windows resize/fidelity confirmed on real trees; an explicit
user decision (§8.3).

### 4.3 Sequencing against the migration

The one bolded rule: **no ecosystem work item may become a dependency of migration
Phases 0–5.** The migration is parity-first against wt.exe-launching C# behavior; adding a
second launch target mid-parity would double the behavioral test surface for zero parity
value.

| Ecosystem milestone | Earliest sensible start | Why |
|---|---|---|
| rusty_term / rush / rusty_lsp gap work (§5) | **now, anytime** | Independent codebases; zero migration risk |
| rusty_lsp dependency fix (path → git tag) | now | Unblocks rusty_term release CI; no naner involvement |
| Level 1 (manual use) | as soon as criteria met | Requires nothing from naner |
| naner terminal abstraction (additive config) | after Phase 2 exits green | Parity-safe but shouldn't complicate the launcher port |
| Level 2/3 vendors.json + profiles | after Phase 3 | Needs the Rust vendor pipeline to exist and be proven |
| Level 4 default flip | strictly after Phase 5 cutover | Never change the default terminal and the launcher implementation in the same wave |

---

## 5. Gap backlogs

Priority-ordered per project. Size guesses: S < 1 day, M = days, L = a week+.

### 5.1 rusty_term

> Full per-gap specs (current behavior with `file:line` evidence at HEAD
> `78a5a92`, targets, implementation sketches, acceptance criteria, sequencing):
> [docs/rusty_term-gaps.md](docs/rusty_term-gaps.md).

| # | Gap | Why it matters | Unblocks | Size |
|---|---|---|---|---|
| 1 | `--cwd <dir>` (or `--starting-directory`): plumb through `Backend::spawn_shell` and pass to `CreateProcessW`'s `lpCurrentDirectory` / chdir-after-fork | The launcher contract's biggest hole; profile `StartingDirectory` is core naner behavior | Level 1 | S–M |
| 2 | `-e/--command "<shell> <args>"` (or positional trailing args) | Lets naner pass the profile's `CustomShell.ExecutablePath + Arguments` without generating a config file per launch | Level 1 | S–M |
| 3 | `--title <t>`: set initial window title, child OSC still wins afterwards | Profile `Name` maps to window title today | Level 1 | S |
| 4 | Unix shell-arg splitting (config `shell = "bash --login -i"` currently execvp's the whole string as argv[0]) | Cross-platform consistency of the terminal→shell contract; Windows already honors args | correctness | S |
| 5 | New tabs/panes inherit the focused pane's cwd (or at least the `--cwd` value) instead of the process cwd | With the §4.1 workaround (naner sets process cwd) this is masked; with `--cwd` it becomes visible | polish for L2 | M |
| 6 | Launch modes: `--maximized` / `--fullscreen` | naner's `LaunchMode` config field; winit supports both | Level 2 | S |
| 7 | Reconcile the Windows host-resize documentation conflict (FEATURES.md "done" vs README/implementation-status "gap") and verify `resize_poll` → `ResizePseudoConsole` on a real Windows box | It gates daily-driver trust on Windows; the code path exists, the docs disagree | Level 1 confidence | S (verify) |
| 8 | Distribution: LICENSE file + `license` metadata; CI (fmt/clippy/test, ubuntu + windows); tagged releases with a prebuilt `win-x64` zip artifact | naner's `github` vendor type consumes exactly this; today there is nothing to vendor | Level 1–2 | M |
| 9 | Replace `rusty_lsp = { path = "../rusty_lsp" }` with a git dependency pinned to a tag | `l13` builds currently require a sibling checkout; blocks any release CI | release hygiene | S (after rusty_lsp tags) |

Explicitly **not** needed: a Windows-Terminal-style profiles system. naner owns the profile
concept (naner.json already models Name/Shell/StartingDirectory/LaunchMode/ColorScheme) and
maps a profile to flags at spawn time. Building a second profile layer inside rusty_term
would duplicate policy the launcher already holds — see §6 recommendation 3.

### 5.2 rush

> Full per-gap specs (current behavior with `file:line` evidence at HEAD
> `fc13153`, targets, implementation sketches, acceptance criteria, sequencing):
> [docs/rush-gaps.md](docs/rush-gaps.md).

Two tiers, so rush can enter the ecosystem early without overpromising.

**Tier 1 — "vendorable as an experimental shell"** (unblocks Level 3):

| # | Gap | Size |
|---|---|---|
| 1 | LICENSE file + Cargo license metadata | S |
| 2 | Tagged release + CI release job producing a Windows artifact (CI matrix already builds windows-latest) | S–M |
| 3 | An honest status block in README (what works, what's foreground-only on Windows) — naner will label the profile "experimental"; the README should say the same | S |
| 4 | `cd -` (small, disproportionately annoying absence for interactive use) | S |

**Tier 2 — "daily driver"** (gates any thought of default-shell status; sequenced by rush's
own roadmap, not naner's):

| # | Gap | Size |
|---|---|---|
| 5 | Tab completion (rustyline `Completer` — files, commands, builtins) | M–L |
| 6 | Startup files (`~/.rushrc` or similar) — also the prerequisite for rusty_term OSC 133 shell integration (§4.1b) | M |
| 7 | Prompt customization (PS1 or equivalent) | S–M |
| 8 | Aliases; `set -e`; `trap` | M–L |
| 9 | Test coverage for the runtime: `exec.rs` and `job.rs` currently have zero tests | M |
| 10 | Fix `2>&1 \|` fd semantics; real forked subshells/compounds in pipelines | L |
| 11 | Validate the MSYS2 (`cfg(unix)`) build — job control would compile there but is untested; decide the Windows strategy (§8.4) | M |

### 5.3 rusty_lsp

> Full per-gap specs (current behavior with `file:line` evidence at HEAD
> `2b0a528`, targets, acceptance criteria, sequencing):
> [docs/rusty_lsp-gaps.md](docs/rusty_lsp-gaps.md).

| # | Gap | Why | Size |
|---|---|---|---|
| 1 | Fix the placeholder `repository` URL (`github.com/example/rusty_lsp` → `github.com/baileyrd/rusty_lsp`) | Broken metadata; blocks crates.io | S |
| 2 | Add LICENSE-MIT / LICENSE-APACHE files (Cargo.toml already declares the dual license) | Legal hygiene; blocks crates.io | S |
| 3 | Tag `v0.1.0` | Gives rusty_term a pin target for §5.1 item 9 | S |
| 4 | CI (fmt/clippy/test on ubuntu + windows — the code is fully portable, prove it continuously) | Library consumers need green CI | S–M |
| 5 | Decide crates.io vs pinned-git-tag distribution (§8.1) | Determines the endgame for the dependency graph | decision |

Non-gaps, deliberately: no LSP client implementation and no TCP convenience constructor —
neither current consumer needs them; adding surface ahead of need would cut against the
library's own philosophy.

---

## 6. Cross-cutting recommendations

1. **Ecosystem is a parallel track; the migration's phases are untouched.** The load-bearing
   commitment of this document (rule and table in §4.3).
2. **Integration is criteria-gated, not dated** (§4.2). Progress is checkable: each level's
   entry criteria either hold or don't.
3. **rusty_term integrates via CLI flags, not a profiles system.** naner keeps sole
   ownership of profiles (mechanism-not-policy on both sides: naner holds policy, rusty_term
   provides mechanism). Bonus: the entire WT post-install/settings-preservation special case
   in the vendor pipeline has no rusty_term analogue — one generated config file and
   `RUSTY_TERM_CONFIG` replace it.
4. **Distribution rides the existing `github` vendor source type — no new vendor
   machinery.** Each project adds license + tags + CI release zips; vendors.json entries
   then look exactly like today's PowerShell entry. (This also sidesteps migration bug B1's
   territory: pinned fallback URLs per vendors.json convention until B1 is fixed
   post-parity.)
5. **Kill the path-dep now.** rusty_term → rusty_lsp becomes a git dependency pinned to a
   tag; crates.io publication is the endgame once rusty_lsp's API is stable enough to honor
   semver. rusty_lsp is the only genuine library in the family and the natural first
   publish.
6. **rush enters early as an opt-in experimental profile; daily-driver is a separate,
   later milestone.** The two-tier backlog in §5.2 encodes this.
7. **Do not harmonize config formats.** naner JSON/YAML, rusty_term TOML-subset, rush none —
   each is idiomatic for its tool, and the integration seam is spawn-time flags + env vars,
   not shared config. A unified config schema would couple release cadences for near-zero
   benefit.
8. **Keep five separate repos; no monorepo.** Independent cadence, and naner vendors
   *released artifacts*, not source. The one real source-level coupling
   (rusty_term↔rusty_lsp) is fixed by recommendation 5, not by merging repos.
9. **l13/MCP synergy stays exploratory.** Recorded, not roadmapped — see below.

**Speculative synergies (ideas on file, no commitment):**
- *`naner doctor` over MCP:* with l13 enabled, a diagnostics command could query the live
  terminal (screen, cwd, dimensions, last command's exit code) through the OSC 5379 channel
  instead of guessing from the outside.
- *MCP as a migration parity harness:* rusty_term's OSC 133 command-lifecycle tools emit
  typed `command_finished { exit }` events — a scriptable way to drive "launch naner,
  observe the resulting environment, compare C# vs Rust" end-to-end tests during Phase 5
  testing. The one place ecosystem work could *help* the migration; worth a small spike
  then, not before (§8.7).
- *A naner-config language server on rusty_lsp:* completion/diagnostics for naner.json and
  vendors.json (schema-driven). Cheap to build on rusty_lsp's example-server skeleton, but
  only worth it if config editing becomes a real user pain point.

---

## 7. Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Ecosystem scope creeps into the migration (the "while we're in the launcher, let's add rusty_term support" trap) | medium | high — parity slips | The §4.3 rule; terminal abstraction explicitly deferred past Phase 2 |
| rush is 3 weeks old with an untested runtime (`exec.rs`/`job.rs`: 0 tests) | high (that's what young means) | low if opt-in experimental; high if default | Two-tier backlog; "experimental" labeling; never default until tier 2 |
| ConPTY/GUI fidelity vs Windows Terminal expectations (fonts, resize, edge-case apps) | medium | medium — user-visible regressions on switch | Level gates; wt.exe remains a profile forever; resize verification (§5.1.7) before Level 1 is declared |
| The polished rusty_term path is the CPU renderer; GPU lacks ligatures/pixel images | known | low — CPU renderer is the default recommendation | Document per-renderer expectations when writing the naner profile docs |
| Single maintainer across four active codebases (bus factor / attention dilution) | high | medium | The ladder localizes work: each level needs only one repo's gaps closed |
| No releases/licenses today — distribution story is entirely unbuilt | certain | blocks Levels 1–3 | §5 items are all S/M; do license+tag+CI first in each repo |
| l13 protocol is private and unstable (stub lsp/acp, off-by-default) | known | low — nothing depends on it | Keep it out of every level's entry criteria (it appears only in Level 4 exploration) |
| 0.1.0 version numbers understate maturity and will confuse vendors.json pins later | low | low | First tagged release per repo picks an honest version (§8.6) |

---

## 8. Open decisions

Mirrors MIGRATION_ANALYSIS §7's format: each with a recommended default; none blocks work
that precedes it.

1. **rusty_lsp distribution** — crates.io now vs pinned git tag. *Default: git tag now,
   crates.io when the API has survived a second consumer.*
2. **rusty_term launch-contract shape** — minimal flags vs a `--profile <file>` vs
   env-var-driven. *Default: minimal flags (§5.1 items 1–3); flags compose with the
   existing config file, and naner already owns profiles.*
3. **When rusty_term becomes naner's default terminal** — criteria live in Level 4, but the
   flip is a product call. *Default: not before the migration cutover has been stable for a
   release cycle; wt.exe stays a profile regardless.*
4. **rush's Windows strategy** — native-Windows foreground-only vs targeting the MSYS2
   environment naner already vendors (full job control, untested). *Default: keep native
   Windows as the supported degraded mode; validate MSYS2 opportunistically — naner profiles
   can offer both.*
5. **Naming** — keep `rusty_*` vs converge on a `naner-*` family. Cosmetic, but it affects
   vendors.json `extractDir` names and docs. *Default: keep current names; revisit at
   Level 4.*
6. **Version renumbering on first tags** — rusty_term at 0.1.0 with 19k LOC and 405 tests
   understates itself. *Default: tag what's true (e.g. 0.9.x for rusty_term if it's
   near-daily-drivable, honest 0.1.0 for the others).*
7. **Spike the MCP parity-harness idea during Phase 5 testing?** *Default: timebox a
   half-day spike when Phase 5 starts; drop it without ceremony if it doesn't pay
   immediately.*

---

## 9. Appendix: sources

Snapshot commits: `naner @ 4f7c623`, `rusty_naner @ 59e63e1`, `rusty_term @ 78a5a92`,
`rusty_lsp @ 2b0a528`, `rush @ fc13153` (all on branch `claude/naner-rust-migration-9frq5c`
at analysis time).

Key files consulted:
- `rusty_naner/MIGRATION_ANALYSIS.md` — §1.3 (launch/CLI contract), §2.4 (Unix-philosophy
  tiers and vocabulary), §6 (phases), §7 (decisions)
- `naner/config/naner.json` (profile schema: Shell/StartingDirectory/CustomShell/…),
  `naner/config/vendors.json` (vendor source types; the `github` + `assetPattern` +
  fallback shape Levels 2–3 reuse)
- `rusty_term/src/main.rs` (CLI surface, TERM/COLORTERM, mode dispatch),
  `src/config.rs` (TOML-subset format + discovery order), `src/shells.rs`,
  `src/backend/{mod,unix,windows}.rs` (spawn contract, `lpCurrentDirectory = NULL`),
  `src/core/channel.rs` (l13/MCP), `Cargo.toml` (features, path dep),
  `docs/FEATURES.md`, `docs/repo-analysis.md`, `docs/research/implementation-status.md`,
  `extra/` (terminfo, shell-integration)
- `rusty_lsp/src/{lib,jsonrpc,transport,server,client,service,text,error}.rs`,
  `src/lsp/`, `tests/integration.rs`, `examples/text_server.rs`, `Cargo.toml`
- `rush/src/{main,lexer,parser,expand,exec,job,builtins,vars,func,glob,arith}.rs`,
  `docs/ARCHITECTURE.md`, `CHANGELOG.md`, `.github/workflows/ci.yml`
