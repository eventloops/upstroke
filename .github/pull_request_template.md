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

Passing review evidence URL:

- [ ] `upstroke-ci` and `upstroke-pr-policy` passed before frontier review began
- [ ] The independent frontier review used `max` effort on the exact current head
- [ ] Every actionable finding is fixed; follow-ups contain only non-blocking suggestions or feature ideas
- [ ] `Reviewed head SHA` above is the head being merged, or differs from it by an exempt-only diff
- [ ] Every review conversation is resolved

## Risk and rollback

<!-- Failure modes, compatibility impact, and the concrete revert/recovery path. -->

## Review finding ledger

| ID | Severity | Reviewed SHA / location | Failure sequence | Provenance | Category | First bad / prior ID | Regression or documented guard | Disposition |
|---|---|---|---|---|---|---|---|---|
| None yet | — | — | — | — | — | — | — | — |
