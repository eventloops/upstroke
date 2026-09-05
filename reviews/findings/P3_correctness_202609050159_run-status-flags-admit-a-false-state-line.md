---
id: SWEEP-RENDER-013
severity: P3
disposition: deferred
category: correctness
pr: 166
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/status.rs:41
provenance: pre_existing
first_bad:
guard: row 57 of `standards/SWEEP.md` (`src/status.rs`), whose `RunStatus` owns the two flags; `render`'s doc in `src/status/render.rs` states the four readings it relies on
---

## Failure sequence

`RunStatus` exposes `running` and `held` as `pub bool` fields (`src/status.rs:41`, `:50`)
while `load` derives `running = held && finished.is_none()` -> a `RunStatus` constructed with
`running: true, held: false` is a value the type admits -> `render` prints `state: running now
(another process holds this run)` while nothing holds it, and `report()` renders the tasks as
live; with `running: false, held: false` and `finished: None` the same value reads as
interrupted, so the two derived flags carry an invariant (`running` implies `held`) that only
one constructor keeps. No production path constructs the type outside `load` today — the
sequence fires only from a caller that builds the struct by hand (the fields are `pub`, and
`engine::tests::replay_of` goes through `load`), which is why this is P3 and a §5 finding
("fields stay private where construction or mutation must preserve an invariant") rather than a
defect an operator can reach.

## What the change that takes this up should do

Replace the pair with one derived reading — an enum such as `Liveness { Running, Interrupted,
Claimed, Ended }` computed once in `load` from `held` and `state.finished` — or keep the two
facts and make `running` a method over them, so `render`'s four-way reading and `report()`'s
`running` argument are one derivation. `render` in `src/status/render.rs` and `RunStatus::report`
are the two readers to move. Out of the render sweep's reach because the type is the parent's
(row 57).
