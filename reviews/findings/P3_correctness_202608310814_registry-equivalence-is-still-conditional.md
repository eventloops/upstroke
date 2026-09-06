---
id: PR4-REG-001-STILL-EQUIVALENT
severity: P3
disposition: deferred
category: correctness
pr: 4
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR4–PR10 implementer
---

## Failure sequence

`PR3-REG-001-CONDITIONAL` becomes live debt the moment any site exposes more than one observable order

## What the change that takes this up should do

Owner, as the ledger records it: PR4–PR10 implementer.

**Re-checked, still conditional.** The same test asserts `Process.Spawn.observable_orders() == [EventBeforeEffect]` — one order — so the order-free registry key stays equivalent for the one site this slice uses. Not closed; re-measured

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
