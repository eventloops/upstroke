---
id: PR125-CLOSE-MACOS-READY-RED-CAUSE-UNKNOWN
severity: P1
disposition: deferred
category: liveness
pr: 125
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/agent/proc.rs:2461
provenance: undetermined
first_bad: W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT is the other macOS fingerprint of the same period, not this one; first sighting of this one 2026-09-03 on master
guard: deferred: the cause is not established and a bigger budget did not help at the exact head, so the next change is a diagnostic, not a budget: on a…
---

## Failure sequence

master's macOS test leg fails "Unix cleanup reaper did not initialize" (2026-09-03: `2de71dd`, `17d41c9`, `ae2a58f`; PR #125's `ecc9aa1`, run 33821116191) -> the parent forks the reaper, which scrubs dispositions, `setpgid`s, closes every descriptor number up to the ceiling one `close` at a time on Darwin, takes each cleanup lease with an `open` and a non-blocking `flock`, and writes READY -> nothing arrives within the budget, and the launch fails with no other information; at a ten-second budget the exact head failed with "waited 10.000190708s of 10s; descriptor ceiling 10240", so the parent polled on time and the child was silent for ten seconds, which the ordinary cost of its work (milliseconds) does not explain

## What the change that takes this up should do

deferred: the cause is not established and a bigger budget did not help at the exact head, so the next change is a diagnostic, not a budget: on a READY failure, read what the parent can see of the child before anything is closed or killed (state from `/proc` on Linux and `proc_pidinfo` on Darwin, open-descriptor count where the host answers, a failed or short query reported as unknown and an `Err` directory entry as unreadable, never as a count), carry `open_max` and the elapsed wait, and do it for the guard as well as the reaper; the closed pull request's `helper_snapshot` at `33604e6` is the shape, minus the two defects in rows PR125-CLOSE-GUARD-TEARDOWN-BEFORE-THE-SNAPSHOT and PR125-CLOSE-READDIR-ERRORS-COUNTED-AS-DESCRIPTORS; what is established: the child's `open` and `close` can block, and a parent that polls on time measures nothing about the child's scheduling

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
