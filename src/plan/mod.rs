//! Extended notes: `docs/internals/plan/mod.md`

pub mod markdown;

use crate::error::UpstrokeError;
use crate::ir::Plan;

#[derive(Debug)]
pub struct Parsed {
    pub plan: Plan,
    pub warnings: Vec<String>,
}

pub trait PlanAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn sniff(&self, raw: &str) -> bool;
    fn parse_with_warnings(&self, raw: &str) -> Result<Parsed, UpstrokeError>;

    fn parse(&self, raw: &str) -> Result<Plan, UpstrokeError> {
        self.parse_with_warnings(raw).map(|p| p.plan)
    }
}

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
pub(crate) mod corpus {
    pub(crate) const BARE_PLAN: &str = include_str!("../../fixtures/bare-plan.md");

    pub(crate) const SAMPLE_PLAN: &str = include_str!("../../fixtures/sample-plan.md");

    pub(crate) const STEPS_PLAN: &str = include_str!("../../fixtures/steps-plan.md");
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
    fn detect_recognizes_lf_crlf_and_lone_cr_plans_alike() {
        for (endings, newline) in [("LF", "\n"), ("CRLF", "\r\n"), ("CR", "\r")] {
            for body in [
                [
                    "Preamble",
                    "## Fix bug",
                    "<!-- upstroke: min=frontier -->",
                    "",
                ],
                ["Preamble", "- [ ] fix bug", "", ""],
                ["Preamble", "1. fix bug", "", ""],
            ] {
                let raw = body.join(newline);
                let adapter =
                    detect(&raw).unwrap_or_else(|error| panic!("{endings} {raw:?}: {error}"));
                assert_eq!(adapter.id(), "markdown", "{endings} {raw:?}");
                let plan = adapter
                    .parse(&raw)
                    .unwrap_or_else(|error| panic!("{endings} {raw:?}: {error}"));
                assert_eq!(plan.tasks.len(), 1, "{endings} {raw:?}");
            }
            let json = ["{\"tasks\": []}", ""].join(newline);
            assert!(
                detect(&json).is_err(),
                "{endings}: normalization must not widen what detection accepts"
            );
        }
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
