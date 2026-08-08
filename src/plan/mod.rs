//! Plan ingestion (DESIGN.md §9): adapters turn raw plan text into the IR.

pub mod markdown;

use crate::error::TactusError;
use crate::ir::Plan;

/// A parsed plan plus non-fatal findings (unknown annotation attributes,
/// heuristic fallbacks). Warnings never block validation.
#[derive(Debug)]
pub struct Parsed {
    pub plan: Plan,
    pub warnings: Vec<String>,
}

/// DESIGN.md §8 `PlanAdapter` — one implementation per plan format. `sniff`
/// supports auto-detection once more than one adapter exists.
pub trait PlanAdapter {
    fn id(&self) -> &'static str;
    fn sniff(&self, raw: &str) -> bool;
    fn parse(&self, raw: &str) -> Result<Plan, TactusError>;
}
