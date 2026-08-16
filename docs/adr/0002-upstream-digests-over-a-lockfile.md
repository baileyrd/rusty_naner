# ADR-0002: Verify vendor downloads against upstream digests, not a lockfile

Status: Accepted
Date: 2026-08-16

## Context

Nothing `naner` downloaded was verified beyond TLS transport trust. All twelve
vendors in `vendors.json` omitted the optional `checksum` object, so
`UnifiedVendorInstaller::verify_checksum` took its "no checksum provided" branch on
every install — including for the three artifacts that are *executed* rather than
unpacked (`rustup-init.exe`, the Miniconda installer, the 7-Zip MSI). `naner-init`
replaced the installed `naner.exe` with whatever bytes arrived.

The verifier itself was complete and correct. The gap was entirely in what it was
given to check against.

The hard part is that six of the twelve vendors resolve dynamically — the version,
file name and URL are only known at install time, and change whenever upstream
publishes. A digest hand-entered into `vendors.json` pins only the static
`fallback` URL, which is precisely the path that is *not* normally taken.

## Decision

Carry the digest the distributor already publishes, per resolver:

| Source | Digest | Mechanism |
| --- | --- | --- |
| `golang-api` | SHA-256 | already present in the `?mode=json` response |
| `nodejs-api` | SHA-256 | `SHASUMS256.txt` for the resolved release |
| `dotnet-api` | SHA-512 | channel `releases.json` (also the authoritative URL) |
| `static` (rustup) | SHA-256 | `.sha256` sidecar, via new `checksumSource` |
| `static` (Miniconda) | SHA-256 | repository listing, via new `checksumSource` |

Two supporting rules:

- A `checksum` pinned in `vendors.json` **outranks** any upstream digest. An
  operator pinning an artifact asserts something stronger than "whatever the
  distributor currently serves"; preferring upstream would let a compromised
  manifest overrule the pin.
- Upstream-derived digests are `required` (a mismatch blocks the install), unlike
  hand-entered ones which default to warn-only. They come from the distributor's
  own manifest for that exact artifact, so a mismatch means the bytes are wrong.

## Alternatives considered

**A lockfile (`naner.lock`), trust-on-first-use.** Record the resolved version, URL
and digest on first install; verify on every subsequent one. Attractive because it
covers *all twelve* vendors uniformly, including MSYS2 and the GitHub-sourced ones
that publish no digest — and because `naner-core/src/lockfile.rs` already sketches
exactly the right `{version, url, sha256}` shape.

It lost on the threat model, not the design. TOFU gives no protection on first
install, which is exactly the fresh-bootstrap case a new user hits — the one moment
where a bad artifact does the most damage and the user has the least ability to
notice. Upstream digests protect that case; a lockfile does not.

The two are complementary rather than exclusive, and the lockfile remains worth
building for the vendors this ADR cannot cover
([#20](https://github.com/baileyrd/rusty_naner/issues/20)).

**Hand-maintained digests for every vendor.** Rejected: for the six dynamic
resolvers it would pin only the fallback URL, and it puts a maintenance burden on
whoever bumps a vendor — one that fails silently by being skipped.

**Nothing beyond a size check.** Catches truncation, not substitution. Kept as a
cheap addition, not as the answer.

## Consequences

- Five of twelve vendors are now verified against a source-authoritative digest,
  including both executed `.exe` installers. The 7-Zip MSI is not.
- MSYS2 (no sidecar published — confirmed 404) and the six GitHub-sourced vendors
  install unverified unless pinned by hand. This is a real, stated gap, not an
  oversight.
- Each of the five adds at most one HTTP request per install; Go adds none.
- A distributor changing its manifest shape breaks digest resolution. That degrades
  loudly to an unverified install rather than blocking, and five `#[ignore]`d
  network tests exist specifically to catch the shape change.
- The `dotnet-api` resolver now follows the channel manifest for its URL instead of
  building one from the version string — better, but a second request and a new
  dependency on that manifest's structure.
