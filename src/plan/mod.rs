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

/// The plan corpus, embedded at compile time from `fixtures/`.
///
/// The four plan files under `fixtures/` at the repository root are the single
/// source of this corpus. Each constant below is [`include_str!`] of one of
/// them: the compiler reads the file, the text is part of the test binary, and
/// nothing reads the file at run time — `plan::markdown`'s and
/// `crate::topology::registry`'s tests take the text from here. The one region
/// that still reads the files from disk at run time is `crate::validate`'s
/// tests, which never stopped: `validate::run` takes a path, and those tests
/// hand it `fixtures/<name>.md` as they always have. Only the plans a
/// compile-time consumer uses are embedded; `cyclic-plan.md` has none — its
/// one reader is `validate`'s refusal test, at run time — so it is a file and
/// nothing else.
///
/// One copy, not two. A literal here would put the plan text in the file and in
/// the source, and a corpus kept in two places drifts — the class this
/// repository has recorded three times. Edit the file; the constant follows.
///
/// The parser is what the bytes matter to: the annotation grammar is column-
/// and delimiter-sensitive, and `steps-plan.md` carries a U+2014 em dash it
/// sees. `Plan.source.hash` is not a byte oracle for them —
/// [`crate::ir::content_hash`] skips every CR deliberately, so a CRLF checkout
/// hashes the same as the LF original, which
/// `markdown::tests::crlf_plans_parse_identically` asserts.
#[cfg(test)]
pub(crate) mod corpus {
    /// No annotations at all, so every field comes from the heuristics: five
    /// tasks inferred from `##` headings, with one acceptance list.
    pub(crate) const BARE_PLAN: &str = include_str!("../../fixtures/bare-plan.md");

    /// The annotated plan: every annotation attribute the grammar carries, a
    /// `min=` clip, path hints, and an artifact wired along the dependency
    /// chain. Four tasks, no cycles.
    pub(crate) const SAMPLE_PLAN: &str = include_str!("../../fixtures/sample-plan.md");

    /// The Claude Code plan-mode shape: an ordered list, no per-task headings,
    /// no annotations. Its third line carries a U+2014 em dash.
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
