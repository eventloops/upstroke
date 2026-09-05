---
id: PR4-R28-NEXT-COORDINATOR-UNWITNESSED
severity: P2
disposition: deferred
category: correctness
pr: 4
reviewed_sha:
location: src/rundir.rs
provenance: undetermined
first_bad:
guard: PR5–PR7 implementer (the slice that owns rundir)
---

## Failure sequence

`src/rundir.rs`'s next-coordinator cleanup-hold check is unwitnessed from both ends. Two withheld-catalogue mutations survive the whole suite: `PR4-WIN-073` turns the `cleanup::is_held` / exclusive-probe would-block branch from refusal into continuation (`src/rundir.rs:383-396`, `:713-747`), and `PR4-WIN-074` replaces the immediate refusal with a polling loop that waits for the hold to release and then continues. Neither is caught, because **no test starts a coordinator while a surviving reaper actually holds R28**

## What the change that takes this up should do

Owner, as the ledger records it: PR5–PR7 implementer (the slice that owns `rundir`).

**Out of PR4's scope, deliberately.** Packet keys: `decisions.resource_accounting.rows[R28].lifecycle.held` and `invariants[17].recovery` (INV-18). PR4's `slice_contract.owned_resources` names **R22, R4 and RunnerPolicy** and its `scope` does not include `src/rundir.rs`, so the refusal these two attack belongs to another slice's ledger. What PR4 does own of R28 is the *reaper's* side, and that is now witnessed: `agent::proc::termination::tests::the_reapers_cleanup_hold_is_shared_between_overlapping_invocations` pins the hold as shared (`PR4-WIN-072`), and `agent::proc::tests::every_unix_containment_point_is_measured_against_its_own_operation` asserts that at `Spawn.ReaperStarted` an exclusive probe of the live lease is already refused. Recorded rather than dropped so the coordinator half is visible as owed

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
