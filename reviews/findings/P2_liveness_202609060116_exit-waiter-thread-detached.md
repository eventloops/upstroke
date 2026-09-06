---
id: PR173-EXIT-WAITER-THREAD-DETACHED
severity: P2
disposition: deferred
category: liveness
pr: 173
reviewed_sha: 429afd082e5628e8131627bc822dcc882de29aed
location: src/agent/proc.rs:499
provenance: introduced_by_feature
first_bad:
guard: the next push to PR #173, or the change that next opens `await_exit_without_reaping`
---

## Failure sequence

`await_exit_without_reaping` spawns a helper thread that blocks in `waitid(P_PID, pid, WEXITED | WNOWAIT)` and drops its `JoinHandle`; the caller's `recv_timeout(budget)` bounds only the receiver -> after the budget the helper stays in `waitid`, the two callers discard the `kill` and `wait` errors, and `wait()` has no bound -> a child in an uninterruptible kernel wait passes the 30-second receiver deadline, keeps the helper thread alive, and the cleanup can hang. This is `standards/10_standards_concurrency.md`'s rule that every spawned thread is joined and a worker's failure becomes a defined outcome, and §12's "every wait is bounded"; the body's "no error is swallowed" was not true of the callers.

## What the change that takes this up should do

Keep the `JoinHandle` and return it to the caller with the timeout, so the caller kills and reaps the child (which ends the helper's `waitid` with `ECHILD`) and then joins the helper, reporting the kill and wait results instead of discarding them; or replace the thread with a bounded poll of `exited_unreaped` at a short interval, stated as a bound on a wedged child rather than a synchronisation sleep. Say which in the notes.

Recorded from the frontier pass of 2026-09-06 (`gpt-5.6-sol`, max effort) on PR #173 at `429afd0`, posted as https://github.com/eventloops/upstroke/pull/173#issuecomment-5556000412. Filed as the reviewer wrote it, with the author's reading beneath.
