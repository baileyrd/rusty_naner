# rusty_naner

Rust migration of [naner](https://github.com/baileyrd/naner), the portable terminal
environment launcher for Windows.

## Status

Phase 0 (scaffolding + console spike) in progress: Cargo workspace with
`naner-core` / `naner` / `naner-init`, the Windows console attach/alloc/
pipe-detection spike, CI (fmt + clippy + test on Linux and Windows), a draft
release workflow with the tag == version guard, and the golden-parity harness
(`scripts/parity.ps1`). The console spike still needs manual validation of the
four launch modes on a real Windows box before Phase 1 work lands on top of it.

- [MIGRATION_ANALYSIS.md](MIGRATION_ANALYSIS.md) — detailed analysis of the existing C#
  codebase and the phased migration plan.
- [ECOSYSTEM.md](ECOSYSTEM.md) — assessment of the sibling Rust projects
  ([rusty_term](https://github.com/baileyrd/rusty_term),
  [rusty_lsp](https://github.com/baileyrd/rusty_lsp),
  [rush](https://github.com/baileyrd/rush)) and the roadmap for integrating them with
  naner into a full-Rust terminal environment.
