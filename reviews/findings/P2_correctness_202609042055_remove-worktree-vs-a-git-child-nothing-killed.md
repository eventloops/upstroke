---
id: PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED
severity: P2
disposition: deferred
category: correctness
pr: 136
reviewed_sha: ead3573882c931f9c7eaf0846a81be3bffd404a8
location: src/workspace_manager.rs:1868
provenance: pre_existing
first_bad:
guard: the change that gives the engine a kill path for its own Git children, or a sampler that models the engine's own death rather than a killed child
---

## Why this file exists at all

`PR136-SAMPLER-FORCED-REMOVAL-DOES-NOT-CONVERGE` is closed and its file deleted: the sampler kills
the process group now, which is what production's only kill path does, and it waits for that group
to empty before it inspects. **What that closes is one of the two error codes that entry recorded,
and the suite is not thereby green** — `Git { … has an empty gitdir }` is untouched by any kill
shape and has its own file. **Nor is the question that finding left open gone with it.** That entry
wrote
down, in its own last paragraph, what its experiment did *not* establish, and a question does not
stop being open because the entry carrying it was resolved. It is here, alone, so that closing the
first one does not close this one by accident — and so that nobody re-runs the sampler comparison
believing it answers this.

## Related, and how to tell the two apart

`SAMPLER-RECOVERY-PROVEN-IS-NOT-PROVEN-FOR-AN-EMPTY-GITDIR` is the other half of the same parent
entry's two error codes, and it is **not** this one. That one is a residue lying still on disk that
forced removal refuses by design; it needs no live writer and it is observed. This one is a live
writer with nothing to kill it, and it is a sequence nothing here has produced. They share a parent
and an assertion and nothing else, and a repair for either leaves the other exactly where it was.

## Failure sequence

The engine dies — a crash, a `SIGKILL`, a host reset — while `WorkspaceManager::add_worktree` has a
`git worktree add` in flight. Nothing kills that child: the engine's own Git children are never
killed, and there is no coordinator left to kill them. Its descendants (`git checkout` and what
that spawns) are reparented and **keep writing into the new worktree**. On the next run, recovery
takes the tabled before-phase action for the residue it finds, which is forced removal of the
worktree and its intent -> `remove_worktree` walks a directory a live writer is still populating ->
the removal fails, `Filesystem { operation: "remove", … DirectoryNotEmpty }`, and recovery does not
converge.

That is a **sequence, not an observation.** No run of anything in this repository has been seen to
produce it. It is written concretely because a finding that says "convergence is unproven" is not
actionable and this one is.

## What was measured, and exactly what it does not answer

Fifty runs per arm, one machine, one load, one commit: the sampler killing the bare child failed
5/50, and killing the child's process group failed 0/50. That experiment discriminates between two
readings of a red suite — "`remove_worktree` is not convergent" against "the sampler manufactured a
state production's kill path does not" — and it fell to the second.

**It says nothing about the sequence above.** It shows that *this harness* was producing a live
writer that production's kill path does not produce. It does not show that no other path produces
one, and the engine's own death is precisely such a path: it orphans Git children the same way, and
nothing in this tree measures it. Reading the 5/50 -> 0/50 as evidence that `remove_worktree` is
convergent is reading a control as a proof.

## What the change that takes this up should do

Model the engine's death rather than a killed child. The sampler cannot do it as it stands: it
kills the Git child, which is the fault it exists to sample, and it now kills the whole group, which
is the fault production produces. What this question needs is the other shape — a `git worktree add`
whose descendants are **left running** while the process that started them goes away — and then
either a measured convergence rate for `remove_worktree` against it, or a repair that makes the
removal converge (a bounded retry against `DirectoryNotEmpty`, an exclusion pass, or a refusal that
names the live writer rather than a removal that quietly does not finish).

Two things to keep whichever way it goes. The **fingerprint** is by assertion and error code, and
`PR136-SAMPLER-FORCED-REMOVAL-DOES-NOT-CONVERGE` recorded two error codes under one assertion — the
`DirectoryNotEmpty` above and `Git { message: "worktree registration … has an empty gitdir" }`.
They are the same assertion and not the same failure. And a repair here is a change to a **funnel**,
not to a test: it belongs to `src/workspace_manager.rs`, under §4's invariants, not to the harness
that found it.
