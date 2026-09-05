//! Extended notes: `docs/internals/engine/topology.md`

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

pub mod preflight;
pub mod recover;
pub mod run;

#[cfg(test)]
mod scaffold;
