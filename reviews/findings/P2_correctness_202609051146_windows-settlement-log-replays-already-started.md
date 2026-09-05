---
id: PR160-WINDOWS-SETTLE-ALREADYSTARTED
severity: P2
disposition: deferred
category: correctness
pr: 160
reviewed_sha: 41eb825d32a598f9c1b19e5ae93ae510786b3d8f
location: src/engine/topology/settle/tests.rs:1512
provenance: pre_existing
first_bad: undetermined
guard: Diagnose the exact Windows replay failure with preserved fixture bytes; verify any fixture repair against stale-directory reuse and native Windows kill tests.
---

## Failure sequence

On Windows CI, the question and retained-generation settlement tests kill
their child at the intended boundary, then replay the recorded log. Both
fail with AlreadyStarted. CI run 33962873616, attempt 1, job 101297764055 at
PR head 41eb825d32a598f9c1b19e5ae93ae510786b3d8f reports 1881 passed, 2 failed, and
37 ignored. The failures are kill_after_failed_settlement_rematerializes_question
at settle/tests.rs:1512 and retained_generation_not_continued_after_kill at
:1552. The raw checkout log identifies the tested PR merge tree as
932597c30d6a1c98f5e773b4ec79d84e7008bf14, combining the candidate with master.
This differs from a local build of the candidate alone.

The log is preserved at
https://github.com/eventloops/upstroke/actions/runs/33962873616/job/101297764055.
A single failed-job rerun on the unchanged head was authorized by the lane
steward. Attempt 2 passed, including Windows job 101301350451 and aggregate
job 101301699652. These two observed attempts on the same head produced one
failure and one pass. The retry does not prove the cause repaired.

A later PR-body edit triggered run 33964833478 on the same unchanged head.
Its Windows job 101303010713 also passed: 1883 library tests passed, 37 were
ignored; 10 binary tests passed, 1 was ignored. Across these three observed
Windows executions, one failed and two passed. This additional successful
execution does not establish the cause of the failure.

The independent review verified that the failed and later successful CI
executions used that same PR merge tree. Both tests also passed in the
independent Linux topology run at the detached candidate itself, queue job
967480d7f9a24a9799ecb1c0b57d1c71. That result does not establish a Windows
repair.

The recurrence is PR107-WINDOWS-SETTLE-REPLAY-ALREADYSTARTED-FINGERPRINT in
reviews/FINDINGS.md:198 and section 43. That record names the same two tests
and replay error in run 33785587535 at 9963fb0, with two successful runs on
the same historical head. Keep it separate from the PR104 trailing-slash
MalformedEntry fingerprint. No edit to the closed historical ledger is
needed.

The current fixture allocator uses a label, process ID, and process-local
counter, accepts an existing directory, and returns an unowned path. That
allocator was introduced at b2f7583dde66b8a41b94e6723d7d016d51e76d15 and later
moved without fixing ownership. Reusing a PID and counter could append a
second RunStarted to residue. This remains a hypothesis: the exact guest
residue was not recovered, and the proposed deterministic witnesses never
ran. Their source and patch are preserved outside the repository under
/srv/worktrees/astra-20260905/agents/astra_impl_160/preserved-fixture-witness/.

## What the change that takes this up should do

The lane responsible for Windows settlement fixture correctness should
preserve the failing event bytes and establish whether a reused fixture
root explains this fingerprint. If so, use fresh owned scratch roots and
RAII cleanup, preserve unrelated residue, and carry a deterministic collision
witness plus native Windows evidence. Diagnose other causes if that sequence
does not reproduce. This P2 is recorded under the owner's docs-only policy;
required CI still has to pass before PR160 merges.
