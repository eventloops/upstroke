---
id: SWEEP-FOLD-APPLY-UNREACHABLE-GUARDS
severity: P3
disposition: deferred     # both are kept deliberately as defence in depth; pinning them directly needs a `&mut RunState`, whose fixture lives in queue row 39's file
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/apply.rs:24
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/fold/tests.rs` (queue row 39), or a later change that decides these two are better deleted than kept
---

## Failure sequence

Location as first recorded: src/topology/fold/apply.rs:24 and 445 at `ee5dc81f`

Two statements in `RunState::apply` cannot be witnessed, because the state they would change is
refused one step earlier. Measured at `ee5dc81f` against the whole `topology::fold` suite (131
tests), each applied alone and reverted; both mutations survive.

**`attempt_interrupted` returning its task to `Pending`.**

```rust
TopologyEventBody::AttemptInterrupted { data } => {
    self.close_generation(data.key);
    self.set_state(data.key, TaskState::Pending);
}
```

Deleting the `set_state` line changes nothing any test sees. `check_dispatched` refuses a dispatch
whose task is not already `Pending`, and no event between the dispatch and the interruption moves a
task's state: `attempt_started` writes only the generation's class and attempt count, and a question
cannot park a task whose lineage holds an open generation (`check_question_can_park_lineage`). So at
every `attempt_interrupted` the task is `Pending` and the assignment is a no-op. The sibling test
`an_interruption_closes_its_generation_and_returns_its_task_to_pending` asserts the state after the
interruption, which is true, and would stay true with the line gone.

**A decline sparing an authorized publication.**

```rust
&& match &transaction.class {
    TransactionClass::VerificationStarted { .. } => true,
    TransactionClass::Prepared { .. } => false,
}
```

Flipping the `Prepared` arm to `true` — so a decline cancels a transaction that has already
authorized a publication — changes nothing any test sees, because `check_question_answered` refuses
that decline before it is appended, on the same root comparison this arm sits behind. The guard is
the application half of a refusal the checker owns, and `design/26` states it once for both ("A
`Prepared` transaction has already authorized publication, so a decline affecting it is refused
before append until that publication completes").

Neither is a defect. Both are kept: the first states the postcondition `design/15`'s interrupted
attempt has, and the second keeps the application honest if the checker's refusal is ever narrowed.
What this records is that the repository cannot tell you whether either still works.

## What the change that takes this up should do

Either give each a direct test, the way queue row 31 pinned its own redundant conjunct — which
means a `&mut RunState`, so it belongs with the sweep of `src/topology/fold/tests.rs` — or decide
that a guard whose mutation no test can see is better deleted than kept, and delete it with the
sentence that says why. Do not leave the third option, which is to keep it and go on calling it
tested.
