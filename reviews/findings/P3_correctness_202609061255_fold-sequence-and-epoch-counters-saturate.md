---
id: SWEEP-FOLD-APPLY-SATURATING-COUNTERS
severity: P3
disposition: deferred     # the refusal belongs in the checkers, which this one-file sweep may not edit
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/apply.rs:71,254,313,330 at `ee5dc81f`
provenance: pre_existing
first_bad:
guard: the sweeps of `src/topology/fold/check_integration.rs` (queue row 33) and `src/topology/fold/check_attempt.rs` (row 30, swept), which own the two refusals; `apply` cannot refuse anything
---

## Failure sequence

`RunState::apply` advances two run-wide counters with `saturating_add(1)` and nothing refuses the
event that would exceed them.

- `next_sequence` (`apply_verification_started`, `apply_merge_prepared`, `apply_merge_rejected`).
  `check_transaction_start` accepts an event whose `sequence.0 == self.next_sequence` and
  `FoldError::NonDenseSequence`'s own message states the invariant: "sequences are dense from 0
  across the run". At `next_sequence == u32::MAX` the increment saturates, so the counter does not
  move: a second `merge_verification_started` recording sequence `u32::MAX` passes the same
  equality check and two integration transactions share one sequence id. Every later event that
  cites a sequence — `merge_prepared`, `merge_rejected`, `task_merged`,
  `merge_verification_unavailable` — resolves against `self.transaction`, so the two are no longer
  distinguishable in the log by the number that is supposed to order them.
- `epoch` (`apply_resumed`). At `epoch == u32::MAX` a `run_resumed` leaves the epoch unchanged, so
  a retained session from the previous incarnation still satisfies `check_attempt_started`'s
  `*incarnation != self.epoch` test and a stale session is resumable across the boundary.

Neither can panic — `saturating_add` is the deliberate non-panicking choice, and `overflow-checks`
would otherwise abort a replay — so this is an invariant that stops holding at the ceiling, not a
crash. Neither ceiling is reachable by any physical run: `next_sequence` counts completed
integration transactions and `epoch` counts resumes.

This is the same shape as `PR152-ATTEMPT-NUMBER-OVERFLOW`, which was fixed in PR #152 by giving
`check_attempt_started` a `checked_add` and a typed refusal
(`an_exhausted_generation_attempt_counter_is_refused_without_panicking`). That repair landed in the
checker, not in the application, because an application cannot refuse: `RunState::apply` returns
`()` and by design decides nothing.

## What the change that takes this up should do

Give the two counters the refusal the attempt counter already has, in the checkers that own them:

- `check_transaction_start` (`src/topology/fold/check_integration.rs`) refuses when
  `self.next_sequence.checked_add(1)` is `None`, with a contextual `FoldError` naming the run's
  exhausted sequence space, so no second transaction can be opened at the saturated value.
- `check_run_resumed` (`src/topology/fold/check_attempt.rs`) refuses when
  `self.epoch.0.checked_add(1)` is `None`, so an incarnation boundary that cannot be represented
  is refused rather than silently merged with the previous one.

Then `apply`'s `saturating_add`s become unreachable-by-check and can say so, or become plain adds
under the checker's guarantee. Each refusal needs the boundary regression the attempt counter got:
construct only the maximum counter, assert the contextual refusal and that state is unchanged, and
witness it by restoring the unchecked form.
