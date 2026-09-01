# Contributing to upstroke

Contributions are welcome. Please open an issue before starting anything substantial — the build
order in `DESIGN.md` §21 is deliberate, and it's worth checking that a change fits where the
project currently is.

Every change enters `master` through the same path: open a draft pull request early, wait for the
deterministic CI and PR-policy gates, then obtain an independent frontier-model review of the exact
green head before merge. The reviewed SHA and a durable link to the verdict are recorded in the
pull request; a new push invalidates the review and restarts the sequence; the owner's merge is the
attestation. See [`MAINTAINING.md`](MAINTAINING.md) for the full lifecycle, trust boundary, and
emergency policy. Contributions from external forks remain provisional: the required checks are
candidate-controlled, so a fork's entire diff — workflow edits included — is reviewed before merge.

## Before you send a PR

The project holds itself to these; CI enforces all eight, verbatim:

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

Run all eight from the repository root. CI splits them across its jobs — `lint` runs rustfmt,
Clippy and the four Bash gates, `test` runs the suite, `msrv` runs the locked check, and Clippy
runs again on the Windows and macOS lint legs — and `upstroke-ci` aggregates every leg.
`CLAUDE.md`'s Gates section records the root-invocation trap and the `jq` prerequisite the
release-record fixture carries; `CODING_STANDARDS.md` §2 is the normative statement of this
baseline, and no gate checks that these copies of it agree.

Use the pull-request template to record the exact commands, implementation provenance, reviewed
SHA, review model and effort, evidence link, risk, and rollback. Resolve every review conversation;
merge commits are the only accepted merge method.

[`CODING_STANDARDS.md`](CODING_STANDARDS.md) is the normative implementation standard; read it
before changing Rust code. Among its hard requirements: edition 2024 with MSRV 1.85, no
`.unwrap()` or `.expect()` in production, `anyhow` only at the binary edge (libraries return typed
`thiserror` errors), and paths represented with `std::path` types. Windows, macOS, and Linux are
supported targets. The eight commands above are the automated baseline, not the whole standard.

## Contributor Licence Agreement

By submitting a contribution you agree to the terms below. There is nothing to sign: opening a
pull request is your acceptance, and it applies to every contribution you make to this project.

1. **You keep your copyright.** You are not assigning ownership of anything.

2. **You grant a licence.** You grant Cameron Lambert (the "Maintainer") a perpetual, worldwide,
   non-exclusive, royalty-free, irrevocable licence to reproduce, modify, distribute and
   sublicense your contribution, **including the right to license it under terms other than the
   Apache License**.

3. **You grant a patent licence.** You grant the Maintainer and all recipients of the software a
   perpetual, worldwide, non-exclusive, royalty-free, irrevocable patent licence covering your
   contribution, on the terms of Apache-2.0 §3.

4. **You confirm you can.** The contribution is your original work, or you have the right to
   submit it. If your employer has rights to work you create, you confirm you have permission to
   contribute, or that your employer has waived those rights.

5. **No warranty.** Contributions are provided as-is, without warranty of any kind.

### Why this exists

Licences are not forever: this project began under the AGPL and was relicensed to Apache-2.0
([the 2026-09-01 decision](decisions/2026-09-01-relicense-apache-2.md)). A move like that is
only ever cheap while one party can license the whole codebase, which is what clause 2
preserves as outside contributions arrive — a future change, such as a licence exception or a
newer licence version, should not require tracking down every past contributor.

The trade is explicit and worth stating plainly: your contribution may later be offered under
terms you did not choose. Everything you contribute also remains available to everyone under
the Apache License 2.0, permanently — that cannot be taken back. If clause 2 isn't acceptable
to you, say so in the PR; a change can often be reworked as a suggestion instead, and that's a
perfectly good way to contribute.
