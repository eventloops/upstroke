---
id: SWEEP-CHECK_CANDIDATE-002
severity: P3
disposition: deferred
category: correctness
pr: 186
reviewed_sha: f301b7b38e60c0df00ba89c0a42fa179bf017c01
location: src/topology/fold/check_candidate.rs:26
provenance: pre_existing
first_bad:
guard: needs a fixture in src/topology/fold/tests.rs (row 39) that constructs a generation off the
  current apply invariant, or a decision that the branch is intentional defense-in-depth and needs
  none
---

## Failure sequence

`check_candidate_prepared` refuses when `generation.candidate.is_some()`, guarding INV-06 ("at most
one candidate per generation") for a generation still `InFlight`. `generation.candidate` is written
in exactly one place, `apply_candidate_prepared` (`src/topology/fold/apply.rs:197`), which always
sets it `Some` and `generation.class = GenerationClass::Promoting` in the same statement block; a
freshly-dispatched generation starts with `candidate: None`. So this branch's precondition —
`candidate.is_some()` while `class` is still `InFlight` — cannot occur through the fold's own apply
path: the preceding `InFlight` guard (line 14) already refuses every state this branch would also
refuse. This mutation (M2 in the pull request body: replace the condition with `false`) survives
the file's entire owning test module and the whole crate suite with zero failures, confirming the
branch is exercised by nothing today, live or unreachable.

## What the change that takes this up should do

No live defect: the branch's own refusal message is exactly the message the preceding guard would
already give, so removing it changes no observable behavior for any state the fold can currently
construct. Two ways to close this, for row 39 (`src/topology/fold/tests.rs`) or a future reviewer
to choose between: (a) build a hostile fixture that sets `candidate: Some(..)` on an `InFlight`
generation directly (bypassing `apply`, as `src/topology/fold/tests.rs:6723`'s `open` helper already
does for other class/candidate combinations) and assert this branch's specific message, proving it
independently of the `InFlight` guard; or (b) remove the branch as dead code and update
`docs/internals/topology/fold/check_candidate.md`'s INV-06 section to say the invariant is enforced
solely by the `InFlight` guard together with `apply_candidate_prepared`'s atomic write. Left as
defense-in-depth rather than removed here: deleting an invariant guard is a design call this file's
own sweep should not make unreviewed.
