---
id: PR160-NOTES-INCOMPLETE-BRANCHES
severity: P2
disposition: deferred
category: docs-contract
pr: 160
reviewed_sha: 41eb825d32a598f9c1b19e5ae93ae510786b3d8f
location: docs/internals/engine/topology/run.md:268
provenance: pre_existing
first_bad: undetermined
guard: Compare both branch descriptions with TopologyRun::step and retry_ready, and retain the_driver_takes_over_from_the_recovery_order_and_steps as executable evidence of dispatch and settlement.
---

## Failure sequence

A maintainer reads the ReadyDispatch section to determine what the branch performs. Lines 268 through 276 say this build stops at OpenNoAttempt before running and settling an attempt. The ReadyRetry section at lines 285 through 288 likewise says running and settling the retry is still owed. Both statements describe completed work as missing and contradict the current descriptions immediately below them.

On the reviewed source, TopologyRun::step calls attempt and settle before returning Progress::Settled. retry_ready does the same at src/engine/topology/run.rs:669. The independent topology suite passed, including the_driver_takes_over_from_the_recovery_order_and_steps. The same stale statements exist in src/engine/topology/run.rs at comparison base 3a08a1f33456cba159d05f667c72d01e4320767f, so the migration preserved an existing documentation defect.

## What the change that takes this up should do

Describe the currently implemented dispatch and retry paths. If the earlier partial implementation remains useful history, label those paragraphs as history and identify the version they describe. This is a documentation follow-up. Under DOCS_FAST_TRACK.md, it does not require repair before this docs-only PR receives approval.
