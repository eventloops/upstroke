---
id: SWEEP-CLASSIFY-009
severity: P2
disposition: deferred
category: correctness
pr: 137
reviewed_sha: 5f661fa7f8d5c45471cc33746a70df1cd192c61e
location: src/rundir.rs:890
provenance: pre_existing
first_bad: 7a83e69
guard: deferred, and read_dir_names is PR #139's live subject while this is written, so it is described here and not touched
---

## Failure sequence

**a committed run's public half, `events.jsonl` included, is deleted, on evidence that the directory was empty, and no resume can recover the run afterwards.** `read_dir_names` folds two different failures into `[]`, which is `unbound_shape`'s reclaiming answer: it answers `[]` when `read_dir` itself fails, and its `.flatten()` silently drops an entry whose iteration step fails, so a directory that opened but could not be walked past `events.jsonl` also reads as bare -- the second reach is pass 1's, from a different file and a different reviewer than the first -> a transient whole-process failure fails `File::open` in the classifier, `fs::read_to_string` on the marker in the proof and that listing at the same moment, so the census classifies `Husk`, the proof answers `NothingBound(Bare)` and the plan is `ReclaimPublicOnly`, with no commit-record check anywhere on that path -> `remove_public_husk` then lists the directory a second time, after the transient has passed, and deletes what it finds: the observation that authorised the deletion and the deletion itself are two different listings. Needs the failure to be transient, which `EMFILE` and `ENFILE` are

## What the change that takes this up should do

deferred, and **`read_dir_names` is PR #139's live subject while this is written**, so it is described here and not touched. Both folds are outside this file -- `src/rundir.rs` (queue row 19) and `src/rundir/ownership.rs` (queue row 15) -- and the fix changes the deletion authority's behaviour, which needs those files' own review; the precedent is explicit, the same repair class having been attempted across `src/workspace_manager.rs` in PR #128 and reverted after three passes, each round's repair introducing the next defect. sweep_coordinator has taken this row as the specification for a targeted change of its own. The corrected argument in `first_committed_line` names the residual where a reader of the fold will see it

Recorded by the PR #137 pass over `src/rundir/classify.rs`; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
