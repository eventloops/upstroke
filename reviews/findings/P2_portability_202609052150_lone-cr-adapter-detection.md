---
id: PR169-LONE-CR-ADAPTER-DETECTION
severity: P2
disposition: deferred
category: portability
pr: 169
reviewed_sha: 985a2a2848af74c41f2acc99757a42586f362800
location: src/plan/markdown.rs:30
provenance: undetermined
first_bad:
guard: Normalize or recognize lone CR at adapter detection and add a production detect/validate regression covering LF, CRLF and CR.
---

## Failure sequence

A plan uses lone-CR line endings, for example `Preamble\r## Fix bug\r<!-- upstroke: min=frontier -->\r`. The production `plan::detect` path calls `sniff`, whose `raw.lines()` does not split lone CR. It rejects the plan before `parser_source` normalizes its line endings. LF and CRLF versions are accepted. The tests at markdown.rs:248-264 and validate.rs:344-375 call `parse_with_warnings` directly and miss this composition. The claimed lone-CR support in design section 9 is therefore incomplete.

## What the change that takes this up should do

Normalize or recognize lone CR at adapter detection and add a production detect/validate regression covering LF, CRLF and CR.

## Review and owner disposition

[Independent gpt-5.6-sol max review](https://github.com/eventloops/upstroke/pull/169#issuecomment-5554892946) returned CHANGES_REQUIRED on the reviewed SHA. The owner directed merging PR #169 and recording both unresolved findings in PR #166 after merging master into its branch. This record carries the finding forward; it does not claim a fix or a passing review. Introduction history has not been established.
