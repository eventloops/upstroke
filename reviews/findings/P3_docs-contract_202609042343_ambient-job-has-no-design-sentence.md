---
id: SWEEP-AMBIENT-002
severity: P3
disposition: deferred
category: docs-contract
pr: 147
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/agent/proc/ambient.rs:127
provenance: pre_existing
first_bad:
guard: a `design/15` sentence on the ambient Job Object and what `INV-18` names — the owner's design decision, beyond any sweep session's reach; a later pass labelling this P1 or P2 escalates to the owner rather than re-deferring
---

## Failure sequence

A Windows write command whose ambient Job Object cannot be created or joined refuses with
`AMBIENT_REFUSAL_PREFIX`: "cannot start a write command: on Windows every child must be a member of
the coordinator's ambient kill-on-close Job Object from creation (INV-18), and …" -> the operator
looks `INV-18` up -> no public document defines it. At `425ad55`,
`grep -rn INV-18 --include=*.md .` hits only `reviews/FINDINGS.md` rows that cite it as already
known, and `design/15`'s containment paragraph describes the private per-invocation Job Object
alone ("each command is created suspended, assigned to a private kill-on-close Job Object, and only
then resumed"); the ambient job, its startup refusal and the no-degraded-mode rule appear nowhere
in `DESIGN.md` or `design/`. The module, `runner::host::contain_write_command`, `src/main.rs`'s
containment and the tests of all three quote `crash_reconstruction`,
`decisions.effect_site_inventory.containment_sub_effects` and
`decisions.admission_and_leases.permits.os_matrix` — the retired v0.2 packet's paths, whose
substance moved to the private archive on 2026-09-03 — as the rule they implement. A reader of the
public tree cannot tell the requirement from the implementation choice, and an operator-facing
diagnostic names an identifier the product's documentation never defines.

## What the change that takes this up should do

The owner decides whether `design/15`'s containment paragraph carries the ambient job. If it does,
one sentence beside the private-job one suffices, saying the three things the code enforces: every
write command joins one non-inheritable kill-on-close Job Object at process start and holds it to
exit; a coordinator that cannot create or join it refuses the write command before any effect,
with no degraded mode; and on Windows there is no reaper, so containers are reclaimed at the next
write-command start. `INV-18` then either becomes a `design/04` invariant under that number or
leaves the refusal text — three Windows-only tests assert the fragment (`runner::host::tests`
twice, `main::tests` once), so the second is a change to those tests as well.

The sweep of `ambient.rs` (PR #147) did not edit the design: a sweep does not manufacture
authority for the module it is sweeping (`PR137-CLASSIFY-DESIGN-AUTHORITY-ABSENT` records the same
choice for `classify.rs`). It stated in the module doc what the citations are instead.
