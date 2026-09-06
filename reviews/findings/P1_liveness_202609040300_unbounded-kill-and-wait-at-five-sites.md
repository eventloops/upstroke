---
id: PR125-CLOSE-UNBOUNDED-KILL-AND-WAIT-AT-FIVE-SITES
severity: P1
disposition: deferred
category: liveness
pr: 125
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/agent/proc.rs:2325
provenance: pre_existing
first_bad: 6798089 (the barrier and the sites)
guard: deferred: a bounded end of a helper, asking waitpid(pid, WNOHANG) first and then polling to a short named budget, after which a child still alive…
---

## Failure sequence

five sites kill a forked helper and then block in `waitpid(pid, 0)` without a bound: `Reaper::abandon` through `close_and_wait`, the `setpgid` failure in `spawn_reaper`, `Guard::abort_setup`, and the descriptor-configuration and READY failures in `spawn_guard` -> the helper is in uninterruptible I/O (a stalled `open` of a cleanup lease or a `close` of an inherited descriptor) with SIGKILL pending -> the reaper's callers hold the launch barrier, under which the signal monitor refuses to kill or stop any registered group, so every running agent outlives a SIGTERM for as long as the kernel takes; the guard's callers hold supervisor initialisation

## What the change that takes this up should do

deferred: a bounded end of a helper, asking `waitpid(pid, WNOHANG)` first and then polling to a short named budget, after which a child still alive is left for the process's exit to collect; it must report only what `waitpid` and `kill` actually returned (row PR125-CLOSE-DISCARDED-KILL-RESULT) and must not claim the pid's identity from a `WNOHANG` zero (row PR125-CLOSE-PID-IDENTITY-UNDER-A-HOST-WILDCARD-WAITER); the acknowledged-exit wait in `close_and_wait` after CLEANUP or CANCEL stays unbounded, because the reaper's exit is what releases the cleanup lease the caller depends on; the closed pull request's `kill_and_reap_helper` at its `33604e6` is a starting shape with those two defects

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
