---
id: PR3-BEFORE-PHASE-SCOPE
severity: P2
disposition: deferred
category: correctness
pr: 3
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR7–PR10 implementer
---

## Failure sequence

Before-phase rows name the site's own artifact, not the transaction's whole durable prefix — so `Worktree.Add/Before` is empty although R9 already holds the intent

## What the change that takes this up should do

Owner, as the ledger records it: PR7–PR10 implementer.

Chosen deliberately by repair round 4, documented on the type and asserted as a test so it reads as a decision rather than an omission. The repair itself names it as the largest remaining place a finding could live, in either direction

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
