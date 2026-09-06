---
id: SWEEP-FOLD-APPLY-FIRST-HALT-UNWITNESSED
severity: P3
disposition: deferred     # the trace needs two in-flight tasks and the RunStarted4/plan/digest fixture that src/topology/fold/tests.rs builds, which this one-file sweep may not edit
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/apply.rs:177
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/fold/tests.rs` (queue row 39), which owns the fixture the trace needs
---

## Failure sequence

Location as first recorded: src/topology/fold/apply.rs:177-182 at `ee5dc81f` (`RunState::record_halt`)

`RunState::record_halt` is first-in-wins:

```rust
fn record_halt(&mut self, key: TaskKey) {
    if self.halted_at.is_none() {
        self.halted_at = Some(key);
        self.halted_epoch = Some(self.epoch);
    }
}
```

Nothing pins that. Measured at `ee5dc81f` against the whole `topology::fold` suite (131 tests):
replacing the body with the two unconditional assignments — so a later halt overwrites the first —
**survives**.

The state it decides is reported. `check_run_finished` refuses a `run_finished` whose `halted_at`
differs from the fold's, so which task a halted run names as its cause is a checked, durable fact,
and `run_finished` is where a wrong one becomes visible. It is also reachable: `check_attempt_finished`
has no run-ending guard, so with two generations in flight, a `Failed { halts_run: true }`
settlement for the first and then another for the second are both accepted, and the mutation makes
the run attribute its halt to the second. `halted_epoch` moves with it, which
`check_question_answered` and `check_defer_wait_elapsed` both read (`self.halted_epoch ==
Some(self.epoch)`), though in the same epoch that comparison does not change.

This is a coverage gap, not a wrong behaviour: the code is right and the notes say so ("`halted_at`
is first in wins, and is never cleared"). What is missing is the test that would notice if it
stopped being true.

## What the change that takes this up should do

In `src/topology/fold/tests.rs`, extend the trace that already settles a halting failure so that a
second task in flight settles `Failed { halts_run: true }` afterwards, and assert
`fold.halted_at() == Some(first)` after both — live and through the replay the other tests in that
file already run. The witness is the mutation above: with `record_halt` assigning unconditionally,
the new assertion must fail, and it must pass with the guard restored.
