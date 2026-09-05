---
id: PR3-FRAMEWORK-SILENT-4
severity: P3
disposition: deferred
category: docs-contract
pr: 3
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR7–PR10 implementer
---

## Failure sequence

`Event.OpenLog`'s `Create` and `TruncateTornTail`: kill → `NextOpenConverges`, error-return → `RefuseResumably`

## What the change that takes this up should do

Owner, as the ledger records it: PR7–PR10 implementer.

The packet elaborates only `SyncPrefix`, giving one action in both modes; this table gives one action in both modes by the same shape

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
