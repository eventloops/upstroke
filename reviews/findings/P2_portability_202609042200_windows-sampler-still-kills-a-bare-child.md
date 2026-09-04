---
id: SAMPLER-WINDOWS-STILL-KILLS-A-BARE-CHILD
severity: P2
disposition: deferred
category: portability
pr: 145
reviewed_sha: e5e42ac6aa7dbb5a14443df41506c53ab6ce723b
location: src/workspace_manager/tests.rs:9866
provenance: pre_existing
first_bad:
guard: the `#117` `agent/proc` family, whose live stream is on `ambient.rs` and whose queue row is 51 -- a test-only seam there mirroring `child_leads_its_own_group`; escalates to the owner if a later pass labels it P1 or P2 rather than accepting the deferral
---

## What is fixed and what is not

The process-group repair is `#[cfg(unix)]` in both halves — `process_group(0)` in
`SampledChild::spawn` and `kill_group` plus `settle_group` in `SampledChild::kill`. **On Windows the
sampler still reaches only `Child::kill()`**, and `Child::kill` is `TerminateProcess` on the direct
process. "Not observed there" was offered for this and is not an argument: Windows is a first-class
target, CI runs a `test (winguest)` leg on every pull request, and the mechanism is identified
rather than suspected.

## Failure sequence

`git worktree add` launches its checkout descendant -> the sampler terminates only the direct Git
process -> the descendant keeps populating the worktree -> classification and the tabled recovery
begin against a tree a live writer is still building -> `remove_worktree` can return
`Filesystem { operation: "remove", … DirectoryNotEmpty }`, which is exactly the failure
`PR136-SAMPLER-FORCED-REMOVAL-DOES-NOT-CONVERGE` recorded and the Unix half now removes.

Unobserved on the Windows guest so far. That is a statement about the sampling, not about the
mechanism: `git worktree add` spawns a descendant on Windows as it does on Unix, and nothing in the
harness kills it.

## Why it is not repaired in the pull request that found it

The engine's Windows containment is a **Job Object**, created and terminated in
`agent::proc`'s private `windows_job` module. `Job` is `pub(super)`, so it is reachable inside
`agent::proc` and nowhere else, and a workspace-manager test module cannot name it.

That leaves two shapes and both are larger than the change that found this. Building a second Job
Object here would be a second copy of production's containment living in a test module — the
duplicated-platform-fact shape this tree has been bitten by repeatedly, and the reason the Unix half
asks `agent::proc::child_leads_its_own_group` rather than writing its own `getpgid`. Widening
`agent::proc`'s surface so a harness can borrow the real one is a change to a funnel module's public
shape, with its own review.

## Why this is deferred under a rule that says P2s are fixed

Two reasons, both measured on PR #145 itself. The fix needs a test-only seam in a production module
(`src/agent/proc.rs`, queue row 51) whose family already has a live stream; and it can only be
*measured* on the winguest leg — no session on the build box can run it — and this pull request is
the one that had a platform claim disproved by CI for exactly that reason. A fix that cannot be
measured is a second unmeasured platform claim, not a fix. **If a later pass labels this P1 or P2
rather than accepting the deferral, it escalates to the owner rather than re-defers.**

## What the change that takes this up should do

Give the sampler the **same** containment production uses rather than a second implementation of it:
a `#[cfg(all(windows, test))] pub(crate)` seam in `agent::proc` that hands a test a job it can
assign a child to and terminate, mirroring `child_leads_its_own_group`'s shape on the Unix side —
test-only, no production surface, one copy of the platform fact.

Then the Windows half of `SampledChild` gets what the Unix half has: the child assigned to a job at
spawn, the job terminated instead of the process, and a barrier that waits for the job to be empty
before the residue is inspected. The Unix barrier's own measurement is the reason the last of those
matters: on `git worktree add`, six to seven of every eight samples had a process group that was
still non-empty at the instant the old code went on to classify and remove.
