---
id: PR73-LINT-SEMANTICS-001
severity: P2
disposition: deferred
category: correctness
pr: 73
reviewed_sha: 0f05b456
location: 
provenance: pre_existing
first_bad: 
guard: the project owner — the deferral carries a named backstop restriction until it is repaired
---

## Failure sequence

A narrowed residual about the lint semantics the tooling assumes, left open by the owner's adjudication of PR #73 on
2026-08-30. The residual is narrowed rather than closed: the behaviour the tooling relies on is not
established, and the restriction that stands in for it is a named backstop rather than a proof.

## What the change that takes this up should do

Repair the residual and remove the backstop restriction the adjudication attached to it.
Until then the restriction is what makes the deferral safe, so a change that relaxes the restriction
without closing the residual reopens the gap. The comparison head for anything measured against this
row is `0f05b456`.

Recorded in `reviews/FINDINGS.md` §29, “2026-08-30 PR #73 owner adjudication — narrowed residuals and temporary restrictions”. Severity is this migration's judgement; the adjudication assigned none.
