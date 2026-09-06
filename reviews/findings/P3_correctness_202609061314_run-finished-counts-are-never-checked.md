---
id: SWEEP-CHECKEND-002
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/check_end.rs:220
provenance: pre_existing
first_bad:
guard: the change that lands the schema-4 `run_finished` emitter, which is what fixes the derivation these counts should be checked against
---

## Failure sequence

`RunFinished4` has four fields -- `outcome`, `halted_at`, `merged` and `parked` --
and `check_run_finished` checks the first two: the outcome against `derived_outcome()` and the
halt attribution against `self.halted_at` -> a `run_finished` recording `merged: 99, parked: 7`
for a run whose fold holds one merged task and no parked one is accepted, and every later reader
of the log takes those counts as the run's own record of what it did -> nothing in the crate
reads them back (`grep -rn 'RunFinished4' src/` finds the fold, two test fixtures and the
census fixture; no production emitter of this event exists yet, because the v0.2 topology
machinery is inert by default), so today the wrong count reaches only an operator reading the
log. The asymmetry is the finding: two fields of one record are derived and refused, and two
are accepted unread, in the file whose stated job is that "the recorded outcome is the derived
one".

## What the change that takes this up should do

Decide the derivation before pinning it, because there are two readings of each count and
nothing in design/ chooses. `merged` is either the tasks whose `TaskState` is `Merged` or the
publication transactions the run committed; `parked` is either the tasks whose state is
`AwaitingInput` or the questions still open, which differ whenever one lineage holds two
questions. Write the chosen derivation into design/26's event authority list beside the other
`run_finished` fields, then refuse a record that disagrees with it the way the halt attribution
is refused. Two fixtures record counts that no state produced and would have to be repointed in
the same change: `run_finished` in src/topology/fold/tests.rs (`merged: 1, parked: 0`, constant
across every outcome the grid asserts) and `run_finished` in src/topology/census.rs
(`merged: 0, parked: 0`, offered as a candidate transition at every explored state, including
states with a merged task).
