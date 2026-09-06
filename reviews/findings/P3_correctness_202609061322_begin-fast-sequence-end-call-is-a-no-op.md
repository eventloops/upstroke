---
id: SWEEP-EFFECTS-HARNESS-001
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/harness.rs:277
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/effects/harness.rs` (queue row 22)
---

## Failure sequence

Location as first recorded: src/topology/effects/harness.rs:277 (as of the reviewed sha)

`HookHarness::begin_fast_sequence` opens with `self.end_fast_sequence();`,
and its doc sentence "A second `begin` closes the first" reads as if that
call is what makes it true. It is not. `end_fast_sequence` does exactly
`self.open_fast = None;`, and `begin_fast_sequence` assigns
`self.open_fast = Some(self.fast.len() - 1)` two statements later, so the
call has no effect any caller or any test can observe: delete it and the
type behaves identically on every input.

That makes it a redundant flag in the shape §12 warns about — the mutation
that removes it cannot be witnessed, so no test can be written against the
sentence it appears to implement, and a reader repairing
`end_fast_sequence` later will believe this call is load-bearing. The
sentence is true of the type; it is true because of the assignment.

## What the change that takes this up should do

Drop the call and keep the sentence, moving its "because `open_fast` is
reassigned below" to the line that does the work — or, if a second
responsibility is wanted for `end_fast_sequence` (recording an end time, or
refusing an unclosed sequence), give it one, at which point the call becomes
observable and `src/topology/effects/tests.rs` can carry the test that
witnesses it.
