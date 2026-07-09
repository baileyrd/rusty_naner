# Post-parity bug-fix wave: B1–B6 + 7-zip.org source rot

Deliberate bug-for-bug preservations to fix after the Phase 5 cutover settles
(MIGRATION_ANALYSIS §4, §6). File as a tracking issue at cutover.

- **B1** — GitHub `assetPattern` globs matched with literal `Contains`; never
  match, so every GitHub vendor silently uses its pinned fallback URL. Fix
  with real glob matching.
- **B2** — Checksum verifier complete but unreachable: no `checksum` field
  wired from vendors.json. Add the field and wire it.
- **B3** — `dependencies` parsed but ignored; install order works only because
  7-Zip is hardcoded first. Implement topological ordering or document as
  inert.
- **B4** — MSYS2 `packages` array inert: prints "will be installed" but no
  pacman run happens. Implement `pacman -S --noconfirm` or remove field +
  message.
- **B5** — Version normalize edge: `"1.2"` vs `"1.2.0"` string-mismatch causes
  spurious (self-healing) "update available". Compare parsed triples.
- **B6** — `.naner-version` written in two formats historically (`0.4.6` build
  vs `v0.4.6` tag). Rust standardizes on tag form; keep normalizing the
  leading `v` on reads forever.

**New (found during Windows validation, 2026-07-09):** 7-zip.org moved its
binaries to GitHub releases. The scrape resolver now builds a mangled URL
(`https://www.7-zip.org/https://github.com/...` → 404, parses "latest
version: 7") AND the pinned fallback `/a/7z2408-x64.msi` is itself a 302 to
GitHub — so both rungs of the 7-Zip cascade dead-end at the same place, and
7-Zip is uninstallable on networks that block GitHub's asset CDN. Fixing B1
(real GitHub source with working asset globs) largely subsumes this; the
7-Zip vendor entry should move to a `github` source.

Also in this wave: tier-3 output changes (auto-quiet when stdout is not a
TTY; loud stderr warnings on fallback-URL use and dropped PATH entries).
