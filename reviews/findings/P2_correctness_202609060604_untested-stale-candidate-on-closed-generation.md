---
id: SWEEP-CHECK_CANDIDATE-004
severity: P2
disposition: deferred
category: correctness
pr: 186
reviewed_sha: f301b7b38e60c0df00ba89c0a42fa179bf017c01
location: src/topology/fold/check_candidate.rs:153
provenance: pre_existing
first_bad:
guard: needs a src/topology/fold/tests.rs (row 39) case that closes a generation holding a
  prepared candidate and then applies a task_candidate_created for it
---

## Failure sequence

`check_candidate_created` reads the open generation's prepared candidate through
`match &generation.candidate { Some(prepared) if generation.class == GenerationClass::Promoting =>
prepared, _ => { return Err(NotTheOpenGeneration) } }`. `close_generation`
(`src/topology/fold/apply.rs:548`) sets `generation.class = GenerationClass::Closed` without
clearing `generation.candidate`, so a generation that prepared a candidate and was then closed for
an unrelated reason (decline, rejection, budget/halt closure) carries `class = Closed` and
`candidate = Some(..)` simultaneously. The `if generation.class == GenerationClass::Promoting`
guard is exactly what tells these two states apart. That guard is deleted or weakened (mutated to
`Some(prepared) => prepared` in the pull request body's M13) -> a `task_candidate_created` event
naming that same key and generation is validated against the stale prepared candidate instead of
being refused -> the checker accepts a promotion for a generation the fold itself already closed.
M13 survives the whole crate's test suite (2,134 passed, 0 failed) with the guard removed.

## What the change that takes this up should do

No live defect: the guard is present and, by inspection, is the only thing distinguishing a live
`Promoting` generation from a closed one that still carries a stale candidate, so it must stay.
Row 39 (`src/topology/fold/tests.rs`) needs a case that: dispatches a generation, applies a
`candidate_prepared` for it (setting `candidate: Some(..)`, `class: Promoting`), closes it by a
path that does not clear the candidate (e.g. the decline/rejection path that reaches
`close_generation`), and then applies (or attempts) a `task_candidate_created` naming that
generation, asserting `FoldError::NotTheOpenGeneration`. Witnessed by re-running
`cargo test --all-targets --all-features` under M13 from the pull request body before and after the
new test exists.
