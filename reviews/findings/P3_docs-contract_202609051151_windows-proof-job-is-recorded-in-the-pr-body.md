---
id: PR139-WINDOWS-CLAIM-LIVES-IN-THE-BODY
severity: P3
disposition: accepted-risk
category: docs-contract
pr: 139
reviewed_sha: 0924bf02de749e9099ae1443cd156dcbcae26054
location: src/rundir/tests.rs:1785
provenance: introduced_by_feature
first_bad:
guard: every_conjunct_of_the_ownership_proof_refuses_on_its_own; exact-candidate Windows CI evidence in PR 139
---

## Failure sequence

A reader follows the ownership-proof test's platform claim expecting a specific Windows CI job in the source comment. The comment directs the reader to the PR's evidence, so the source alone does not identify that job. This does not establish a code failure or a failed platform check.

## What the change that takes this up should do

Keep the Windows result tied to the tested commit in the PR evidence. If the project later requires CI provenance within source documentation, define a durable reference format and apply it consistently. This PR retains the platform regression and records the exact candidate's Windows result when available; it does not embed a changing job ID in the test comment.
