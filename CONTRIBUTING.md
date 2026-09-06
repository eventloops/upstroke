# Contributing to upstroke

upstroke is not accepting outside contributions at present. The project is developed by its
owner, and opening it to other contributors is a question for later, once the v0.2 build order in
`DESIGN.md` §21 has settled; it is not on offer now. Bug reports and questions are welcome as
issues. A pull request from outside the project will be closed without review.

The rest of this file records how a change enters `master` today and the terms that would apply
to contributions if they are accepted, so that nothing here needs deciding at that point.

Every change enters `master` the same way: a draft pull request opened early, the deterministic CI
and PR-policy gates green, one independent frontier-model review of the exact green head, findings
triaged (serious P1s fixed and re-reviewed; `MUST` deviations in touched code and evidence-backed
findings fixed whatever their label; the rest fixed or logged as tech debt), and the owner's merge
as the attestation. [`MAINTAINING.md`](MAINTAINING.md) has the full lifecycle, trust boundary
and release contract.

## Before a change is sent

CI enforces all nine of these, verbatim, from the repository root:

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

`CODING_STANDARDS.md` §2 is the normative statement of this baseline and says how CI splits it
across jobs. `test-release-record.sh` needs `jq`; `test-pr-policy.sh` only works from the root.

Use the pull-request template to record the exact commands, implementation provenance, reviewed
SHA, review model and effort, evidence link, risk and rollback. Resolve every review conversation;
merge commits are the only accepted merge method.

[`CODING_STANDARDS.md`](CODING_STANDARDS.md) indexes the implementation standards; read the
sections a change touches before changing Rust. Among the hard requirements: edition 2024 with MSRV
1.85, no `.unwrap()` or `.expect()` in production, `anyhow` only at the binary edge (libraries
return typed `thiserror` errors), paths through `std::path` types, and no shared ownership, locks
or clones without a stated reason. Windows, macOS and Linux are supported targets. The nine
commands above are the automated baseline, not the whole standard.

## Contributor Licence Agreement

These are the terms that would govern contributions if the project opens to them. They are recorded
now so that the decision to accept contributions, when it comes, does not have to settle them. By
submitting a contribution you would agree to the terms below. There is nothing to sign: opening a
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

Licences are not forever: this project began under the AGPL and was relicensed to Apache-2.0 on
2026-09-01. A move like that is only ever cheap while one party can license the whole codebase,
which is what clause 2 preserves as outside contributions arrive: a future change, such as a
licence exception or a newer licence version, should not require tracking down every past
contributor.

The trade is explicit and worth stating plainly: your contribution may later be offered under
terms you did not choose. Everything you contribute also remains available to everyone under
the Apache License 2.0, permanently — that cannot be taken back. If clause 2 isn't acceptable
to you, say so in the PR; a change can often be reworked as a suggestion instead, and that's a
perfectly good way to contribute.
