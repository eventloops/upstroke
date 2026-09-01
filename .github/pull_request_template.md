## Summary

<!-- What changes, and why is this the smallest coherent outcome? -->

## Scope

<!-- What is intentionally included and excluded? Link any issue or decision. -->

## Validation

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test --all-targets --all-features`
- [ ] `cargo +1.85.0 check --locked --all-targets --all-features`
- [ ] `bash .github/scripts/test-release-record.sh`
- [ ] `bash .github/scripts/test-pr-policy.sh`
- [ ] `bash .github/scripts/test-pr-ledger-evidence.sh`
- [ ] `bash .github/scripts/test-docs-consistency.sh`

Exact commands and results:

## Review evidence

Implementation model and effort (or `human`):

Reviewed head SHA:

Frontier reviewer model and effort:

Review transport and per-pass wall-clock limit:

Durable review verdict URL (the verdict as written):

- [ ] `upstroke-ci` and `upstroke-pr-policy` passed before frontier review began
- [ ] The independent frontier review used `max` effort on the reviewed head recorded above
- [ ] Every serious P1 is fixed and its repaired head received a fresh pass; every `MUST` deviation in materially touched code and every finding carrying a failing test, reproduction, or mutation witness is `fixed`, or `rejected` only by a row showing the evidence is not valid (a `MUST` the code does not breach, a witness that does not reproduce on the head), never `accepted-risk` or `deferred`; every other finding carries a ledger row (`fixed`, `rejected`, `deferred`, or `accepted-risk`)
- [ ] `Reviewed head SHA` above is the head being merged, or the delta to it is exempt-only or a non-serious repair-only delta owner-verified and disclosed (single-reviewer passes only: a panel-reviewed candidate re-runs every seat on any head movement)
- [ ] Every review conversation is resolved

## Risk and rollback

<!-- Failure modes, compatibility impact, and the concrete revert/recovery path. -->

## Review finding ledger

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| None yet | — | — | — | — | — | — | — | — |
