---
id: SAMPLER-RECOVERY-PROVEN-IS-NOT-PROVEN-FOR-AN-EMPTY-GITDIR
severity: P2
disposition: deferred
category: crash-consistency
pr: 136
reviewed_sha: 93d63378f746dae74e36ff556287a3c0f900dea9
location: src/workspace_manager.rs:3018
provenance: pre_existing
first_bad:
guard: the change that reconciles the tabled recovery for `registered_unpopulated_worktree` with `revalidate_removal`'s refusal -- a funnel and contract decision, not a harness one
---

## The two fingerprints, now split

`PR136-SAMPLER-FORCED-REMOVAL-DOES-NOT-CONVERGE` recorded **two error codes under one assertion**
and declined to split them: `Filesystem { operation: "remove", … DirectoryNotEmpty }` and
`Git { message: "worktree registration … has an empty gitdir" }`, "same test, same assertion, same
sampled site, different residue according to where the kill landed … A later reader who needs them
split can split them."

The process-group repair splits them by outcome, which is why this file exists. The first is a live
writer racing a forced removal and the group kill removes it. The second is **not a race with
anything**: it is a residue sitting still on disk that forced removal deliberately refuses to act
on. No kill shape touches it and it survives at the repaired head. Measured at `2f85b6c`, fifty runs per arm alternating under load: the `DirectoryNotEmpty` half went 2/50 to 0/50 with the group kill, and this half was 0/50 against 1/50 -- one event, which is what it should be, since no kill shape can reach a residue that races nothing.

## Related, and how to tell the two apart

`PR136-REMOVE-WORKTREE-VS-A-GIT-CHILD-NOTHING-KILLED` is the other half of the same parent entry,
and it is **not** this one. That one is a live writer with nothing left alive to kill it -- the
engine dies and its Git descendants keep writing -- and it is a sequence nothing here has produced.
This one needs no writer at all: the residue is inert, and the removal refuses to touch it. They
share a parent and an assertion and nothing else. `SAMPLER-SIBLING-KILLS-A-BARE-CHILD` is a third
thing again and belongs to the harness rather than the funnel.

## This is a refusal by design, and that is the point

`revalidate_removal` says so in its own words:

> Any unreadable, empty or partial `gitdir` refuses; guessing from the admin directory's basename
> would authorize deletion from a Git-generated, collision-suffixed name.

That reasoning is sound and this finding does not ask for it to be dropped. **The defect is that the
contract on the other side promises what this refuses.** `effects/residue-classes.json` declares
`Worktree.Add`'s only residue element as `registered_unpopulated_worktree`, classified `internal`,
labelled **`recovery_proven`** -- and the tabled before-phase action for `internal` is the forced
removal that refuses. So `recovery_proven` is not proven for every instance of the element it is
attached to, and the sampler's `expect("forced removal converges")` is the assertion that says so
out loud.

## Failure sequence

`git worktree add` is interrupted after creating `.git/worktrees/<name>/` and opening `gitdir`, but
before its bytes are written -> a zero-length `gitdir` is on disk: a registration that names nothing
-> the residue classifies `Internal`, because `add_state` sees a record whose worktree has no git
dir behind it, which is `registered_unpopulated_worktree` exactly -> recovery takes that class's
tabled action, forced removal of the worktree and its intent -> `revalidate_removal` enumerates
`.git/worktrees/*`, hands each `gitdir`'s bytes to `registration_checkout`, and the empty one takes
the first refusing row -> `UpstrokeError::Git { message: "worktree registration … has an empty
gitdir" }` propagates out of the enumeration, the removal refuses, and the residue that caused the
refusal is the residue the removal was called to clear.

Observed at `2f85b6c`, `Worktree.Add`, sample `kalpha-g1`:

```
forced removal converges: Git { message: "worktree registration
/tmp/upstroke-wm-sample-add-…/repo/.git/worktrees/kalpha-g1 has an empty gitdir" }
```

**Reachable in production**, which is what makes this a finding rather than a harness note. The
engine never kills its own Git children, so the sampler's kill is not the production path -- but it
does not need to be. A crash of the engine, an OOM kill of the `git` process, or a host reset
between that `open` and that `write` leaves the same zero-length file, and the next run's recovery
takes the same tabled action against it.

## Why P2, and not P1 or P3

**Not P1.** The refusal is *before mutation* and it is loud: nothing is deleted, nothing is
corrupted, and the message names the exact path. That is the safe direction for a recovery path to
fail in, and it is the direction this module's table takes on every refusing row.

**Not P3.** It is observed rather than hypothesised, it makes a test intermittently red on master
and on every branch that merges master, and the code path is the engine's own recovery rather than a
test's.

## What the change that takes this up should do

Reconcile the two sides, and that is a **funnel and contract** decision under DESIGN.md §4, not a
change to the harness that found it. Three shapes, and they are not the same change:

* **Narrow the label.** `recovery_proven` is wrong for this element if forced removal cannot clear
  every instance of it; the honest artifact would say what is proven and what is refused, and the
  sampler's assertion would then be over the proven part.
* **Give the removal a safe path for a registration that names nothing.** The caller already knows
  the target checkout -- it is `slot_path(slot)`, not something derived from the registration -- so
  the missing evidence is only *which admin directory belongs to the target*. A registration that
  names nothing cannot be shown to belong to it, which is the whole difficulty; any repair here has
  to say why removing it is safe when the basename cannot be trusted, and `revalidate_removal`'s
  sentence is the argument it has to answer.
* **Skip it for identification.** `find`ing the target's registration could `continue` past an empty
  `gitdir` exactly as it does past a `NotFound`. That converges the *caller* and leaves the
  unparseable registration on disk for Git's own enumeration to trip over later, so it is a smaller
  claim than it looks.

`registration_still_names` already treats a **missing** `gitdir` as convergence and an unreadable
one as an error. Whatever is chosen, the empty case belongs beside those two, stated in the same
table.

**Do not repair this by making the sampler tolerate it.** The assertion that fires is
`expect("forced removal converges")`, and the tabled recovery either converges against the residue
it is tabled for or the table is wrong. Weakening the assertion would delete the only thing in this
tree that knows.
