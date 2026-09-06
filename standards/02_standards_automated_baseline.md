## 2. Automated baseline

Every change MUST pass what CI runs, from the repository root:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo +1.85.0 check --locked --all-targets --all-features
bash .github/scripts/test-release-record.sh
bash .github/scripts/test-pr-policy.sh
bash .github/scripts/test-pr-ledger-evidence.sh
bash .github/scripts/test-docs-consistency.sh
bash .github/scripts/test-internals-notes.sh
```

CI splits the nine across jobs: `lint` runs rustfmt, Clippy and the five Bash gates; `test` runs
the suite on Linux and macOS, and `test (winguest)` runs it on Windows on a self-hosted ephemeral
runner; `msrv` runs the locked check on all three; `lint (windows)` runs Clippy natively and builds
every target as the hosted Windows compile witness; `lint (macos)` runs Clippy natively;
`upstroke-ci` aggregates every leg and is the required context. The `--locked` MSRV check is what
proves the floor against the dependency set a release ships. `test-release-record.sh` needs `jq`, and `test-pr-policy.sh` only works from the root.

Edition 2024, MSRV 1.85.0; code and dependencies stay compatible with both the MSRV and current
stable. A green baseline is necessary, not sufficient: nothing past this section is checked by a
compiler.

Lints:

- Lint levels are repository policy and live only in `Cargo.toml`'s `[lints]` tables. A crate-root
  `#![allow]` or `#![deny]` does not loosen or narrow a lint `[lints]` already governs. This does
  not cover a module-level `#![deny]` that activates a lint `[lints]` does not yet govern, ahead of
  the crate-wide entry `standards/SWEEP.md`'s activation rule says is owed once a file is swept
  (`src/validate/graph.rs`'s `clippy::indexing_slicing`/`clippy::unreachable`, §7) — that file is
  stricter than the rest of the tree until the transitional wording retires, which is the point of
  a per-file activation, not a policy this bullet forbids. Nor does it cover `clippy.toml`'s
  `disallowed-*` census, which is enforced only through the module-level attributes the next bullet
  describes.
- Fix warnings at their cause. A suppression is as narrow as practical and says why. Prefer
  `#[expect(lint, reason = "…")]`, which retires itself when its cause goes; it only works on a leg
  that compiles the region and promotes warnings to errors (§11).
- Enable a new lint in the commit that makes the tree clean under it, and only if the lint exists
  on the MSRV. Never enable `pedantic`, `nursery` or `restriction` wholesale. Do not rewrite clear
  code to satisfy a stylistic lint; configure or narrowly suppress it.
- `clippy.toml`'s `disallowed-*` denylist is effect-funnel enforcement (§3), not style. A denied
  path enforces nothing unless a Clippy leg compiles the platform where it exists; the resolution
  census in `src/effects/tests.rs` checks every entry against a linked probe and carries an injected
  misspelt control. A per-site `#[expect]` of a governed lint may stand below module level only in
  a file whose `effects/allowlist.toml` row records the lint and the exact annotation count.

Enforced by: the nine commands; `[lints]`; `clippy.toml` with the effects census.
