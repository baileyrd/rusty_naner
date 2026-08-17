<!-- Use when cutting a release. Append ?template=release.md to the PR URL. -->

## Version

- Tag to be pushed: `v`
- `workspace.package.version` in `Cargo.toml`:

These two **must** match — a deployed `naner-init` looks up the GitHub release whose
tag equals its own embedded version, and strands if they drift. The release workflow
enforces this, but confirm it here before tagging.

## Pre-tag checklist

- [ ] `RELEASE_NOTES.md` has an entry for this version, with known limitations stated
- [ ] `CHANGELOG.md` `[Unreleased]` section rolled into the new version heading
- [ ] Validation gates in [docs/VALIDATION.md](../../docs/VALIDATION.md) signed off
      on a real Windows box
- [ ] CI green on both `ubuntu-latest` and `windows-latest`

## Release assets

The workflow publishes `naner.exe`, `naner-init.exe`, `naner-bundle.zip` and
`SHA256SUMS` to this repo. `naner-init` **fails closed** without the
manifest, so a release missing `SHA256SUMS` is uninstallable, not merely unverified.

- [ ] Asset names unchanged from the previous release
- [ ] `SHA256SUMS` present and covering all three binaries

## Upgrade notes

<!-- Anything a user upgrading from the previous version must know. "None" is fine. -->
