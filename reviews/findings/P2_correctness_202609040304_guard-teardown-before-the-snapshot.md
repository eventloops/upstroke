---
id: PR125-CLOSE-GUARD-TEARDOWN-BEFORE-THE-SNAPSHOT
severity: P2
disposition: deferred
category: correctness
pr: 125
reviewed_sha: 33604e648aa06fdd0551526b3b8f95d3676df7ae
location: src/agent/proc.rs:3234
provenance: introduced_by_feature
first_bad: 77be7c3
guard: deferred: the snapshot is read before any descriptor is closed, at both helpers, and both messages carry open_max; a test drives the ordering with…
---

## Failure sequence

on a guard READY failure the closed pull request closed the command and acknowledgement descriptors before reading `helper_snapshot` -> a slow guard reaching its READY write just then finds no reader, takes EPIPE and exits -> the snapshot reports "exited and not yet reaped" for a guard that was alive at the deadline and was killed by the diagnostic's own teardown; the guard's message also omitted `open_max` while the body claimed both messages carried it

## What the change that takes this up should do

deferred: the snapshot is read before any descriptor is closed, at both helpers, and both messages carry `open_max`; a test drives the ordering with a helper that writes READY at the deadline

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
