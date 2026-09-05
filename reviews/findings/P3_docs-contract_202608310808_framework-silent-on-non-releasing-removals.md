---
id: PR3-FRAMEWORK-SILENT-1
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

Non-releasing removals leave `rows: []` — the packet fixes the pruning case (R27) but says nothing about removals with no objects to release

## What the change that takes this up should do

Owner, as the ledger records it: PR7–PR10 implementer.

Derived by applying the pruning reading: the row that accounted for what was removed no longer holds it. After stays distinguishable from Before by artifact (`Removed` vs `Nothing`) and by action

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
