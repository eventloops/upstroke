---
id: PR180-DUPLICATED-WRONGATTEMPT-CHECK
severity: P3
disposition: accepted-risk     # a real duplication, but de-duplicating it changes observable error-priority for a multi-defect record and is not proven regression-safe
category: correctness
pr: 180
reviewed_sha: 50bcfab07c89488e354a7183ecbddcf35f18c2cc
location: src/topology/fold/check_attempt.rs:552-560,621-629
provenance: pre_existing
first_bad:
guard: a future change to check_attempt_finished's error-priority contract, if one is ever specified
---

## Failure sequence

`check_attempt_finished`'s `match &finished.settlement` has two arms, `Retained` and `Closed`.
Both arms contain a byte-identical block:

```rust
if finished.record.attempt != finished.attempt.0 {
    return Err(FoldError::WrongAttempt {
        kind: KIND,
        key: finished.key.0,
        generation: finished.generation.0,
        attempt: finished.record.attempt,
        expected: finished.attempt.0.to_string(),
    });
}
```

Both operands (`finished.record.attempt`, `finished.attempt.0`) come from the event envelope, not
from either arm's per-variant data, so the check is unconditional on which settlement variant
fired — a candidate for hoisting above the `match` entirely. Mutation testing confirms both copies
are live (killing either independently fails a distinct set of tests, not the same set), so this
is exact duplication, not one dead copy.

Hoisting it changes behaviour for a record that violates more than one condition at once. Today,
for `Retained`, a record with both a stale incarnation *and* a wrong attempt number reports
`StaleIncarnation` (checked first); for `Closed`, a record with `transition == Succeeded` *and* a
wrong attempt number reports the `Succeeded`-is-invalid `InconsistentRecord` (also checked first).
Hoisting the `WrongAttempt` check to the top of the function would make it win in both cases
instead. I did not find or run a test that pins today's priority for either multi-defect
combination, so I cannot rule out that hoisting is a silent behaviour change rather than a pure
simplification.

## What the change that takes this up should do

Before deduplicating: write (or find) a test that constructs a record violating two of these
conditions at once for each arm, confirm today's priority is not asserted anywhere as a contract,
and only then hoist the shared check — or leave it duplicated and say in a comment why the
duplication is deliberate (mirroring this file's own documented principle, in
`docs/internals/topology/fold/check_attempt.md`, that "a door is not fixed until every arm through
it asks the same question": here both arms already ask it, just at different points in their own
sequence). Not attempted in this PR because the assigned scope is this one file, and priority
verification would need to read the wider `topology::fold::tests` suite, which reaches well beyond
this file's own review.
