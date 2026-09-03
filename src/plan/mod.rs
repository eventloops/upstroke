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

/// The plan corpus, inlined.
///
/// Until 2026-09-02 these four plans were files in a `fixtures/` directory at
/// the repository root: read from disk by three separate test regions —
/// `plan::markdown`, `crate::validate` and `crate::topology::registry` — and
/// shipped as four standalone paths inside the published crate. They are
/// constants here, and every one of those consumers now reads this one source.
/// Nothing enforces that. A new test module can declare its own literal and
/// compile, and the two can then drift with nothing failing; this is where the
/// corpus lives, not a guarantee that it is the only copy.
///
/// **Do not reflow, re-indent, or tidy them.** Each constant below is the file
/// it replaced byte for byte, LF endings and final newline included, and what
/// pins it there is the external SHA-256 comparison recorded in the pull
/// request that inlined them. The suite computes no such hash, so it is not a
/// second opinion on the bytes.
///
/// The parser is what the bytes matter to. The annotation grammar is column-
/// and delimiter-sensitive, and dropping the final LF changes
/// `Plan.source.hash`. That hash is **not** a byte oracle, though:
/// [`crate::ir::content_hash`] skips every CR deliberately, so a CRLF checkout
/// of a plan hashes the same as its LF original — which is what
/// `markdown::tests::crlf_plans_parse_identically` asserts. Read a changed
/// `source.hash` as evidence that something moved, never a stable one as
/// evidence that nothing did.
///
/// `crate::validate`'s tests need them as files, because `validate::run` reads
/// its plan from a path; each such test writes the one plan it needs into its
/// own scratch root, under the name the plan carried.
#[cfg(test)]
pub(crate) mod corpus {
    /// No annotations at all, so every field comes from the heuristics: five
    /// tasks inferred from `##` headings, with one acceptance list.
    pub(crate) const BARE_PLAN: &str = r"# Search improvements

## Design the search index schema

Sketch the fields, analyzers, and ranking signals.

Acceptance:
- Field list agreed
- Ranking signals documented

## Implement the batch indexer

Build the batch indexer over the document store.

## Fix stale-cache invalidation

## Test the reindex path

## Update search docs
";

    /// Deliberately cyclic — `a -> c -> b -> a`. It is a refusal fixture and
    /// the cycle is the point; do not repair it.
    pub(crate) const CYCLIC_PLAN: &str = r"# Cyclic plan (must fail validation)

## Task A
<!-- upstroke: id=a depends=c -->

## Task B
<!-- upstroke: id=b depends=a -->

## Task C
<!-- upstroke: id=c depends=b -->
";

    /// The annotated plan: every annotation attribute the grammar carries, a
    /// `min=` clip, path hints, and an artifact wired along the dependency
    /// chain. Four tasks, no cycles.
    pub(crate) const SAMPLE_PLAN: &str = r"# Pagination rework

## Design the pagination API
<!-- upstroke: id=api-design kind=design depends= tier=frontier out=api-contract -->
Define cursor format, page-size limits, and error contract.

Acceptance:
- Cursor format documented
- Error contract covers empty pages

## Implement cursor encoding
<!-- upstroke: id=cursors kind=implement depends=api-design needs=api-contract paths=src/api/** -->
Implement opaque cursor encode/decode per the contract.

## Fix off-by-one in list endpoint
<!-- upstroke: id=fix-obo kind=fix depends=cursors min=mid paths=src/api/** -->

## Update API docs
<!-- upstroke: id=docs kind=docs depends=fix-obo -->
";

    /// The Claude Code plan-mode shape: an ordered list, no per-task headings,
    /// no annotations. Its third line carries a U+2014 em dash, which is one of
    /// the bytes this corpus exists to keep exact.
    pub(crate) const STEPS_PLAN: &str = r"# Add rate limiting to the API

Claude Code plan-mode shape: numbered implementation steps, no headings per
task, no annotations — everything comes from heuristics.

1. Design the limiter interface and storage schema
2. Implement the token-bucket middleware
   - keep counters in `src/limit/bucket.rs`
3. Fix the flaky retry test that hits the limiter
4. Document the rate-limit headers
";
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
