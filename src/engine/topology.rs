//! `TopologyRun` — the schema-4 run lifecycle.
//!
//! `decisions.sequential_substrate.engine`: "`src/engine/topology.rs`
//! TopologyRun drives schema 4 at max_parallel = 1 synchronously; every path
//! exists here before Tokio."
//!
//! This module is the **conductor** of a schema-4 run. Almost nothing here is
//! new machinery: the event funnel and its stable-prefix barrier, the git
//! funnels of [`crate::workspace_manager`], the container census, and the
//! private-half ownership proof were all built and tested by PR4–PR6 and are
//! waiting for their first production caller. What arrives here is the
//! *ordering* over them — P0–P8, the recovery order, the candidate sequence —
//! and orderings are where this project's defects actually live: of 29
//! classified findings across PR3–PR6, `wrong_internal_assumption` is 48.3%,
//! three times `wrong_external_fact`.
//!
//! That measurement is why the shape here is what it is. A rule this module
//! could get wrong at runtime is preferably a rule the type checker refuses to
//! compile: the recovery order is a chain of witnesses each consuming the last,
//! the creator's one deletion boundary is a token minted from the outcome of
//! the step that could have crossed it, and P0–P8 is a typestate. That is the
//! same device [`crate::rundir::PrivateHalfProof`],
//! [`crate::runner::container::intent::IntentWritten`] and
//! [`crate::topology::fold::TopologyDelta`] already use, for the same reason.
//!
//! # Nothing here is a production path yet
//!
//! `decisions.pr_sequence[8].production_effect` is "none (TopologyPreview
//! selector only)". `upstroke run` still writes schema 3 and still drives
//! [`super::coordinator`]; a schema-4 run is reachable only from a
//! `#[cfg(test)]` writer selector. PR12 activates it.

pub mod seams;

pub use seams::{
    HarnessTopologyHooks, IdSource, NoTopologyHooks, RealIds, SystemTime, TimeSource,
    TopologyHooks, is_within,
};
