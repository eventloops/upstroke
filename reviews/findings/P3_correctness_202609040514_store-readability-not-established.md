---
id: PR128-STORE-READABILITY-NOT-ESTABLISHED
severity: P3
disposition: deferred
category: correctness
pr: 128
reviewed_sha: f161c9e8555c88474f09f89f095737346070d334
location: src/workspace_manager.rs:3004
provenance: pre_existing
first_bad: 7a83e69
guard: deferred, with the proposal: proving a store readable means reading it the way git does — objects/info/alternates followed to each alternate, a…
---

## Failure sequence

git reports an object it cannot read as one it does not have: with a pack unreadable, `rev-parse --verify --quiet <id>` with a peel exits 1 in silence, exactly as for a missing object (measured on git 2.43) -> nothing at this layer distinguishes a storage failure from absence -> `object_exists` answers `false`, and `refuse_absent_source` refuses a dispatch naming a lost candidate, for a repository whose store is unreadable; latent, since a store the engine cannot read fails at its next write anyway

## What the change that takes this up should do

deferred, with the proposal: proving a store readable means reading it the way git does — `objects/info/alternates` followed to each alternate, a file-type check before every open, and a bound on every open so a FIFO cannot block — which belongs to whoever owns the object store in `src/workspace_manager.rs`, not to a residue classifier; this pull request's first two attempts at it produced the rows above, and the whole programme is reverted at 59f2bd99c95c80f1a2a011a78ebe34b43fbf4555, so nothing in this pull request claims a store is readable and the parent's `object_exists` answers exactly as it does on master

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
