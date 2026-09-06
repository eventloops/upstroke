---
id: EXIT-WAITER-CENSUS-IGNORES-SYSCALL-NUMBER
severity: P3
disposition: deferred
category: correctness
pr: 179
reviewed_sha: 8193d6422391986852f78b4cc2917f7f92fd8b2a
location: src/agent/proc/tests.rs:826
provenance: introduced_by_feature
first_bad: PR173-EXIT-WAITER-THREAD-DETACHED
guard: the change that next opens `syscall_line_waits_on` or its notes
---

## Failure sequence

`syscall_line_waits_on` accepts any non-negative syscall number and then checks only
`arg0 == P_PID` and `arg1 == pid` -> a `/proc/self/task/<tid>/syscall` line for a DIFFERENT syscall
whose first two arguments happen to be `1` and this body's child pid is counted as a `waitid` on
that child -> the positive control can report an extra waiter, or the post-fix absence assertion can
fail although no thread is waiting on the child. The existing negative fixture varies the first
argument rather than holding both `waitid`-shaped arguments constant while varying the number, so
it does not expose the collision.

Recorded from recovery review 1 of PR #179 (`gpt-5.6-sol`, high effort) at `8193d642`, posted as
https://github.com/eventloops/upstroke/pull/179#issuecomment-5561173719. Filed as the reviewer wrote
it, with the author's reading beneath: the collision needs a thread of the test process to be inside
another syscall with `arg0 == 1` and `arg1 ==` a pid only this body knows, which nothing in the suite
does, so the risk is a latent attribution weakness rather than a reachable flake; the reason the
number was not matched is stated in the notes (a syscall number differs by architecture), which the
correction below addresses with the per-target constant.

## What the change that takes this up should do

Require the parsed syscall number to equal `libc::SYS_waitid` (a per-target constant, so no
architecture is pinned) in addition to `P_PID` and the pid; add a negative fixture with the same
first two arguments and a different number; classify `SYS_waitid` in `effects/wrappers.toml`; and
update the notes' claim that the arguments alone identify the wait.
