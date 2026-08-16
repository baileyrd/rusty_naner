## What changed

<!-- The change itself, not the ticket number. One or two sentences. -->

## Why

<!-- What was wrong, or what this enables. If it fixes an issue, link it. -->

Closes #

## How it was verified

<!-- Delete what doesn't apply; don't claim what you didn't run. -->

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] Exercised on a real Windows install (the launcher, vendor pipeline and
      console behaviour are the parts CI cannot fully prove — see
      [docs/VALIDATION.md](../docs/VALIDATION.md))

## Risk and limitations

<!-- What this does NOT cover, and anything a reviewer should look at hardest.
     "None" is a valid answer, but state it rather than leaving it blank. -->
