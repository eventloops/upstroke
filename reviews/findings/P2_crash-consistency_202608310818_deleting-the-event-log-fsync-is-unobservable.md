---
id: PR5-C-FSYNC-UNOBSERVABLE
severity: P2
disposition: deferred
category: crash-consistency
pr: 5
reviewed_sha:
location: src/events/log.rs:934
provenance: undetermined
first_bad:
guard: PR7–PR11 implementer (the slice that owns the two-crash proof)
---

## Failure sequence

**Deleting the `sync_all()` call in `events::log::sync_log_file` is undetectable by any test on this machine.** An fsync has no user-space observable effect: the ledger entry the suite reads would still be written, the byte length would still be the filesystem's own answer, and only a power loss could tell the difference. Every `SyncPrefix` test therefore proves that the funnel *reached* the sync and *recorded* it, not that the data reached the platter

## What the change that takes this up should do

Owner, as the ledger records it: PR7–PR11 implementer (the slice that owns the two-crash proof).

**Carried, not hidden.** The residual boundary is stated on the function itself (`src/events/log.rs:934`) rather than left for a reviewer to discover, and the mitigation that *is* possible is taken: the sync and its ledger entry are **one call**, because with them written as two statements a mutation that moves the `SyncPrefix` consult to *between* them puts the injection after the syscall and before the only thing that can see it — measured surviving the suite. Fused, the only place the consult can move to is after the record, where `an_injected_sync_failure_at_open_names_syncprefix_and_hands_out_no_handle` kills it. The packet names the test that would close this for real — `transaction_fault_matrix[T-PREPARED].test`'s `unsynced_merge_prepared_two_crash_barrier_before_cas_then_power_loss_keeps_log_and_ref_agreeing` — and it needs a coordinator, a CAS and a simulated power loss, none of which are PR5's

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
