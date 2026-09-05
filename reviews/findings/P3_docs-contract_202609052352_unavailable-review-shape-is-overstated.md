---
id: PR166-UNAVAILABLE-REVIEW-SHAPE-DOCUMENTATION
severity: P3
disposition: deferred
category: docs-contract
pr: 166
reviewed_sha: fb46ebec8297099dec72ae6655d662f2d7975758
location: docs/internals/status.md:218
provenance: undetermined
first_bad:
guard: Describe the three production mappings, cite review_failure, and align future ledger claims with the tests at src/engine/tests.rs:2976-3000.
---

## Failure sequence

The status notes at lines 218-223, status/render notes at lines 154-164 and the SWEEP-RENDER-002 PR ledger row claim unavailable reviews always carry ReviewUnavailable and cite nonexistent engine::attempt::evaluate_review. Production review_failure in src/engine/attempt.rs maps rate-limited unavailability to RateLimited, a timeout to Timeout, and other unavailability to ReviewUnavailable. All have a reviewer unavailable reason. The renderer correctly combines the recorded reason with ReviewPassOutcome; this finding concerns the documentation and evidence claim.

## What the change that takes this up should do

Describe the three production mappings, cite review_failure, and align future ledger claims with the tests at src/engine/tests.rs:2976-3000.

## Review and disposition

The [gpt-5.6-sol max review of PR #166](https://github.com/eventloops/upstroke/pull/166#issuecomment-5555195504) reported this as a low-impact documentation finding without a numeric severity. P3 is delivery triage, not a reviewer-assigned label. The owner merged #166 and requested these two documentation findings be recorded on #172 after master integration. This record is unresolved and claims no repair. The original review verdict remains CHANGES_REQUIRED.
