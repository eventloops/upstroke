---
id: PR135-FIXTURE-COMMITS-ARE-STRINGS-WHERE-THE-TREE-HAS-OBJECTID
severity: P3
disposition: deferred
category: docs-contract
pr: 135
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager/fixture.rs:75
provenance: pre_existing
first_bad: 61529ab
guard: the sweep of src/workspace_manager.rs (queue row 11), with snapshot_ref.rs's own deferred row
---

## Failure sequence

`Fixture`'s `seed`, `head` and `side` are `String` and its fields are `pub(crate)`. The tree has a
validated `ObjectId`, which `snapshot_ref.rs`'s sweep introduced for exactly this mix-up, so a ref
name, a short id and a full object id are one type here. A suite can hand the manager a value the
production types would refuse and measure a refusal rather than the behaviour it meant to test.

Every value these fields hold today comes from `rev-parse` and is a full id, so no caller is wrong
now.

## What the change that takes this up should do

Give the three fields `ObjectId` in the same change that consolidates it beside its predicates and
gives the ref primitives and the engine's `CommitSha` the same type — `snapshot_ref.rs`'s own
deferred row proposes that, and these three fields belong in it rather than in a change of their own.
