---
id: PR139-LOCATOR-STAT-ABSENCE
severity: P2
status: deferred
category: correctness
pr: 139
reviewed_sha: d91e84a74b2a3b94b077efe2e7f2a2a342be8e8a
location: src/rundir/ownership.rs:164
provenance: pre_existing
first_bad:
guard: the sweep of `src/rundir/ownership.rs`, `standards/SWEEP.md` review queue row 15
---

## Failure sequence

`prove_private_half_ownership` asks whether the marker's recorded private target exists with
`if fs::symlink_metadata(&locator).is_err() { return NothingBound(UnboundShape::TargetAbsent) }`.
**Every** error is read as absence, and `TargetAbsent` is a reclaiming answer: the plan is
`ReclaimPublicOnly`, and `remove_public_husk` deletes the public directory including `.creating`.

`.creating` is the private half's only locator — `src/engine/topology/create.rs` states it: "a
private half no marker names is one no census, no `status` and no deferred prune can ever reach
again". So a stat the filesystem declined to answer — an `EACCES` on a parent component of the
recorded locator, an `ELOOP`, an `ENOTDIR`, a Windows sharing violation — deletes the locator of a
private half that is still there, and orphans it permanently.

It reaches a committed run through one more step: a classification probe that fails while the
marker still parses (`events.jsonl` unreadable, `.creating` readable) classifies `Husk`, and the
census then runs the proof over a directory whose log committed.

This is the same class as `SWEEP-CLASSIFY-009` — a failure folded into an absence that is the
deleting branch — through a different door. It is a *different* door: `lstat(2)` consumes no file
descriptor, so the descriptor exhaustion that drives `SWEEP-CLASSIFY-009` does not reach this
conjunct, and the fix for that finding does not close this one.

## Disposition

Deferred, and found by the author of PR #139 rather than by a review of it. PR #139 is a targeted
fix for `SWEEP-CLASSIFY-009` under an explicit scope bound: the listing folds, and what they force.
This conjunct is not one of them, and the honest repair is the same shape as that change doubled —
a second `RetainReason` variant with its `KINDS`, `kind()` and `Display` arms, a portable proof-grid
case, and a witness — inside a file whose own sweep is review queue row 15 and has not run.

The conjunct's own comment ("Existence is asked of the link itself, so a dangling symlink counts as
present and is refused below rather than reclaimed past") shows the fold was written deliberately
for the link case and never asked what a *failed* stat means, which is the sentence that sweep
should correct.

Reachability and the harm are stated above rather than left to be re-derived; nothing here is
measured by a test yet, and the row is what a later pass or that sweep starts from.
