---
id: PR172-DARWIN-GUARD-LOOP-BLIND-TO-PARENT-DEATH
severity: P2
disposition: deferred
category: liveness
pr: 172
reviewed_sha: b0ff0edf6629bd105ccecbbc89dcf7f51a6c765e
location: src/agent/proc.rs:2897
provenance: pre_existing
first_bad: —
guard: deferred: the guard's command and wake pipes are FIFOs on Darwin and `guard_loop` waits on them with `poll(-1)` while not stopping; `poll` on a Darwin FIFO never reports the writer's close (PR172's `wait_readable` note), so a conductor that dies leaves its guard blocked until the machine restarts; the change that takes this up bounds the guard's idle wait the way `reaper_loop` bounds its own (a slice and a `getppid` check) or waits with `select`, and adds a native macOS test that a guard whose parent has exited ends
---

## Failure sequence

The conductor forks its job-control guard (`spawn_guard`), whose `guard_loop` waits on its command
and wake FIFOs with `poll(…, -1)` whenever it is not in the armed-and-stopping state -> the
conductor exits or is killed, closing the only writers of both FIFOs -> on Linux the pipes report
hangup, the guard reads zero bytes and `_exit(0)`s; on Darwin `poll` on a FIFO reports data and
never the writer's close, so the guard stays blocked in `poll` with no timeout -> one orphaned
guard process per conductor that ever ran on a macOS host, until the host restarts or something
signals it (reasoned from the measured `poll` behaviour and the loop as written; not yet observed on a macOS host). The reaper does not have this defect: `reaper_loop` polls in 10 ms slices and checks
`getppid` on every one, which its note now says is the reason for the slice.

## What the change that takes this up should do

Bound the guard's idle wait as the reaper's is bounded — a slice and a `getppid` check on every
timeout, or a `select` through `wait_readable`'s Darwin path extended to two descriptors — and add
a test on the macOS leg that a guard whose parent has exited ends within a bound the test observes
by reaping it, not by a wall-clock assertion on the guard's own work. Nothing in PR #172 changed
the guard's loop: that pull request bounded the parent's READY wait and left the child's loops as
they were, so this row records the sibling defect the same mechanism implies.
