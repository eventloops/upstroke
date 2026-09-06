---
id: PR125-CLOSE-MACOS-READY-RED-CAUSE-UNKNOWN
severity: P1
disposition: deferred
category: liveness
pr: 125
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/agent/proc.rs:2461
provenance: pre_existing
first_bad: W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT is the other macOS fingerprint of the same period, not this one; first sighting of this one 2026-09-03 on master; the parent-side `setpgid(pid, pid)` that races the reaper's own predates the first sighting
guard: deferred: PR #172 established both halves of the cause and repaired both (the Darwin READY wait could not see the helper end, `a_helper_that_has_already_exited_ends_the_acknowledgement_wait_at_end_of_file`; the parent's `setpgid(pid, pid)` raced the reaper's `setpgid(0, 0)` into `EPERM` on Darwin, removed, with the reaper now reporting the step that refused, `a_reaper_refused_its_cleanup_lease_says_which_lease_and_why`); the row stays open only for confirmation, and closes when the macOS leg has shown no READY failure across the pull requests that merge after #172 for a week, or sooner if the owner reads the evidence as sufficient; a READY failure after #172 carries the step and errno and is a new row, not this one
---

## Failure sequence

master's macOS test leg fails "Unix cleanup reaper did not initialize" (2026-09-03: `2de71dd`, `17d41c9`, `ae2a58f`; PR #125's `ecc9aa1`, run 33821116191) -> the parent forks the reaper, which scrubs dispositions, `setpgid`s, closes every descriptor number up to the ceiling one `close` at a time on Darwin, takes each cleanup lease with an `open` and a non-blocking `flock`, and writes READY -> nothing arrives within the budget, and the launch fails with no other information; at a ten-second budget the exact head failed with "waited 10.000190708s of 10s; descriptor ceiling 10240", so the parent polled on time and the child was silent for ten seconds, which the ordinary cost of its work (milliseconds) does not explain

## What the change that takes this up should do

deferred: the cause is not established and a bigger budget did not help at the exact head, so the next change is a diagnostic, not a budget: on a READY failure, read what the parent can see of the child before anything is closed or killed (state from `/proc` on Linux and `proc_pidinfo` on Darwin, open-descriptor count where the host answers, a failed or short query reported as unknown and an `Err` directory entry as unreadable, never as a count), carry `open_max` and the elapsed wait, and do it for the guard as well as the reaper; the closed pull request's `helper_snapshot` at `33604e6` is the shape, minus the two defects in rows PR125-CLOSE-GUARD-TEARDOWN-BEFORE-THE-SNAPSHOT and PR125-CLOSE-READDIR-ERRORS-COUNTED-AS-DESCRIPTORS; what is established: the child's `open` and `close` can block, and a parent that polls on time measures nothing about the child's scheduling

Recorded by PR #125, closed after eight frontier passes; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.

### Recurrence on PR #156, 2026-09-05

At PR #156 head `20f0665a526ea89e15e422239834dc07c9efc28d`, the first
attempt of [CI run 33985546484](https://github.com/eventloops/upstroke/actions/runs/33985546484/attempts/1)
failed in the [macOS test job](https://github.com/eventloops/upstroke/actions/runs/33985546484/job/101358262737).
Both `engine::tests::an_aborting_error_still_leaves_a_replayable_log` and
`engine::tests::an_exhausted_pool_and_a_silent_operator_still_terminate` failed
because the Unix cleanup reaper did not initialize. The reported waits were
2.001084542s and 2.008473791s against a two-second budget, with descriptor
ceiling 10240. In both cases the diagnostic said SIGKILL was delivered and
the wait collected a child that had already exited with status 1.

The macOS suite reported 1,967 passed, two failed and 37 ignored. All other
jobs passed. One failed-jobs-only retry,
[attempt 2](https://github.com/eventloops/upstroke/actions/runs/33985546484/attempts/2),
passed at the unchanged PR head. This is another occurrence of the recorded
startup failure, not evidence that the retry fixed it or established its cause.

The original job log and attempt metadata are retained at
`/srv/worktrees/astra-20260905/sequential-delivery/156/ci-33985546484-attempt-1`.
The macOS log SHA-256 is
`b219174301bf67b6f000117df22585d0676315b0f84ff13834b88ea8a9f9f150`.
#154 owns consolidated evidence; #167 owns the startup diagnostic work.
Diagnostics are not themselves a verified runtime repair.

The repository owner explicitly permits #156 to merge with this known issue
still open and directed that this recurrence be documented in #171, without
changing #156's head or restarting its CI. This is a PR-specific acceptance
of the existing runtime risk, not a severity downgrade, a claim of a fix, or
a general waiver for other PRs. The canonical P1 remains open with its
historical metadata and repair requirements unchanged.

### What PR #172 established, 2026-09-05

**The shape of every recorded occurrence is explained; the helper's own exit is not.** Two facts
were established, one by measurement on the macOS runner CI uses and one by reading the tree.

**Why a bigger budget could not help, and why every failure read as a timeout.** The helper
channel on Darwin is a FIFO (`create_cloexec_pipe`, because Darwin has no `pipe2` and a FIFO gives
an atomic close-on-exec open), and the parent's READY wait was `poll(2)`. `poll` on a Darwin FIFO
reports data and never the last writer's close: XNU routes a FIFO's `EVFILT_READ` knote through
the generic vnode filter, whose readable test is the queued byte count (`fifo_charcount`), and the
writer's `fifo_close` wakes the read socket's selinfo, not the vnode's knotes. So a helper that
ended before READY was invisible to the wait, which ran to its budget (two seconds on master, ten
at PR #125's head) and then reported the exit status of a child that had been dead the whole time.
Measured on macOS 26.5.2 / xnu-12377.121.10 (run 33989444028 and 33989492728 on branch
`scratch/darwin-fifo-eof-experiment`, `exp/fifo_eof.c`): with the channel built exactly as the
crate builds it and a forked child that closes its end and exits, `poll` returned 0 after the full
3 s; `select` on the same FIFO returned readable at once and `read` then returned 0; `poll` on a
`pipe(2)` returned `POLLIN|POLLHUP` at once; one byte written by the child woke `poll` on the FIFO
at once. Linux passed all five at once. The same fact was visible in CI before the experiment:
`a_helper_that_never_acknowledged_reports_what_ending_it_answered`, whose two helpers exit before
READY, completed about 4 s after its neighbours on the macOS leg of run 33987067020 and about 6 ms
after them on the Linux leg. PR #172 makes the Darwin wait a `select` (`wait_readable`), so the wait
now ends when the helper ends; `a_helper_that_has_already_exited_ends_the_acknowledgement_wait_at_end_of_file`
fails on the tree before it on macOS and passes after.

**Why the helper exits: the parent's `setpgid` raced the reaper's own.** Every recorded occurrence
with the PR #134 diagnostic in place (#154 at `d7e0c5d`, #156 at `20f0665`, #171 at `dea50f9`)
collected the reaper "having already exited with status 1", which is reached only through the
reaper's own `_exit(1)` sites before READY: installing its signal dispositions, `setpgid(0, 0)`,
opening a cleanup lease, or taking the shared `flock` on one. PR #172 made the helper write a report
naming the step, the lease and the errno on the acknowledgement pipe before it ends, and the first
run carrying that report (CI run 33992302665 at `a9f11da`, the second run of the same head,
`engine::tests::second_reviewer_spawn_failure_settles_worker_and_first_review_evidence`) read:

    Unix cleanup reaper did not initialize; waited 24.333µs of 2s; descriptor ceiling 10240; the
    reaper reported that moving into its own process group failed: Operation not permitted (os
    error 1); ending it: SIGKILL was delivered, and the wait collected it, having already exited
    with status 1

So the step is `setpgid(0, 0)` and the errno is `EPERM`, and the wait ended in 24 µs rather than
two seconds, which is the first half of the repair at work. The second half: until #172 the parent
called `setpgid(pid, pid)` on the reaper right after the fork, "to close the parent's race with the
child-side setpgid; either call may win". On Darwin the two calls racing on the same new group make
the child's fail with `EPERM`. Measured on the macOS runner CI uses (macOS 26.6.2, run 33992718199
on branch `scratch/darwin-fifo-eof-experiment`, `exp/setpgid_race.c`): with the parent's call the
child's `setpgid(0, 0)` returned `EPERM` in 1 to 4 of every 20,000 forks across nine tallies, and in
none of 120,000 forks without it; Linux returned none in 60,000 with the parent's call. The rate,
roughly one in ten thousand forks, fits a suite that forks a reaper per agent launch and reds a
handful of launches a day across all branches. The parent's call is removed in #172: `begin` returns
only after READY, and the child writes READY only after its own `setpgid` succeeded, so the reaper is
in its own group before any agent exists in either shape, and the child's call stays checked and
reported.

What the recorded occurrences before #172 cannot say is which of the four sites they took; the
status-1 signature is shared. The one site with a measured Darwin failure mode is this one, and the
one recorded occurrence that could name its step named it. Two things the reading rules out or
narrows: the reported waits (2.0005–2.008 s against a 2 s budget) were the wait ending at its budget,
not the child taking that long, since a child still running when the parent gave up would have been
collected "killed by signal 9"; and within one test process the launch barrier serialises reaper
spawns against agent spawns, so a sibling fork holding the pipe's write end is not what kept the
wait from ending. The wait could not see the end at all.

**A candidate the code shows, not observed.** `rundir::cleanup::is_held` probes the cleanup lease
with `flock(LOCK_EX | LOCK_NB)` and releases it; a reaper whose `LOCK_SH | LOCK_NB` lands inside
that window is refused and exits 1 through the fourth site. Within the test process the in-process
claim check answers before the probe for a run this process holds, so nothing in the suite drives
it; a `status` from another process against a live run could. It is recorded for the reader of a
report that names the lease.

**Occurrences recorded above stand as written.** This section adds what the wait was doing and what
the reaper was refused; it changes nothing about when the failures happened or what they reported.
