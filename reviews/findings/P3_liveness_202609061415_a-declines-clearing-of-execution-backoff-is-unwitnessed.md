---
id: SWEEP-FOLD-APPLY-DECLINE-BACKOFF-UNWITNESSED
severity: P3
disposition: deferred     # both halves are in this file, but the trace needs a deferred settlement inside a declined lineage and so needs the fixture src/topology/fold/tests.rs builds
category: liveness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/apply.rs:461
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/fold/tests.rs` (queue row 39), which owns the fixture the trace needs
---

## Failure sequence

Location as first recorded: src/topology/fold/apply.rs:461 and 519-536 at `ee5dc81f` (`fail_lineage`'s `deferred_tasks.remove`, and `set_state`'s)

`design/26` says a decline "closes their generations, removes candidates and questions, **clears
execution backoff**, and releases their generation, candidate and lineage holdings". Two statements
implement the clearing, both in `src/topology/fold/apply.rs`:

- `fail_lineage`'s `self.deferred_tasks.remove(&member)`, once per lineage member;
- `set_state`'s `self.deferred_tasks.remove(&key)` on every non-`Deferred`, non-`AwaitingInput`
  state, which the `set_state(member, TaskState::Failed)` two lines later reaches.

Neither is witnessed, and neither is the pair. Measured at `ee5dc81f` against the whole
`topology::fold` suite (131 tests), applied one at a time and reverted, and then both together:

| Mutation | Result |
|---|---|
| `fail_lineage` stops removing its members from `deferred_tasks` | survives |
| `set_state` stops removing the key on the five non-waiting states | survives |
| both at once | survives |

The pair surviving together is the part that matters: this is not two mechanisms covering each
other, it is a design sentence with no test behind it.

What a regression would cost: `deferred_tasks` is what `RunState::backoff_pending` reports, and
`derived_outcome` returns `NotEnding` while `backoff_pending()` is true. A declined lineage holding
a member that had settled `Deferred` would leave that member's key in the set forever — the member
is `Failed`, so nothing wakes it, and `wake_backoff` only drains on a `defer_wait_elapsed` or a
resume. `check_run_finished` would then refuse every `run_finished` the run could honestly record,
because the fold derives `NotEnding` for a run with nothing left to run.

## What the change that takes this up should do

In `src/topology/fold/tests.rs`, build the trace that crosses the two: a lineage whose member
settles `Deferred` (so it is in `deferred_tasks` and `backoff_pending()` is true), a question on
that lineage, and a decline. Assert `backoff_pending()` is false afterwards and that
`derived_outcome()` reaches its ending outcome rather than `NotEnding` — the assertion
`declining_a_repair_admission_fails_the_lineage_and_allows_the_run_to_end` already makes for the
run, but over a lineage with no backoff in it. The witness is either mutation above, or both:
with the clearing removed, the new assertion must fail.
