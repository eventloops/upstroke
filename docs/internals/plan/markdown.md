# `src/plan/markdown.rs`

Extended notes for [`src/plan/markdown.rs`](../../../src/plan/markdown.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Markdown plan adapter (DESIGN.md §9).

`##`/`###` sections become tasks (heading → title, prose → body). A plan
with no such sections falls back to top-level checklist items or numbered
plan-mode steps. The `<!-- upstroke: ... -->` annotation grammar overrides
the heuristics;
annotations are read from pulldown-cmark HTML events, never regexed out of
raw text. Unknown annotation attributes warn and never error.

### The concerns are private children; this module is their facade

`sections` finds the heading boundaries, `annotation` extracts and parses
the upstroke comments, `hints` harvests path mentions, `drafts` turns
either intake shape into one draft per task-to-be, and `assemble` resolves
ids, kinds, dependencies and artifacts into the IR. The dependency
direction is one-way and acyclic: `annotation`, `hints` and `sections` feed
`drafts`; `drafts` and `hints` feed `assemble`; every child feeds this
module and none of them is reachable from outside it.

The one thing that flows the other way is `md_options`, the single
`pulldown_cmark::Options` value every child parses with — it stays here so
that the three parse sites cannot drift apart.

## `fn is_ordered_item(line: &str) -> bool {`

`1. step` / `1) step` — Claude Code plan mode often writes numbered steps.

## `fn bare_fixture_uses_heuristics()` › `assert!(tasks[0].depends_on.is_empty());`

Document-order dependencies: task N depends on task N-1.

## `fn bare_fixture_uses_heuristics()` › `assert_eq!(parsed.plan.artifacts.len(), 1);`

Bare plan with a Design task gets the default conventions brief.

## `fn malformed_attribute_and_bad_values_warn()` › `assert_eq!(parsed.plan.tasks[0].suggested_tier, None);`

Bad values fall back rather than erroring.

## `fn acceptance_heading_forms_arm_without_becoming_tasks()` › `let parsed = parse("## Task\n\n#### Done when\n- it works\n");`

Deeper heading form inside a section arms too.
