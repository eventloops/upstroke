---
id: RESIDUE-LOCKED-INVALID-REGISTRATION-OUTLIVES-FORCED-CLEANUP
severity: P3
disposition: accepted-risk
category: crash-consistency
pr:
reviewed_sha: 88c41a32a70c0464ba7e8dad413710a49df30d3d
location: src/workspace_manager.rs:3099
provenance: pre_existing
first_bad:
guard: the change that gives forced cleanup a live-writer check (the guard of PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED); until then `an_add_killed_before_it_wrote_gitdir_does_not_block_forced_cleanup` asserts the directory stays and that unlocking it is what clears it
---

## Failure sequence

`git worktree add` is killed between writing `locked` and writing `gitdir` (measured on git
2.43, the add writes the admin directory, `locked`, the checkout directory, `gitdir`, the
checkout's `.git` pointer, `HEAD`, `commondir`, in that order) -> `.git/worktrees/<name>/`
holds `locked` and either no `gitdir` or a zero-length one -> `remove_worktree` for the slot
converges: identification skips the registration because it names no checkout, the checkout goes
under containment, and the trailing `git worktree prune` skips the entry because it is locked ->
the next add for the slot succeeds under a Git-generated, collision-suffixed admin name and is
bound through its own `gitdir` -> the stale directory is never removed by anything in the tree.
One per crash in that window, accumulating under `.git/worktrees/` for the life of the
repository.

Nothing fails. `git worktree list` does not enumerate the entry, `git worktree prune` will not
touch it while `locked` is present, and no code binds an admin directory by basename. What is
left is residue, and this file exists so that it is recorded as a chosen boundary rather than
rediscovered as a defect.

## Why the removal does not clear it

The only evidence that the directory is dead is that its `gitdir` is unreadable-as-a-path and
that no process in the engine is adding. Git's own rule for `locked` is hands off, and
`git worktree add` writes `locked` first precisely so that a concurrent prune cannot remove an
add in flight. Deleting `locked` from an entry the removal cannot bind would need an argument
that no live `git worktree add` is writing it, and that argument is the open question of
`PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED`: a crashed engine's orphaned Git
descendants are exactly the live writer nothing in the tree can rule out. Guessing from the
basename is refused by `revalidate_removal` for the reason it states, and that reason is not
changed here.

## What the change that takes this up should do

Once forced cleanup can establish that no writer holds the entry, remove `locked` from a
registration whose `gitdir` is absent or zero-length and let the trailing prune take it; the
test named in `guard` pins both halves of today's boundary (the directory stays, and unlocking
it is what clears it), so the change flips one assertion and keeps the other. Do not clear it by
basename, and do not clear it while a writer can still be alive.
