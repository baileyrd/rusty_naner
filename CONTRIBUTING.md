# Contributing

## Before you start
- Match surrounding conventions when editing existing code.
- Keep diffs focused — one logical change per PR.
- For large or hard-to-reverse changes (schema/data migrations, public API changes,
  deletions, dependency/toolchain bumps), open an issue or draft PR to discuss first.

## Workflow
1. Branch off the default branch.
2. Make your change. State the *why* in commit messages or PR description for any
   non-obvious decision.
3. Add tests for non-trivial logic — happy path and at least one failure/boundary case.
   Spikes/prototypes are exempt but should say so in the PR.
4. Add or update doc comments (`///`, `//!`) on any public surface you touched.
5. Run the gates locally before pushing — they are exactly what CI runs:
   ```
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
   Tests that hit the network are `#[ignore]`d and excluded from CI; run them with
   `cargo test -- --ignored` when touching a vendor resolver, since they are the
   only check that catches an upstream manifest changing shape.

   If you add or bump a dependency, also run the supply-chain gate:
   ```
   cargo deny check
   ```
   It checks advisories, licences and sources against `deny.toml`. A new
   dependency under a licence not yet in the allow list will fail it — add the
   licence in the same PR, so the decision is reviewed alongside the dependency.
6. Open a PR and fill in the template, including how you verified the change.
   The default template covers most changes. Two specialised ones exist for cases
   with different verification obligations — append the query parameter to the PR
   URL to pick one:
   - `?template=vendor_change.md` — adding or changing a `vendors.json` entry
   - `?template=release.md` — cutting a release

## Code style
- Explicit over implicit. Prefer a named type or a small struct over a tuple or a
  bare `bool` parameter whose meaning is only clear at the call site.
- Flat control flow — guard clauses, early returns, avoid >3 levels of nesting.
- Short, single-purpose functions.
- Minimal dependencies — justify any new third-party one in the PR description.
- Never commit or log secrets/credentials. Validate external input at the boundary.
- Never silently discard an error. `let _ = fallible()` on a path where failure
  changes the outcome is a bug, not a style choice — handle it, propagate it with
  context, or log it loudly. Several real defects in this repo were exactly this.
- `unwrap()` / `expect()` are for cases that are provably infallible (a fixed-size
  slice conversion) or for tests. On a user-facing path they abort the process:
  `naner.exe` builds with `panic = "abort"` and `windows_subsystem = "windows"`, so
  a panic makes a GUI launch vanish with no message at all.

## Review & merge
- Every change lands through a PR — no direct pushes to the default branch.
- CI must be green before merge.
- At least one approval required (see CODEOWNERS if present).
- Reviewers: check for scope creep, missing tests, and unexplained non-obvious decisions.
- Merge with a **merge commit** ("Create a merge commit" — merge and sync). Do **not**
  squash-merge or rebase-merge: full commit history is preserved deliberately.
