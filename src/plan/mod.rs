//! Plan ingestion (DESIGN.md §9): adapters turn raw plan text into the IR.
//!
//! Adapters live in a sniff-ordered registry; `detect` picks the first one
//! that recognizes the input. v0.1 ships markdown only (Claude Code plan-mode
//! shapes plus the annotation grammar); v0.2 appends generic checklist, JSON
//! schema, and claude-task-master import.

pub mod markdown;

use crate::error::UpstrokeError;
use crate::ir::Plan;

/// A parsed plan plus non-fatal findings (unknown annotation attributes,
/// heuristic fallbacks). Warnings never block validation.
#[derive(Debug)]
pub struct Parsed {
    pub plan: Plan,
    pub warnings: Vec<String>,
}

/// DESIGN.md §8 `PlanAdapter` — one implementation per plan format.
pub trait PlanAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn sniff(&self, raw: &str) -> bool;
    fn parse_with_warnings(&self, raw: &str) -> Result<Parsed, UpstrokeError>;

    /// The §8 signature; the warning-carrying form above is what `validate`
    /// consumes.
    fn parse(&self, raw: &str) -> Result<Plan, UpstrokeError> {
        self.parse_with_warnings(raw).map(|p| p.plan)
    }
}

/// Registry in sniff order; first match wins.
pub static ADAPTERS: &[&dyn PlanAdapter] = &[&markdown::MarkdownPlanAdapter];

pub fn detect(raw: &str) -> Result<&'static dyn PlanAdapter, UpstrokeError> {
    ADAPTERS
        .iter()
        .copied()
        .find(|a| a.sniff(raw))
        .ok_or_else(|| UpstrokeError::Parse {
            message: format!(
                "no plan adapter recognizes this file (available: {})",
                ADAPTERS
                    .iter()
                    .map(|a| a.id())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_picks_markdown_for_markdown_shapes() {
        assert_eq!(detect("# Title\n").expect("heading").id(), "markdown");
        assert_eq!(detect("- [ ] item\n").expect("checklist").id(), "markdown");
        assert_eq!(detect("1. first step\n").expect("ordered").id(), "markdown");
    }

    #[test]
    fn detect_rejects_unrecognized_input() {
        let err = detect("{\"tasks\": []}\n")
            .map(|a| a.id())
            .expect_err("json is not markdown");
        let message = err.to_string();
        assert!(
            message.contains("no plan adapter recognizes"),
            "got: {message}"
        );
        assert!(message.contains("markdown"), "lists available adapters");
    }
}
