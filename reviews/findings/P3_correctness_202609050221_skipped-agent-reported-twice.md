---
id: SWEEP-CONNECT-RENDER-009
severity: P3
disposition: deferred
category: correctness
pr: 168
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/connect.rs:173
provenance: pre_existing
first_bad:
guard: the sweep of `src/connect.rs` (`standards/SWEEP.md` row 62)
---

## Failure sequence

For an agent whose probe or discovery fails, `run_with` does two things with the one error: it
pushes `"{id}: no pool written — {error}"` onto `warnings`, and it records the agent with
`outcome: Err(error)`. The summary renders both, so the operator reads the same fact twice in
different words:

```
copilot: skipped — binary not found on PATH
warning: copilot: no pool written — binary not found on PATH
```

This is every single-vendor machine — the normal case, as the field's own doc says — not an edge.
The renderer cannot fold them honestly: a warning is a `String` and it does not know which one
restates an agent's error.

## Why this is recorded rather than fixed

The duplication is the parent's: it decides what goes into `warnings`. Dropping the `skipped` line
in the renderer instead would move the one place an agent's failure is attributed to a free-text
list, and `a_missing_agent_skips_its_pool_without_taking_the_others_with_it` asserts the warning.

## What the change that takes this up should do

Stop pushing the warning for a skipped agent — the report already carries the error, and the
summary prints it beside the agent's name — or keep the warning and have the summary print only
the outcome line. One surface per fact.
