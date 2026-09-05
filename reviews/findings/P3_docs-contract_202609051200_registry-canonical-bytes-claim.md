---
id: ASTRA165-006
severity: P3
disposition: deferred
category: docs-contract
pr: 165
reviewed_sha: 9699c27396e6eff34be9a86bc634f042080cb280
location: docs/internals/topology/registry.md:396
provenance: pre_existing
first_bad: —
guard: Compare canonical_bytes and digest contracts with their implementations after a dynamic entry is registered
---

## Failure sequence

The `canonical_bytes` note claims to describe the exact bytes hashed by `digest`,
but dynamic registration makes `canonical_bytes` include every entry while `digest`
hashes only the original prefix. A consumer reconstructing the authentication
digest from the stated contract hashes a different entry count and different bytes.
The implementations distinguish those paths at `src/topology/registry.rs:411` and
line 429.

## What the change that takes this up should do

Qualify the note so it describes the relationship between canonical bytes, dynamic
entries, and the digest computation accurately. The reviewed implementations and
the correct distinction already written at `docs/internals/topology/registry.md:387`
establish this finding by inspection.
