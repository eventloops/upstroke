---
id: PR7-R4-LOOP-004
severity: P2
disposition: deferred
category: correctness
pr: 7
reviewed_sha: 
location: src/topology/fold.rs
provenance: pre_existing
first_bad: 
guard: the slice that implements closure — PR8/PR10
---

## Failure sequence

`select` can return `Closure(DerivedOutcome::NotEnding)`. `RunState::derived_outcome`
returns `NotEnding` whenever a generation blocks run end, and an `OpenNoAttempt` generation does —
exactly the fold the ending guard was written for. `Step::Closure`'s own doc says "run-end closure
is due, with the outcome the fold derives", so the value contradicts itself as documented, and
`checkpoint` refuses with "closure derives NotEnding" to the operator of a run that is in fact
budget-stopped.

## What the change that takes this up should do

Choose one of the two shapes and say so: either closure closes the open generation first and
re-derives, or `derived_outcome` learns to answer for a run ending with work still open — the
second touches a `src/topology/**` reader. PR7 carried it rather than repairing it because this
build refuses run-end closure outright (`checkpoint_refusals`), so no run acts on the value today,
and because the choice is a design decision that slice had no standing to make.

**What is owed whichever shape wins is the diagnostic.** An operator told "closure derives
NotEnding" about a budget-stopped run is being told the wrong thing, and that is true before either
repair lands.

Recorded in `reviews/FINDINGS.md` §20. Severity is this migration's judgement: nothing acts on the value today, and the operator-facing message is wrong today.
