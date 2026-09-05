# `src/plan/mod.rs`

Extended notes for [`src/plan/mod.rs`](../../../src/plan/mod.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Plan ingestion (DESIGN.md §9): adapters turn raw plan text into the IR.

Adapters live in a sniff-ordered registry; `detect` picks the first one
that recognizes the input. v0.1 ships markdown only (Claude Code plan-mode
shapes plus the annotation grammar); v0.2 appends generic checklist, JSON
schema, and claude-task-master import.

## `pub struct Parsed {`

A parsed plan plus non-fatal findings (unknown annotation attributes,
heuristic fallbacks). Warnings never block validation.

## `pub trait PlanAdapter: Send + Sync {`

DESIGN.md §8 `PlanAdapter` — one implementation per plan format.

## `pub trait PlanAdapter: Send + Sync` › `fn parse(&self, raw: &str) -> Result<Plan, UpstrokeError> {`

The §8 signature; the warning-carrying form above is what `validate`
consumes.

## `pub static ADAPTERS: &[&dyn PlanAdapter] = &[&markdown::MarkdownPlanAdapter];`

Registry in sniff order; first match wins.

## `pub(crate) mod corpus {`

The plan corpus, embedded at compile time from `fixtures/`.

The four plan files under `fixtures/` at the repository root are the single
source of this corpus. Each constant below is [`include_str!`] of one of
them: the compiler reads the file, the text is part of the test binary, and
nothing reads the file at run time — `plan::markdown`'s and
`crate::topology::registry`'s tests take the text from here. The one region
that still reads the files from disk at run time is `crate::validate`'s
tests, which never stopped: `validate::run` takes a path, and those tests
hand it `fixtures/<name>.md` as they always have. Only the plans a
compile-time consumer uses are embedded; `cyclic-plan.md` has none — its
one reader is `validate`'s refusal test, at run time — so it is a file and
nothing else.

One copy, not two. A literal here would put the plan text in the file and in
the source, and a corpus kept in two places drifts — the class this
repository has recorded three times. Edit the file; the constant follows.

The parser is what the bytes matter to: the annotation grammar is column-
and delimiter-sensitive, and `steps-plan.md` carries a U+2014 em dash it
sees. `Plan.source.hash` is not a byte oracle for them —
[`crate::ir::content_hash`] skips every CR deliberately, so a CRLF checkout
hashes the same as the LF original, which
`markdown::tests::crlf_plans_parse_identically` asserts.

## `pub(crate) mod corpus` › `pub(crate) const BARE_PLAN: &str = include_str!("../../fixtures/bare-plan.md");`

No annotations at all, so every field comes from the heuristics: five
tasks inferred from `##` headings, with one acceptance list.

## `pub(crate) mod corpus` › `pub(crate) const SAMPLE_PLAN: &str = include_str!("../../fixtures/sample-plan.md");`

The annotated plan: every annotation attribute the grammar carries, a
`min=` clip, path hints, and an artifact wired along the dependency
chain. Four tasks, no cycles.

## `pub(crate) mod corpus` › `pub(crate) const STEPS_PLAN: &str = include_str!("../../fixtures/steps-plan.md");`

The Claude Code plan-mode shape: an ordered list, no per-task headings,
no annotations. Its third line carries a U+2014 em dash.
