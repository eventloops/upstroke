---
id: PR153-FOLD-ANSWER-RETURNS-TO-PENDING
severity: P2
disposition: deferred
category: correctness
pr: 152
reviewed_sha: a00bebab3e9a597e361b18585eb1d9908a6fbc59
location: src/topology/fold/apply.rs:552
provenance: pre_existing
first_bad:
guard: reopened by #152 with evidence. The obvious in-file fix (restore a state snapshotted at raise time) is unsound; the sound fix derives the return state from the fold's current facts. Owner-directed deferral (coordinator, pass 5) to a successor rather than attempted further in this §5/§6/§7 sweep. Escalate if reclassified — if a later pass disputes the deferral or the derivation direction, it escalates to the owner rather than remaining silently deferred.
---

## Failure sequence

`apply_answer` returns an answered task to a state chosen by `QuestionOrigin`: a verification park to
`AwaitingMerge`, every other origin to `Pending` (master's behaviour, restored by #152 after the
attempt below). That is wrong for a **bare `question_raised`** that parked a task from `AwaitingMerge`
or `Deferred` — `design/12` and `select/tests.rs` make such a question valid input — because its
answer returns the task to `Pending`, losing the state it was parked from and leaving a rejected or
queued original structurally re-dispatchable against `design/14`'s awaits-repair rule.

**#152 attempted the obvious fix and withdrew on evidence.** The fix was to record the task's state
at raise time (`OpenQuestion.parked_from`) and restore it at answer time. Across five review passes,
**four independent stalenesses** defeated it — each a way the snapshot goes wrong that a guard over
the snapshot cannot catch, because the task moves through events that never touch the question. The
conclusion is structural: **a state snapshotted at raise time cannot be sound in a fold where the
task may move arbitrarily while parked. The answer-return must be *derived* from the fold's current
facts — queue membership, open transaction, lineage — not restored from a snapshot.**

## The four stalenesses, each a replayable sequence

A successor can build each as a fold test; each is why a snapshot-plus-guard is not the fix.

1. **Merged while parked (pass 3).** Task at `AwaitingMerge`; `question_raised` parks it
   `AwaitingInput` and snapshots `AwaitingMerge`; a `task_merged` under the open transaction moves it
   to `Merged` while the question is open; the answer restores `AwaitingMerge` — **un-merging a merged
   task** and making `derived_outcome` a `FoldError`. A "still `AwaitingInput`" guard catches this
   one.

2. **Two open questions, guard satisfied (pass 4).** `q1` parks the task from `AwaitingMerge`; `q2`
   is raised while the task is already `AwaitingInput` (`check_new_question` forbids only an
   incomplete or duplicate question). Answering `q1` finds the task still `AwaitingInput` and restores
   `AwaitingMerge` **while `q2` is open** — un-parking a task with input outstanding, which
   integration then verifies and merges. The "still `AwaitingInput`" guard is satisfied and still
   wrong; a "last open question" guard is needed on top of it.

3. **A `Deferred` park loses a durable wake (pass 5).** Task at `Deferred` (backoff pending);
   `question_raised` snapshots `Deferred` and parks it `AwaitingInput`. When the backoff elapses,
   `wake_backoff` (`apply.rs`) reads only the **visible** state — `AwaitingInput`, not `Deferred` — so
   it does not wake the task, and the durable wake is consumed with no effect. The later answer
   restores `Deferred`, but the elapsed wake will not fire again: the task is stuck. The snapshot and
   the live wake mechanism disagree about what state the task is in.

4. **Move while parked defeats episode inheritance (pass 5).** The pass-4 fix made a second question
   inherit the first's snapshot, on the theory that a task has one parking *episode*. It does not:
   `q1` parks from `AwaitingMerge`; the task **merges** to `Merged`; `q2` is raised on the now-`Merged`
   task. There are two overlapping episodes, not one, so `q1` and `q2` do not share a correct
   `parked_from`. Answering `q2` then `q1` restores the stale `AwaitingMerge` into a fold whose queue
   entry is gone. **Not every open question of a task has the same parked-from state.**

## What the change that takes this up should do

Derive the return state at answer time from the fold's current facts rather than restoring a snapshot:
a task with a live integration transaction it owns is `AwaitingMerge`; one already `Merged` stays
`Merged`; one with a pending backoff is `Deferred`; one with another open question stays
`AwaitingInput`; otherwise `Pending`. Reconcile it with `wake_backoff`'s notion of the visible state
so a `Deferred` park does not lose its wake. This is within `apply.rs`'s reach but is a correctness
redesign beyond a §5/§6/§7 sweep's charter, which is why it is deferred rather than taken here; the
`QuestionOrigin` mechanism stays in the meantime as master's behaviour.
