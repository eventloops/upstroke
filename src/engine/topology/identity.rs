//! Extended notes: `docs/internals/engine/topology/identity.md`

use std::collections::BTreeMap;

use crate::error::UpstrokeError;
use crate::runner::invocation::{AttemptRole, SequenceRole};
use crate::runner::{AgentId, InvocationId, ProbeTarget};
use crate::topology::events::{AttemptNumber, GenerationId, SequenceId};
use crate::topology::registry::TaskKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptIdentities {
    key: TaskKey,
    generation: GenerationId,
    attempt: AttemptNumber,
}

impl AttemptIdentities {
    #[must_use]
    pub const fn new(key: TaskKey, generation: GenerationId, attempt: AttemptNumber) -> Self {
        Self {
            key,
            generation,
            attempt,
        }
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceIdentities {
    sequence: SequenceId,
}

impl SequenceIdentities {
    #[must_use]
    pub const fn new(sequence: SequenceId) -> Self {
        Self { sequence }
    }

    #[must_use]
    pub const fn gate(&self, gate: u32, ordinal: u32) -> InvocationId {
        InvocationId::sequence(self.sequence, SequenceRole::Gate(gate), ordinal)
    }

    #[must_use]
    pub const fn review_pass(&self, pass: u32, ordinal: u32) -> InvocationId {
        InvocationId::sequence(self.sequence, SequenceRole::ReviewPass(pass), ordinal)
    }

    #[must_use]
    pub const fn review_reask(&self, reask: u32, ordinal: u32) -> InvocationId {
        InvocationId::sequence(self.sequence, SequenceRole::ReviewReask(reask), ordinal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreflightIdentities;

impl PreflightIdentities {
    pub fn shell(ordinal: u32) -> Result<InvocationId, UpstrokeError> {
        InvocationId::probe(ProbeTarget::Shell, ordinal)
    }

    pub fn agent(agent: &str, ordinal: u32) -> Result<InvocationId, UpstrokeError> {
        InvocationId::probe(ProbeTarget::Agent(AgentId::new(agent)), ordinal)
    }
}

#[derive(Debug, Default)]
pub struct SlotAssertion {
    held: Option<(InvocationId, SlotPair)>,
    granted: u32,
    released: u32,
}

#[must_use]
pub fn is_slotted(invocation: &InvocationId) -> bool {
    match invocation {
        InvocationId::Attempt { role, .. } => !matches!(role, AttemptRole::Gate(_)),
        InvocationId::Sequence { role, .. } => !matches!(role, SequenceRole::Gate(_)),
        InvocationId::Probe { target, .. } => matches!(target, ProbeTarget::Agent(_)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotPair {
    pub agent: String,
    pub pool: Option<String>,
}

impl SlotAssertion {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(
        &mut self,
        invocation: &InvocationId,
        pair: SlotPair,
    ) -> Result<(), UpstrokeError> {
        if !is_slotted(invocation) {
            return Err(UpstrokeError::Refused {
                message: format!(
                    "`{invocation}` is a gate or the shell probe and acquires no slot: \
                     `permits.agent_pool_slots` excludes both by name"
                ),
            });
        }
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

    #[must_use]
    pub fn pair_of(&self, invocation: &InvocationId) -> Option<&SlotPair> {
        self.held
            .as_ref()
            .filter(|(held, _)| held == invocation)
            .map(|(_, pair)| pair)
    }

    #[must_use]
    pub fn held(&self) -> Option<&InvocationId> {
        self.held.as_ref().map(|(invocation, _)| invocation)
    }

    #[must_use]
    pub fn holds(&self, invocation: &InvocationId) -> bool {
        self.held
            .as_ref()
            .is_some_and(|(held, _)| held == invocation)
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.held.is_none()
    }

    #[must_use]
    pub const fn balances(&self) -> bool {
        self.granted == self.released && self.held.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReservationKind {
    Dispatch,
    Retry,
    Integration,
}

impl ReservationKind {
    #[must_use]
    pub const fn entitlements(self) -> u32 {
        match self {
            Self::Dispatch | Self::Retry => 1,
            Self::Integration => 2,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::Retry => "retry",
            Self::Integration => "integration",
        }
    }
}

#[derive(Debug, Default)]
pub struct Reservations {
    held: Option<(TaskKey, ReservationKind)>,
    taken: u32,
    converted: u32,
    cancelled: u32,
}

impl Reservations {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

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

    pub fn convert(&mut self, key: TaskKey, kind: ReservationKind) -> Result<(), UpstrokeError> {
        self.settle(key, kind, "converted")?;
        self.converted += 1;
        Ok(())
    }

    pub fn cancel(&mut self, key: TaskKey, kind: ReservationKind) -> Result<(), UpstrokeError> {
        self.settle(key, kind, "cancelled")?;
        self.cancelled += 1;
        Ok(())
    }

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

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.held.is_none()
    }

    #[must_use]
    pub fn entitlements_held(&self) -> u32 {
        self.held.map_or(0, |(_, kind)| kind.entitlements())
    }

    #[must_use]
    pub const fn balances(&self) -> bool {
        self.taken == self.converted + self.cancelled && self.held.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Registration {
    Running,
    Completed,
    Cancelled,
}

#[derive(Debug, Default)]
pub struct InvocationLedger {
    entries: BTreeMap<String, Registration>,
    duplicates: u32,
}

impl InvocationLedger {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

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

    pub fn complete(&mut self, invocation: &InvocationId) -> Result<(), UpstrokeError> {
        self.settle(invocation, Registration::Completed)
    }

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

    #[must_use]
    pub const fn duplicates(&self) -> u32 {
        self.duplicates
    }

    #[must_use]
    pub fn completed(&self) -> usize {
        self.count(Registration::Completed)
    }

    #[must_use]
    pub fn cancelled(&self) -> usize {
        self.count(Registration::Cancelled)
    }

    fn count(&self, state: Registration) -> usize {
        self.entries.values().filter(|seen| **seen == state).count()
    }

    #[must_use]
    pub fn balances(&self) -> bool {
        self.entries
            .values()
            .all(|state| *state != Registration::Running)
    }

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

        assert_ne!(first.gate(0, 0), first.gate(0, 1));
        assert_ne!(first.gate(0, 1), first.gate(1, 0));
    }

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

    #[test]
    fn a_gate_and_the_shell_probe_are_refused_a_slot_pair() {
        let attempt = ids(1);
        let seq = SequenceIdentities::new(SequenceId(1));

        for (label, id) in [
            ("an attempt's gate", attempt.gate(0, 0)),
            ("a sequence's gate", seq.gate(0, 0)),
            (
                "the shell probe",
                PreflightIdentities::shell(0).expect("the shell probe"),
            ),
        ] {
            assert!(!is_slotted(&id), "{label} was classified as slotted");
            let mut slots = SlotAssertion::new();
            let refused = slots
                .acquire(&id, pair("claude"))
                .expect_err("{label} was given a slot pair");
            assert!(
                refused.to_string().contains("acquires no slot"),
                "{label}: {refused}"
            );
            assert!(slots.is_empty(), "{label} left a pair held");
            assert!(slots.balances());
        }

        for (label, id) in [
            ("the worker", attempt.worker()),
            ("a review pass", attempt.review_pass(0, 0)),
            ("a re-ask", attempt.review_reask(0, 0)),
            (
                "an agent probe",
                PreflightIdentities::agent("claude", 0).expect("an agent probe"),
            ),
        ] {
            assert!(is_slotted(&id), "{label} was classified as non-slotted");
            let mut slots = SlotAssertion::new();
            slots
                .acquire(&id, pair("claude"))
                .unwrap_or_else(|error| panic!("{label} was refused a slot pair: {error}"));
            assert_eq!(
                slots.pair_of(&id).map(|p| p.agent.as_str()),
                Some("claude"),
                "{label} held a pair the table cannot name"
            );
            slots.release(&id).expect("released");
        }
    }

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

        ledger
            .complete(&worker)
            .expect("a duplicate is not an error");
        ledger.cancel(&worker).expect("nor is a late cancel");
        assert_eq!(ledger.duplicates(), 2);
        assert!(ledger.balances());
    }

    #[test]
    fn settling_an_unregistered_invocation_is_refused() {
        let mut ledger = InvocationLedger::new();
        let worker = ids(1).worker();
        assert!(ledger.complete(&worker).is_err());
        assert!(ledger.cancel(&worker).is_err());
    }

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
