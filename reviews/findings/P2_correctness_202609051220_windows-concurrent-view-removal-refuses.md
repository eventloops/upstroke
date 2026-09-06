---
id: PR154-WINDOWS-CENSUS-VIEW-REMOVAL-ACCESS-DENIED
severity: P2
disposition: deferred
category: correctness
pr: 154
reviewed_sha: 6574728ac17745c2c39ccd7768308a2b74314f79
location: src/runner/container/census/tests.rs:4618
provenance: pre_existing
first_bad:
guard: PR 139 owns source investigation and validated repair; final handoff requires actual resolving merged ancestry or an evidenced contract disposition, preserving permanent-denial refusal and intent retention
---

## Failure sequence

FreshRun and Resume race to reclaim one view -> a view-path operation reports AccessDenied5
-> one racer refuses instead of converging. The generic error does not identify the operation
or establish retry exhaustion. The failing test is
`a_fresh_and_a_resuming_census_race_one_container_and_converge`, round 23, at head
605e1fc23f6d2bd135cf94be8c9f39d6742397ea, run 33957924675, attempt 1, Windows job 101284523309.
The retained result was 1865 passed, 1 failed, 37 ignored in 114.60 seconds. Windows passed
attempt 2. Those two observations do not establish a general failure rate.

The same test failed at #164 head 4a763d52768f6a752b31297be5db1d95c497a6ef, run 33966318043,
attempt 1, Windows job 101306998047, round 12. That job recorded 1882 passed, 1 failed and
37 ignored in 118.88 seconds. Its [public PR ledger](https://github.com/eventloops/upstroke/pull/164)
retains this canonical ID. A later metadata update triggered separate run 33969285309,
attempt 1, Windows job 101314812026 at the unchanged head; the test passed, with 1883 library
passes and 37 ignored in 116.13 seconds. This was a separate run, not an explicitly requested
failed-job retry. The steward verified its checkout 18fa7c721ee28c1431b55de8c3c88436216d98c3,
which #164 identifies as the failed job's checkout too. The changed outcome does not establish
cause, harmlessness or repair.

The steward retained both #164 logs. The failed log SHA256 is
4687f36966e7e28fe83ba0f609d8ea6f2859cd05c32643b5da4d259315820786; the later passing log is
53abb644d963bc4ad6c846cb5140cc86bb9d379a2456c2ae503a84356f2958dc. Native references are
[#152's failure](https://github.com/eventloops/upstroke/actions/runs/33957924675/job/101284523309)
and [#164's failure](https://github.com/eventloops/upstroke/actions/runs/33966318043/job/101306998047).

The #154 steward retained the failed log and metadata from #152. At the inspected local anchor
above, the relevant source bytes match the failing head:

| File | SHA256 |
|---|---|
| src/runner/container.rs | 63e5f72511b9b3b354175f36c795588f6bd65ff06f7b9a17b43aef8f54324364 |
| src/runner/container/census/tests.rs | b594df575d2ff9e6db442fdb6f984f3581b5a604d730665bc5cc1f274ad39607 |

The test uses FakeRuntime and DisposableDirView, not real Docker. Reclaim reaches
`racing_removal` around `fs::remove_dir_all`, which tries 64 times with yield_now, accepts
NotFound as already removed, and otherwise reports an I/O error. The generic error display says
"failed to read" even for removal. Neither a read syscall nor Docker-daemon interference is
established by that text.

Delete-pending state outlasting the loser's retries is a hypothesis. Error 5 alone cannot
distinguish it from persistent access denial. Returning refusal retains the intent; treating
PermissionDenied as absence would conceal a real failure. The convergence contract nevertheless
failed in this observed run, which warrants P2. This is separate source ownership from #154's
drain repair. PR #139 owns source investigation and validated repair; #154 owns this canonical
observation record. Final handoff requires an actual resolving carrier with verified merged
ancestry and evidence, or an evidence-based contract disposition that retains the witness.
Deferred records do not constitute an accepted-risk or P2-rule exemption. PR #163 owns the
container documentation and is not the repair carrier.

This is related occurrence evidence for W2-WINDOWS-RACING-REMOVAL-DELETE-PENDING in the closed
`reviews/FINDINGS.md` section 43, rather than a competing defect. Its historical delete-pending
attribution, rate, execution count, job-overlap claim and rerun licence are not newly verified
facts about these occurrences. The test itself runs two racing threads even without overlapping
jobs. The #163 note at 12b6d204988064b40b6ec66c6245504022b2e590 documents fixed-retry refusal;
that contract context does not identify this native cause or waive the assigned P2 investigation.

PR #139's first native diagnostic at 297c475dedfe77f3ea3f6200d7f0228d9a6dbf42, run 33970010010,
attempt 1, Windows job 101317366410, passed its selected cases. The retained handle reported
DeletePending=true and zero links, but the path was already NotFound before the real helper
ran. Removal then made one NotFound attempt and succeeded while the handle stayed live.
The delayed-close cases likewise returned before close completion. This fixture did not reach
AccessDenied or exhaust retries, so it supports neither a wait-based repair nor closing the
original observation. The steward verified the preserved log SHA256
8dba1d6a82eabd7a1c7d6a2eacbc9e4e44cc5cc304a26d220408303580bec02b. Investigation of the actual
concurrent recursive-removal sequence remains with #139; no resolving ancestry is claimed.

## What the change that takes this up should do

Record native Windows removal state around a forced concurrent deletion. Determine whether the
winner's live deletion state exceeds the loser's retry budget. Add a regression at the
view/census boundary that preserves permanent-denial refusal and intent retention. Use that
evidence to choose the repair. A broad retry increase or weakened convergence assertion is not
supported by the current evidence.

## Owner disposition and carry-forward on 2026-09-05

After the integrated PR #164 passed CI, the owner explicitly cleared its merge hold and
merged it as 37d74db0d0c58453b4919254986ff5b13303e3f0. The owner directed that this finding
remain open and be committed to the next PR after that branch integrates master. PR #169
carries this existing record under its original identifier; it does not repair the Windows
census behavior or establish that AccessDenied is harmless.

The earlier merge-hold descriptions above record the disposition at those observations.
The owner's later decision supersedes the PR #164 hold. The observed failures, passing
observations, log hashes and unresolved native cause remain evidence for a separate
investigation. This record is separate from the macOS reaper READY failure.
