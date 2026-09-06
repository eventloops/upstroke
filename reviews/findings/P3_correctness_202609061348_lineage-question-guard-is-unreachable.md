---
id: SWEEP-FOLD-OUTCOME-LINEAGE-QUESTION-UNREACHABLE
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/outcome.rs:109
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/fold/outcome.rs`, queue row 34
---

## Failure sequence

`RunState::eligible_continuation` opens with

    if self.run_is_ending() || self.lineage_has_question(key) {
        return None;
    }

and then requires the task's open generation to be `OpenNoAttempt`. The second
disjunct guards a state no legal log reaches, so the reader carries a branch
that only a hand-built fold can enter.

The state it guards is: task `key` holds an open generation **and** some member
of `key`'s lineage has an open question. Both event orders that would produce it
are refused by the fold itself, measured at this sha:

- question first, then dispatch -- `check_task_dispatched` refuses with
  "`task_dispatched` disagrees with the record it cites: dispatch requires no
  outstanding lineage question or candidate for this task";
- dispatch first, then question -- `check_question_raised` refuses with
  "lineage N has task M generation G still open with no attempt; settle it
  before parking its tasks".

The same holds for a task with no lineage, where `lineage_root(key)` is `key`
itself: a bare question on a task with an open generation is the first refusal
above, and a dispatch of a task that is `AwaitingInput` is refused on task state.

This is not a wrong answer -- `None` is what the selector should get in that
state -- and the branch costs nothing at run time. It is recorded because a
reader cannot tell defensive depth from a live path without running the two
refusals, and because a test that wants to cover the branch has to build the
state through `RunState::open_question` rather than by folding events. The sweep
of `src/topology/fold/tests.rs` added such a test
(`a_continuation_is_offered_only_for_an_open_generation_no_attempt_has_used`,
with the fixture `lineage_with_a_dispatched_repair`), and its fixture asserts
both refusals above before reaching past them, so the unreachability is executed
rather than believed.

## What the change that takes this up should do

Row 34's pass owns the decision, which is one of:

- keep the disjunct and say at the site that it is defence in depth, naming the
  two checks that make it unreachable, so the next reader does not have to
  re-derive it; or
- drop it, on the grounds that `check_task_dispatched` and `check_question_raised`
  are the authorities and a second copy of their rule in a reader is a place for
  the three to drift apart.

Either way, keep the test: if the disjunct goes, the test's last two arms become
a statement about the two checks instead, and the fixture's refusals are already
the assertions that carry it.
