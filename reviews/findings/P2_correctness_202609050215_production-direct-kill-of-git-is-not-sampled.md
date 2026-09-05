---
id: SAMPLER-PRODUCTION-DIRECT-KILL-OF-GIT-IS-NOT-SAMPLED
severity: P2
disposition: deferred
category: correctness
pr: 145
reviewed_sha: 84c21e01676c9e38c79321984441da86d220ce7e
location: src/workspace_manager.rs:2900
provenance: pre_existing
first_bad:
guard: the `remove_worktree` convergence stream (#151's RESIDUE-LOCKED-INVALID-REGISTRATION-OUTLIVES-FORCED-CLEANUP shares the remedy, writer liveness) -- a bare-child arm is sound only once forced removal converges against a live writer; escalates to the owner if a later pass labels it P1 or P2 rather than accepting the deferral
---

## The gap, and why it is a gap rather than a defect

Production has two kill shapes for a Git child, not one. `agent::proc` kills the process group;
the workspace manager spawns its own Git through `Command::output` at `workspace_manager.rs:2900`,
outside that containment, so an operator, a supervisor or the OOM killer ending only the Git
leader is a production sequence too. PR #145's sampler models the first shape and **does not model
the second**: every sample group-kills and waits for the group. That is the state master was in as
well -- master modelled neither correctly -- so the head is not worse than master in this respect;
it is a coverage gap with a name.

## What was tried, and withdrawn on evidence

PR #145 added a bare-child arm alternating with the group arm, held to a bounded set of recovery
outcomes. Its fourth frontier pass found two defects in that machinery and both were real:

1. **False recovery evidence.** `recover_sample` short-circuited at `remove_worktree?` before
   `remove_intent`, so a known non-convergence left the intent behind while `SamplingRecord.recovered`
   stayed `true` -- the artifact reported a recovery that had not happened, contradicting the field's
   documented meaning at `registry.rs:176` and bypassing `UnrecoveredSampling`.
2. **Hygiene that could delete a valid registration.** The clean-up after a known non-convergence
   discarded `remove_dir_all` errors, ignored a failed `read_dir`, dropped per-entry errors through
   `flatten()`, read a failed `gitdir` as "not empty", and removed every whitespace-`gitdir`
   registration without binding it to the current slot -- so a writer filling a `gitdir` between the
   read and the recursive delete lost a valid registration. The same read-then-delete race #151 is
   measuring.

Master has neither piece. Both were withdrawn rather than repaired, because a bare arm cannot be
sound until forced removal converges against a live writer, and that is the `remove_worktree`
convergence stream's subject: the arm's every non-convergence is that finding's sequence.

## Failure sequence, for the record

`git worktree add` starts its checkout -> an operator, supervisor or OOM kill ends only the Git
leader -> the checkout keeps writing -> the durable intent is later reclaimed -> `remove_worktree`
races the writer and returns `DirectoryNotEmpty`. Measured by the sampler's bare-child arm before it
was withdrawn: 7 in 100 across two independent runs. Not sampled at this head.

## What the change that takes this up should do

Once forced removal converges against a live writer, add the bare arm back **without** a bounded
exemption: alternate the two kill shapes across the ladder, witness the bare arm by the same kernel
oracle in the other direction (`child_leads_its_own_group` must be `false`), and assert full
convergence for both. Do not add a hygiene step; if the funnel converges there is nothing to clean.
