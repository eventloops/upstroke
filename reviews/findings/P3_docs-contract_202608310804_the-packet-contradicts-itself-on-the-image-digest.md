---
id: PR3-RUNNER-DIGEST
severity: P3
disposition: deferred
category: docs-contract
pr: 3
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: project owner
---

## Failure sequence

The packet contradicts itself: `decisions.task_registry.validation_at_fold` requires the container image digest "when Container"; `INV-23` has it "when reported"

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

A Container run whose runtime reports no manifest digest is legitimate under one reading and refused under the other. PR3 implemented INV-23 consistently across A1 and A2 and said so per refusal

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
