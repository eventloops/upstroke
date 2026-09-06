---
id: SWEEP-FOLD-APPLY-QUESTION-ORIGIN-UNREAD
severity: P3
disposition: deferred     # `QuestionOrigin` and `OpenQuestion` are declared in src/topology/fold.rs (queue row 40) and the trace that would pin the distinction belongs in row 39's suite; this sweep owns neither
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold.rs:416-426 (the enum and the field); src/topology/fold/apply.rs:47,91,95,163,292 at `ee5dc81f` (every write)
provenance: pre_existing
first_bad:
guard: the sweeps of `src/topology/fold.rs` (queue row 40), which owns `QuestionOrigin`, `OpenQuestion` and `Derived`, and `src/topology/fold/tests.rs` (row 39), which owns the suite that would pin the distinction
---

## Failure sequence

`OpenQuestion::origin: QuestionOrigin` is a public field of a public struct, reachable from outside
the crate through `TopologyFold::open_questions()`. `RunState::open_question` in
`src/topology/fold/apply.rs` is its only writer, at five call sites: `VerificationPark` for a
parked verification outage, and `Admission` for the four other questions this run can ask (a
`HumanRequired` admission, a `HumanBinding` admission, a parked attempt settlement, and a bare
`question_raised`).

Nothing consults it. Across the whole tree, `grep -rn '\.origin' src/ --include=*.rs` finds exactly
one read of this field, `src/topology/fold/check_end.rs:197`'s `Ok(open.origin)`, which hands it to
`Derived::Answer`; every other hit names a different `origin` — a failure record's, a task
entry's, `originals`, or an export field. The seven files that call
`TopologyFold::open_questions()` outside the fold
(`src/engine/topology/run.rs`, `src/engine/topology/select.rs`, `src/engine/coordinator.rs`,
`src/engine/topology/settle/tests.rs`, `src/status/render.rs`, `src/topology/census.rs` and
`src/events/mod.rs`) take the map's keys or `open.question`. From `check_end.rs` the origin goes
into `Derived::Answer(origin)`, and `RunState::apply`'s `question_answered` arm then matches
`Derived::Answer(QuestionOrigin::VerificationPark | QuestionOrigin::Admission)` — both variants to
the same body. No test asserts an origin anywhere either: the fold's whole suite names
`QuestionOrigin` exactly once, at `src/topology/fold/tests.rs:7003`, and that is a grid fixture
*constructing* a question with `QuestionOrigin::Admission`, not an assertion about one.

Measured at `ee5dc81f` against the whole `topology::fold` suite (131 tests): recording a bare
`question_raised` as `QuestionOrigin::VerificationPark` instead of `Admission` — a one-token
mutation of `RunState::apply` — **survives**. A fold that mislabels every question it opens is
indistinguishable, to this repository, from one that labels them correctly.

The value the field still has is compile-time: the or-pattern in `apply`'s answer arm is exhaustive
over `QuestionOrigin`, so a third variant would fail to compile there rather than be silently
absorbed, which is what §5 asks of a closed domain. That is a real property, and it is the whole of
what the type buys today.

This is not a claim that any behaviour is wrong. The fold's decisions about a question are made
from the question's key and the current state, which is what PR #152 settled on after the
origin-driven state restoration was withdrawn (`SWEEP-FOLD-APPLY-ORIGIN-SUPERSEDED`,
`PR152-ORIGIN-RECORD-STALE`). The residue is a public field carrying a fact with no reader and no
test.

## What the change that takes this up should do

Decide, in the sweep of `src/topology/fold.rs`, which of these the origin is:

- **A supported observable.** Then it needs a reader that makes it one — status or the notifier
  distinguishing "a person was asked because the merge queue stalled" from "a person was asked
  because the task could not be admitted" — and a test in `src/topology/fold/tests.rs` that opens
  one question of each origin and asserts what `open_questions()` reports, which kills the mutation
  above.
- **Internal bookkeeping.** Then it is not `pub`, and `Derived::Answer`'s payload goes with it —
  but only after replacing the exhaustiveness the or-pattern in `apply` currently provides, because
  deleting the payload deletes the one thing that forces a decision at that site when the enum
  grows.
