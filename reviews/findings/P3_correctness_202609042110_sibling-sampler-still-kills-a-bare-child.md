---
id: SAMPLER-SIBLING-KILLS-A-BARE-CHILD
severity: P3
disposition: deferred
category: correctness
pr: 136
reviewed_sha: 93d63378f746dae74e36ff556287a3c0f900dea9
location: src/workspace_manager/fixture.rs:418
provenance: pre_existing
first_bad:
guard: PR #135, whose live subject is this file, or the next change that gives `KillableGitChild` a command with descendants to sample
---

## Why this is here rather than fixed

There are exactly two Git-child kill samplers in this tree — `mod tests`'s `SampledChild`, which
samples four commands, and `fixture::KillableGitChild`, which samples the two capture commands for
`T-ATTEMPT`. The first has just been repaired to kill the child's **process group**, because a bare
child kill leaves `git worktree add`'s descendants writing into a worktree forced removal then
races. The second still kills the bare child.

**It is not a defect today**, which is why this is P3 and not the severity of the finding it
mirrors. `KillableGitChild` is driven at `Object.CandidateStage` and `Object.CandidateWriteTree`
only (`sampled_argv` panics on any other site), and `git add -A` and `git write-tree` spawn no
children **only where the user's Git configuration assigns no filter**: PR #145's fourth pass
probed a global-style attributes file with a clean filter and `git add` spawned it even under every
pin `WorkspaceManager::command` sets, which is
`WORKSPACE-GIT-BUILDER-DOES-NOT-PIN-ATTRIBUTES-OR-FILTERS`. So "nothing to leak" is a property of
the machine, not of the harness, which §12 forbids relying on. Build the child through
`WorkspaceManager::command` as the four-command sampler does — strictly better than transcribing a
pin — and do not claim childlessness until that finding is closed.

`src/workspace_manager/fixture.rs` is PR #135's live subject and was under a frontier pass while
this was written, so the repair is not made here on the coordinator's instruction rather than on a
judgement that it does not matter.

## Related

The repair this mirrors closes the `DirectoryNotEmpty` half of
`PR136-SAMPLER-FORCED-REMOVAL-DOES-NOT-CONVERGE` and nothing else.
The other half — a zero-length `gitdir` that `remove_worktree` refused — turned out to be the same
missing capability as the live-writer question, recorded under PR #151's RESIDUE-LOCKED-INVALID-REGISTRATION-OUTLIVES-FORCED-CLEANUP; it would not have been helped by making this sampler kill a group either.

## Failure sequence

`sampled_argv` gains a third site whose command spawns a Git subprocess — `worktree add`,
`cherry-pick`, anything that runs a hook, or the same two commands in a fixture that configures a
filter -> `KillableGitChild::kill` kills the direct child only -> the descendants survive and keep
writing into the sample worktree -> `sample_once`'s `remove_worktree(...).expect("forced removal
converges")` races a live writer -> `Filesystem { operation: "remove", … DirectoryNotEmpty }` and an
intermittently red suite whose rate depends on the machine's load.

That is the sequence measured next door at 5 failures in 50 runs, one machine, one load. It is
written here as a sequence and not as an observation: nothing has produced it from this sampler,
because this sampler has nothing to orphan yet.

## What the change that takes this up should do

The same repair, and it is **not** four lines — PR #145's first pass showed the four-line version
insufficient, and a durable finding that advertises a known-insufficient repair is worse than none.
It is two pieces:

1. **The group kill.** Spawn with `process_group(0)` on Unix, `SIGKILL` the group before the child,
   refuse to aim a group kill at a child the kernel does not confirm leads its own group, and record
   the outcome inside the kill so that deleting it is a compile error rather than a weaker sampler.
2. **The barrier, without which the first piece closes nothing.** `kill(-pgid, SIGKILL)` queues
   signals and returns; `agent::proc::tests::kill_tree_settles_the_whole_unix_group_before_it_returns`
   says so. So after reaping the leader, poll `kill(-pgid, 0)` until it answers **`ESRCH`** — only
   `ESRCH` is absence; `EPERM` is an entry that remains and is not yours to signal (on Darwin, a
   zombie awaiting launchd), so keep polling; anything else is a bug — bounded, and treat a
   timeout as a hard failure **at the point it is detected**, before the sample reads anything. Measured next door: on `git worktree add`, six to seven of every eight
   samples still had a live group when the four-line version went on to classify and remove.

`SampledChild` in `src/workspace_manager/tests.rs` is both pieces written out — `kill_group` and
`settle_group` — with the witness for the first (`crate::agent::proc::child_leads_its_own_group`,
production's own kernel oracle for the same fact), the Linux-only delivery assertion and why it is
Linux-only, and the argument for the Unix `cfg`. Do not retry a refused inspection: PR #145 tried
a bounded retry and withdrew it after three passes each found it unsound on the platform its
refusal occurs on.

Doing it before a site with descendants is added is worth more than doing it after, because after
means diagnosing an intermittent red first. The alternative — a comment on `sampled_argv` saying
the two capture commands must stay childless — is not equivalent: it constrains the fixture rather
than the harness, and the fixture is where a filter or a hook would be added.
