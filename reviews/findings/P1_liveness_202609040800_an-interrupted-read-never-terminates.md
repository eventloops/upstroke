---
id: SWEEP-CLASSIFY-001
severity: P1
disposition: deferred
category: liveness
pr: 137
reviewed_sha: 5f661fa7f8d5c45471cc33746a70df1cd192c61e
location: src/rundir/classify.rs:236
provenance: pre_existing
first_bad: 7a83e69
guard: open, and carrying the repair that must not be attempted. A retry cap is the wrong shape and this pull request wrote one and withdrew it: an…
---

## Failure sequence

**the classification probe does not terminate on a source that answers `Interrupted`, and `startup_census` holds the physical worktree lock across `classify_run_dir`, so the lock is never released and no later command in that worktree can run.** There are two independent unbounded doors and a correct repair has to close both. (i) `first_line_within`'s two reads go through `Take` and `read_to_end`: `std::io` retries `Interrupted` without limit, and an interrupted read spends none of a `Take`'s byte budget -> measured at rustc 1.85, a reader answering `Interrupted` unconditionally was still being called after five million reads with the limit untouched. (ii) `newline_offset_from`'s own `Err(error) if error.kind() == io::ErrorKind::Interrupted => continue` spends no budget either, in this crate's code rather than in `std::io`'s -> a successor who closes only (i) will believe the probe terminates. Method, so it can be re-run against master today: lift those two functions verbatim into a standalone binary, diff them byte-identical against the source apart from `pub(super)`, drive them with an always-`Interrupted` reader, and time it -- master's own copy did not return within 25 seconds

## What the change that takes this up should do

**open, and carrying the repair that must not be attempted.** A retry cap is the wrong shape and this pull request wrote one and withdrew it: an exhausted cap has to answer something, `RunDirClass` is `Committed` or `Husk`, and `Husk` is the *reclaiming* answer -- so a finite burst of interruptions above the cap turns a run the source would have delivered into one `list_runs` hides, `resolve_run_id` refuses and the reclaim path may delete once the transient listing failure of `SWEEP-CLASSIFY-009` occurs. A liveness defect became a data-loss defect, and the pull request's own test asserted it while passing, because it recorded what the new code does rather than what changed. The rule a repair needs is the one PR #139 is landing one door down: an observation that could not be completed yields the retaining answer, never the reclaiming one. That needs a classification which is neither `Committed` nor `Husk`, so it needs `classify_run_dir`'s signature, which reaches the `RunDirClass` re-export, `RunDirEntry`, `list_runs` and `list_husks` -- three production call sites in three files and thirty-six in all, with queue row 13 in scope. It is queue row 12's successor and it is not a sweep's own-file work

Recorded by the PR #137 pass over `src/rundir/classify.rs`; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
