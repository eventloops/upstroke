---
id: PR173-EARLY-ASSERTION-LEAKS-A-ZOMBIE
severity: P2
disposition: deferred
category: correctness
pr: 173
reviewed_sha: 429afd082e5628e8131627bc822dcc882de29aed
location: src/agent/proc/tests.rs:371
provenance: introduced_by_feature
first_bad:
guard: the next push to PR #173, or the change that next opens these two tests
---

## Failure sequence

`an_exited_but_unreaped_child_still_answers_for_its_own_group` (from `src/agent/proc/tests.rs:371`) and `a_child_left_in_this_processs_group_never_answers_for_its_own` (from `:424`) assert several times before `finish()` and `wait()` -> in the scheduling case the body itself describes, the child killed before the first look, the premise assertion panics and the reap is skipped; `std::process::Child` does not reap on drop -> the exited child stays a zombie for the rest of the suite. The supervisor settles the group but does not reap the direct child, and the control has no supervisor at all.

## What the change that takes this up should do

Hold the child in a guard whose `Drop` kills and waits, so every exit from either test reaps it; keep the assertions where they are. The same guard is the natural owner of the `JoinHandle` `PR173-EXIT-WAITER-THREAD-DETACHED` asks to be joined.

Recorded from the frontier pass of 2026-09-06 (`gpt-5.6-sol`, max effort) on PR #173 at `429afd0`, posted as https://github.com/eventloops/upstroke/pull/173#issuecomment-5556000412. Filed as the reviewer wrote it, with the author's reading beneath.
