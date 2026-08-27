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
//! That measurement is the argument for the shape this module is being built
//! into: a rule it could get wrong at runtime is preferably a rule the type
//! checker refuses to compile. Three such rules were named here. **One of them
//! is now written**: P0–P8 is a typestate, in `create`'s private `steps`
//! module — eleven witnesses from `PublicDirCreated` to `Started` beside the
//! `Facts` every prefix carries, each
//! constructor taking its predecessor by value, no field nameable outside the
//! module and no `Clone`, `Copy` or `Default` on any of them, so a step cannot
//! run out of order, run twice, or run on a witness whose step failed.
//!
//! The other two are still **planned and not yet written**: a recovery order
//! that is a chain of witnesses each consuming the last — `RecoveryStep` is a
//! `Vec` pushed at runtime, which is a record of the order rather than an
//! enforcement of it — and a creator deletion boundary minted only from the
//! outcome of the step that could have crossed it, where
//! [`crate::rundir::CommitRecordPresence`] is derived by a `stat` after the
//! fact and says so in its own doc.
//!
//! The two are stated in the future tense deliberately, and the first in the
//! past. A module comment that describes an intended shape as an existing one
//! is a false claim about the code, and this project treats that as a defect
//! rather than as optimism — but the converse is a defect too, and this
//! paragraph was one for as long as it said "none of them exists here today"
//! about a typestate the module below had already implemented.
//!
//! # Nothing here is a production path yet
//!
//! `decisions.pr_sequence[8].production_effect` is "none (TopologyPreview
//! selector only)". `upstroke run` still writes schema 3 and still drives
//! [`super::coordinator`]; a schema-4 run is reachable only from a
//! `#[cfg(test)]` writer selector. PR12 activates it.

// **`dead_code` is allowed here for a lib build only, and the shape is the
// point.** With `engine::topology` narrowed to `pub(crate)`, this subsystem has
// no non-`#[cfg(test)]` caller — which is exactly what
// `production_effect = "none"` asserts, and `pub` was what kept the compiler
// from saying so. Narrowing it made rustc report **328 items** across this
// module tree as never used.
//
// `cfg_attr(not(test), …)` rather than a bare allow, deliberately. A blanket
// `#![allow(dead_code)]` would hide a genuinely dead item added later, which is
// the class this slice's own review rounds kept finding. Under this form the
// **test** build carries no allow, so anything not reached even by a test is
// still an error at `-D warnings`. What is silenced is precisely the one true
// fact — the production binary does not drive schema 4 yet.
//
// **Remove this when PR12 activates the driver.** At that point the items have
// production callers and the allow stops being true rather than stops being
// convenient.
#![cfg_attr(not(test), allow(dead_code))]

pub mod attempt;
pub mod candidate;
pub mod create;
pub mod dispatch;
pub mod emit;
pub mod identity;
pub mod prelock;
pub mod seams;
pub mod select;
pub mod settle;
pub(crate) mod startup;

/// The schema-4 attempt-plan assembler, which lives engine-side.
///
/// Re-exported here rather than from the `engine` facade, whose contents are
/// enumerated by the packet and asserted by
/// `the_engine_facade_exposes_exactly_the_items_the_packet_enumerates`. This is
/// schema-4 vocabulary, and this module is where the schema-4 vocabulary is.
///
/// It is engine-side rather than here for the reason [`attempt::AttemptPlans`]
/// exists at all: building a plan materializes the permissions file that
/// defines the attempt's sandbox, and a topology module may not be allowlisted
/// for that write.
// **The schema-4 facade's re-exports, reduced to the ones the crate uses.**
//
// This file carried ~45 lines of `pub use` flattening every submodule's types
// into `engine::topology::*`. Narrowing `engine::topology` to `pub(crate)` made
// rustc report almost all of them **unused**: nothing in the crate used the
// shortened path — callers name `topology::attempt::AttemptPlan` and so on
// directly — so they existed to populate a public path that should not have
// existed. `pub` was hiding dead code, which is the second thing the frontier
// review's finding 1 bought.
//
// What remains below is exactly what a compile with `-D warnings` says is
// reached, derived by deleting the block and re-adding only the names that
// failed to resolve. Deleted rather than `#[allow]`-ed: an allow hides the same
// dead code one level in.
//
// Seven names, all reached from test modules through the flattened path.
pub mod preflight;
pub mod recover;
pub mod run;

// **No re-exports at all, and the last attempt is why.** Gating the seven
// test-reached names with `#[cfg(test)]` put a `#[cfg(test)]` in front of a
// `pub use` rather than a `mod`, which truncates this file's
// `effects::production_region` at that line — and everything below a cut is
// invisible to every census consulting that region, silently.
// `effects::tests::every_production_region_that_stops_early_stops_at_a_module`
// pins the ten files whose cut lands on something that is not a module, its doc
// says "this file is not one of them and must not become one", and it caught
// this within one `cargo test`. The seven call sites name their submodule
// directly instead.
/// The schema-4 run that `dispatch.rs` and `attempt.rs` are tested against.
///
/// One fixture for both, because the two halves of one lifecycle are tested
/// against one run: an attempt test needs a dispatched generation and a
/// dispatch test needs the attempt that never started. Declared here rather
/// than under either module for the same reason.
///
/// **Private, and declared last.** Private because a module's own descendants
/// can see it — `dispatch::tests` and `attempt::tests` both are — so a `pub`
/// qualifier would widen it for nothing and would stop the two source censuses
/// that recognise a whole-file test module by the literal `#[cfg(test)] mod `
/// from recognising this one. Last because `effects::production_region` cuts a
/// file at its first `#[cfg(test)]`, and a declaration higher up would take the
/// re-exports above it out of every census that reads that region.
#[cfg(test)]
mod scaffold;
