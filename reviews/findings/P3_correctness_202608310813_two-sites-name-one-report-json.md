---
id: PR3-REPORT-DOUBLE-NAME
severity: P3
disposition: deferred
category: correctness
pr: 3
reviewed_sha:
location: report.json
provenance: undetermined
first_bad:
guard: project owner
---

## Failure sequence

`RunDir.WriteReport` and the `Report` group both name `report.json`, so ST-07 will demand two hook executions for one write

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

Found by A3, implemented as written and reported

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
