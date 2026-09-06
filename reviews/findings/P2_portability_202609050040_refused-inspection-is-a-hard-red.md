---
id: PR137-REFUSED-INSPECTION-IS-A-HARD-RED
severity: P2
disposition: deferred
category: portability
pr: 137
reviewed_sha: 6d24837942ca331ffc3e027d7658cec791dc5100
location: src/workspace_manager/tests.rs:8049
provenance: fix_regression
first_bad: 6d24837942ca331ffc3e027d7658cec791dc5100
guard: the Windows-containment owner (`SAMPLER-WINDOWS-STILL-KILLS-A-BARE-CHILD`, the `#117` `agent/proc` family): a sound retry needs a barrier on the platform the refusal occurs on; escalates to the owner if a later pass labels it P1 or P2 rather than accepting the deferral
---

## Reopened

This finding was dispositioned `fixed` on PR #145 by a bounded inspection retry, and the retry
was **withdrawn at that pull request's third frontier pass**. A finding that stops being fixed
gets its file back; this is the delete-on-resolve rule run in reverse, and its history is in that
pull request's ledger and `git log --diff-filter=D -- reviews/findings/`.

## Failure sequence

On the self-hosted Windows runner a residue inspection fails for the machine's own reasons —
`failed to read` on a temp object file whose delete-pending removal still holds the name — ->
`classify_object_residue` returns an error for that one sample -> the assertion PR #136 added
fires, correctly refusing to read a failed inspection as a residue in no class -> the whole suite
is red, on master and on every branch that merges master, for an environmental event in a test
whose subject is classification and not read reliability. #137 spent a rerun on it the day it
landed. The rate is reported as around 2% and is not measurable from the Linux build box.

## Why the retry was withdrawn, so nobody rebuilds it the same way

Three passes found the same thing from three sides. A second read is only a read of the same tree
when nothing of the sampled command is alive, and on Windows nothing establishes that: the
sampler's barrier is `cfg(unix)`, `Child::kill` terminates the leader only, and a
`git worktree add` checkout keeps writing. So a sharing violation from the live checkout, retried,
becomes a completed checkout classified `After` under a kill sample — completion state recorded
under a kill. Gating the retry on established rest (the child's own exit) narrowed it to the
samples where it could not help. And the observability meant to make a rescued error visible does
not exist where it matters: passing-test stdout is captured in CI and the artifact is gitignored
and never uploaded, so a rescued corruption left a green job and no diagnostic.

## What the change that takes this up should do

First give Windows the barrier — `SAMPLER-WINDOWS-STILL-KILLS-A-BARE-CHILD` names the route, a
test-only seam in `agent::proc` handing out the real Job Object — and only then a retry, gated on
the barrier having settled, classifying the two delete-pending errors specifically
(`ERROR_ACCESS_DENIED` and `ERROR_SHARING_VIOLATION`, the latter of which Rust maps to
`Uncategorized`), and recording any rescued refusal somewhere CI actually keeps. Without the first
step there is no sound retry to write, and a hard red at a ~2% rate is the honest state.
