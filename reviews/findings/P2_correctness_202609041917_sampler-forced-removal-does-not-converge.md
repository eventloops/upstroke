---
id: PR136-SAMPLER-FORCED-REMOVAL-DOES-NOT-CONVERGE
severity: P2
disposition: deferred
category: correctness
pr: 136
reviewed_sha: ead3573882c931f9c7eaf0846a81be3bffd404a8
location: src/workspace_manager/tests.rs:9464
provenance: pre_existing
first_bad:
guard: the next change that opens this sampler — the repair is measured below, not proposed
---

## Failure sequence

`sampled_git_child_kills_every_residue_classified_and_recovered` kills the Git child of
`Worktree.Add` at a rung of its delay ladder, then `recover_sample` runs the tabled recovery —
forced removal of the worktree and its intent — and asserts it converges. It does not always
converge. `remove_worktree` fails, `recover_sample`'s `expect("forced removal converges")` panics,
and the suite is red.

Observed on CI at `2ee5595`, `test (ubuntu-latest)`, run 33904978763 job 101137256760, `1919
passed; 1 failed`:

```
Filesystem { operation: "remove",
             path: ".../private/workspaces/<key>/<run>/tasks/kalpha-g3",
             source: Os { code: 39, kind: DirectoryNotEmpty } }
```

**Two fingerprints, one assertion.** Reproduced locally the failure also appears as:

```
Git { message: "worktree registration .../worktrees/kalpha-g2 has an empty gitdir" }
```

Same test, same assertion, same sampled site (`Worktree.Add`), different residue according to where
the kill landed. Treating them as one flake is a judgement recorded as one: §12 asks for a
fingerprint by assertion or error code, and these share the first and not the second. A later
reader who needs them split can split them.

## Rate, and the provenance it establishes

Twenty-five runs of that one test on one machine under artificial load, per head:

| Tree | Failures |
|---|---|
| master `d91e84a`, its own copy of the file, predating this pull request entirely | 3/25 |
| `966775e`, this pull request before pass 2 | 2/25 |
| `ead3573`, this pull request after pass 2 | 2/25 |

A flat rate across the change is what excludes the change, which a story about which files moved
cannot. `recover_sample` is byte-identical to this sweep's base but for PR #131's accessor rename,
and it reproduces on master untouched.

## The discriminating experiment, and which way it fell

Two readings were open: (1) `remove_worktree` is genuinely not convergent against the residue an
interrupted `git worktree add` leaves — a product defect; or (2) the sampler kills a **bare child**
while `git worktree add`'s descendants survive and keep writing, so forced removal races a live
writer that no contract promises it can beat — a defect in the sampler.

The discriminator is what production does. Production's only kill path,
`src/agent/proc.rs`, kills the **process group** (`libc::kill(-pid, SIGKILL)` after a pre-exec
`setpgid(0, 0)`); the workspace manager's own Git children are never killed at all — they run to
completion. So the sampler models a fault production does not produce, and produces it in a way
that leaves a live writer.

Measured, fifty runs per arm under the same load, same machine, same commit:

| Sampler kill | Failures |
|---|---|
| bare child, as it is today | 5/50 |
| the child's process group | 0/50 |

**It falls to reading (2).** The rate collapses when the group is killed.

## What the change that takes this up should do

Spawn the sampled child with `process_group(0)` and kill the group rather than the child, which is
the experiment above and measured 0/50 against 5/50. Deliberately **not done in PR #136**: that
pull request is under a narrowing ruling whose terms are withdrawals, deletions, rows and a base
merge-in, and a process-group kill is added machinery that would earn another frontier pass.

What is **not** established, and should not be read into this entry: that `remove_worktree` is
convergent against every residue an interrupted add can leave. The experiment shows the sampler
manufactures a state production's kill path does not, not that no such state exists. A crash of the
engine itself orphans its Git children the same way, and nothing here measures that.
