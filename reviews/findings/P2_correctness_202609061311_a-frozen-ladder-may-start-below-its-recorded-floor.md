---
id: SWEEP-START-001
severity: P2
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/start.rs:164
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/fold/tests.rs` (queue row 38's sibling, row 39), or any change that may edit both that fixture and `check_ladder` in one commit
---

## Failure sequence

`check_ladder` is the fold's boundary check on a `FrozenLadder` — the one place
that decides whether a recorded ladder may enter a fold's state. It compares the
recorded floor with the recorded ceiling:

```rust
if let (Some(floor), Some(ceiling)) = (ladder.floor, ladder.ceiling) {
    if floor > ceiling { /* refused */ }
}
```

It never compares the floor with where the chain actually *starts*. The design
says the floor is a clip on the start, not a bound on the end:
`design/07_design_core_data_model.md` writes the field as
`min_tier: Option<Tier>, // clips the chain start (binding)`;
`design/10_design_routing.md` §2 says "path-glob overrides truncate the chain
start"; and `design/26_design_merge_queue_protocol.md` ¶6 says a repair's "`mid`
minimum tier is a floor inside the already-frozen pin/maximum constraints, never
a reason to override them", with §26's payload section adding "`min_tier = mid`.
That floor is intersected with the original task's recorded hard pin and
maximum."

The router honours that. `src/route.rs:84` calls `raise_start`, which is
`tiers.retain(|(t, _)| *t >= floor)` and pushes the floor itself when the retain
empties the list (`src/route.rs:109-123`), so every tier a router-produced chain
carries is at or above the floor. `src/topology/registry.rs:536-544`
(`frozen_ladder`) then copies `task.min_tier` into `floor` and `chain.tiers`
into `tiers` verbatim, checking neither against the other.

So a ladder whose first tier is below its floor is invisible to the registry,
which is the class of malformation `check_ladder` exists for — its own notes say
so: "the registry derives a ladder from whatever the run recorded, and this
decides whether that ladder may enter a fold's state."

Concretely, a `task_spawned` may register a repair with

    floor:  Some(Mid)
    tiers:  [Small, Mid, Frontier]
    rungs:  one binding per tier
    ceiling: Some(Frontier)

`check_ladder` accepts it: `Mid <= Frontier`, the tiers ascend, the ceiling is
the highest tier, the rungs align. `TaskFold.rung` then starts at 0, and
`check_attempt_started` (`src/topology/fold/check_attempt.rs:437-459`) validates
the attempt's binding against `entry.ladder.rungs[0]` — the `Small` rung. The
repair's first attempt runs one tier below the floor §26 requires, and the
recorded floor was never consulted by anything: `ladder.floor` has no reader in
the topology path other than the ceiling comparison above (grep-verified across
`src/`, production code, at this SHA).

The same shape reaches `run_started`, where the ladders are derived from the
event's own `chains`: a `ChainSummary` whose `tiers` start below the plan's
`min_tier` is accepted, and every attempt on that task runs below the floor.

## What the change that takes this up should do

Compare the floor with the start of the chain, not only with its end — in
`check_ladder`, beside the existing floor/ceiling refusal:

```rust
if let (Some(floor), Some(start)) = (ladder.floor, ladder.tiers.first().copied()) {
    if floor > start { /* refuse */ }
}
```

Two consequences put this past a one-file sweep's bound, both measured on
`sweep/topology-fold-start` at this base:

1. **It subsumes the existing refusal.** The tiers ascend by the check above, so
   `ceiling` is the last tier and `floor > ceiling` implies `floor > tiers[0]`.
   Adding the stronger check first makes the floor-above-ceiling refusal
   unreachable and changes which `defect` string the existing
   `a_malformed_ladder_is_refused_before_it_is_stored` case "floor above
   ceiling" gets — a case whose assertion is that each case's defect string is
   distinct from every other's. Either the two refusals are merged into one
   with one message, or the weaker one is deleted; both are edits to row 39's
   file.

2. **The fold suite's own plan fixture records such a ladder.** In
   `src/topology/fold/tests.rs`, `plan()` gives task `mid`
   `min_tier = Some(Tier::Mid)` while `chain("mid")` gives it
   `tiers = [Small, Frontier]` — a ladder `src/route.rs` cannot produce. Every
   fold test built on `started()` is therefore refused at `run_started`.
   Measured: with the check above applied and nothing else changed,
   `cargo test --all-features topology::` goes from 634 passed / 0 failed to
   522 passed / **112 failed**.

So the fix is a two-file change — `src/topology/fold/start.rs` and
`src/topology/fold/tests.rs` — and row 39 is being swept by a sibling session
concurrently with row 38. It belongs to whichever change may hold both files at
once.

If a later pass reclassifies this as P1, it is fixed there and then rather than
carried: the deferral is a scope judgement about which change may edit row 39's
fixture, not a judgement that running below a recorded floor is acceptable.
