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
guard: an owner decision in `DESIGN.md` on what recovery does with an add abandoned before its registration was written, and -- if that decision is reclaim -- the live-writer check that is the guard of `PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED`. **Escalated to the project owner and merge-blocking, not ordinary deferred debt**: this finding carries a deterministic reproduction (`an_add_killed_before_it_wrote_gitdir_is_unlisted_and_refuses_forced_cleanup`), so `MAINTAINING.md`'s witness rule makes it fixable regardless of severity; but the fix is a design choice and no session edits `DESIGN.md`, so there is no head at which PR #151 could close it, and the sweep coordinator has put it to the owner as the fifth such instance. A later pass that labels this P1 or P2 escalates to the owner rather than re-deferring.
---

## What this supersedes, and why the earlier framing was wrong

`SAMPLER-RECOVERY-PROVEN-IS-NOT-PROVEN-FOR-AN-EMPTY-GITDIR` (on PR #145's branch; that pull
request's row records the id as superseded by this one, not fixed) read the sampler's `forced
removal converges: Git { message: "worktree registration … has an empty gitdir" }` as the
contract in `effects/residue-classes.json` promising, for the element
`registered_unpopulated_worktree`, a recovery the funnel rightly refuses. Measured, the residue
is not an instance of that element. Git's enumerator skips a registration it reads zero bytes
from -- `git worktree list --porcelain` does not list it, exit 0 -- so `record_for` answers
`None`, `add_state` answers `Unregistered`, the classifier answers `None` and
`observed_residue_elements` is empty. The `recovery_proven` label is a claim about the element
as the classifier defines it, and that claim holds; `effects/residue-classes.json` is not what is
wrong. This file is the single home of the finding.

## The residue, measured (git 2.43.0 on the build box, strace of `git worktree add --detach`)

The add writes, in order: `mkdir .git/worktrees/<name>`; open+write `locked` with
`initializing\n`; `mkdir <checkout>`; open+write `gitdir`; open+write `<checkout>/.git`;
`HEAD`; `commondir`; then the checkout through child processes; on the run measured,
`unlink .git/worktrees/<name>/locked` was the last syscall. So a kill between opening `gitdir`
-- which creates it, zero-length -- and writing it leaves exactly:
`.git/worktrees/<name>/{locked, gitdir(0 bytes)}` and an empty checkout directory with no
pointer. `git worktree prune` skips it while `locked` is present and removes it as an "invalid
gitdir file" once unlocked; a later add at the same checkout path gets a Git-generated
collision-suffixed admin name and works.

What `locked` does on the failure paths, measured on the same version rather than read from
source: an add that fails before the checkout is populated (the checkout's `mkdir` refused,
parent mode 555) exits 128 and removes its whole admin directory; an add whose `post-checkout`
hook fails exits 1 and leaves a **complete, unlocked** registration -- `locked` unlinked, `gitdir`
written (119 bytes), `HEAD`, `commondir`, `index` present. With hooks disabled, as this crate
runs Git, the same add exits 0. Neither failure path produces a zero-length `gitdir`: the one that
can, the kill in the window, is the one that keeps `locked`.

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
deletes beneath a live writer. On disk that window and the kill residue are identical: `locked`
is present in both, because it is written before `gitdir` and, on every path measured above, is
gone only together with a written `gitdir` or with the whole directory. So no file on disk
discriminates in-flight from abandoned, and a `locked`-gated skip would protect the sequence
while converging nothing observed. The discriminator the sequence needs is liveness of the writer,
which is exactly `PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED`'s open question: the two
findings are one missing capability seen from two residues.

## What the change that takes this up should do

First a design sentence, because successful cleanup that refuses on, retains, or removes an
abandoned add's admin directory is externally observable crash-recovery behaviour and
`DESIGN.md` is the only authority for it. Two branches, and this finding chooses neither:

* **Keep the refusal.** Then the behaviour is already what master does; the sentence says so,
  names the operator step that clears the residue (remove `locked`, `git worktree prune`), and
  the test above stands as it is. The residue's cost is a halted run per crash in the window.
* **Reclaim.** Then the sentence states the liveness argument under which a zero-length
  registration may be treated as abandoned, the mechanism that establishes it lands with the
  live-writer check the sibling finding needs, and the skip withdrawn at `88c41a3` on this branch
  becomes the funnel change once that check gates it; the test's two refusal blocks become
  convergence assertions and its first three facts stay.

Under either branch: do not gate on `locked` alone, it is present in both states; and do not
guess from the admin directory's basename -- `revalidate_removal` says why, and the
collision-suffixed `wt-e1` is the measured form of that reason.
