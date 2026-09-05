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
guard: the change that gives the schema-4 topology a shipped entry point (`src/engine/mod.rs` says a schema-4 run is reachable only from a `#[cfg(test)]` writer today), or the sweep of `src/workspace_manager.rs` (queue row 11), whichever comes first; `an_add_killed_before_it_wrote_gitdir_is_unlisted_and_refuses_forced_cleanup` pins the behaviour a sentence has to describe
---

## What is missing

`DESIGN.md` §15's reclaim sentence -- "Exact gate/review worktrees likewise record and sync a
private intent before `git worktree add`; resume reclaims every such registration before it
switches branches or dispatches another worker" -- is about the gate/review snapshot worktrees
that `upstroke resume` reclaims through `Workspace::reclaim_gate_workspaces`, which removes the
intent-named checkout and never decodes a registration's `gitdir`. The v0.2 workspace manager's
task, staging and snapshot slots (`src/workspace_manager.rs`) are a different funnel with a
different reclaim -- `remove_worktree` binds a registration only through its `gitdir` bytes and
refuses on one it cannot bind -- and no sentence in `design/` says what that funnel does with a
registration it cannot bind. That funnel has no shipped caller today: its callers are the
schema-4 topology, which this build's recovery refuses to resume, and `reclaim_intents` has no
non-test caller. So this is a gap in the design for behaviour that is in the tree and not in the
product, which is why it is P3 and docs-contract: the sentence is owed when the behaviour ships,
and the facts it needs are measured now so they are not re-derived then.

## The behaviour a sentence has to describe, measured (git 2.43.0 on the build box)

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
`remove_worktree` refuses before mutation, for the slot and for every other slot through this
funnel, with a diagnostic naming the admin directory; the test named in `guard` pins all of it
with a whole-tree byte comparison around each refusal.

## Why the sentence cannot simply promise reclaim

`.git/worktrees/` is per repository, not per run or per execution root; a registration whose
`gitdir` names nothing cannot be attributed to this run -- it may be a sibling run's add in flight
in the same repository or a human's killed `git worktree add` -- and the admin directory's name
proves nothing (`revalidate_removal` says why; the collision-suffixed name is the measured form).
So no sound implementation deletes such a registration. What an implementation could do is
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

Write the sentence for this funnel, in §15 beside the gate/review one or in the v0.2 section that
owns the workspace manager: a registration reclaim cannot bind through its `gitdir` bytes is
never deleted by name; one with no `gitdir` is skipped and left to prune; one with a `gitdir` it
cannot decode halts reclaim through this funnel before mutation, and the residue an add killed
before writing `gitdir` leaves is one such. If the product wants that residue converged rather
than halted, the sentence has to state the liveness argument, and the mechanism -- the manager's
Git children under the ownership protocol -- lands with it; then the withdrawn skip (`88c41a3`
on PR #151's branch) is the funnel change and the classifier learns to see the state so the
site's sampling evidence is over what the funnel meets. Never gate on `locked` alone.
