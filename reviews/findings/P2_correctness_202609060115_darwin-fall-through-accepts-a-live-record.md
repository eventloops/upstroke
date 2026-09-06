---
id: PR173-DARWIN-FALL-THROUGH-ACCEPTS-A-LIVE-RECORD
severity: P2
disposition: deferred
category: correctness
pr: 173
reviewed_sha: 429afd082e5628e8131627bc822dcc882de29aed
location: src/agent/proc.rs:360
provenance: introduced_by_feature
first_bad: W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT
guard: the next push to PR #173, or the change that next opens `GroupObservation`
---

## Failure sequence

`GroupObservation::leads_own_group` on macOS lets a `getpgid` `ESRCH` fall through to the `proc_pidinfo(PROC_PIDT_SHORTBSDINFO, 1)` record and accepts any record whose `pgid == pid`, reading neither the record's `status` nor the two `waitid` fields -> XNU's non-zero argument only *enables* the zombie lookup; the call tries `proc_find(pid)` first and can return a live process's record (`bsd/kern/proc_info.c`, `proc_pidinfo`) -> a child that missed containment, exited, and was reaped by an embedding host's wildcard wait (DESIGN §15 names that host) has its pid reused by an unrelated live process that leads group `pid`; `getpgid` answers `ESRCH`, the record is the stranger's, both `waitid` fields are `ECHILD`, and the oracle answers `true` for the wrong process. The control test covers an unreaped child in the parent's group only, not this case, and the body's claims "only when the exited record answers" and "a reaped child … reads false" are not what the code checks.

## What the change that takes this up should do

Require the record's `status` to be `SZOMB` and `exited_before` to be `Ok(true)` before the fall-through answers `true`: `waitid(P_PID, pid, WNOWAIT)` answers only for this process's own child, so a reused pid cannot satisfy it. Add a control that reaps the child first and asserts the observation reads `false` with `exited_before == Err(ECHILD)`, and correct the body's three sentences. The decision stays narrow on every other platform and errno.

Recorded from the frontier pass of 2026-09-06 (`gpt-5.6-sol`, max effort) on PR #173 at `429afd0`, posted as https://github.com/eventloops/upstroke/pull/173#issuecomment-5556000412. Filed as the reviewer wrote it, with the author's reading beneath.
