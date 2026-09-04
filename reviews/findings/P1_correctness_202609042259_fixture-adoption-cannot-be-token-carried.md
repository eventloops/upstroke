---
id: PR135-FIXTURE-RECLAMATION-IS-NOT-TOKEN-CARRIED
severity: P1
disposition: deferred
category: correctness
pr: 135
reviewed_sha: 9008405625d7ad36d3de029741e35999d299b63f
location: src/workspace_manager/fixture.rs:213
provenance: pre_existing
first_bad: 61529ab
guard: an authenticated ownership handoff, then the sweep of src/workspace_manager.rs (queue row 11) or whoever owns the kill protocol in src/engine/topology/scaffold.rs
---

## Failure sequence

`scratch` builds a directory name from the tag, the process id and a per-call ordinal, and calls
`remove_dir_all` on it **before acquiring anything**. After process-id reuse, or with two process-id
namespaces sharing one temporary directory, that deletes an earlier occupant's tree; creating
exclusively afterwards does not authorise the deletion that preceded it. `Fixture::drop` then removes
the root recursively, from a path the fixture was built or handed rather than one it can prove it
owns. §8 requires both to carry the `cfg(test)` scratch-tree token.

## Why it is still here after four passes

Three shapes were tried and all three were faulted by a frontier pass:

1. **The token on both paths.** `rundir::scratch_tree` closes `scratch`, but a token cannot be minted
   from a path, so a fixture *adopted* from a tree another process built can hold none. Reclaiming
   nothing then leaks a tree per kill test — the class that has twice exhausted this box's inodes.
2. **A parent minting the tree and handing the child a variable naming it.** Pass 2: set
   `UPSTROKE_TEST_KILL_SCRATCH` to a directory containing `repo/`, run any topology test, and the
   fixture reinitialises that repository, rewrites its config, commits and checks out branches. An
   unauthenticated protocol.
3. **A two-armed reclaim, token for the ordinary path and master's removal for adoption.** Pass 3
   found the droppable value was built before validation, so an unwind deleted the supplied tree.
   Pass 4 found that moving construction after the four checks only protects *invalid*
   repositories: a stale handoff naming another **valid** fixture passes all four, `Run::adopt`
   then fails replaying that tree's event log, and unwinding deletes it.

## What the change that takes this up should do

Authenticate the handoff, or accept that the fixture cannot own an adopted tree. A parent cannot
prove it owns a tree its child created, and a child that dies by `std::process::abort()` cannot
hand anything back. Any design that closes this needs the token to survive a process boundary —
which is a design question for `rundir::scratch_tree`, not a repair to a fixture. Until then both
removals stay untokened and this file stays open.
