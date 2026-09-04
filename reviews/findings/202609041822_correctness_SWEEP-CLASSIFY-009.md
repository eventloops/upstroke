---
id: SWEEP-CLASSIFY-009
severity: P2
status: fixed
category: correctness
pr: 139
reviewed_sha: 5f661fa7f8d5c45471cc33746a70df1cd192c61e
location: src/rundir.rs:890
provenance: pre_existing
first_bad: 7a83e698824ee516b71d31c6728b64149f5642a0
guard: `a_committed_run_the_census_could_not_read_is_not_reclaimed`, `a_public_removal_whose_listing_does_not_answer_removes_nothing`, and the `listing-unreadable` case of `every_conjunct_of_the_ownership_proof_refuses_on_its_own`
---

## Failure sequence

A transient whole-process descriptor exhaustion — `EMFILE`, `ENFILE`, ordinary on a busy machine
and reachable without anything hostile — fails the classification probe's `open`, the marker read
at conjunct 1 of the ownership proof, and the public directory's listing at the same moment.

`read_dir_names` answered `Vec::new()` for a `read_dir` that failed, and dropped every per-entry
error with `flatten()`, so a directory it could not read and one that is empty were the same
value. Empty is the reclaiming answer: `unbound_shape`'s `[]` arm is
`NothingBound(UnboundShape::Bare)`, whose plan is `ReclaimPublicOnly`, and that plan carries no
commit-record check anywhere on its path. `remove_public_husk` then listed the directory a second
time, after the transient had cleared, and removed what it found — so a committed run's public
half, `events.jsonl` included, was deleted on the evidence that it was empty.

The removal's own listing had the same shape from the other side: run under a listing that does
not answer, the loop removed nothing, the marker was unlinked anyway and the final `remove_dir`
failed on the non-empty directory, leaving a husk carrying content whose private half no marker
names any more.

## Disposition

Recorded by PR #137 (the `src/rundir/classify.rs` sweep, `reviews/FINDINGS.md` §56) and deferred
there: both folds are in files that pull request does not own, and the repair changes the deletion
authority's behaviour, which needs those files' own review.

Fixed by PR #139, scoped to this row alone and not a sweep of either file. The listing returns
`io::Result<Vec<String>>` and privileges no error kind, `NotFound` included — only a listing that
completed establishes what is in a directory, and this crate has already measured the Windows
guest mapping a stat beneath a file ancestor to `NotFound`
(`PR77-WIN-UNDECIDABLE-STAT-ORACLE`), so a rule reading one `ErrorKind` as established absence
would not mean the same thing on both platforms. Per-entry errors propagate for the same reason: a
directory that listed partially is not one whose contents are known.

Both consumers on the reclaim path refuse rather than delete. `unbound_shape` answers the new
`RetainReason::ListingUnreadable`, a retention that mints no token and reports the husk with the
path and the error that stopped the listing; `remove_public_husk` takes its whole listing before
it removes anything, so a second observation that does not answer refuses the removal with nothing
touched. No consumer needed a new match arm.

Witnessed against the head: with the pre-fix body restored under the post-fix signature, the
end-to-end test reports `an unreadable listing answered Bare, the reclaiming answer; the reclaim
it licenses returned Ok(()) and the committed log is GONE`. The per-entry `entry?` has no witness
— a `read_dir` that succeeds and then yields a failing entry is not deterministically
constructible through the real filesystem in a single-threaded test — and that gap is disclosed in
PR #139's body rather than covered by a claim.
