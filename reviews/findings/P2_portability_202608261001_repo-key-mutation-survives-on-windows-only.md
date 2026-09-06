---
id: PR5-WORKSPACE-003
severity: P2
disposition: deferred
category: portability
pr: 5
reviewed_sha: 
location: src/workspace_manager.rs
provenance: pre_existing
first_bad: 
guard: the project owner — the G2 adjudication sitting
---

## Failure sequence

A withheld-mutation-catalogue entry targeting `WorkspaceManager::repo_key`. It is `KILLED`
on Linux and `SURVIVED` on the Windows guest — the reverse of `PR5-EVENTS-006`'s pattern. A
Linux-only run reports the detection as adequate; it is not adequate on Windows.

## What the change that takes this up should do

Measure it on the guest and adjudicate: either the Windows path has a genuine detection gap
and needs an assertion that kills there, or the re-expressed mutation is equivalent on that
platform. Do not carry it forward as a regression without that measurement — that is the claim §15
exists to avoid making without evidence.

Recorded in `reviews/FINDINGS.md` §15 as one of the six entries needing adjudication. Severity is this migration's judgement.
