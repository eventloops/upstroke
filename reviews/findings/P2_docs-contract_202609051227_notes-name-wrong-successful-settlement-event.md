---
id: PR160-NOTES-SUCCESS-SETTLEMENT
severity: P2
disposition: deferred
category: docs-contract
pr: 160
reviewed_sha: 41eb825d32a598f9c1b19e5ae93ae510786b3d8f
location: docs/internals/engine/topology/run.md:620
provenance: pre_existing
first_bad: undetermined
guard: the_driver_carries_an_accepted_attempt_through_the_candidate_sequence checks the successful durable event sequence; candidate_prepared_is_the_sole_successful_settlement checks the settlement contract.
---

## Failure sequence

A reader uses the Progress::Settled contract to reconstruct a successful attempt's durable record. The notes say the branch ends with attempt_finished. For Progress::Settled with accepted true, the reviewed source instead calls promote_candidate and records candidate_prepared followed by task_candidate_created. There is no successful attempt_finished event to find.

src/engine/topology/run.rs:831 takes the successful path before the failure-settlement path. The successful driver test at src/engine/topology/recover/tests.rs:3816 explicitly expects candidate_prepared and task_candidate_created, with no attempt_finished. It passed in the independent 304-test topology run. design/15_design_event_log_resume_run_layout.md:53 states the same successful-settlement rule. The inaccurate sentence already exists in the source at comparison base 3a08a1f33456cba159d05f667c72d01e4320767f.

## What the change that takes this up should do

Describe the success and failure settlement events separately. Keep the documentation consistent with the accepted flag and the existing event-sequence tests. This is a documentation follow-up and does not block approval under DOCS_FAST_TRACK.md.
