---
id: RESIDUE-EMPTY-GITDIR-REGISTRATION-BLOCKS-EVERY-REMOVAL
severity: P2
disposition: deferred
category: crash-consistency
pr: 151
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/workspace_manager.rs:3083
provenance: pre_existing
first_bad: SAMPLER-RECOVERY-PROVEN-IS-NOT-PROVEN-FOR-AN-EMPTY-GITDIR
guard: **escalated to the project owner, not merge-blocking**: a ruling on `DESIGN.md` §15's sentence "resume reclaims every such registration", which as written promises what the repository's structure forbids (below), so what is owed is a design decision and not a code fix, and no session edits the design. The coordinator's decision dossier is `/srv/worktrees/sweep-coordinator-decision-registration-reclaim.md` on the build box. The reproduction (`an_add_killed_before_it_wrote_gitdir_is_unlisted_and_refuses_forced_cleanup`) stands and pins the refusal as behaviour that must not change silently; a later pass that labels this P1 or P2 escalates to the owner rather than re-deferring. `MAINTAINING.md`'s witness rule is answered, not evaded: the witness reproduces a refusal the ruling below says is correct, and PR #151 changes no production code.
---

## The sentence, the deviation, and why the sentence is the thing that is wrong

`DESIGN.md` §15 (`design/15_design_event_log_resume_run_layout.md`, the crash-containment
paragraph): "Exact gate/review worktrees likewise record and sync a private intent before
`git worktree add`; **resume reclaims every such registration before it switches branches or
dispatches another worker.**" The residue below is such a registration -- its intent was synced,
its `git worktree add` began -- and resume does not reclaim it: it refuses, and the refusal halts
every removal in the repository. Read alone, that is code deviating from the design. It is not the
whole sentence. `.git/worktrees/` is per **repository**, not per run or per execution root, and
`revalidate_removal` enumerates every registration in it; a registration whose `gitdir` names
nothing cannot be attributed to this run -- it may be a sibling run's add in flight in the same
repository, or a human's killed `git worktree add` -- and the admin directory's name proves
nothing (`revalidate_removal` says why; the collision-suffixed `wt-e1` is the measured form of
it). So no sound implementation may delete such a registration, with or without liveness of this
run's own writers, and "reclaims **every** such registration" states a guarantee the repository's
structure forbids. **The refusal the test pins is correct behaviour, and the sentence is what
needs the owner's ruling.** What a ruling could still choose is a weaker reclaim -- converge the
removal of this run's own checkout, which is attributable by path, and leave the registration to
Git's prune under its `locked` marker -- and that choice is the owner's, which is why this file
escalates rather than proposes.

## What the residue is, measured (git 2.43.0 on the build box, strace of `git worktree add --detach`)

The add writes, in order: `mkdir .git/worktrees/<name>`; open+write `locked` with
`initializing\n`; `mkdir <checkout>`; open+write `gitdir`; open+write `<checkout>/.git`;
`HEAD`; `commondir`; then the checkout through child processes; on the run measured,
`unlink .git/worktrees/<name>/locked` was the last syscall. A kill between opening `gitdir` --
which creates it, zero-length -- and writing it leaves exactly
`.git/worktrees/<name>/{locked, gitdir(0 bytes)}` and an empty checkout directory with no pointer.
On the same version: an add that fails before the checkout is populated (the checkout's `mkdir`
refused) exits 128 and removes its whole admin directory; an add whose `post-checkout` hook fails
exits 1 and leaves a complete, unlocked registration (`locked` unlinked, `gitdir` written); with
hooks disabled, as this crate runs Git, the same add exits 0. On none of the paths measured is
`gitdir` zero-length without `locked` beside it. `git worktree prune` skips the entry while
`locked` is present and removes it as an "invalid gitdir file" once unlocked; a later add at the
same checkout path gets a Git-generated collision-suffixed admin name and works.

## How the classifier reads it, and what that does to the site's evidence

The classifier reads registration through `git worktree list --porcelain`, and Git's enumerator
skips a registration it reads zero bytes from, silently, exit 0. So `record_for` answers `None`,
`add_state` answers `Unregistered`, `classify_object_residue` answers `ObjectResidue::None`, and
`observed_residue_elements` is empty. `None` is defined as "nothing was written". Here an intent,
an admin directory, `locked`, a `gitdir` and a checkout directory were written: **durable state
the classifier reads as nothing, because its source does not list it.** That is the mechanism the
prior finding lacked. Its claim stands: the site's `recovery_proven` evidence is not proven for
this residue. Not through the element it named -- `registered_unpopulated_worktree` is defined by
Git listing the worktree, and this one is not listed -- but through the sampling half of the
evidence: `SamplingRecord.recovered` requires every sampled residue to recover by its classified
action, the sampler runs forced removal for every sample, and a sample landing in this window has
forced removal refuse. `effects/residue-classes.json` and `residue_authority.rs` are therefore
implicated: the site's label claims a convergence the funnel does not deliver for a state the
classifier cannot see. This file is the single home of that finding; PR #145's row records the
prior id as superseded by this one.

## Failure sequence

`git worktree add` for slot X is killed in that window -> the residue above -> on the next run
recovery calls `remove_worktree` for X, or for **any** slot: `revalidate_removal` enumerates every
registration and hands each `gitdir`'s bytes to `registration_checkout`, and the zero-length one
takes the first refusing row -> `UpstrokeError::Git { message: "worktree registration … has an
empty gitdir" }` -> every removal in the repository refuses, and resume does not reclaim the
registration §15 says it reclaims. The sampler reproduces it at about 1/50 under load (the handing
session's measurement, two independent trees) as `forced removal converges`; the test named in
`guard` constructs it deterministically. Loud, before mutation, nothing deleted -- which is why it
is P2 and not P1.

## Why the obvious repair is unsafe today, measured, and what would still not make it a reclaim

Skipping the zero-length registration for identification, as an absent one is skipped, converges
the caller and binds nothing. It was built on PR #151 and withdrawn after its first review pass:
conductor A's add is descheduled between open and write of `gitdir`; A is killed and its Git child
survives; recovery B reads zero bytes, skips, and removes the checkout while A's Git resumes and
populates it -- B deletes beneath a live writer. On disk that window and the kill residue are
identical, `locked` being present in both. No file on disk tells them apart; what tells them apart
is whether a Git child of the dead conductor is still alive, and **the tree has no evidence of
that today.** §15's crash containment covers agent processes: on Unix the cleanup reapers hold the
run's shared cleanup lease for their agent groups, and the manager spawns `git` with a plain
`Command` -- no process group of its own, no parent-death signal -- so a `SIGKILL`ed conductor's
`git worktree add` survives it, and `resume` can take the exclusive side of the lease while that
child still writes. That is `PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED`'s open question
seen from the other side: the two findings are one missing capability. The successor stream the
coordinator opened for a fix confirmed both readings at the head -- `workspace_manager.rs:2903`
spawns `git` with no process group, no `pre_exec` and no parent-death signal, and
`cleanup::take` at `rundir.rs:1486` probes the exclusive lease and releases it at once, retaining
nothing forked children would inherit -- and then found the per-repository fact above, which is
why it stood down: liveness of this run's writers would make the checkout removal safe, and would
still not authorise deleting a registration this run cannot show is its own.

The same precondition binds an operator. "Remove `locked` and prune" is only safe once no Git
child of the dead conductor survives: with one paused holding `gitdir` open, the prune removes
the admin directory and the child resumes writing through an unlinked descriptor into a missing
path. The tree offers no check for that, so this finding gives no operator step.

## What the ruling has to decide, and what follows from each answer

The sentence in §15. If the owner narrows it -- resume reclaims every such registration **it can
bind to a checkout under its own execution root**, and refuses on one it cannot -- then the code
already conforms, the test above is its witness, and what remains is that the residue halts the
run until an operator acts, with no safe operator step until the run's own Git children are under
the ownership protocol §15 describes for agents (the sibling finding's subject). If the owner
chooses the weaker reclaim -- converge the removal of this run's own checkout past a registration
that names nothing, leave the registration to Git's prune under its `locked` marker -- then the
order is: containment of the manager's Git children first, so a dead conductor's add cannot
outlive the run's cleanup lease; then the seven-line skip withdrawn at `88c41a3` on PR #151's
branch, gated on that guarantee; then a classifier that sees the state, so the site's sampling
evidence is over what the funnel meets; and in the test the first block stays while the two
refusal blocks become convergence assertions. Under either answer: never gate on `locked` alone,
it is present in both states; never delete an admin directory by its name; and the classifier's
hole is real either way and wants closing so that the site's evidence stops claiming a
convergence for a state it cannot see.
