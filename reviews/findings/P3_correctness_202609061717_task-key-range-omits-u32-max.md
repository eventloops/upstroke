---
id: REVIEW212-TASK-KEY-RANGE-OMITS-MAX
severity: P3
disposition: deferred
category: correctness
pr: 212
reviewed_sha: 2c6d93d66d6144d39a3b1414f2cc7f9b20910a4c
location: src/topology/fold/outcome.rs:35
provenance: introduced_by_feature
first_bad: 303a023a75185b837a06c1ae057d46e30ce90006
guard: the next change to `src/topology/fold/outcome.rs`
---

## Failure sequence

Location as first recorded: `src/topology/fold/outcome.rs:35`,
`structurally_admissible` (as of the reviewed sha).

Finding 7 of PR #212's frontier pass.

`structurally_admissible` converts `self.tasks.len()` with
`u32::try_from(..).unwrap_or(u32::MAX)` and then iterates `0..keys`. A
half-open range stops one short of its bound, so the last key it examines is
`TaskKey(keys - 1)`.

`TaskRegistry::index_key` (`src/topology/registry.rs:463`) derives a key from
a dense index with `u32::try_from(index)`, so `TaskKey(u32::MAX)` is a valid
key: it is the key of the task at index `u32::MAX`, in a registry of
`u32::MAX + 1` tasks. At that length the conversion saturates to `u32::MAX`,
the range is `0..u32::MAX`, and `TaskKey(u32::MAX)` is never examined. If it
is the only ready or retry-ready task and no queued candidate is eligible,
`structurally_admissible` answers false, the run derives an ending outcome
instead of `NotEnding`, and a `run_finished` recording that outcome is
accepted while a dispatchable task remains.

The cardinality is extreme and no plan the ingestion accepts reaches it. The
boundary is still wrong, and it is wrong in the direction that ends a run
with work outstanding rather than the direction that refuses one.

## What the change that takes this up should do

Derive the bound so that no valid key is excluded -- iterate the registry's
own keys, or convert with `saturating_add(1)` on an inclusive range, or
refuse a registry this fold cannot enumerate rather than silently truncating
it. Whichever shape is taken, the test has to distinguish it from today's:
an assertion over a small fixture registry passes under both, so the witness
has to be the boundary itself, at a stubbed length, or the conversion has to
stop being a place where a boundary can hide.
