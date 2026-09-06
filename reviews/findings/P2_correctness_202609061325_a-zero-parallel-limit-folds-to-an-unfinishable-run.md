---
id: SWEEP-FOLD-OUTCOME-ZERO-PARALLEL
severity: P2
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/start.rs:6
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/fold/start.rs` (queue row 38 of `standards/SWEEP.md`), which owns `check_run_started`
---

## Failure sequence

`check_run_started` checks the schema, the runner record, both digests and every frozen ladder.
It does not check `started.limits`. A `run_started` recording `max_parallel: 0` is therefore
accepted, and the limits it records are what every later event is folded against.

`RunState::pipeline_reservable` is `pipeline_held() < max_parallel`, which is `0 < 0` for the life
of the run. `ready` and `ready_retry` both take that clause, and so does
`eligible_integration_candidate`, so `structurally_admissible` is false in every state the run can
reach. With no deferred task and no question, `derived_outcome` falls past `questions_open` to
`complete_shape`, which is false while any task is `Pending` and unblocked — and answers
`DerivedOutcome::FoldError`, the arm `topology::census::tests::the_derived_outcome_is_total_over_every_explored_state`
asserts no explored state reaches ("the arm the design argues is unreachable was reached at
states ...").

`check_run_finished` then refuses all four outcomes, because `DerivedOutcome::FoldError` matches
none of them and renders as the word "unreachable". Measured at `ee5dc81f` on a four-task plan with
`max_parallel: 0`, one `run_started` and nothing else:

```
run_finished records `complete`,        and the outcome derived from durable state is unreachable
run_finished records `parked`,          and the outcome derived from durable state is unreachable
run_finished records `halted`,          and the outcome derived from durable state is unreachable
run_finished records `budget exceeded`, and the outcome derived from durable state is unreachable
```

The run admits no work and cannot be ended. Resume does not help: `apply_resumed` raises the epoch
and clears the budget stop, and leaves the limits alone. Nothing in the fold can move the state,
which is what makes this the fold's problem rather than a driver's: `src/config.rs` refuses
`max_parallel` above 1 today, but the fold's input is a log on disk, which §8 treats as untrusted
including "data written by an older or interrupted version".

## What the change that takes this up should do

Refuse it at the door. `check_run_started` should reject `limits.max_parallel == 0` — a run whose
entitlement admits nothing cannot start — with a `FoldError` variant that names the limit and the
value, alongside the ladder check that is already there. That is one refusal in one function in
`src/topology/fold/start.rs`, with a test that the event is refused and a second that a
`max_parallel: 1` run still starts.

Two things to decide while there, both of them that file's to make: whether the other limits
(`max_defers`, `max_merge_repairs`) have a value that is likewise unfoldable, and whether
`check_run_resumed` needs the same guard, since a resume carries a runner record and the fold
compares it field by field.

It is recorded here rather than repaired because the repair is in another row's file. The sweep
of `src/topology/fold/outcome.rs` that found it could not take it: `pipeline_reservable` returning
false at a limit of zero is the correct answer to the question it is asked, and no ordering of
`derived_outcome` can invent an outcome for a run that has none.
