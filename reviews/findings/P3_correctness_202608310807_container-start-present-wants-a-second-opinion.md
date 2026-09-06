---
id: PR3-CONTAINER-START-ROW
severity: P3
disposition: deferred
category: correctness
pr: 3
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR6/PR7 implementer
---

## Failure sequence

`Container.Start → Present` is the least obvious row in the semantics table

## What the change that takes this up should do

Owner, as the ledger records it: PR6/PR7 implementer.

Flagged by repair round 4 as the row most worth a second opinion

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
