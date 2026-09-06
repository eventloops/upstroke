---
id: SWEEP-RENDER-011
severity: P3
disposition: deferred
category: docs-contract
pr: 166
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/status/render.rs:165
provenance: pre_existing
first_bad:
guard: a future FailureKind spelling policy in src/ladder.rs, consumed by describe_task_failure; existing status tests pin the current text
---

## Failure sequence

A terminal failure renders its FailureKind through Debug -> follow output says `GateFailed` while JSON uses `gate_failed`. This is a consistency debt between human and machine spelling. The current contract does not require these strings to match. The existing `describe_atomic_attempt_transitions` and `a_terminal_failure_says_whether_the_run_halts_in_both_wire_shapes` tests pin the human spelling, so a variant rename would fail them. The earlier statement that this changed CLI output silently was inaccurate. No failing supported-contract witness or MUST violation is established.

## What the change that takes this up should do

Choose a type-owned human spelling for FailureKind in src/ladder.rs and use it from the single `describe_task_failure` helper. If that policy adopts the serde spelling, move the output tests with the deliberate compatibility change. The type owner should keep one source of spelling truth. This remains a deferred P3 design change.
