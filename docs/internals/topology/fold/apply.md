# `src/topology/fold/apply.rs`

Extended notes for [`src/topology/fold/apply.rs`](../../../../src/topology/fold/apply.rs).

## Module

The application half of INV-02: what a checked transition does to the
state, and nothing that decides whether it may.

Application is a deterministic function of the prior state, event and
checked derivation. It reads no clock, environment or randomness and performs
no I/O. Live/replay equality requires the caller to check each event against
the current state, append it once and apply its delta once before checking
the next event. Purity and [`RunState`]'s `PartialEq` support testing that
protocol; they do not enforce single application or reject a stale delta.
[`super::Derived`] carries facts checked before application, including a
question's origin before [`RunState::apply_answer`] removes the question.

Clones retain event data in fold state, which outlives the borrowed event.
Registry entries, lease regions, queued candidates and generation records
each own the data they retain. Rejection borrows the recorded candidate
region while updating the disjoint lease table.

## `pub(super) fn apply(&mut self, body: &TopologyEventBody, derived: &Derived) {`

Apply a transition the check accepted.

Checking establishes the lookup preconditions for immediate application
to that same state. A lookup miss is not generally inert: candidate
creation still enqueues, verification start advances its sequence,
verification unavailability takes the transaction, and dispatch grants
its lease before the corresponding lookup. Callers must preserve the
check/append/apply protocol instead of relying on these fallbacks.

## `apply` › `Derived::None | Derived::Registry(_) => {}`

Exhaustive over `Derived`, so a new variant is a compile error
here rather than a silent no-op: the check pairs every
`question_answered` with `Derived::Answer`, and these two
cannot be its delta, so they change nothing.

## `wake_backoff` › `for key in std::mem::take(&mut self.deferred_tasks) {`

Move the keys out before deriving visible states. An open question
keeps a task parked but cannot preserve an already elapsed wait.

## `apply_verification_unavailable` › `match &unavailable.outcome {`

Exhaustive over the outcome, so a new `UnavailableOutcome` variant is
a compile error here rather than a silent no-op: two separate
`if let`s over a closed two-variant enum is a wildcard by another
spelling, which §5 forbids over a domain a new variant should force a
decision at. Behaviour is unchanged — both outcomes drop the open
sequence from the queue entry, only a defer sets the backoff, and only
a park raises a question and moves the task to awaiting input.

## `apply_merge_prepared` › `match prepared.disposition {`

Exhaustive over the disposition, so a new self-opening
`PreparedDisposition` is a compile error here rather than one that
silently skips the increment: a fast publication opens and closes its
own transaction and consumes a sequence, while a stale-clean or
already-present one was opened by its `merge_verification_started`,
which already advanced `next_sequence`.

## `apply_merge_rejected` › `match rejected.disposition {`

Exhaustive over the disposition, same reason as `apply_merge_prepared`:
a conflict is decided at the cherry-pick and opens and closes its own
transaction, consuming a sequence, while a code rejection was opened
by its `merge_verification_started`, which already advanced
`next_sequence`.

## `fn refresh_task_state(&mut self, key: TaskKey) {`

Derive the visible state after answering or waking a task. Question
origin does not capture a queued candidate, later wake or other answer.

## `fn fail_lineage(&mut self, key: TaskKey) {`

Decline terminates all unpublished work in the lineage. Already merged
work stays merged; a human answer cannot undo a recorded publication.

## `fail_lineage` › `TransactionClass::Prepared { .. } => false,`

The answer check refuses decline before append in this
class. Its already authorized publication must survive.

## `fail_lineage` › `let members: Vec<TaskKey> = self`

Owned keys let each member's resources be consumed without retaining
a registry borrow across those mutations.

## `pub(super) fn release_holdings_of(&mut self, key: TaskKey) {`

Remove this member's queued candidates and candidate/lineage holdings.
The caller closes its generation and visits every affected member.

## `pub(super) fn open_generation_mut(&mut self, key: TaskKey) -> Option<&mut GenerationFold> {`

`None` means this key has no task or no open generation. Checked callers
establish which generation they need before application mutates it.

## `close_generation` › `let releases_own_region = match generation.lease {`

Exhaustive over the lease, so a new `GenerationLease` is a compile
error here rather than one that silently keeps its region held: an
own generation holds its predicted region and releases it when it
closes, and an inherited-lineage generation took none of its own and
releases nothing.

## `apply` › `}`

**Not counted here.** An attempt that has *started* has not
yet spent anything: `ladder::spends_allowance` is total over
`FailureKind` and its line is "the worker ran and produced
work to judge", which `attempt_started` cannot know. Counting
here made this fold a second authority for a rule that has one
production implementation, and made every interruption, park
and outage burn a rung the packet says they do not —
`transaction_fault_matrix[T-ATTEMPT]`'s "unknown spend,
**allowance refunded**". The count is taken at the settlement,
in `apply_settlement`, from the record the settlement carries.

## `apply` › `self.close_generation(data.key);`

T-ATTEMPT: generation Closed, task Pending, later dispatch a
new generation. The close releases the ordinary generation's
own region exactly as every other closing settlement does.

## `apply` › `self.open_question(&data.question, QuestionOrigin::Admission, None);`

A bare `question_raised` carries no admission and so
authorizes no binding.

## `apply_resumed` › `self.budget_stop = None;`

The stop belongs to the epoch that hit the old ceiling; the next
epoch starts without one, which is what makes "raise the budget and
resume" the response to it.

## `apply_resumed` › `self.wake_backoff();`

Deferred items are woken by a resume exactly as they are by an
elapsed wait.

## `register` › `self.open_question(question, QuestionOrigin::Admission, Some(options.clone()));`

The one admission that authorizes an override, and the one
place its option list is frozen.

## `apply_dispatched` › `let (lease, region) = match &dispatched.lease {`

The recorded region and `predicted_region(entry)` are one value by the
time this runs: `check_dispatched` refuses an ordinary dispatch whose
`Predicted { paths }` is anything else. Granting the event's copy is
therefore granting the derivation, and it stays the event's copy so
that the region in the lease table is demonstrably the region the log
holds rather than a second derivation of it.

## `apply_settlement` › `self.charge_allowance(finished.key, &finished.record);`

**The allowance, decided once, by the one function that decides it.**

`ladder::spends_allowance` is documented as "the single production
implementation of the allowance rule" and is total over `FailureKind`
so a new variant stops the build rather than taking a default. This
fold consumes it; it does not re-derive it. `FailureRecord::shape`
exists for exactly this call — "a settlement holds a record rather
than the live failure, and the allowance decision is the same decision
either way".

**Taken at the settlement, which is what makes the refund free.**
T-ATTEMPT refunds an interrupted attempt's allowance. An attempt that
never settled never counted, so there is nothing to give back and no
second rule to keep in step with the first — the refund is the absence
of a charge rather than a subtraction that could be forgotten.

Before the `Escalated` arm below, which resets the count: an attempt
that escalates spent its allowance on the rung it is leaving, and the
rung it climbs onto starts again at zero.

Nested rather than a `let`-chain: `if cond && let Some(x) = ..` is
unstable on **1.85**, which this crate's MSRV pins, and stable rustc
accepts it — so the local gates pass and only the MSRV leg refuses.

## `apply_settlement` › `SettlementTransition::Succeeded => {}`

Unreachable: `check_attempt_finished` refuses this
transition before `apply` is called, because
`candidate_prepared` is the sole successful settlement. The
arm stays so the match is total over the wire vocabulary —
the variant is still a legal *shape*, it is simply not a
settlement this fold accepts — and it does nothing, so a
check that stopped refusing would produce a generation stuck
in flight rather than a silently-promoted one.

## `apply_settlement` › `if let Some(task) = self.tasks.get_mut(finished.key.index()) {`

The settlement's own number: the packet defines it as the
rung the escalation climbs *onto*. The allowance is per
rung, so it starts again here.

## `apply_settlement` › `self.set_defers(finished.key, *defers);`

The settlement's own number, not this fold's plus one.
`settle_failed` computed it as `defers.saturating_add(1)`
and appended it; recomputing here would be a second
derivation of a value the log already holds, and a replay
of the same log would then disagree with the process that
wrote it.

## `pub(super) fn record_halt(&mut self, key: TaskKey) {`

`halted_at` is first in wins, and is never cleared.

## `pub(super) fn charge_allowance(&mut self, key: TaskKey, record: &crate::events::AttemptRecord) {`

One settled attempt against its rung's allowance.

**The single write, and both settlements reach it through here.** The
increment used to live inline in [`Self::apply_settlement`], which was
fine while `attempt_finished` was the only settlement — and stopped being
fine on 2026-08-27, when `candidate_prepared` became the sole successful
one. The settlement moved and the counting did not, so **a successful
attempt stopped spending anything**: a first-attempt success left
`attempts_on_rung` at zero, replay reproduced the undercount, and a later
allowance reader could grant an extra attempt on a rung already paid for.
The round-4 review of `09f9a99` found it, and the Class B approval this
change was made under says the thing that did not happen — *"settlement
counting moves to the sole event"*.

A shared core rather than a second increment, because two increments are
two rules: `the_rungs_allowance_is_counted_in_one_production_place` exists
to forbid exactly that, and it counts **calls to this** so a settlement
that stops charging is a failing census rather than a silent undercount.

It consults `spends_allowance` and answers nothing itself. A successful
record carries no failure, and `spends_allowance(None)` is `true`: the
worker ran and produced work that was judged and accepted.

## `apply_candidate_prepared` › `generation.class = GenerationClass::Promoting;`

**The settlement, which used to arrive on its own event.** A
candidate-producing attempt has exactly one successful
settlement and this is it, so the class transition belongs here
rather than to an `attempt_finished` the 2026-08-12 record says
is not emitted.

## `apply_candidate_prepared` › `self.charge_allowance(prepared.key, &prepared.attempt);`

**The settlement's accounting, which moved with the settlement.**
Same core as the failure path, so there is one increment in this
build and both settlements reach it.

## `apply_merge_rejected` › `let held = self`

The rejected candidate's own holding becomes the lineage's,
widened by the region the conflict named.

## `pub(super) fn open_question(`

Open a question, carrying the binding authority it was asked under.

`binding` is `Some` for a `HumanBinding` admission and `None` for every
other question this run can ask — a `HumanRequired` admission, a parked
settlement, a verification park, a bare `question_raised`. That is the
whole of what an override may be validated against.

## `pub(super) fn set_defers(&mut self, key: TaskKey, defers: u32) {`

Record the deferral count a `Deferred` settlement carried.

Assignment rather than increment: the number is the settlement's, which
is what makes a replay of the same log reach the same count as the
process that wrote it.

## `pub(super) fn close_generation(&mut self, key: TaskKey) {`

Close the open generation, releasing the region it held on its own.
