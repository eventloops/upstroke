---
status: owner attention required
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

## Owner attention required

Recorded 2026-09-06T05:30:30.030152+00:00. Workflow task 9dc6604a62e3.

Review pass 2 of 2 returned CHANGES_REQUIRED with one blocking P2, so both passes of this
workflow's budget are spent and the ticket is parked for owner attention rather than merged.
Nothing has been merged into master; PR #179 stays open as a draft with its branch, worktree and
evidence retained.

WHAT IS DONE AND GREEN. The finding itself, PR173-EXIT-WAITER-THREAD-DETACHED, is fixed at
127f35abc3352295a49543c3c413fddafe81ca5e: `await_exit_without_reaping` no longer spawns a helper
thread at all. It polls `exited_unreaped` — the `WEXITED | WNOHANG | WNOWAIT` question the
supervisor loop already asks — every 5 ms until the answer is yes or the budget is spent, so there
is no worker to join and nothing outlives the call. `ReapedChild` settles through one bounded,
reporting path instead of the discarded `kill`/`wait` pair with no bound. The nine-command baseline
passes at that head (ALL 9 PASS at 127f35a) and all eleven CI legs plus both required contexts are
green on that exact head. The witness fails on the base and passes on the fix, re-measured against
the repaired tests, and four mutations kill it — including the one that matters, a blinded census,
which fails at its own control bound rather than passing vacuously.

WHAT REMAINS, AND WHY IT NEEDS THE OWNER.

  EXIT-WAITER-CONTROL-DETACHED-ON-FAILURE, P2, liveness, raised at pass 1 against
  16cd9ddc00775a9522401e5ca5123343cfa9d988 and RAISED AGAIN at pass 2 against
  127f35abc3352295a49543c3c413fddafe81ca5e (src/agent/proc/tests.rs:790), in code this PR adds.

  Pass 2's sequence, verbatim in substance: if the control child remains wedged after `kill`,
  `child.settle()` times out while the control thread is still blocked in `waitid`; `join_within`
  then returns the unfinished `JoinHandle`, and `ControlWaiter::settle` drops it on that arm.
  `self.handle` has already been taken, so `Drop` cannot recover or join the worker, and during an
  unwind the cleanup diagnostic is discarded too. The guard therefore still abandons a blocked
  worker on the cleanup-timeout path.

  The finding is factually correct about the code as written, and the pass-1 repair narrowed it
  rather than closing it: the abandonment moved from three fallible steps in the test body into one
  arm of one method, and that arm is reported rather than silent, but a dropped handle is a dropped
  handle.

  WHY IT WAS NOT FIXED A THIRD TIME. A thread that cannot be made to return cannot be joined
  without an unbounded wait, so any bounded cleanup must eventually either block or abandon. The
  reviewer names the only two ways out — "make the control worker cancellable or isolate the
  blocking control" — and both are larger than a repair this workflow may make without a review to
  read it:

    - CANCELLABLE. `waitid` returns `EINTR` when a signal reaches the thread, so the control could
      be cancelled with `pthread_kill` against a handler installed for the purpose. That installs a
      PROCESS-WIDE signal disposition from inside one test of a suite whose other bodies spawn,
      supervise and reap children and assert on their signal outcomes, and `libc::sigaction` and
      `libc::pthread_sigmask` are governed effect primitives. It is a real design, not a patch.
    - ISOLATED. The control could run in a forked child process instead of a thread. The census
      then has to read `/proc/<other>/task` while the claim reads `/proc/self/task`, so the control
      would exercise the instrument in a different domain from the one it is vouching for, which
      weakens exactly the property the control exists to supply.

  There is a third answer the owner may prefer and which no reviewer can grant from inside this
  workflow: JUDGE THE ARM UNREACHABLE. The control's child is `/bin/sh` blocked reading a pipe.
  That is an interruptible sleep; SIGKILL always terminates it, and it never enters the
  uninterruptible kernel wait the timeout arm is written for. On that reading the arm is defensive
  code for a state this fixture cannot reach, and the honest disposition is an accepted-risk ledger
  row rather than a redesign. That is a judgement about acceptable residual risk in test-support
  code, and it belongs to the owner, not to an implementor with no passes left.

WHAT THE OWNER MIGHT DECIDE. Any of: authorise a third review pass and let the cancellable or
isolated control be built and read; accept the residual with a ledger row and merge as is; drop the
end-to-end positive control and keep only the classifier and census unit controls, at the cost of
the instrument's end-to-end vouching; or take the production fix without the Linux census body at
all, keeping `an_exit_wait_on_a_child_that_never_exits_ends_at_its_bound` as the portable witness —
which does bound the wait but does not witness the detached thread.

EVIDENCE. Both verdicts are published verbatim as SHA-bound comments on PR #179:
pass 1 https://github.com/eventloops/upstroke/pull/179#issuecomment-5557036877 and
pass 2 https://github.com/eventloops/upstroke/pull/179#issuecomment-5557197371.
The reproduction, the passing run, the mutation catalogue and the effect-site measurement are in
/home/ubuntu/findings-workflow/tasks/9dc6604a62e3/evidence/. Both reviews were `gpt-6-astra` at
high effort under the owner's override, read-only, with a 90-minute limit; each recorded that it
did not independently rerun the tests or mutations.
