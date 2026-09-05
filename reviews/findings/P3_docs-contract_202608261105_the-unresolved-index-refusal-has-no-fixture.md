---
id: PR7-R3-CONTRACT-004-UNRESOLVED-INDEX-REFUSAL-UNREACHABLE
severity: P3
disposition: deferred
category: docs-contract
pr: 7
reviewed_sha: 
location: 
provenance: pre_existing
first_bad: 
guard: the project owner, as a G2 erratum question
---

## Failure sequence

`expected_failures_refusals` names "empty-diff **and unresolved-index** attempt failures".
The empty-diff half is produced and named. No fixture reaches the unresolved-index half, so a
packet-required refusal path has no executed proof and may not be implemented at all.

## What the change that takes this up should do

Answer the prior question first — whether the clause is this slice's at all — and then either
produce the refusal with a fixture that reaches it, or amend the clause. Raising it as an erratum
rather than a repair is deliberate: the ownership is genuinely unclear and guessing it would put a
fixture in the wrong slice.

Recorded in `reviews/FINDINGS.md` §20. Severity is this migration's judgement.
