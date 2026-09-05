---
id: PR125-CLOSE-READDIR-ERRORS-COUNTED-AS-DESCRIPTORS
severity: P2
disposition: deferred
category: correctness
pr: 125
reviewed_sha: 33604e648aa06fdd0551526b3b8f95d3676df7ae
location: src/agent/proc.rs:3940
provenance: introduced_by_feature
first_bad: d530899
guard: deferred: an Err entry makes the whole count "not readable"; the test that proves the reader has an arm for it
---

## Failure sequence

the Linux descriptor reader counted `read_dir` entries with `entries.count()` -> `ReadDir` yields `Result<DirEntry>`, so an entry that errors is counted as a descriptor -> the stated contract, a failed query is "not readable" and never a count, did not hold for a partially readable table

## What the change that takes this up should do

deferred: an `Err` entry makes the whole count "not readable"; the test that proves the reader has an arm for it

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
