# `src/validate/graph.rs`

Extended notes for [`src/validate/graph.rs`](../../../src/validate/graph.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The plan's dependency graph, as `analyze` checks it: duplicate ids, unknown
`depends` targets, cycles, and the artifact wiring that only warns.

Split out of `super` unchanged — every function here is the one that stood
beside `analyze_captured`, moved rather than rewritten, and `check_graph` is
still the single entry point it calls.

**The denial is restored rather than inherited.** `super` carries
`#![allow(clippy::disallowed_methods)]` because `write_normalized_json`
writes the normalized plan with `fs::write`, and a module-level allow reaches
every child of the module it sits in. Nothing here writes anything — these
are pure functions over a parsed [`Plan`] — so that allowance has no business
extending here, and this line is what stops it. That is also what keeps this
file out of `effects/allowlist.toml`: an allowance is what that file records,
and this module takes none.

## `pub(super) fn check_graph(plan: &Plan, warnings: &mut Vec<String>) -> Result<(), UpstrokeError> {`

Duplicate ids, unknown `depends` targets, then cycles — all collected so a
broken plan reports everything in one run. On a clean graph, artifact
wiring that contradicts the dependency order is surfaced as warnings.

## `pub(super) fn check_graph(plan: &Plan, warnings: &mut Vec<String>) -> Result<(), UpstrokeError> {` › `if problems.is_empty() {`

Cycle detection only makes sense on a graph whose edges all resolve.

## `fn check_artifact_wiring(plan: &Plan, warnings: &mut Vec<String>) {`

A task that `needs` an artifact should depend — directly or transitively —
on its producer, or execution order cannot guarantee the artifact exists.
The plan is frozen (§5), so this warns rather than inventing edges.

## `fn check_artifact_wiring(plan: &Plan, warnings: &mut Vec<String>) {` › `let Some(producer) = producer else { continue };`

Unknown producers already warned during parsing.

## `fn index_by_id(plan: &Plan) -> BTreeMap<&str, &Task> {`

Id → task, built once per pass and shared by the graph checks.
