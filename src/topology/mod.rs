//! The v0.2 execution topology (DESIGN.md; `decisions/2026-08-12-merge-queue-execution-topology.md`).
//!
//! Schemas 1–3 are legacy sequential runs and stay exactly as they are; the
//! topology is what a schema-4 run is made of. It arrives in slices, and only
//! the task registry exists so far: the storage identity every later piece —
//! the checked fold, the candidate queue, the merge queue, the repair lineage —
//! addresses tasks by.
//!
//! Nothing here is wired into a production path yet. [`registry`] is pure
//! construction over inputs a run already froze, so a run's status, report, and
//! export are byte-for-byte what they were before this module existed.

pub mod registry;
