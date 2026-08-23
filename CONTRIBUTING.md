# Contributing to upstroke

Contributions are welcome. Please open an issue before starting anything substantial — the build
order in `DESIGN.md` §21 is deliberate, and it's worth checking that a change fits where the
project currently is.

Every change enters `master` through the same path: open a draft pull request early, wait for the
deterministic CI and PR-policy gates, then obtain an independent frontier-model review of the exact
green head before merge. The default-branch attestation workflow reruns the mechanical gates on
that exact SHA and uses a repository-scoped GitHub App to publish the required
`upstroke-frontier-review` check. The branch rules bind that check to the App's integration id, and a
new push invalidates it and restarts the sequence. See [`MAINTAINING.md`](MAINTAINING.md) for the
full lifecycle, attestation step, and emergency policy. Contributions from external forks remain
provisional until the separately owned fork canary described there has passed.

## Before you send a PR

The project holds itself to these; CI enforces all four, verbatim:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo +1.85.0 check --locked --all-targets --all-features
```

Use the pull-request template to record the exact commands, implementation provenance, reviewed
SHA, review model and effort, evidence link, risk, and rollback. Resolve every review conversation;
merge commits are the only accepted merge method.

Conventions worth knowing: edition 2024, no panicking `.unwrap()` or `.expect()` calls outside
tests, `anyhow` only at the binary edge (libraries return `thiserror` types), and all paths through
`std::path` — Windows is a first-class target, not an afterthought.

## Contributor Licence Agreement

By submitting a contribution you agree to the terms below. There is nothing to sign: opening a
pull request is your acceptance, and it applies to every contribution you make to this project.

1. **You keep your copyright.** You are not assigning ownership of anything.

2. **You grant a licence.** You grant Cameron Lambert (the "Maintainer") a perpetual, worldwide,
   non-exclusive, royalty-free, irrevocable licence to reproduce, modify, distribute and
   sublicense your contribution, **including the right to license it under terms other than the
   AGPL**, such as a commercial licence.

3. **You grant a patent licence.** You grant the Maintainer and all recipients of the software a
   perpetual, worldwide, non-exclusive, royalty-free, irrevocable patent licence covering your
   contribution, on the terms of Apache-2.0 §3.

4. **You confirm you can.** The contribution is your original work, or you have the right to
   submit it. If your employer has rights to work you create, you confirm you have permission to
   contribute, or that your employer has waived those rights.

5. **No warranty.** Contributions are provided as-is, without warranty of any kind.

### Why this exists

upstroke is released under the AGPL, which some organisations cannot use — a policy prohibition, or
a product they need to keep closed. Being able to offer those users a commercial licence is part
of how the project intends to sustain itself. That is only possible if one party can license the
whole codebase, which is what clause 2 preserves.

The trade is explicit and worth stating plainly: your contribution may end up in a commercially
licensed copy of upstroke. Everything you contribute also remains available to everyone under the
AGPL, permanently — that cannot be taken back. If clause 2 isn't acceptable to you, say so in the
PR; a change can often be reworked as a suggestion instead, and that's a perfectly good way to
contribute.
