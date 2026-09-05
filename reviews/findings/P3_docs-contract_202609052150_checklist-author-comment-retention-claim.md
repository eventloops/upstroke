---
id: PR169-CHECKLIST-AUTHOR-COMMENT-RETENTION-CLAIM
severity: P3
disposition: deferred
category: docs-contract
pr: 169
reviewed_sha: 985a2a2848af74c41f2acc99757a42586f362800
location: docs/internals/plan/markdown/annotation.md:46
provenance: undetermined
first_bad:
guard: Qualify the documentation to describe section and checklist behavior, or deliberately specify and implement consistent retention with coverage.
---

## Failure sequence

The annotation notes say an ordinary author comment stays in the body. In a checklist task, the annotation sink declines a comment such as `<!-- keep rollback enabled -->`, then the drafts builder omits Event::Html. Section tasks retain it. The documentation claim is broader than the implemented behavior. No design requirement to retain arbitrary HTML was established by the review.

## What the change that takes this up should do

Qualify the documentation to describe section and checklist behavior, or deliberately specify and implement consistent retention with coverage.

## Review and owner disposition

[Independent gpt-5.6-sol max review](https://github.com/eventloops/upstroke/pull/169#issuecomment-5554892946) returned CHANGES_REQUIRED on the reviewed SHA. The owner directed merging PR #169 and recording both unresolved findings in PR #166 after merging master into its branch. This record carries the finding forward; it does not claim a fix or a passing review. Introduction history has not been established.
