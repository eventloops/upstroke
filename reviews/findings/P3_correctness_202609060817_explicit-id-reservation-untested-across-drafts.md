---
id: SWEEP-ASSEMBLE-001
severity: P3
disposition: deferred
category: correctness
pr: 196
reviewed_sha: 2b511e0dccca33c08b6e0dd175a1a4ee7e0f0264
location: src/plan/markdown/assemble.rs:15
provenance: pre_existing
first_bad:
guard: a future change to `src/plan/markdown.rs`'s test suite, or to `assemble`'s id-assignment logic
---

## Failure sequence

`assemble` reserves every draft's explicit `id=` into `taken` before deriving any slug, so a
derived id can never collide with one an author wrote, regardless of which draft comes first in
document order. Mutating that reservation away (`let mut taken: Vec<String> = Vec::new();` in
place of the `annotated.iter().filter_map(|(_, ann)| ann.id.clone()).collect()` prepass) makes
`unique_slug` blind to every explicit id in the plan -> a title that slugifies to the same string
as a later draft's explicit id derives that colliding id first, since nothing in `taken` yet knows
the explicit id exists -> two tasks would carry the same `TaskId`, which `Plan`'s callers (the
dependency graph, artifact wiring) assume is unique -> the crate's full `cargo test --all-targets
--all-features` suite (2,137 tests) passes unchanged with the mutation in place, so nothing in the
tree currently exercises a plan where a derived slug and a later draft's explicit id are the same
string.

## What the change that takes this up should do

Add a regression that builds a plan (through `MarkdownPlanAdapter::parse_with_warnings`, since
`Draft`'s `ann` field is private to `drafts.rs` and cannot be constructed directly from
`assemble.rs`) with an early untitled/heuristic draft whose title slugifies to some `X`, and a
later draft carrying `<!-- upstroke: id=X -->`, and assert both tasks keep distinct ids
(the later, explicit one keeps `X`; the earlier gets `X-2`). That fixture belongs in
`src/plan/markdown.rs`'s existing black-box suite, a separate, currently unswept queue row (66's
sibling family, not row 64) that this sweep's `require_scope` does not allow editing. The
alternative — adding a private, plain-value seam inside `assemble.rs` itself so the reservation
logic can be unit-tested without a full `Draft`/`Annotation` build — was considered and rejected for this pass: the seam's own input construction would need to clone
every draft's title and explicit id an extra time in production to hand them across the seam,
trading a real runtime cost for a test-only benefit that the extraction itself would not exercise
any differently from what `unique_slug`'s own logic already does. Whichever change takes this up
should also settle whether such a private, plain-value extraction is worth that added clone, or
whether the fixture belongs in `markdown.rs` instead.
