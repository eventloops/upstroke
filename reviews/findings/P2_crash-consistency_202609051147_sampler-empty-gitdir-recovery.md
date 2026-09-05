---
id: SAMPLER-RECOVERY-PROVEN-IS-NOT-PROVEN-FOR-AN-EMPTY-GITDIR
severity: P2
disposition: deferred
category: crash-consistency
pr: 161
reviewed_sha: 787d945010bfc02d7584af6076f8fe942bc57a41
location: src/workspace_manager/tests.rs:9443
provenance: pre_existing
first_bad:
guard: PR #145 sampler/recovery/containment correction, coordinated with PR #151's deterministic refusal witness
---

## Failure sequence

`sampled_git_child_kills_every_residue_classified_and_recovered` interrupts
`Worktree.Add`. A sample leaves a worktree registration with an empty `gitdir`.
`recover_sample` expects forced removal to converge, but `registration_checkout`
refuses the registration at `src/workspace_manager/parsers.rs:111`. The expectation
at the location above panics with `forced removal converges` and `has an empty
gitdir`.

The full baseline on the recorded SHA observed this failure on 2026-09-05:
1,924 tests passed, one failed and 38 were ignored. Source contents were unchanged
by the locked mtime refresh; Cargo compiled this worktree in 30.13 seconds. The
preserved evidence is `baseline-787d945-first` in the PR #161 implementation
artifacts. One subsequent exact-test diagnostic passed after a fresh compilation:
eight `Worktree.Add` samples yielded two `None`, six `Internal` and zero `After`
residues. Its log is `sampler-targeted-787d945.log`. That pass does not establish a
fix or a failure rate. `reviewed_sha` here identifies the observed candidate; it
does not claim an independent review of that candidate.

The stable finding belongs to the [PR #145 ledger](https://github.com/eventloops/upstroke/pull/145).
The older, broader [PR136-SAMPLER-FORCED-REMOVAL-DOES-NOT-CONVERGE](P2_correctness_202609041917_sampler-forced-removal-does-not-converge.md)
record also includes a live-writer `DirectoryNotEmpty` failure. Its process-group
experiment does not prove recovery from the empty-`gitdir` residue. The exact first
bad commit remains unknown. This documentation migration leaves the sampler and
parser unchanged and defers this P2 under the owner's documentation policy.

## What the change that takes this up should do

PR #145 owns the sampler, recovery and containment correction for this finding.
Preserve PR #151's separate deterministic refusal and snapshot witness. Do not
silently skip an unbindable registration: that state can be indistinguishable from
another live add. Establish which residue the sampler may claim to recover, retain
the refusal boundary, and provide a regression witness for the corrected claim.
Neither a passing retry nor this documentation deferral closes the finding.
