---
id: SWEEP-RENDER-011
severity: P3
disposition: deferred
category: docs-contract
pr: 166
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/status/render.rs:165
provenance: pre_existing
first_bad:
guard: the sweep of `src/ladder.rs`, which owns `FailureKind`; the two `{:?}` sites in `src/status/render.rs` (`describe_transition`'s `Fail` arm and the `TaskFailed` arm) are the consumers to repoint
---

## Failure sequence

An attempt fails and the ladder settles it as a terminal failure -> `describe` renders the kind
with `{:?}` (at the reviewed SHA in the atomic `Fail` arm; after PR 166 in the standalone
`task_failed` line as well) -> the operator's `--follow` reads `task failed (GateFailed)`
while the log they can open beside it spells the same fact `"kind":"gate_failed"` (serde
`rename_all = "snake_case"`), and a rename of the variant changes the CLI line silently
because a derived `Debug` is a Rust identifier, not a contract (CODING_STANDARDS.md §13:
process output is product surface). Reproduces on any run whose task fails: the `Fail` case of
`status::tests::describe_atomic_attempt_transitions` pins the `Debug` spelling today.

## What the change that takes this up should do

Give `FailureKind` a `Display` whose words match the serde spelling (one table, or derive the
spelling from the same `rename_all`), and have both `render.rs` sites use it. The
`describe_atomic_attempt_transitions` and
`a_terminal_failure_says_whether_the_run_halts_in_both_wire_shapes` pins in `src/status.rs`
move with the spelling. Out of a render sweep's reach because the type is `ladder.rs`'s and a
local spelling table in the renderer would be a second copy of serde's.
