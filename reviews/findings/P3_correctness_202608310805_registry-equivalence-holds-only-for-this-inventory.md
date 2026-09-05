---
id: PR3-REG-001-CONDITIONAL
severity: P3
disposition: deferred
category: correctness
pr: 3
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR4-PR10 implementer
---

## Failure sequence

`A3-REG-001` is equivalent *for the current inventory*, because every constructible site exposes zero or one observable order

## What the change that takes this up should do

Owner, as the ledger records it: PR4-PR10 implementer.

It becomes live debt the moment any site exposes more than one observable order. Conditional debt, not closed

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
