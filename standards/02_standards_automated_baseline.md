## 2. Automated baseline

Every change MUST pass the same commands as CI, from the repository root:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo +1.85.0 check --locked --all-targets --all-features
bash .github/scripts/test-release-record.sh
bash .github/scripts/test-pr-policy.sh
bash .github/scripts/test-pr-ledger-evidence.sh
bash .github/scripts/test-docs-consistency.sh
```

Run all eight commands from the repository root. `CLAUDE.md`'s Gates section records the known
root-invocation trap and the extra `jq` prerequisite for the release-record fixture.
`CLAUDE.md`, `CONTRIBUTING.md` and `.github/pull_request_template.md` each list all eight
verbatim, and this section is the normative statement all three follow. CI splits the eight
across its jobs — `lint` runs `cargo fmt`, `cargo clippy` and the four Bash gates, `test` runs
`cargo test`, `msrv` runs the locked `check`, and `lint (windows)` and `lint (macos)` run Clippy
again on their own platforms — and `upstroke-ci` aggregates every leg.

**No gate checks that those copies agree.** `test-docs-consistency.sh` names that claim among
the ones it withdrew deliberately: it asserts nothing about which cargo commands CI runs, whether
CI executes them, or which commands the documents list, because a text checker cannot separate a
command that is present from one that runs. What it does pin is the `test-*.sh` half of the
inventory — the set of gate scripts in the tree equals the set the `lint` job invokes, and
`CLAUDE.md`'s count of them equals the tree. Keeping the cargo half in step is a review duty, and
Appendix A records it as review-only.

These eight commands run on one operating system, which bounds what they can establish about a
`cfg` region compiled only for another target. Parsing and formatting still reach an inactive
inline block — `cfg` stripping happens after parsing, and rustfmt formats disabled source — so a
syntax error or a formatting violation inside one fails the baseline on any host. Nothing past
that does: a stripped block is never type-checked, its lint attributes are never evaluated, and
its behaviour is never run. CI carries the other platforms; §11 states which leg evidences which
kind of platform-gated claim.

The project uses Rust edition 2024 and has an MSRV of 1.85.0. Code and dependencies MUST remain
compatible with both the MSRV and the current stable toolchain. A green baseline is necessary,
not sufficient: the compiler, rustfmt, and Clippy cannot establish the behavioural rules below.

Lint policy:

- Fix warnings at their cause. Do not use crate- or module-wide `allow(warnings)`.
- A lint suppression MUST be as narrow as practical and explain why the flagged construction is
  correct. Test-only generated code is the usual exception to the explanation requirement.
- Lint levels are repository policy and are set only in `Cargo.toml`'s `[lints]` tables — one
  diffable authority. A crate-root `#![allow]` or `#![deny]` does not change policy.
- Prefer `#[expect(lint, reason = "…")]` to `#[allow]`. An expectation that stops firing becomes a
  warning, so a suppression that outlives its cause removes itself from review instead of
  surviving unnoticed. That self-retirement needs a leg that both compiles the annotated region
  and promotes warnings to errors. §11 governs which legs those are for platform-gated code; an
  expectation in a region no such leg compiles does not operate at all — it suppresses nothing and
  cannot become the warning that retires it.
- Add targeted lints only after the repository is clean under them and the lint is available on
  the MSRV. Do not enable Clippy's `pedantic`, `nursery`, or `restriction` groups wholesale;
  those groups intentionally contain contextual, experimental, or mutually incompatible lints.
- Do not rewrite clear code merely to satisfy a stylistic lint. Configure or narrowly suppress
  the lint, with rationale, when its premise does not hold.
- The `clippy.toml` denylist is effect-funnel enforcement rather than style. Every
  denied path MUST resolve under a Clippy CI leg that compiles the platform where the symbol
  exists. An unresolved denial enforces nothing; Clippy reports it only as a bare configuration
  warning, and `-D warnings` does not promote that warning.
- A denylist MUST have a resolution census run by a named gate. The census links the probe against
  every dependency needed to resolve its paths, enumerates the declared platform exceptions, and
  injects a misspelled control that it must detect. These requirements attach to `disallowed-*`
  entries, and they are active: `clippy.toml` carries 102 denied paths — 95 methods, 3 types
  and 4 macros — beside the `allow-*-in-tests` booleans, and the census is
  `effects::tests` in `src/effects/tests.rs`, which resolves every denied path
  against a linked probe and injects a misspelled control it must detect.
