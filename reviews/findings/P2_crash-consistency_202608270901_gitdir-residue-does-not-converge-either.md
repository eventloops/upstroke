---
id: PR5-RD-003
severity: P2
disposition: deferred
category: crash-consistency
pr: 5
reviewed_sha: 327cce3
location: src/workspace_manager.rs
provenance: pre_existing
first_bad: PR5-RD-002
guard: the slice that next changes the worktree removal or residue-recovery path in `src/workspace_manager.rs`
---

## Failure sequence

The same kill inside `git worktree add`'s registration window can also leave `gitdir`
absent, zero-length, partial, or holding valid **non-UTF-8** Unix path bytes. Both the
record-based scan and the fallback directory scan use `read_to_string` and silently skip such
entries, so the residue does not converge — and would not have converged even with round 7's
(reverted) fix applied.

## What the change that takes this up should do

Read the registration files the way Git reads them — bytes, not `String` — and propagate a
read failure rather than skipping the entry. The round-7 review examined nine neighbour cases and
found **three uncovered**: `PR5-RD-003` and two others it did not name. It ruled explicitly that
`PR5-RD-003` is not the only uncovered case, so a repair that closes this one row and stops has not
closed the shape. Carried only because the slice was landing; not settled.

Recorded in `reviews/FINDINGS.md` §13 alongside `PR5-RD-002`. Severity is this migration's judgement: same mechanism as `PR5-RD-002`, one file over, with no measured rate of its own.
