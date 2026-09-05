---
id: RESIDUE-UNBINDABLE-TASK-REGISTRATION-HAS-NO-DESIGN-SENTENCE
severity: P3
disposition: deferred
category: docs-contract
pr: 151
reviewed_sha: ac00723e980cb9549aae76ae0fea24797bd25d7d
location: design/15_design_event_log_resume_run_layout.md:32
provenance: pre_existing
first_bad: RESIDUE-EMPTY-GITDIR-REGISTRATION-BLOCKS-EVERY-REMOVAL
guard: the next task-registration recovery policy change or the WorkspaceManager sweep must decide and document the unbindable-registration policy; this is already a public library API; `an_add_killed_before_it_wrote_gitdir_is_unlisted_and_refuses_forced_cleanup` records its current behaviour, while PR145 owns the separate P2 sampler/classifier defect
---

## Failure sequence

`DESIGN.md` §15's reclaim sentence -- "Exact gate/review worktrees likewise record and sync a
private intent before `git worktree add`; resume reclaims every such registration before it
switches branches or dispatches another worker" -- is about the gate/review snapshot worktrees
that `upstroke resume` reclaims through `Workspace::reclaim_gate_workspaces`, which removes the
intent-named checkout and never decodes a registration's `gitdir`. The v0.2 workspace manager's
task, staging and snapshot slots (`src/workspace_manager.rs`) are a different funnel with a
different reclaim -- `remove_worktree` binds a registration only through its `gitdir` bytes and
refuses on one it cannot bind -- and no sentence in `design/` says what that funnel does with a
registration it cannot bind. That task-registration policy remains unspecified in the design.
`src/lib.rs` publicly exports `workspace_manager`, including `WorkspaceManager::derive` and
`remove_worktree`, so external library consumers can use this API today. The engine's task-slot
callers are in its crate-private schema-4 topology, and `reclaim_intents` has no non-test caller
within this repository. Those facts do not make the library API unshipped.

This P3 records the missing design policy. It does not establish a violation of section 15's
gate/review sentence or reproduce a shipped CLI resume failure. It also does not replace the
separate P2 assertion, `SAMPLER-RECOVERY-PROVEN-IS-NOT-PROVEN-FOR-AN-EMPTY-GITDIR`, whose permanent
record is [PR145's body ledger](https://github.com/eventloops/upstroke/pull/145). That lane owns
its repair and the correction of its historical supersession record. The broader parent,
`PR136-SAMPLER-FORCED-REMOVAL-DOES-NOT-CONVERGE`, included multiple failure fingerprints. The old
CLI/design claim was rejected against its actual subject and caller, not because the task-slot
refusal or the sampler failure disappeared.

## Historical Git 2.43.0 observations on the build box

`git worktree add --detach` under strace writes, in order: `mkdir .git/worktrees/<name>`; open+write
`locked` (`initializing\n`); `mkdir <checkout>`; open+write `gitdir`; open+write
`<checkout>/.git`; `HEAD`; `commondir`; then the checkout through child processes; on the run
measured, `unlink locked` was the last syscall. A kill between opening `gitdir` and writing it
leaves `.git/worktrees/<name>/{locked, gitdir(0 bytes)}` and an empty checkout directory. An add
failing before the checkout removes its whole admin directory; one whose `post-checkout` hook
fails leaves a complete unlocked registration; with hooks disabled, as this crate runs Git, it
exits 0. `git worktree list` skips a zero-length `gitdir` silently (exit 0) and lists a
whitespace-only one as a registration of an empty path; `git worktree prune` skips the entry while
`locked` is present and removes it as an "invalid gitdir file" once unlocked; a later add at the
same checkout path gets a Git-generated collision-suffixed admin name.

Against that residue, in this funnel: the classifier reads registration through `git worktree
list`, so `record_for` answers `None`, `classify_object_residue` answers `ObjectResidue::None`
("nothing was written") for durable state, and the element list is empty -- a hole, and the
reason the site's `recovery_proven` sampling half (`SamplingRecord.recovered`) is falsified when
the sampler's kill lands in that window (`forced removal converges: Git { message: "worktree
registration … has an empty gitdir" }`, about 1/50 under load by the handing session's measurement).
`remove_worktree` refuses for the slot and for an unrelated populated slot through this API,
with a diagnostic naming the admin directory. The test named in `guard` constructs the residue
deterministically and compares four trees around each refusal: both checkouts, the whole Git
worktrees store and the intents. Entries record directories, file bytes and link targets. This
test does not reproduce the sampler's rate or establish that all other repository state is
unchanged.

## Why the sentence cannot simply promise reclaim

`.git/worktrees/` is per repository, not per run or per execution root; a registration whose
`gitdir` names nothing cannot be attributed to this run -- it may be a sibling run's add in flight
in the same repository or a human's killed `git worktree add` -- and the admin directory's name
does not identify its owner (`revalidate_removal` says why; the collision-suffixed name is the
measured form). Those fields alone do not authorize this run to delete the registration. One
possible policy is to
converge the removal of this run's own checkout past it, which is attributable by path; a skip of
exactly that shape was built on PR #151 and withdrawn because, on disk, the kill residue and an
add in flight are identical (`locked` present in both), and the manager's Git children are
outside the crash containment §15 describes for agents: `git` is spawned with a plain
`Command`, and the cleanup lease is held by agent reapers for agent groups only, so a
`SIGKILL`ed conductor's add survives it and a resume can take the exclusive lease while that
child still writes -- `PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED`'s subject. A
"remove `locked` and prune" operator step has the same precondition and is not safe advice
without it.

## What the change that takes this up should do

Decide and document the policy for this public task-registration API in the design section that
owns the workspace manager. Describe the current distinction between an absent `gitdir`, which
is skipped and left to prune, and a present file the manager cannot decode, which refuses
removal. If the policy keeps that refusal, specify its scope and recovery conditions. If it
instead promises convergence, the implementation needs both deletion authority and evidence
that its writers have stopped, with deterministic tests for those conditions. The withdrawn
skip at `88c41a3` is historical evidence, not a prescribed implementation. Keep the classifier
and sampling contract consistent with the adopted behavior. `locked` alone is not a liveness
proof.
