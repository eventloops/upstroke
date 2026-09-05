# `src/plan/markdown.rs`

Extended notes for [`src/plan/markdown.rs`](../../../src/plan/markdown.rs).

These notes preserve the module comments after the annotation repairs. Item headings quote source lines for navigation.

## Module

Markdown plan adapter (DESIGN.md §9).

`##`/`###` sections become tasks (heading → title, prose → body). A plan
with no such sections falls back to top-level checklist items or numbered
plan-mode steps. The `<!-- upstroke: ... -->` annotation grammar overrides
the heuristics;
annotations are read from pulldown-cmark HTML events, never regexed out of
raw text. Unknown annotation attributes warn and never error.

# The concerns are private children; this module is their facade

`sections` finds the heading boundaries, `annotation` extracts and parses
the upstroke comments, `hints` harvests path mentions, `drafts` turns
either intake shape into one draft per task-to-be, and `assemble` resolves
ids, kinds, dependencies and artifacts into the IR. The dependency
direction is one-way and acyclic: `annotation`, `hints` and `sections` feed
`drafts`; `drafts` and `hints` feed `assemble`; every child feeds this
module and none of them is reachable from outside it.

`md_options` supplies the same parser options to every child. `parser_source`
makes lone-CR line boundaries visible to the parser without moving source
byte offsets. Both stay here so the three walks use the same input rules.

## `fn parser_source(raw: &str) -> Cow<'_, str> {`

pulldown-cmark 0.13.4's HTML-block scanner advances only at LF, although
its paragraph scanner also recognizes lone CR. Give every walk the same
line boundaries. Replacing a lone ASCII CR by LF keeps byte ranges valid
against the original source used for bodies and the input hash. CRLF is
already supported and stays unchanged. The owned copy exists only when
this parser boundary needs normalization.

## `fn is_ordered_item(line: &str) -> bool {`

`1. step` / `1) step` — Claude Code plan mode often writes numbered steps.

## `assert!(tasks[0].depends_on.is_empty());`

Document-order dependencies: task N depends on task N-1.

## `assert_eq!(parsed.plan.artifacts.len(), 1);`

Bare plan with a Design task gets the default conventions brief.

## `assert_eq!(parsed.plan.tasks[0].suggested_tier, None);`

Bad values fall back rather than erroring.

## `let parsed = parse("## Task\n\n#### Done when\n- it works\n");`

Deeper heading form inside a section arms too.
