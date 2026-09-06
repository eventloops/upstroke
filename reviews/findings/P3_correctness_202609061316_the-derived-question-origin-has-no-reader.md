---
id: SWEEP-CHECKEND-004
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/check_end.rs:197
provenance: pre_existing
first_bad:
guard: the sweep of src/topology/fold/apply.rs (queue row 29) and src/topology/fold.rs (queue row 40), which own the delta variant and its only consumer
---

## Failure sequence

`check_question_answered` returns the answered question's `QuestionOrigin`, src/topology/fold/start.rs
carries it into `Derived::Answer(origin)`, and the only consumer matches
`Derived::Answer(QuestionOrigin::VerificationPark | QuestionOrigin::Admission)` in one arm and
calls `apply_answer` -> the two variants are indistinguishable at the only place the value is
read, and `Derived` is private with no accessor, so nothing outside that match can observe it
-> a `check_question_answered` that returned the wrong origin for every answer would pass every
test in the tree, because no assertion can reach the value. The `Derived::Answer` variant itself
is load-bearing and must stay: it is the proof that the checker ran, and a delta carrying
`Derived::None` for a `question_answered` applies nothing. It is the payload that has no reader.

## What the change that takes this up should do

Either give the origin an effect and a test -- a verification park and an admission park differ
in what an answer resumes, and if that difference belongs in the fold it belongs in
`apply_answer`'s two arms -- or drop the payload, making the variant a unit
`Derived::Answer` and `check_question_answered` return `Result<(), FoldError>`. Dropping it
touches `Derived` in src/topology/fold.rs, the match in src/topology/fold/apply.rs and the one
call site in src/topology/fold/start.rs, and it cascades: `Ok(open.origin)` is the **only**
reader of `OpenQuestion::origin` in the crate (`grep -rn '\.origin\b' src/topology/` at this sha
finds this line and three `entry.origin` uses of the unrelated registry field), so the field
would then be written by `open_question` and never read, which is a `dead_code` failure under
`-D warnings` and takes `OpenQuestion` in src/topology/fold.rs with it. Whichever is chosen, say
in docs/internals/topology/fold.md which it is, since the field records a distinction the fold
currently derives and discards.
