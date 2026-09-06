---
id: PR207-AUDIT-WRITE-WINDOWS-OPEN
severity: P2
disposition: owner attention required
category: security-trust
pr: 223
reviewed_sha: 8dea964caf6917a8ea1e94dbc84bf3235307b1a5
location: scripts/pr-ready-audit.sh:496
provenance: introduced_by_feature
first_bad: PR207-READY-LABEL-ON-MOVED-HEAD
guard: owner attention required 2026-09-06: PR #223 closed the label windows, replaced the merging enqueue with `enqueuePullRequest` and made the withdrawal read-back-confirmed, but review pass 2 of 2 found the window still open on the FAILED enqueue branch. The workflow's two-pass cap is spent, so the repair is not attempted here.
---

## Failure sequence

the audit reads head H and base B and judges them READY -> a push or a retarget lands between that read and the label write, or between the last re-read and the enqueue call -> the label names a head or base the audit never judged, and an enqueue bound to H by `--match-head-commit` but only re-checked for the base can queue a diff the review did not see

## What the change that takes this up should do

close the windows where GitHub lets them be closed: bind the enqueue to the base as well as the head (a merge-queue API that takes both, or a check that the queue entry's ref names the base the review used), and stop using a label as any kind of signal to other tooling; until then nothing may treat `ready-to-merge` as permission, and the residual after-enqueue retarget is read off the pull request's timeline by the next audit

Recorded by PR #207 from review passes 6, 7 and 12; the label re-check, the head-bound enqueue and the base re-read were the repairs, and this file is what they leave open.

## 2026-09-06: parked for owner attention after PR #223, review pass 2 of 2

PR #223 (branch `codex/findings-cf59b3e9b951`, head `8dea964caf6917a8ea1e94dbc84bf3235307b1a5`)
took this up and landed most of it. It is parked, not merged, and this file is restored because
the finding is not closed.

What #223 does close, with regressions in `.github/scripts/test-pr-ready-audit.sh`:

- the head and base are read together, in one call, before the ready label, after it, before the
  enqueue and again after it, and the ready label is reconciled against the state the run *ends*
  on, so a label the run wrote itself is taken back when the identity moved;
- the enqueue is the GraphQL `enqueuePullRequest` mutation with `expectedHeadOid` instead of
  `gh pr merge --merge --auto`, which merges an already-mergeable pull request outright when the
  base carries no queue rule — pass 1 found that the old call could land an unreviewed diff before
  any post-call check could run;
- the withdrawal is `dequeuePullRequest` and is confirmed by reading the queue state back, because
  `gh pr merge --disable-auto` exits 0 on a queued pull request having removed nothing;
- `retarget_status` answers retargeted / none / unknown, so an unreadable timeline stops reading as
  an absent retarget.

What review pass 2 (`gpt-6-astra`, high effort, 2026-09-06) found still open, and why the ticket is
parked rather than repaired: the window survives on the **failed** enqueue branch. A nonzero exit
from the enqueue client does not establish that the remote mutation had no effect — the mutation
can be accepted and the response lost — and that branch reconciles nothing: it prints the queue
state, leaves the run READY and leaves the ready label standing. The reviewer reproduced it in
memory with an `enqueue_pr` that retargets the base to `release`, records the enqueue and returns
1: the run ended with the base moved, the entry queued and the label still on. The same branch
also leaves a stale label standing after an ordinary `expectedHeadOid` rejection caused by a push.

The repair the reviewer names: reconcile identity, timeline and queue evidence after **every**
enqueue attempt, nonzero exits included; separate a confirmed rejection from an indeterminate
outcome; clear readiness when the identity moved or the evidence is unreadable; and capture enough
pre-attempt queue identity to reconcile an entry this run may have created without withdrawing one
that was already there and is not this run's to remove. Regressions wanted: an accepted mutation
followed by a transport failure, a head-mismatch rejection after a push, and an unreadable state on
the failure path.

The findings workflow's cap is two review passes per finding and both are spent, so the repair is
left for the owner. The branch, its worktree, the draft pull request, both verdicts and the
reproduction and mutation evidence are retained.
