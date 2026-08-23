//! Process-lifetime identity: invocations, slot pairs, provisional reservations.
//!
//! `decisions.admission_and_leases.invocation_identity` defines the value;
//! `src/runner/invocation.rs` defines the type. Neither allocates one — that
//! module says so in as many words: *"PR4 owns the type and its properties.
//! **PR7 assigns them**. No ledger, no broker and no allocation policy lives
//! here."* This is the ledger, the assignment, and the policy.
//!
//! Three concerns live together because they share one lifetime and one
//! failure mode. An [`InvocationId`], a slot pair and a provisional
//! reservation are all **process-local**: `crash_reconstruction` requires that
//! "provisional reservations, slot table, invocation ledger, and the
//! coordinator's own lock holds are empty at process start", and a resume
//! rebuilds none of them. A ledger that survived a process would be a claim
//! about a dead coordinator's state, which is precisely what the recovery order
//! exists to avoid making.
//!
//! # Assertion, not brokerage
//!
//! At `max_parallel = 1` the packet asks for **assertions**:
//! `state_resource_ownership_matrix` records R3 as "assertion only" and the
//! pipeline entitlement as "sequential assertion". PR11 replaces these with a
//! `PermitBroker` that waits. Nothing here waits: a second concurrent slotted
//! invocation is not contention to be queued, it is a bug in the caller, and it
//! refuses.

use std::collections::BTreeMap;

use crate::error::UpstrokeError;
use crate::runner::invocation::{AttemptRole, SequenceRole};
use crate::runner::{AgentId, InvocationId, ProbeTarget};
use crate::topology::events::{AttemptNumber, GenerationId, SequenceId};
use crate::topology::registry::TaskKey;

// ---------------------------------------------------------------------------
// Assignment
// ---------------------------------------------------------------------------

/// Every invocation identity of one attempt.
///
/// A value rather than four free functions, because the three coordinates that
/// must not vary within an attempt — key, generation, attempt number — are then
/// fixed once at the top of the attempt and cannot be mistyped at the fourth
/// call site. `decisions.admission_and_leases.invocation_identity`'s first
/// form is exactly this tuple.
///
/// **A retry is a new attempt number, so it is a new `AttemptIdentities`.**
/// INV-20: "every Runner process carries a unique typed `InvocationId` that
/// changes with every attempt". Reusing this value across a retry would give
/// the retry's worker the identity of the attempt that was retained, and a
/// completion arriving late from the first would then apply to the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptIdentities {
    key: TaskKey,
    generation: GenerationId,
    attempt: AttemptNumber,
}

impl AttemptIdentities {
    /// The identities of `(key, generation, attempt)`.
    #[must_use]
    pub const fn new(key: TaskKey, generation: GenerationId, attempt: AttemptNumber) -> Self {
        Self {
            key,
            generation,
            attempt,
        }
    }

    /// The worker process.
    #[must_use]
    pub const fn worker(&self) -> InvocationId {
        InvocationId::attempt(
            self.key,
            self.generation,
            self.attempt,
            AttemptRole::Worker,
            0,
        )
    }

    /// Gate `gate` of this attempt's gate list, on its `ordinal`-th run.
    ///
    /// Two numbers because they mean different things and the packet keeps
    /// them apart: `gate` is *which gate*, `ordinal` is *which run of it*. A
    /// gate re-dispatched inside one attempt is a new identity rather than a
    /// reused one, which is what makes a stale completion from the first run
    /// discardable.
    #[must_use]
    pub const fn gate(&self, gate: u32, ordinal: u32) -> InvocationId {
        InvocationId::attempt(
            self.key,
            self.generation,
            self.attempt,
            AttemptRole::Gate(gate),
            ordinal,
        )
    }

    /// Review pass `pass`, on its `ordinal`-th run.
    #[must_use]
    pub const fn review_pass(&self, pass: u32, ordinal: u32) -> InvocationId {
        InvocationId::attempt(
            self.key,
            self.generation,
            self.attempt,
            AttemptRole::ReviewPass(pass),
            ordinal,
        )
    }

    /// Re-ask `reask` of a review pass, on its `ordinal`-th run.
    #[must_use]
    pub const fn review_reask(&self, reask: u32, ordinal: u32) -> InvocationId {
        InvocationId::attempt(
            self.key,
            self.generation,
            self.attempt,
            AttemptRole::ReviewReask(reask),
            ordinal,
        )
    }
}

/// Every invocation identity of one integration transaction.
///
/// The packet's second form, "`(sequence, role, ordinal)` with role in
/// {gate(n), review_pass(n), review_reask(n)}" — **no worker**. A sequence
/// integrates candidates other processes produced, so there is no worker of a
/// sequence to identify, and [`SequenceRole`] makes that a compile error rather
/// than a refusal.
///
/// Present in this slice because the identities are PR7's to assign and the
/// type has to exist for `checkpoint_refusals` to refuse an integration
/// *before any append*. The transaction itself is PR8's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceIdentities {
    sequence: SequenceId,
}

impl SequenceIdentities {
    /// The identities of `sequence`.
    #[must_use]
    pub const fn new(sequence: SequenceId) -> Self {
        Self { sequence }
    }

    /// Gate `gate` of this transaction, on its `ordinal`-th run.
    #[must_use]
    pub const fn gate(&self, gate: u32, ordinal: u32) -> InvocationId {
        InvocationId::sequence(self.sequence, SequenceRole::Gate(gate), ordinal)
    }

    /// Review pass `pass`, on its `ordinal`-th run.
    #[must_use]
    pub const fn review_pass(&self, pass: u32, ordinal: u32) -> InvocationId {
        InvocationId::sequence(self.sequence, SequenceRole::ReviewPass(pass), ordinal)
    }

    /// Re-ask `reask`, on its `ordinal`-th run.
    #[must_use]
    pub const fn review_reask(&self, reask: u32, ordinal: u32) -> InvocationId {
        InvocationId::sequence(self.sequence, SequenceRole::ReviewReask(reask), ordinal)
    }
}

/// The `RunnerPreflight`'s identities: one shell probe, one probe per agent.
///
/// INV-23: "one non-slotted shell probe (the recorded shell executing `exit 0`)
/// and one slotted probe per recorded agent, each a registered invocation
/// through the run's Runner". The asymmetry is the whole point of keeping them
/// apart here — see [`SlotAssertion`].
///
/// These identities **repeat across incarnations** by construction: a probe is
/// `(probe, target, ordinal)` and carries no run or epoch. That is deliberate
/// and is why a container name additionally carries the coordinator incarnation
/// id — without it a resuming incarnation's probe container would collide with,
/// and overwrite the ownership evidence of, the dead incarnation's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreflightIdentities;

impl PreflightIdentities {
    /// The shell probe. Non-slotted.
    ///
    /// # Errors
    ///
    /// Never in practice — [`InvocationId::probe`] refuses only on an agent id
    /// this target does not carry — but the fallibility is [`ProbeTarget`]'s
    /// and is not worth a second, unfalsifiable, constructor to hide.
    pub fn shell(ordinal: u32) -> Result<InvocationId, UpstrokeError> {
        InvocationId::probe(ProbeTarget::Shell, ordinal)
    }

    /// The probe of one recorded agent. Slotted.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError`] when `agent` is not a name an invocation id can carry
    /// — outside `[0-9A-Za-z_-]`, or too long. A probe identity is a path and
    /// a container-name component, so the refusal is a containment refusal.
    pub fn agent(agent: &str, ordinal: u32) -> Result<InvocationId, UpstrokeError> {
        InvocationId::probe(ProbeTarget::Agent(AgentId::new(agent)), ordinal)
    }
}

// ---------------------------------------------------------------------------
// Slot pairs — asserted, never awaited
// ---------------------------------------------------------------------------

/// The sequential substrate's assertion that one slotted invocation runs at a
/// time.
///
/// `permits.agent_pool_slots`: "every agent CLI invocation acquires its atomic
/// `{agent, pool?}` pair: worker, review_pass, review_reask, integration
/// review_pass/review_reask, and agent probe; gate invocations and the shell
/// probe acquire no slot."
///
/// At `max_parallel = 1` this is R3 "assertion only". A second concurrent
/// slotted acquisition is refused rather than queued, because at this parallel
/// width there is no legitimate way to reach one: the loop runs a single
/// attempt to completion, and an overlap means a caller leaked a hold. PR11's
/// `PermitBroker` is where waiting arrives.
///
/// The ledger balances at process end, which
/// `permits.provisional_reservations` requires of every process-local grant.
#[derive(Debug, Default)]
pub struct SlotAssertion {
    held: Option<(InvocationId, SlotPair)>,
    granted: u32,
    released: u32,
}

/// The atomic pair a slotted invocation holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotPair {
    /// The agent whose per-agent slot this is.
    pub agent: String,
    /// The pool, when the agent is in one.
    pub pool: Option<String>,
}

impl SlotAssertion {
    /// An empty table, which is what `crash_reconstruction` requires at
    /// process start.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take the pair for `invocation`.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when a pair is already held. Refusing rather
    /// than waiting is the assertion.
    pub fn acquire(
        &mut self,
        invocation: &InvocationId,
        pair: SlotPair,
    ) -> Result<(), UpstrokeError> {
        if let Some((held, _)) = &self.held {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "`{invocation}` asked for a slot pair while `{held}` holds one: at \
                     max_parallel = 1 the substrate asserts a single slotted invocation \
                     rather than queueing"
                ),
            });
        }
        self.held = Some((invocation.clone(), pair));
        self.granted += 1;
        Ok(())
    }

    /// Release the pair `invocation` holds.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when `invocation` does not hold one. A
    /// release naming another invocation is the stale-completion shape INV-20
    /// refuses, not a no-op.
    pub fn release(&mut self, invocation: &InvocationId) -> Result<(), UpstrokeError> {
        match &self.held {
            Some((held, _)) if held == invocation => {
                self.held = None;
                self.released += 1;
                Ok(())
            }
            Some((held, _)) => Err(UpstrokeError::Refused {
                message: format!("`{invocation}` released a slot pair held by `{held}`"),
            }),
            None => Err(UpstrokeError::Refused {
                message: format!("`{invocation}` released a slot pair nothing holds"),
            }),
        }
    }

    /// Whether `invocation` holds the pair.
    #[must_use]
    pub fn holds(&self, invocation: &InvocationId) -> bool {
        self.held
            .as_ref()
            .is_some_and(|(held, _)| held == invocation)
    }

    /// Whether any pair is held.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.held.is_none()
    }

    /// Whether every grant was released — the process-end condition.
    #[must_use]
    pub const fn balances(&self) -> bool {
        self.granted == self.released && self.held.is_none()
    }
}

// ---------------------------------------------------------------------------
// Provisional reservations
// ---------------------------------------------------------------------------

/// What a provisional reservation bridges to.
///
/// `permits.provisional_reservations`: "process-lifetime bridge between a
/// selection decision and its first append: dispatch selection reserves
/// {pipeline} until `task_dispatched`; retry selection reserves {pipeline}
/// until `attempt_started(retry)`; integration selection reserves
/// {pipeline, merge} until `merge_prepared(fast)`, `merge_verification_started`
/// or `merge_rejected(conflict)`."
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReservationKind {
    /// A fresh dispatch, converted at `task_dispatched`.
    Dispatch,
    /// A same-generation retry, converted at `attempt_started(retry)`.
    Retry,
    /// An integration transaction. Holds `{pipeline, merge}`, not `{pipeline}`.
    /// PR8's, and here so the checkpoint refusal can name it.
    Integration,
}

impl ReservationKind {
    /// How many entitlements this reservation holds.
    ///
    /// Dispatch and retry hold `{pipeline}`; integration holds
    /// `{pipeline, merge}`. The count is what has to balance.
    #[must_use]
    pub const fn entitlements(self) -> u32 {
        match self {
            Self::Dispatch | Self::Retry => 1,
            Self::Integration => 2,
        }
    }

    /// The name the refusal messages use.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::Retry => "retry",
            Self::Integration => "integration",
        }
    }
}

/// The process-local provisional-reservation ledger.
///
/// "the sequential substrate asserts at most one", "crash reset: none exist at
/// process start", "process-local ledger balances at process end". Every one of
/// those three is a property of this type rather than a comment: `new` is
/// empty, `take` refuses a second, and `balances` is the process-end check.
///
/// Cancellation is not an error path — it is one of four ordinary outcomes.
/// `cancellation`: "provisional reservations cancelled on pre-append failure or
/// a poisoned fold", and `permits`: "cancellation on any pre-append failure,
/// run end, shutdown, or a poisoned fold".
#[derive(Debug, Default)]
pub struct Reservations {
    held: Option<(TaskKey, ReservationKind)>,
    taken: u32,
    converted: u32,
    cancelled: u32,
}

impl Reservations {
    /// An empty ledger, which is what process start requires.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserve for `key`.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when one is already held.
    pub fn take(&mut self, key: TaskKey, kind: ReservationKind) -> Result<(), UpstrokeError> {
        if let Some((held, held_kind)) = self.held {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "a {} reservation for task {held} is already held; the sequential \
                     substrate asserts at most one, and a {} reservation for task {key} \
                     would be a second",
                    held_kind.name(),
                    kind.name()
                ),
            });
        }
        self.held = Some((key, kind));
        self.taken += 1;
        Ok(())
    }

    /// Convert the reservation at its append.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when nothing is held, or when the held
    /// reservation is another task's or another kind. A conversion that
    /// silently accepted a mismatch is how an entitlement gets counted against
    /// the wrong generation.
    pub fn convert(&mut self, key: TaskKey, kind: ReservationKind) -> Result<(), UpstrokeError> {
        self.settle(key, kind, "converted")?;
        self.converted += 1;
        Ok(())
    }

    /// Cancel it: a pre-append failure, run end, shutdown, or a poisoned fold.
    ///
    /// # Errors
    ///
    /// As [`Self::convert`].
    pub fn cancel(&mut self, key: TaskKey, kind: ReservationKind) -> Result<(), UpstrokeError> {
        self.settle(key, kind, "cancelled")?;
        self.cancelled += 1;
        Ok(())
    }

    /// Cancel whatever is held, if anything, without naming it.
    ///
    /// The append-error protocol's shape: the fold is poisoned and the
    /// coordinator is ending, so it cancels what it holds rather than asserting
    /// what that is. Returns whether anything was held.
    pub fn cancel_any(&mut self) -> bool {
        if self.held.take().is_some() {
            self.cancelled += 1;
            return true;
        }
        false
    }

    fn settle(
        &mut self,
        key: TaskKey,
        kind: ReservationKind,
        verb: &str,
    ) -> Result<(), UpstrokeError> {
        match self.held {
            Some((held, held_kind)) if held == key && held_kind == kind => {
                self.held = None;
                Ok(())
            }
            Some((held, held_kind)) => Err(UpstrokeError::Refused {
                message: format!(
                    "a {} reservation for task {key} was {verb}, but the held one is a {} \
                     reservation for task {held}",
                    kind.name(),
                    held_kind.name()
                ),
            }),
            None => Err(UpstrokeError::Refused {
                message: format!(
                    "a {} reservation for task {key} was {verb} while none is held",
                    kind.name()
                ),
            }),
        }
    }

    /// Whether a reservation is held.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.held.is_none()
    }

    /// The entitlements the held reservation accounts for, zero when none is.
    #[must_use]
    pub fn entitlements_held(&self) -> u32 {
        self.held.map_or(0, |(_, kind)| kind.entitlements())
    }

    /// Whether every reservation was converted or cancelled exactly once.
    #[must_use]
    pub const fn balances(&self) -> bool {
        self.taken == self.converted + self.cancelled && self.held.is_none()
    }
}

// ---------------------------------------------------------------------------
// The invocation ledger
// ---------------------------------------------------------------------------

/// What an invocation's registration is currently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Registration {
    Running,
    Completed,
    Cancelled,
}

/// R4: every Runner process registered exactly once, settled exactly once.
///
/// `permits.protocol`: "the invocation ledger records registered/completed/
/// cancelled exactly once and balances at process end"; and "duplicate
/// complete/cancel ignored and counted", which is why a duplicate is not an
/// error here but a counter — INV-20 asks for "discard with a non-durable
/// warning", not a refusal.
#[derive(Debug, Default)]
pub struct InvocationLedger {
    entries: BTreeMap<String, Registration>,
    duplicates: u32,
}

impl InvocationLedger {
    /// An empty ledger, which is what process start requires.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `invocation` as running.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when this identity is already registered.
    /// That is aliasing (ST-04), not a duplicate completion, and it refuses.
    pub fn register(&mut self, invocation: &InvocationId) -> Result<(), UpstrokeError> {
        let key = invocation.render();
        if self.entries.contains_key(&key) {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "`{key}` is already registered: two processes would share one identity"
                ),
            });
        }
        self.entries.insert(key, Registration::Running);
        Ok(())
    }

    /// Settle `invocation` as completed. A duplicate is counted, not refused.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when `invocation` was never registered.
    pub fn complete(&mut self, invocation: &InvocationId) -> Result<(), UpstrokeError> {
        self.settle(invocation, Registration::Completed)
    }

    /// Settle `invocation` as cancelled. A duplicate is counted, not refused.
    ///
    /// # Errors
    ///
    /// [`UpstrokeError::Refused`] when `invocation` was never registered.
    pub fn cancel(&mut self, invocation: &InvocationId) -> Result<(), UpstrokeError> {
        self.settle(invocation, Registration::Cancelled)
    }

    fn settle(&mut self, invocation: &InvocationId, to: Registration) -> Result<(), UpstrokeError> {
        let key = invocation.render();
        match self.entries.get(&key) {
            None => Err(UpstrokeError::Refused {
                message: format!("`{key}` was settled without ever being registered"),
            }),
            Some(Registration::Running) => {
                self.entries.insert(key, to);
                Ok(())
            }
            Some(_) => {
                self.duplicates += 1;
                Ok(())
            }
        }
    }

    /// Cancel every still-running invocation, returning how many.
    ///
    /// The append-error protocol's "in-flight invocations are cancelled through
    /// the Runner" — this is the ledger half of that; the Runner half is the
    /// caller's.
    pub fn cancel_all_running(&mut self) -> usize {
        let running: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, state)| **state == Registration::Running)
            .map(|(key, _)| key.clone())
            .collect();
        for key in &running {
            self.entries.insert(key.clone(), Registration::Cancelled);
        }
        running.len()
    }

    /// How many duplicate settlements were discarded.
    #[must_use]
    pub const fn duplicates(&self) -> u32 {
        self.duplicates
    }

    /// Whether every registration was settled — the process-end condition.
    #[must_use]
    pub fn balances(&self) -> bool {
        self.entries
            .values()
            .all(|state| *state != Registration::Running)
    }

    /// The identities still running.
    #[must_use]
    pub fn running(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(_, state)| **state == Registration::Running)
            .map(|(key, _)| key.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: TaskKey = TaskKey(3);
    const OTHER: TaskKey = TaskKey(4);
    const GEN: GenerationId = GenerationId(2);

    fn ids(attempt: u32) -> AttemptIdentities {
        AttemptIdentities::new(KEY, GEN, AttemptNumber(attempt))
    }

    fn pair(agent: &str) -> SlotPair {
        SlotPair {
            agent: agent.to_owned(),
            pool: None,
        }
    }

    // --- assignment --------------------------------------------------------

    /// Every identity of one attempt is distinct, and distinct from every
    /// identity of the next attempt.
    ///
    /// ST-04 is "no two … invocations share an InvocationId", and INV-20 adds
    /// "changes with every attempt". Both are asserted over the whole set
    /// rather than pairwise on a sample, because the failure this guards is a
    /// role whose ordinal was forgotten and which therefore collides with its
    /// own neighbour.
    #[test]
    fn every_invocation_of_an_attempt_is_distinct_and_a_retry_reuses_none_of_them() {
        let first = ids(1);
        let retry = ids(2);

        let of = |a: &AttemptIdentities| {
            let mut v = vec![a.worker()];
            for n in 0..3 {
                v.push(a.gate(n, 0));
                v.push(a.gate(n, 1));
                v.push(a.review_pass(n, 0));
                v.push(a.review_reask(n, 0));
            }
            v.into_iter().map(|id| id.render()).collect::<Vec<_>>()
        };

        let a = of(&first);
        let b = of(&retry);
        let unique: std::collections::BTreeSet<&String> = a.iter().chain(b.iter()).collect();
        assert_eq!(
            unique.len(),
            a.len() + b.len(),
            "two invocations share an identity: {a:?} {b:?}"
        );

        // The ordinal is load-bearing: a gate re-dispatched inside one attempt
        // is a new identity, so a completion from the first run cannot apply
        // to the second.
        assert_ne!(first.gate(0, 0), first.gate(0, 1));
        // And the gate number is load-bearing separately from the ordinal.
        assert_ne!(first.gate(0, 1), first.gate(1, 0));
    }

    /// The same tuple renders the same identity in any process.
    ///
    /// "deterministic in the sequential substrate" is what lets a container
    /// name be predicted, and what makes an intent path stable across the
    /// incarnation that wrote it and the one that reclaims it.
    #[test]
    fn an_identity_is_a_pure_function_of_its_tuple() {
        assert_eq!(ids(1).worker(), ids(1).worker());
        assert_eq!(ids(1).gate(2, 3), ids(1).gate(2, 3));
        assert_eq!(
            PreflightIdentities::shell(0).expect("the shell probe"),
            PreflightIdentities::shell(0).expect("the shell probe")
        );
        assert_eq!(
            PreflightIdentities::agent("claude", 0).expect("an agent probe"),
            PreflightIdentities::agent("claude", 0).expect("an agent probe")
        );
    }

    /// A sequence has gates and reviews and no worker, and its identities do
    /// not collide with an attempt's.
    #[test]
    fn a_sequence_has_no_worker_and_shares_no_identity_with_an_attempt() {
        let seq = SequenceIdentities::new(SequenceId(1));
        let attempt = ids(1);
        let rendered: std::collections::BTreeSet<String> = [
            seq.gate(0, 0).render(),
            seq.review_pass(0, 0).render(),
            seq.review_reask(0, 0).render(),
            attempt.gate(0, 0).render(),
            attempt.review_pass(0, 0).render(),
            attempt.review_reask(0, 0).render(),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            rendered.len(),
            6,
            "a sequence identity collided with an attempt's"
        );
    }

    /// Probe identities repeat across incarnations, deliberately.
    ///
    /// This is not a defect to fix here: it is why a container name carries the
    /// coordinator incarnation id. Asserting it keeps the reason visible — a
    /// later change that made probe identities unique per incarnation would
    /// make the incarnation component of a container name dead weight, and this
    /// test is where that shows up.
    #[test]
    fn a_probe_identity_carries_no_epoch_and_therefore_repeats_across_incarnations() {
        let first = PreflightIdentities::agent("claude", 0).expect("an agent probe");
        let second = PreflightIdentities::agent("claude", 0).expect("an agent probe");
        assert_eq!(
            first, second,
            "probe identities repeat by construction; the container name's \
             incarnation component is what separates two epochs' probes"
        );
        assert!(!first.render().contains("01KZT"), "{}", first.render());
    }

    /// An agent name an identity cannot carry is refused, because that identity
    /// becomes a path component and a container-name component.
    #[test]
    fn an_agent_probe_refuses_a_name_that_is_not_a_safe_component() {
        for hostile in ["../escape", "has space", "semi;colon", ""] {
            assert!(
                PreflightIdentities::agent(hostile, 0).is_err(),
                "`{hostile}` was accepted as an agent probe target"
            );
        }
        assert!(PreflightIdentities::agent("claude-code_1", 0).is_ok());
    }

    // --- slot pairs --------------------------------------------------------

    /// One slotted invocation at a time, asserted rather than queued.
    #[test]
    fn a_second_slot_pair_is_refused_rather_than_queued() {
        let mut slots = SlotAssertion::new();
        let worker = ids(1).worker();
        let review = ids(1).review_pass(0, 0);

        assert!(
            slots.is_empty(),
            "a process starts with an empty slot table"
        );
        slots
            .acquire(&worker, pair("claude"))
            .expect("the first pair");
        assert!(slots.holds(&worker));

        let refused = slots
            .acquire(&review, pair("claude"))
            .expect_err("a second concurrent pair must refuse");
        let message = refused.to_string();
        assert!(message.contains("max_parallel = 1"), "{message}");
        assert!(
            slots.holds(&worker),
            "the refusal must not disturb the held pair"
        );

        slots.release(&worker).expect("release the first");
        slots
            .acquire(&review, pair("claude"))
            .expect("then the second");
        slots.release(&review).expect("release the second");
        assert!(slots.balances());
    }

    /// A release naming another invocation is refused, not ignored.
    #[test]
    fn a_release_naming_another_invocation_is_refused() {
        let mut slots = SlotAssertion::new();
        let worker = ids(1).worker();
        let review = ids(1).review_pass(0, 0);
        slots.acquire(&worker, pair("claude")).expect("the pair");

        assert!(slots.release(&review).is_err(), "a stale release applied");
        assert!(slots.holds(&worker));
        assert!(
            !slots.balances(),
            "the ledger cannot balance while a pair is held"
        );

        slots.release(&worker).expect("the real holder");
        assert!(slots.balances());
        assert!(
            slots.release(&worker).is_err(),
            "a second release of a released pair applied"
        );
    }

    // --- provisional reservations ------------------------------------------

    /// One reservation at a time, converted or cancelled exactly once.
    #[test]
    fn a_reservation_is_asserted_singly_and_settles_exactly_once() {
        let mut r = Reservations::new();
        assert!(r.is_empty(), "a process starts with no reservation");
        assert!(r.balances());

        r.take(KEY, ReservationKind::Dispatch).expect("the first");
        assert_eq!(r.entitlements_held(), 1, "dispatch holds {{pipeline}}");
        assert!(
            r.take(OTHER, ReservationKind::Dispatch).is_err(),
            "a second reservation was taken"
        );
        assert!(!r.balances(), "an outstanding reservation cannot balance");

        r.convert(KEY, ReservationKind::Dispatch)
            .expect("converted at its append");
        assert!(r.balances());
        assert!(
            r.convert(KEY, ReservationKind::Dispatch).is_err(),
            "a reservation converted twice"
        );
    }

    /// A settlement naming another task or another kind is refused.
    ///
    /// This is the shape that would count an entitlement against the wrong
    /// generation, which is the accounting INV-22 asks to balance.
    #[test]
    fn a_reservation_settled_under_the_wrong_name_is_refused() {
        let mut r = Reservations::new();
        r.take(KEY, ReservationKind::Retry)
            .expect("a retry reservation");

        assert!(
            r.convert(OTHER, ReservationKind::Retry).is_err(),
            "wrong task"
        );
        assert!(
            r.convert(KEY, ReservationKind::Dispatch).is_err(),
            "wrong kind"
        );
        assert!(
            !r.is_empty(),
            "a refused settlement must not release the hold"
        );

        r.cancel(KEY, ReservationKind::Retry)
            .expect("the right name");
        assert!(r.balances());
    }

    /// The append-error protocol cancels what it holds without naming it.
    #[test]
    fn cancel_any_releases_an_unnamed_reservation_and_reports_whether_there_was_one() {
        let mut r = Reservations::new();
        assert!(!r.cancel_any(), "nothing was held");

        r.take(KEY, ReservationKind::Integration)
            .expect("an integration reservation");
        assert_eq!(
            r.entitlements_held(),
            2,
            "integration holds {{pipeline, merge}}, not {{pipeline}}"
        );
        assert!(
            r.cancel_any(),
            "the poisoned-fold path cancels what it holds"
        );
        assert!(r.balances());
        assert!(!r.cancel_any());
    }

    // --- the invocation ledger ---------------------------------------------

    /// Registered once, settled once; a duplicate settlement is counted, not
    /// refused.
    #[test]
    fn the_invocation_ledger_refuses_aliasing_and_counts_duplicate_settlements() {
        let mut ledger = InvocationLedger::new();
        let worker = ids(1).worker();

        assert!(ledger.balances(), "an empty ledger balances");
        ledger.register(&worker).expect("the first registration");
        assert!(!ledger.balances(), "a running invocation is unsettled");
        assert_eq!(ledger.running(), vec![worker.render()]);

        assert!(
            ledger.register(&worker).is_err(),
            "two processes were given one identity"
        );

        ledger.complete(&worker).expect("settled");
        assert!(ledger.balances());
        assert_eq!(ledger.duplicates(), 0);

        // "duplicate complete/cancel ignored and counted" — INV-20 asks for a
        // discard with a warning, not a refusal.
        ledger
            .complete(&worker)
            .expect("a duplicate is not an error");
        ledger.cancel(&worker).expect("nor is a late cancel");
        assert_eq!(ledger.duplicates(), 2);
        assert!(ledger.balances());
    }

    /// Settling something never registered is refused.
    #[test]
    fn settling_an_unregistered_invocation_is_refused() {
        let mut ledger = InvocationLedger::new();
        let worker = ids(1).worker();
        assert!(ledger.complete(&worker).is_err());
        assert!(ledger.cancel(&worker).is_err());
    }

    /// The append-error protocol's half: every still-running invocation is
    /// cancelled, and the ledger then balances.
    #[test]
    fn cancel_all_running_settles_every_in_flight_invocation() {
        let mut ledger = InvocationLedger::new();
        let attempt = ids(1);
        let worker = attempt.worker();
        let gate = attempt.gate(0, 0);
        let review = attempt.review_pass(0, 0);

        for id in [&worker, &gate, &review] {
            ledger.register(id).expect("registered");
        }
        ledger
            .complete(&gate)
            .expect("one finished before the error");
        assert_eq!(ledger.running().len(), 2);

        assert_eq!(
            ledger.cancel_all_running(),
            2,
            "the two still in flight are cancelled, the finished one is not re-settled"
        );
        assert!(ledger.balances());
        assert!(ledger.running().is_empty());
        assert_eq!(
            ledger.duplicates(),
            0,
            "cancelling a running invocation is not a duplicate settlement"
        );
    }
}
