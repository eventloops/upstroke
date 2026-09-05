# `src/engine/topology/identity.rs`

Extended notes for [`src/engine/topology/identity.rs`](../../../../src/engine/topology/identity.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Process-lifetime identity: invocations, slot pairs, provisional reservations.

`decisions.admission_and_leases.invocation_identity` defines the value;
`src/runner/invocation.rs` defines the type. Neither allocates one — that
module says so in as many words: *"PR4 owns the type and its properties.
**PR7 assigns them**. No ledger, no broker and no allocation policy lives
here."* This is the ledger, the assignment, and the policy.

Three concerns live together because they share one lifetime and one
failure mode. An [`InvocationId`], a slot pair and a provisional
reservation are all **process-local**: `crash_reconstruction` requires that
"provisional reservations, slot table, invocation ledger, and the
coordinator's own lock holds are empty at process start", and a resume
rebuilds none of them. A ledger that survived a process would be a claim
about a dead coordinator's state, which is precisely what the recovery order
exists to avoid making.

### Assertion, not brokerage

At `max_parallel = 1` the packet asks for **assertions**:
`state_resource_ownership_matrix` records R3 as "assertion only" and the
pipeline entitlement as "sequential assertion". PR11 replaces these with a
`PermitBroker` that waits. Nothing here waits: a second concurrent slotted
invocation is not contention to be queued, it is a bug in the caller, and it
refuses.

## `pub struct AttemptIdentities {`

---------------------------------------------------------------------------
Assignment
---------------------------------------------------------------------------

## `pub struct AttemptIdentities {`

Every invocation identity of one attempt.

A value rather than four free functions, because the three coordinates that
must not vary within an attempt — key, generation, attempt number — are then
fixed once at the top of the attempt and cannot be mistyped at the fourth
call site. `decisions.admission_and_leases.invocation_identity`'s first
form is exactly this tuple.

**A retry is a new attempt number, so it is a new `AttemptIdentities`.**
INV-20: "every Runner process carries a unique typed `InvocationId` that
changes with every attempt". Reusing this value across a retry would give
the retry's worker the identity of the attempt that was retained, and a
completion arriving late from the first would then apply to the second.

## `impl AttemptIdentities` › `pub const fn new(key: TaskKey, generation: GenerationId, attempt: AttemptNumber) -> Self {`

The identities of `(key, generation, attempt)`.

## `impl AttemptIdentities` › `pub const fn worker(&self) -> InvocationId {`

The worker process.

## `impl AttemptIdentities` › `pub const fn gate(&self, gate: u32, ordinal: u32) -> InvocationId {`

Gate `gate` of this attempt's gate list, on its `ordinal`-th run.

Two numbers because they mean different things and the packet keeps
them apart: `gate` is *which gate*, `ordinal` is *which run of it*. A
gate re-dispatched inside one attempt is a new identity rather than a
reused one, which is what makes a stale completion from the first run
discardable.

## `impl AttemptIdentities` › `pub const fn review_pass(&self, pass: u32, ordinal: u32) -> InvocationId {`

Review pass `pass`, on its `ordinal`-th run.

## `impl AttemptIdentities` › `pub const fn review_reask(&self, reask: u32, ordinal: u32) -> InvocationId {`

Re-ask `reask` of a review pass, on its `ordinal`-th run.

## `pub struct SequenceIdentities {`

Every invocation identity of one integration transaction.

The packet's second form, "`(sequence, role, ordinal)` with role in
{gate(n), review_pass(n), review_reask(n)}" — **no worker**. A sequence
integrates candidates other processes produced, so there is no worker of a
sequence to identify, and [`SequenceRole`] makes that a compile error rather
than a refusal.

Present in this slice because the identities are PR7's to assign and the
type has to exist for `checkpoint_refusals` to refuse an integration
*before any append*. The transaction itself is PR8's.

## `impl SequenceIdentities` › `pub const fn new(sequence: SequenceId) -> Self {`

The identities of `sequence`.

## `impl SequenceIdentities` › `pub const fn gate(&self, gate: u32, ordinal: u32) -> InvocationId {`

Gate `gate` of this transaction, on its `ordinal`-th run.

## `impl SequenceIdentities` › `pub const fn review_pass(&self, pass: u32, ordinal: u32) -> InvocationId {`

Review pass `pass`, on its `ordinal`-th run.

## `impl SequenceIdentities` › `pub const fn review_reask(&self, reask: u32, ordinal: u32) -> InvocationId {`

Re-ask `reask`, on its `ordinal`-th run.

## `pub struct PreflightIdentities;`

The `RunnerPreflight`'s identities: one shell probe, one probe per agent.

INV-23: "one non-slotted shell probe (the recorded shell executing `exit 0`)
and one slotted probe per recorded agent, each a registered invocation
through the run's Runner". The asymmetry is the whole point of keeping them
apart here — see [`SlotAssertion`].

These identities **repeat across incarnations** by construction: a probe is
`(probe, target, ordinal)` and carries no run or epoch. That is deliberate
and is why a container name additionally carries the coordinator incarnation
id — without it a resuming incarnation's probe container would collide with,
and overwrite the ownership evidence of, the dead incarnation's.

## `impl PreflightIdentities` › `pub fn shell(ordinal: u32) -> Result<InvocationId, UpstrokeError> {`

The shell probe. Non-slotted.

### Errors

Never in practice — [`InvocationId::probe`] refuses only on an agent id
this target does not carry — but the fallibility is [`ProbeTarget`]'s
and is not worth a second, unfalsifiable, constructor to hide.

## `impl PreflightIdentities` › `pub fn agent(agent: &str, ordinal: u32) -> Result<InvocationId, UpstrokeError> {`

The probe of one recorded agent. Slotted.

### Errors

[`UpstrokeError`] when `agent` is not a name an invocation id can carry
— outside `[0-9A-Za-z_-]`, or too long. A probe identity is a path and
a container-name component, so the refusal is a containment refusal.

## `pub struct SlotAssertion {`

---------------------------------------------------------------------------
Slot pairs — asserted, never awaited
---------------------------------------------------------------------------

## `pub struct SlotAssertion {`

The sequential substrate's assertion that one slotted invocation runs at a
time.

`permits.agent_pool_slots`: "every agent CLI invocation acquires its atomic
`{agent, pool?}` pair: worker, review_pass, review_reask, integration
review_pass/review_reask, and agent probe; gate invocations and the shell
probe acquire no slot."

At `max_parallel = 1` this is R3 "assertion only". A second concurrent
slotted acquisition is refused rather than queued, because at this parallel
width there is no legitimate way to reach one: the loop runs a single
attempt to completion, and an overlap means a caller leaked a hold. PR11's
`PermitBroker` is where waiting arrives.

The ledger balances at process end, which
`permits.provisional_reservations` requires of every process-local grant.

## `pub fn is_slotted(invocation: &InvocationId) -> bool {`

Whether `invocation`'s process takes an atomic `{agent, pool?}` slot pair.

`permits.agent_pool_slots` lists the slotted roles and then excludes two by
name: "**gate invocations and the shell probe acquire no slot**". Both
exclusions are recoverable from the identity alone — a gate is
`AttemptRole::Gate`/`SequenceRole::Gate`, the shell probe is
`ProbeTarget::Shell` — so this is a total function of the id rather than a
second field a caller could set wrongly.

[`crate::runner::ExecutionRole::is_slotted`] states the same rule over the
request's role. The two agree by construction because both read the packet
sentence, and `a_gate_and_the_shell_probe_are_refused_a_slot_pair` pins
this side of it; `src/runner/**` is frozen, so they cannot be unified here.

## `pub struct SlotPair {`

The atomic pair a slotted invocation holds.

## `pub struct SlotPair` › `pub agent: String,`

The agent whose per-agent slot this is.

## `pub struct SlotPair` › `pub pool: Option<String>,`

The pool, when the agent is in one.

## `impl SlotAssertion` › `pub fn new() -> Self {`

An empty table, which is what `crash_reconstruction` requires at
process start.

## `impl SlotAssertion` › `pub fn acquire(`

Take the pair for `invocation`.

### Errors

[`UpstrokeError::Refused`] when a pair is already held. Refusing rather
than waiting is the assertion.

## `impl SlotAssertion` › `pub fn release(&mut self, invocation: &InvocationId) -> Result<(), UpstrokeError> {`

Release the pair `invocation` holds.

### Errors

[`UpstrokeError::Refused`] when `invocation` does not hold one. A
release naming another invocation is the stale-completion shape INV-20
refuses, not a no-op.

## `impl SlotAssertion` › `pub fn pair_of(&self, invocation: &InvocationId) -> Option<&SlotPair> {`

The pair `invocation` holds, if it holds one.

Present so a test can assert **which** pair was taken. Without it the
stored [`SlotPair`] is write-only, and "agent probes acquire their slot
pair (asserted)" would assert only that *a* pair was held.

## `impl SlotAssertion` › `pub fn held(&self) -> Option<&InvocationId> {`

The invocation holding the pair, if any.

[`Self::holds`] answers "is it this one"; this answers "which one". A
cancellation needs the second: `permits.protocol` cancels "a granted or
non-slotted running invocation", and at a halt the coordinator knows
that *a* pair is held without knowing whether it belongs to the worker,
a reviewer or a re-ask. Releasing by guess would leave the ledger
unbalanced at process end with nothing to say so.

## `impl SlotAssertion` › `pub fn holds(&self, invocation: &InvocationId) -> bool {`

Whether `invocation` holds the pair.

## `impl SlotAssertion` › `pub const fn is_empty(&self) -> bool {`

Whether any pair is held.

## `impl SlotAssertion` › `pub const fn balances(&self) -> bool {`

Whether every grant was released — the process-end condition.

## `pub enum ReservationKind {`

---------------------------------------------------------------------------
Provisional reservations
---------------------------------------------------------------------------

## `pub enum ReservationKind {`

What a provisional reservation bridges to.

`permits.provisional_reservations`: "process-lifetime bridge between a
selection decision and its first append: dispatch selection reserves
{pipeline} until `task_dispatched`; retry selection reserves {pipeline}
until `attempt_started(retry)`; integration selection reserves
{pipeline, merge} until `merge_prepared(fast)`, `merge_verification_started`
or `merge_rejected(conflict)`."

## `pub enum ReservationKind` › `Dispatch,`

A fresh dispatch, converted at `task_dispatched`.

## `pub enum ReservationKind` › `Retry,`

A same-generation retry, converted at `attempt_started(retry)`.

## `pub enum ReservationKind` › `Integration,`

An integration transaction. Holds `{pipeline, merge}`, not `{pipeline}`.
PR8's, and here so the checkpoint refusal can name it.

## `impl ReservationKind` › `pub const fn entitlements(self) -> u32 {`

How many entitlements this reservation holds.

Dispatch and retry hold `{pipeline}`; integration holds
`{pipeline, merge}`. The count is what has to balance.

## `impl ReservationKind` › `pub const fn name(self) -> &'static str {`

The name the refusal messages use.

## `pub struct Reservations {`

The process-local provisional-reservation ledger.

"the sequential substrate asserts at most one", "crash reset: none exist at
process start", "process-local ledger balances at process end". Every one of
those three is a property of this type rather than a comment: `new` is
empty, `take` refuses a second, and `balances` is the process-end check.

Cancellation is not an error path — it is one of four ordinary outcomes.
`cancellation`: "provisional reservations cancelled on pre-append failure or
a poisoned fold", and `permits`: "cancellation on any pre-append failure,
run end, shutdown, or a poisoned fold".

## `impl Reservations` › `pub fn new() -> Self {`

An empty ledger, which is what process start requires.

## `impl Reservations` › `pub fn take(&mut self, key: TaskKey, kind: ReservationKind) -> Result<(), UpstrokeError> {`

Reserve for `key`.

### Errors

[`UpstrokeError::Refused`] when one is already held.

## `impl Reservations` › `pub fn convert(&mut self, key: TaskKey, kind: ReservationKind) -> Result<(), UpstrokeError> {`

Convert the reservation at its append.

### Errors

[`UpstrokeError::Refused`] when nothing is held, or when the held
reservation is another task's or another kind. A conversion that
silently accepted a mismatch is how an entitlement gets counted against
the wrong generation.

## `impl Reservations` › `pub fn cancel(&mut self, key: TaskKey, kind: ReservationKind) -> Result<(), UpstrokeError> {`

Cancel it: a pre-append failure, run end, shutdown, or a poisoned fold.

### Errors

As [`Self::convert`].

## `impl Reservations` › `pub fn cancel_any(&mut self) -> bool {`

Cancel whatever is held, if anything, without naming it.

The append-error protocol's shape: the fold is poisoned and the
coordinator is ending, so it cancels what it holds rather than asserting
what that is. Returns whether anything was held.

## `impl Reservations` › `pub const fn is_empty(&self) -> bool {`

Whether a reservation is held.

## `impl Reservations` › `pub fn entitlements_held(&self) -> u32 {`

The entitlements the held reservation accounts for, zero when none is.

## `impl Reservations` › `pub const fn balances(&self) -> bool {`

Whether every reservation was converted or cancelled exactly once.

## `enum Registration {`

---------------------------------------------------------------------------
The invocation ledger
---------------------------------------------------------------------------

## `enum Registration {`

What an invocation's registration is currently.

## `pub struct InvocationLedger {`

R4: every Runner process registered exactly once, settled exactly once.

`permits.protocol`: "the invocation ledger records registered/completed/
cancelled exactly once and balances at process end"; and "duplicate
complete/cancel ignored and counted", which is why a duplicate is not an
error here but a counter — INV-20 asks for "discard with a non-durable
warning", not a refusal.

## `impl InvocationLedger` › `pub fn new() -> Self {`

An empty ledger, which is what process start requires.

## `impl InvocationLedger` › `pub fn register(&mut self, invocation: &InvocationId) -> Result<(), UpstrokeError> {`

Register `invocation` as running.

### Errors

[`UpstrokeError::Refused`] when this identity is already registered.
That is aliasing (ST-04), not a duplicate completion, and it refuses.

## `impl InvocationLedger` › `pub fn complete(&mut self, invocation: &InvocationId) -> Result<(), UpstrokeError> {`

Settle `invocation` as completed. A duplicate is counted, not refused.

### Errors

[`UpstrokeError::Refused`] when `invocation` was never registered.

## `impl InvocationLedger` › `pub fn cancel(&mut self, invocation: &InvocationId) -> Result<(), UpstrokeError> {`

Settle `invocation` as cancelled. A duplicate is counted, not refused.

### Errors

[`UpstrokeError::Refused`] when `invocation` was never registered.

## `impl InvocationLedger` › `pub fn cancel_all_running(&mut self) -> usize {`

Cancel every still-running invocation, returning how many.

The append-error protocol's "in-flight invocations are cancelled through
the Runner" — this is the ledger half of that; the Runner half is the
caller's.

## `impl InvocationLedger` › `pub const fn duplicates(&self) -> u32 {`

How many duplicate settlements were discarded.

## `impl InvocationLedger` › `pub fn completed(&self) -> usize {`

How many registrations settled as **completed**.

Kept apart from [`Self::cancelled`] because R3 keeps them apart:
"requested: released on **cancel**" and "granted: released on complete
**or** cancel" are two rows, so a ledger that reported only "settled"
could not tell a process that ran from one that never started, and a
caller that completed a refused spawn would balance and be wrong.

## `impl InvocationLedger` › `pub fn cancelled(&self) -> usize {`

How many registrations settled as **cancelled**.

## `impl InvocationLedger` › `pub fn balances(&self) -> bool {`

Whether every registration was settled — the process-end condition.

## `impl InvocationLedger` › `pub fn running(&self) -> Vec<&str> {`

The identities still running.

## `mod tests` › `fn every_invocation_of_an_attempt_is_distinct_and_a_retry_reuses_none_of_them() {`

--- assignment --------------------------------------------------------

## `mod tests` › `fn every_invocation_of_an_attempt_is_distinct_and_a_retry_reuses_none_of_them() {`

Every identity of one attempt is distinct, and distinct from every
identity of the next attempt.

ST-04 is "no two … invocations share an InvocationId", and INV-20 adds
"changes with every attempt". Both are asserted over the whole set
rather than pairwise on a sample, because the failure this guards is a
role whose ordinal was forgotten and which therefore collides with its
own neighbour.

## `fn every_invocation_of_an_attempt_is_distinct_and_a_retry_reuses_none_of_them() {` › `assert_ne!(first.gate(0, 0), first.gate(0, 1));`

The ordinal is load-bearing: a gate re-dispatched inside one attempt
is a new identity, so a completion from the first run cannot apply
to the second.

## `fn every_invocation_of_an_attempt_is_distinct_and_a_retry_reuses_none_of_them() {` › `assert_ne!(first.gate(0, 1), first.gate(1, 0));`

And the gate number is load-bearing separately from the ordinal.

## `mod tests` › `fn an_identity_is_a_pure_function_of_its_tuple() {`

The same tuple renders the same identity in any process.

"deterministic in the sequential substrate" is what lets a container
name be predicted, and what makes an intent path stable across the
incarnation that wrote it and the one that reclaims it.

## `mod tests` › `fn a_sequence_has_no_worker_and_shares_no_identity_with_an_attempt() {`

A sequence has gates and reviews and no worker, and its identities do
not collide with an attempt's.

## `mod tests` › `fn a_probe_identity_carries_no_epoch_and_therefore_repeats_across_incarnations() {`

Probe identities repeat across incarnations, deliberately.

This is not a defect to fix here: it is why a container name carries the
coordinator incarnation id. Asserting it keeps the reason visible — a
later change that made probe identities unique per incarnation would
make the incarnation component of a container name dead weight, and this
test is where that shows up.

## `mod tests` › `fn an_agent_probe_refuses_a_name_that_is_not_a_safe_component() {`

An agent name an identity cannot carry is refused, because that identity
becomes a path component and a container-name component.

## `mod tests` › `fn a_second_slot_pair_is_refused_rather_than_queued() {`

--- slot pairs --------------------------------------------------------

## `mod tests` › `fn a_second_slot_pair_is_refused_rather_than_queued() {`

One slotted invocation at a time, asserted rather than queued.

## `mod tests` › `fn a_gate_and_the_shell_probe_are_refused_a_slot_pair() {`

A gate and the shell probe are refused a slot pair.

`permits.agent_pool_slots` excludes both by name. Asserted over every
shape of identity rather than one, because the rule is three separate
exclusions — `AttemptRole::Gate`, `SequenceRole::Gate`,
`ProbeTarget::Shell` — and a check that knew only the first would pass a
suite testing only attempts.

## `fn a_gate_and_the_shell_probe_are_refused_a_slot_pair()` › `for (label, id) in [`

And the four slotted shapes are still accepted, so the refusal is a
rule rather than a blanket.

## `mod tests` › `fn a_release_naming_another_invocation_is_refused() {`

A release naming another invocation is refused, not ignored.

## `mod tests` › `fn a_reservation_is_asserted_singly_and_settles_exactly_once() {`

--- provisional reservations ------------------------------------------

## `mod tests` › `fn a_reservation_is_asserted_singly_and_settles_exactly_once() {`

One reservation at a time, converted or cancelled exactly once.

## `mod tests` › `fn a_reservation_settled_under_the_wrong_name_is_refused() {`

A settlement naming another task or another kind is refused.

This is the shape that would count an entitlement against the wrong
generation, which is the accounting INV-22 asks to balance.

## `mod tests` › `fn cancel_any_releases_an_unnamed_reservation_and_reports_whether_there_was_one() {`

The append-error protocol cancels what it holds without naming it.

## `mod tests` › `fn the_invocation_ledger_refuses_aliasing_and_counts_duplicate_settlements() {`

--- the invocation ledger ---------------------------------------------

## `mod tests` › `fn the_invocation_ledger_refuses_aliasing_and_counts_duplicate_settlements() {`

Registered once, settled once; a duplicate settlement is counted, not
refused.

## `fn the_invocation_ledger_refuses_aliasing_and_counts_duplicate_settlements() {` › `ledger`

"duplicate complete/cancel ignored and counted" — INV-20 asks for a
discard with a warning, not a refusal.

## `mod tests` › `fn settling_an_unregistered_invocation_is_refused() {`

Settling something never registered is refused.

## `mod tests` › `fn cancel_all_running_settles_every_in_flight_invocation() {`

The append-error protocol's half: every still-running invocation is
cancelled, and the ledger then balances.
