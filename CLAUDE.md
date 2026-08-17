# Standing instructions for Claude

## Git workflow

- Always open a PR for any change, however small — never push directly to
  `main`, even when working on a branch that already exists for the task.
- After opening a PR, merge it once it's ready (green CI, template filled in)
  and sync the local checkout to the merged `main` — don't leave a merged
  PR's branch as the last local state.
- Merge with a **merge commit** (not squash, not rebase) — see
  [CONTRIBUTING.md](./CONTRIBUTING.md#review--merge) for why.
- Use `.github/PULL_REQUEST_TEMPLATE/vendor_change.md` for any change to
  `dist-assets/config/vendors.json`; the default template otherwise.
