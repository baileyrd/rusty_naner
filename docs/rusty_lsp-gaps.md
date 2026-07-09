# rusty_lsp gaps — full definitions

Actionable expansion of [ECOSYSTEM.md §5.3](../ECOSYSTEM.md). rusty_lsp is the
**protocol substrate**: its `jsonrpc` + `transport` layers are the reusable piece,
consumed today only by rusty_term's `l13` side-channel (ECOSYSTEM §4.1c). **naner
never touches it.** So these gaps don't gate any naner integration level directly —
they gate rusty_term's release hygiene (rusty_term G9 needs a tag to pin).

**Verified against** `baileyrd/rusty_lsp` @ `2b0a528` (2026-06-02, the same HEAD
ECOSYSTEM.md is pinned to — the repo has not moved). Gap IDs match §5.3's table.

## Current surface (baseline)

- A disciplined one-commit library: `src/{jsonrpc,client,error,service,text}.rs` +
  `src/lsp/{base,diagnostics,document,enums,features,lifecycle}.rs`, an
  `examples/text_server.rs`, and `tests/integration.rs`.
- **Well tested** — 27 tests (16 unit + 11 integration; `tests/integration.rs:11`,
  `jsonrpc 7`, `text 5`, `enums 4`). **No functional or test-coverage gap.**
- **Fully portable** — pure async/tokio over any byte stream, no platform code.
- Every gap below is **distribution/metadata hygiene**, not missing capability.

All five gaps are S / S–M / a one-time decision. This is the smallest of the three
sibling backlogs.

---

## G1 — Fix the placeholder `repository` URL

**Current.** `Cargo.toml:7` — `repository = "https://github.com/example/rusty_lsp"`
(the `cargo new` placeholder, never updated).

**Target.** `repository = "https://github.com/baileyrd/rusty_lsp"`.

**Acceptance.** `cargo metadata` shows the real URL; a would-be `cargo publish`
doesn't point consumers at a dead repo.

**Why** broken metadata; blocks crates.io · **Size** S · **Deps** none.

## G2 — Add LICENSE-MIT and LICENSE-APACHE files

**Current.** `Cargo.toml:6` declares `license = "MIT OR Apache-2.0"`, but **no
license files exist** (no `LICENSE*`/`COPYING*`). The declaration promises a dual
license the tree doesn't ship.

**Target.** Add both `LICENSE-MIT` and `LICENSE-APACHE` at the repo root (the
standard Rust dual-license pair), matching the declared SPDX expression.

**Acceptance.** Both files present; `cargo publish --dry-run` stops flagging the
missing license; the dual-license claim is backed by actual text.

**Why** legal hygiene; blocks crates.io and any redistribution · **Size** S · **Deps**
none.

## G3 — Tag `v0.1.0` (the keystone)

**Current.** No tags (`git tag` empty; no remote tags).

**Target.** An annotated `v0.1.0` tag on the released commit.

**Why it matters most.** It is the **pin target for rusty_term G9** — rusty_term's
`rusty_lsp = { path = "../rusty_lsp" }` becomes `{ git = "…", tag = "v0.1.0" }` only
once this tag exists. Without it, rusty_term's `l13` builds stay path-dep-bound and
its release CI stays blocked. This one S-sized action unblocks a cross-repo chain.

**Acceptance.** `git ls-remote --tags` shows `v0.1.0`; rusty_term can add a
`tag = "v0.1.0"` git dependency that resolves from a clean checkout.

**Size** S · **Deps** should land *after* G1+G2 so the tagged commit has correct
metadata and license files.

## G4 — CI (fmt / clippy / test on ubuntu + windows)

**Current.** No `.github/workflows/`. The code is fully portable but nothing proves
it stays green.

**Target.** A CI workflow running `fmt --check`, `clippy`, and `test` on
`ubuntu-latest` + `windows-latest`.

**Acceptance.** CI is green on both OSes; library consumers (rusty_term) can trust a
tagged commit builds and tests clean cross-platform.

**Why** library consumers need continuous green CI · **Size** S–M · **Deps** none
(parallel to G1–G3).

## G5 — Decide crates.io vs pinned-git-tag distribution

**Current.** Undecided (ECOSYSTEM §8.1). The dependency endgame for the whole graph
hangs on this: pinned git tags keep everything in-org and tag-gated; crates.io makes
rusty_lsp a first-class published crate (and requires G1+G2 to be publish-clean).

**Target.** An explicit decision recorded in ECOSYSTEM §8.1.

**Acceptance.** A one-line decision: "rusty_lsp distributes via `git` tag" **or** "via
crates.io," with rusty_term's dependency form following it.

**Type** decision (no code) · **Deps** informed by G1+G2 (crates.io needs them).

---

## Non-gaps (deliberately out of scope)

Confirmed against source, not just asserted:

- **No LSP *client* implementation.** `src/client.rs`'s `Client` (line 27) is the
  **server→editor handle** the framework hands your backend (`service.rs:30`), and
  `ClientInfo`/`ClientCapabilities` (`lsp/lifecycle.rs`) are protocol types describing
  a connected editor — none of this is a client-role tool that connects *to* a server.
  The §5.3 non-gap holds.
- **No TCP convenience constructor.** No `TcpListener`/`TcpStream`/`connect` in the
  tree — transport is generic over any async byte stream, which is the point. Neither
  consumer needs TCP; adding it ahead of need would cut against the library's
  minimal-surface philosophy.

---

## Sequencing

A tight, mostly-parallel bundle — no functional work, so this is a single small wave:

| Order | Gaps | Rationale |
|---|---|---|
| 1 | **G1 + G2** | Metadata + license correct *before* tagging, so the tag captures a publish-clean commit. |
| 2 | **G3** | Tag `v0.1.0` — the keystone that unblocks rusty_term G9. |
| ∥ | **G4** | CI, in parallel with G1–G3. |
| — | **G5** | The distribution decision; record in §8.1. Only G5 (crates.io branch) adds any later requirement beyond G1+G2. |

**Cross-repo chain this unblocks:** rusty_lsp G1+G2+G3 → rusty_term G9 (path dep →
`tag = "v0.1.0"`) → rusty_term's `l13` release CI (part of rusty_term G8). rush is
independent of all of this. naner is independent of all of this.

All five gaps live in the **rusty_lsp** repo.
