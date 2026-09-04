---
id: PR135-REVIEW2-KILL-DIR-LEAKS-AND-THE-INODE-CLASS-IS-OPEN
severity: P2
disposition: deferred
category: liveness
pr: 135
reviewed_sha: bac2248e43cad55e1523589f83a212d37d99fc52
location: src/engine/topology/scaffold.rs:1392
provenance: pre_existing
first_bad:
guard: the sweep of src/engine/topology/scaffold.rs, which owns kill_dir
---

## Failure sequence

`kill_dir` builds a directory per kill test from a tag, the process id and an ordinal, creates it
with `create_dir_all`, and removes it never. Every kill call leaves that directory and its handoff
file behind. Two process-id namespaces sharing a temporary directory share the name, so one parent
can read the other child's handoff and adopt a tree the other process is still using.

**Measured rather than argued.** 12,427 such directories were present on the review box at pass 2,
including handoffs from this pull request's own reviewed head whose guarded fixture targets had
already been reclaimed. Measured separately on the build box the same day: 1,221,988 entries in
`/tmp`, 996,414 of them `upstroke-*` older than six hours, and deleting those moved inodes from 68%
to 60%.

## What the change that takes this up should do

The class is repository-wide rather than fixture-local, and this box has been stopped by it twice.
A per-kill-test directory that nothing removes is the mechanism; the remedy belongs with whoever
owns the kill protocol, alongside the handoff file's own defects recorded beside this.
