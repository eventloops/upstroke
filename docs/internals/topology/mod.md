# `src/topology/mod.rs`

Extended notes for [`src/topology/mod.rs`](../../../src/topology/mod.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The v0.2 execution topology (DESIGN.md; `decisions/2026-08-12-merge-queue-execution-topology.md`).

Schemas 1–3 are legacy sequential runs and stay exactly as they are; the
topology is what a schema-4 run is made of. It arrives in slices, and only
the task registry exists so far: the storage identity every later piece —
the checked fold, the candidate queue, the merge queue, the repair lineage —
addresses tasks by.

Nothing here is wired into a production path yet. [`registry`] is pure
construction over inputs a run already froze, [`schema`] and [`events`]
describe a log no production writer produces, and [`paths`] is the
vocabulary they record regions in — so a run's status, report, and export
are byte-for-byte what they were before this module existed.
