---
id: RESIDUE-EMPTY-GITDIR-REGISTRATION-BLOCKS-EVERY-REMOVAL
severity: P2
disposition: deferred
category: crash-consistency
pr: 151
reviewed_sha: b5556eecd2a0d334f9544afcb8ca89bfd07a1be6
location: src/workspace_manager.rs:3083
provenance: pre_existing
first_bad: SAMPLER-RECOVERY-PROVEN-IS-NOT-PROVEN-FOR-AN-EMPTY-GITDIR
guard: beyond reach of any sweep session -- the remedy needs a live-writer check for the engine's own Git children (the guard of PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED; the two are one problem) and a DESIGN.md sentence on abandoned-add residue, and DESIGN.md is the owner's. A later pass that labels this P1 or P2 escalates to the owner rather than re-deferring. Until then `an_add_killed_before_it_wrote_gitdir_is_unlisted_and_refuses_forced_cleanup` pins the state and the refusal.
---

## What this supersedes, and why the earlier framing was wrong

`SAMPLER-RECOVERY-PROVEN-IS-NOT-PROVEN-FOR-AN-EMPTY-GITDIR` (on PR #145's branch) read the
sampler's `forced removal converges: Git { message: "worktree registration … has an empty
gitdir" }` as the contract in `effects/residue-classes.json` promising, for the element
`registered_unpopulated_worktree`, a recovery the funnel rightly refuses. Measured, the residue
is not an instance of that element. Git's enumerator skips a registration it reads zero bytes
from -- `git worktree list --porcelain` does not list it, exit 0 -- so `record_for` answers
`None`, `add_state` answers `Unregistered`, the classifier answers `None` and
`observed_residue_elements` is empty. The `recovery_proven` label is a claim about the element
as the classifier defines it, and that claim holds; `effects/residue-classes.json` is not what is
wrong. This file is the single home of the finding; #145's row records its own id as superseded by
this one, not fixed.

## The residue, measured (git 2.43.0, strace of `git worktree add --detach`)

The add writes, in order: `mkdir .git/worktrees/<name>`; open+write `locked` with
`initializing\n`; `mkdir <checkout>`; open+write `gitdir`; open+write `<checkout>/.git`;
`HEAD`; `commondir`; then the checkout through child processes; and `unlink locked` as the
last syscall of the run. A failed add (mkdir of the checkout refused) removes the whole admin
directory. So a kill between opening `gitdir` -- which creates it, zero-length -- and writing it
leaves exactly: `.git/worktrees/<name>/{locked, gitdir(0 bytes)}` and an empty checkout
directory with no pointer. `git worktree prune` skips it while `locked` is present and removes
it as an "invalid gitdir file" once unlocked; a later add at the same checkout path gets a
Git-generated collision-suffixed admin name and works.

## Failure sequence

`git worktree add` for slot X is killed in that window -> the residue above -> on the next run
recovery calls `remove_worktree` for X, or for **any** slot: `revalidate_removal` enumerates
every registration and hands each `gitdir`'s bytes to `registration_checkout`, and the
zero-length one takes the first refusing row -> `UpstrokeError::Git { message: "worktree
registration … has an empty gitdir" }` -> every removal in the repository refuses until a human
removes `locked` and prunes. The sampler reproduces it at about 1/50 under load (the handing
session's measurement, two independent trees); the test named in `guard` constructs it
deterministically. Loud, before mutation, nothing deleted -- which is why it is P2 and not P1.

## Why the obvious repair is wrong, measured

Skipping the zero-length registration for identification, as an absent one is skipped, converges
the caller and binds nothing. It was built on this pull request and withdrawn after pass 1: the
sequence engine A's add descheduled between open and write of `gitdir`; A killed with its Git
child surviving (the manager's Git children are never killed) and its lease released; recovery B
reads zero bytes, skips, and removes the checkout while A's Git resumes and populates it -- B
deletes beneath a live writer. On disk that window and the kill residue are identical:
`locked` is present in both, because it is written before `gitdir` and unlinked only on
success, and `locked` absent with a zero-length `gitdir` is a state Git never produces. So no
file on disk discriminates in-flight from abandoned. The discriminator the sequence needs is
liveness of the writer, which is exactly `PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED`'s
open question: the two findings are one missing capability seen from two residues.

## What the change that takes this up should do

Two things, neither inside a sweep session's reach. A design sentence saying what recovery does
with an add abandoned before its registration was written -- refuse until a human prunes, or
reclaim under a stated liveness argument -- because successful cleanup that retains or removes a
locked admin directory is externally observable crash-recovery behaviour and `DESIGN.md` is the
only authority for it. And the mechanism that sentence needs: a way for forced cleanup to know
that no Git child of a dead engine still holds the registration, after which skipping the
zero-length `gitdir` for identification is the whole repair (the withdrawn diff is in this pull
request's history at `88c41a3`). Do not gate on `locked`: it is present in both states. Do not
guess from the admin directory's basename: `revalidate_removal` says why, and the
collision-suffixed `wt-e1` is the measured form of that reason.
