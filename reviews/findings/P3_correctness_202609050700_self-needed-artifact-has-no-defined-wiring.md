---
id: SWEEP-GRAPH-004
severity: P3
disposition: deferred
category: correctness
pr: 167
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/validate/graph.rs:71
provenance: pre_existing
first_bad:
guard: the change that settles `SWEEP-GRAPH-009` in `src/plan/markdown/assemble.rs`; `check_artifact_wiring` follows it
---

## Failure sequence

A task annotated `out=contract needs=contract`, and no other task declaring `out=contract`:

```
## D
<!-- upstroke: id=d out=contract needs=contract depends= -->
```

`collect_artifacts` records `d` as the producer of `contract`; `check_artifact_wiring` finds the
recorded producer equal to the needing task and says nothing. The plan validates silently, though
no order puts `contract` in place before `d` runs.

**Why it is deferred rather than fixed, and why a warning was withdrawn.** PR #167 first made this
case warn (`needs artifact ... that it produces itself`); its pass 1 showed the warning wrong for an
input the adapter accepts:

```
## D1
<!-- upstroke: id=d1 depends=d2 needs=contract out=contract -->

## D2
<!-- upstroke: id=d2 depends= out=contract -->
```

Here `d2` produces `contract` first and `d1` updates it, a valid order; but `plan.artifacts` holds
one producer per artifact and the adapter records the first declaration (`d1`), keeping `d2`'s only
in `d2.artifacts_out` (`SWEEP-GRAPH-009`). From the recorded producer alone the two plans are
indistinguishable, so the check cannot warn on the first without lying on the second. The graph
check *could* scan `plan.tasks[*].artifacts_out` for other producers, but what a second `out=`
means is undefined in `design/09`, and a check that guesses the meaning is the shape pass 1
refused. Pinned at the head by
`a_task_that_needs_what_it_is_recorded_as_producing_is_not_warned_about` and
`a_task_updating_what_an_earlier_producer_made_is_not_warned_about_through_the_adapter`, which
would both fail on a warning.

## What the change that takes this up should do

Settle `SWEEP-GRAPH-009` first: decide what a second `out=` for one artifact means, in
`src/plan/markdown/assemble.rs` and `design/09`. Once the record says whether an artifact has one
producer or several, this case has a definite answer — with one producer that is the needing task,
warn; with an earlier producer the task depends on, do not — and `check_artifact_wiring` implements
it against the record rather than against a scan.
