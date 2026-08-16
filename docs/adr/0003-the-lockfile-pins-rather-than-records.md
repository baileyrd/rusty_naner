# ADR-0003: `naner.lock` pins installs; `update-vendors` rewrites it

Status: Accepted
Date: 2026-08-16

## Context

[ADR-0002](./0002-upstream-digests-over-a-lockfile.md) closed artifact
verification for the five vendors whose distributor publishes a digest, and
recorded that a lockfile was still worth building for the ones it could not
cover — MSYS2 and the six GitHub-sourced vendors, which publish nothing.

`naner-core/src/lockfile.rs` already existed with roughly the right shape
(`{version, url, sha256}` per vendor) but was reachable from nothing
([#20](https://github.com/baileyrd/rusty_naner/issues/20)).

Making it real forces a question the struct alone does not answer: what does an
entry *mean* at install time?

## Decision

An entry is a **pin**, not a record.

- `naner install` on a pinned vendor installs exactly the locked version and URL
  and verifies the locked SHA-256 as `required`. Resolution is skipped entirely —
  upstream is not consulted.
- A successful install of an *unpinned* vendor writes the pin, including the
  digest of the bytes that actually arrived.
- `naner update-vendors` / `update_vendor` ignores the pin and rewrites it.
- `naner lock --refresh [vendor...]` drops pins so the next install re-resolves.

Precedence for verification is unchanged from ADR-0002 and extends cleanly: a
`checksum` pinned in `vendors.json` still outranks everything, so a tampered
lockfile cannot overrule an operator's explicit assertion. The lock's digest
simply becomes the download's checksum, so the existing precedence code needed no
change.

## Alternatives considered

**Record-only (pure trust-on-first-use).** Write what was installed; on the next
install, resolve normally and update the entry if it differs. Rejected: it
provides essentially no security value, because any attacker-supplied "newer"
version silently overwrites the entry it was supposed to be checked against. It
also delivers nothing on reproducibility, which is the property a file literally
called a lockfile is expected to have.

**Pin, but let `update-vendors` honour the pin too.** Rejected: that makes
`update-vendors` a permanent no-op on every pinned vendor, which is precisely the
opposite of what the verb means. Updating is the explicit request for a newer
artifact.

**Require an explicit `naner lock` before anything is pinned.** Rejected as the
default: it means the common path stays unverified for the vendors that need it
most, and users who never run the extra command get no benefit. Writing the pin
as a byproduct of install costs one SHA-256 pass over a file already on disk.

## Consequences

- An environment becomes reproducible after its first install, and every
  subsequent install of a pinned vendor is verified — including MSYS2 and the
  GitHub-sourced vendors that ADR-0002 could not reach.
- **The first install is still trust-on-first-use** for any vendor without an
  upstream digest. The lock records what arrived; it cannot know whether that was
  the right thing. This is a weaker guarantee than an upstream digest and is
  stated as such in the module docs, the README and `naner lock`'s own output.
- Install behaviour changes: a pinned vendor no longer tracks upstream. That is
  the point, but it is a behavioural change for anyone who installed before this
  and expects `install` to fetch latest. `update-vendors` remains the way to move
  forward, and `naner lock --refresh` the way to unpin.
- Adds one SHA-256 pass over the downloaded artifact per fresh install (~1s for
  the ~400 MB MSYS2 archive). Re-installing from an existing pin skips it.
- A pin whose `sha256` is absent — possible if hashing failed — fixes the URL and
  version but cannot verify the bytes. `naner lock` reports the count of such
  entries rather than presenting them as verified.
