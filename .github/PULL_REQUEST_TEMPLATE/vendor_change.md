<!-- Use for adding or changing an entry in dist-assets/config/vendors.json.
     Append ?template=vendor_change.md to the PR URL to select this one. -->

## Vendor and change

- Vendor:
- Source type (`github` / `web-scrape` / `static` / `*-api`):
- What changed:

## Integrity

- [ ] The artifact is verified — either a resolver-supplied upstream digest, or a
      `checksum` pinned in `vendors.json`
- [ ] If it installs unverified, that is stated below with the reason (upstream
      publishes no digest)
- [ ] If this vendor is *executed* (`.exe` / `.msi`), the digest is `required`

Verification status:

## Verification

- [ ] `cargo test --workspace`
- [ ] `cargo test -- --ignored` — the network-dependent resolver tests, which are
      the only check that catches an upstream manifest changing shape
- [ ] `dist-assets/config/vendors.json` still validates against `vendors-schema.json`
- [ ] Installed for real on Windows

## Blast radius

<!-- Does this change what a deployed installation resolves on its next run?
     Does it affect the dependency order in `dependency_order`? -->
