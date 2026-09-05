---
id: PR125-CLOSE-GROUP-SCANNER-RETRIES-UNDER-THE-BARRIER
severity: P2
disposition: deferred
category: liveness
pr: 125
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/agent/proc.rs:2341
provenance: pre_existing
first_bad: 6798089 (the barrier); the scanner's retry is older
guard: deferred: named for the file's owner; any change that makes the READY wait give way to a termination must cover this retry too, or the coverage…
---

## Failure sequence

`spawn_reaper` calls `verify_group_scanner` under the launch barrier, before the reaper's READY wait -> the scanner retries for up to two seconds without looking at a pending termination -> a running group outlives a SIGTERM by that interval while the launch holds the barrier; found by pass 7 while the READY wait was interruptible and left as master has it when that was withdrawn

## What the change that takes this up should do

deferred: named for the file's owner; any change that makes the READY wait give way to a termination must cover this retry too, or the coverage claim is false

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
