---
id: PR125-CLOSE-BODY-ACCOUNTING-AND-TITLE
severity: P3
disposition: deferred
category: docs-contract
pr: 125
reviewed_sha: 33604e648aa06fdd0551526b3b8f95d3676df7ae
location: src/agent/proc.rs:1451
provenance: introduced_by_feature
first_bad: 77be7c3
guard: deferred: recorded so the next attempt counts the calls from the code and retitles on a narrowing
---

## Failure sequence

the closed pull request's title still claimed a load-tolerant READY budget after the budget was withdrawn, and its body counted the child's pre-READY work as "eight `sigaction`s" -> `scrub_private_helper_dispositions` attempts `sigaction` for every signal from 1 through 128 except SIGKILL and SIGSTOP, 126 calls, before the explicit setup -> a body's accounting of a child's work is a claim a reviewer checks against the code, and a title is retitled when the change it names is withdrawn

## What the change that takes this up should do

deferred: recorded so the next attempt counts the calls from the code and retitles on a narrowing

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
