---
id: SWEEP-FOLD-OUTCOME-ADMISSIBILITY-NOT-REFUSED
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/check_attempt.rs:211
provenance: pre_existing
first_bad:
guard: `src/topology/fold/check_integration.rs`'s sweep (queue row 33 of `standards/SWEEP.md`) can take its half; `check_attempt.rs` is already swept (row 30, `fada7445`), so its half needs a change of its own
---

## Failure sequence

`RunState::ready` (`src/topology/fold/outcome.rs`) admits a dispatch on seven clauses.
`check_dispatched` refuses on four of them — the task's state, an outstanding lineage question or
candidate, an already-open generation, and generation density — and on the recorded region's
derivation. It checks none of the other three:

* **dependencies.** `check_spawn` does check them — a *repair* spawn is refused unless every
  dependency it records is already `Merged` — but that is registration, and it reaches only the
  tasks a rejection registers. An original task is registered from the frozen plan at
  `run_started`, and no refusal reads its `deps` again. Outside `check_spawn`, `entry.deps` is read
  in exactly two places in the whole fold, `ready` and `blocked_tasks`, both in
  `src/topology/fold/outcome.rs`, and both are selection predicates. A log that dispatches `mid`
  while `alpha`, which `mid` depends on, is still `Pending` folds without complaint, and a replay
  of it reconstructs a run that ran the plan out of dependency order and reports the log valid.
* **the predicted region.** `dispatch_lease_check` refuses a dispatch whose predicted region
  overlaps an active lease of another owner. `check_dispatched` verifies that the recorded region
  *equals* the one the entry derives, which is a different question, and admits the dispatch
  however many other owners hold that region. `apply_dispatched` then grants the lease, so the
  table holds two overlapping generation leases — the state the region machinery exists to make
  impossible.
* **the entitlement and the ending.** `pipeline_reservable` and `!run_is_ending` are clauses of
  `ready`, `ready_retry` and `eligible_integration_candidate`; no dispatch or transaction refusal
  takes either. `check_transaction_start` (`src/topology/fold/check_integration.rs:7`) has the same
  two gaps: it refuses a second open transaction and a non-dense sequence, and derives the first
  eligible candidate, but admits an integration at `max_parallel` and admits one after a halting
  settlement.

`docs/internals/topology/fold/outcome.md` states the intent for the third of these in as many
words — the entitlement "is a clause of admissibility here ... and not a check the caller is
trusted to remember" — and the caller is exactly what the checker is protecting the log from.
`docs/internals/topology/fold/check_attempt.md` records why the region's *derivation* is checked
("nothing stopped the next caller — or a later slice's second writer — from constructing a
`task_dispatched` the fold would accept and the lease table would honour") and that argument is the
same argument for the overlap.

Not observed in a run: the only producer today is `engine::topology::select`, which asks the
predicates first, so every dispatch it emits satisfies all seven clauses. This is a refusal gap,
not a live defect, which is why it is a P3.

## What the change that takes this up should do

Decide, in the file that owns each check, whether admissibility is part of what the fold refuses.
If it is, `check_dispatched` gains the three clauses as refusals — deps merged, no overlapping
lease of another owner, entitlement free and the run not ending — and `check_transaction_start`
gains the last two; each with a test that a hand-built event carrying the violation is refused.
If it is not, the reason belongs in `docs/internals/topology/fold/check_attempt.md` beside the
refusals that are there, because the asymmetry is currently silent and the next reader will read
`ready` as the contract.

Whichever way it goes, dependency order deserves the same treatment at dispatch that
`check_spawn` already gives it at registration: it is the plan's own semantics, and a fold that
checks it for repairs and not for originals accepts a log no plan could have produced.
