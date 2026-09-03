//! The observer seam and the funnel protocol itself.
//!
//! `decisions.effect_site_inventory.identity`: "every effectful funnel API
//! takes its group's site by value, and the funnel itself calls
//! `hook(Before, site) -> primitive -> hook(After, site)`, so hooks exist for
//! every site by construction". [`funnel`] is that sentence, written once, and
//! every primitive in the parent goes through it.
//!
//! **The protocol, not the effects.** Nothing here opens a file, starts a
//! process, or touches a path: [`funnel`] takes the primitive as a closure and
//! the parent supplies it, so every effect site in this module tree stays in
//! `src/workspace_manager.rs` where the allowlist row and the reviewed
//! prologue are. [`apply`] can end the process -- `Injection::Kill` aborts --
//! and `std::process::abort` is not a governed primitive: it is what
//! [`crate::agent::proc`] already uses for the same reason.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree rather than by the file, so an out-of-line
// child of `src/workspace_manager.rs` inherits that file's inner
// `#![allow(clippy::disallowed_methods, disallowed_types, disallowed_macros)]`
// unless it says otherwise -- `PR6-LANEF-004`, and the mistake two W1 pull
// requests then made independently (#100 and #102). Nothing here reaches a
// governed primitive, so all three are DENIED and this module takes no
// `effects/allowlist.toml` row: a row records an allowance, and this module
// takes none.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use std::sync::{Arc, Mutex};

use crate::error::UpstrokeError;
use crate::topology::effects::{EffectSiteId, HookHarness, HookPhase, Injection, SubEffectPoint};
use crate::util::DurabilityLedger;

/// What a funnel tells whoever is watching, at both hook phases and at the
/// parent-side sub-effect points.
///
/// The shape mirrors [`crate::agent::proc::SpawnHooks`], which PR4 wired onto
/// the same [`HookHarness`], except that these funnels serve many sites each,
/// so the site travels with the call.
///
/// `phase` takes `&mut self` so that an observer owns its own state outright:
/// the test doubles in the parent's suite count refusals and snapshot ledgers
/// in plain fields, with no lock, because the funnel holds the one mutable
/// borrow for the whole call. Only [`HarnessEffects`] locks, and it locks
/// because the value it records into is shared, not because the trait is.
pub trait EffectHooks {
    /// The funnel reached `phase` of `site`. The answer says what it must do.
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection;

    /// Where this observer wants the funnel's durability primitives recorded.
    ///
    /// A *handle*, taken before the funnel body runs, rather than a method the
    /// body calls back into: `funnel` already holds `&mut dyn EffectHooks` for
    /// the whole call, so a body that also needed the observer would be a
    /// second mutable borrow of it. The handle is cloneable and shares its log,
    /// so what the body records is what the caller reads.
    ///
    /// The default records nothing, which is what production passes and what
    /// every observer that does not care about durability inherits.
    fn durability_ledger(&self) -> DurabilityLedger {
        DurabilityLedger::off()
    }
}

/// What production passes: nothing is armed and nothing is recorded.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl EffectHooks for NoHooks {
    fn phase(&mut self, _site: EffectSiteId, _phase: HookPhase) -> Injection {
        Injection::Proceed
    }
}

/// Wires these funnels onto PR3's [`HookHarness`], the way
/// [`crate::runner::HarnessHooks`] wires the process funnel onto it.
///
/// **Why the harness is shared.** One [`HookHarness`] is the coverage record
/// of a whole suite. The test that owns it reads it after every observer that
/// recorded into it has been dropped, and the coverage suites wire one
/// harness onto all five funnel families at once (the engine's topology
/// scaffold builds it that way), so the record must outlive each of its
/// holders individually. That is the multi-owner lifecycle the `Arc` exists
/// for. The `Mutex` belongs to the harness rather than to this observer:
/// every holder locks the same way, and what the lock protects is stated at
/// this type's [`EffectHooks::phase`].
///
/// Cloning shares both the harness and the ledger, for the reason
/// [`DurabilityLedger`] gives: a test hands a clone into a funnel and still
/// reads what the funnel recorded. `Default` is an observer on a fresh
/// harness that only [`Self::harness`] can reach.
#[derive(Debug, Clone, Default)]
pub struct HarnessEffects {
    harness: Arc<Mutex<HookHarness>>,
    ledger: DurabilityLedger,
}

impl HarnessEffects {
    /// Observe through `harness`.
    ///
    /// Durability recording starts **off**, as it does for every sibling
    /// adapter: a recording ledger costs an allocation per primitive, and only
    /// a test that asked for one wants it.
    #[must_use]
    pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {
        Self {
            harness,
            ledger: DurabilityLedger::off(),
        }
    }

    /// The harness this observer records into.
    #[must_use]
    pub fn harness(&self) -> &Arc<Mutex<HookHarness>> {
        &self.harness
    }

    /// Also record every durability primitive the funnels perform.
    #[must_use]
    pub fn recording_durability(mut self) -> Self {
        self.ledger = DurabilityLedger::recording();
        self
    }

    /// The durability ledger this observer records into.
    ///
    /// A handle clone: the ledger is an optional shared log and cloning it
    /// shares that log, which is how the caller reads what a funnel wrote.
    #[must_use]
    pub fn ledger(&self) -> DurabilityLedger {
        self.ledger.clone()
    }
}

impl EffectHooks for HarnessEffects {
    /// One `hook` call under the harness's lock, or a refusal if the lock is
    /// poisoned.
    ///
    /// **What the lock protects.** `HookHarness::hook` writes up to three of
    /// the harness's records in one call (the open fast sequence's touched
    /// sites, the reached points, the observed phases), and the other
    /// holders of the same lock arm and disarm injections and open and close
    /// fast sequences. The lock makes each of those one step: the ordering
    /// tests read `coverage()` by position, and an arming or a sequence
    /// boundary must not land between the fields a single `hook` call
    /// writes. The critical section is the one call, and the guard is
    /// dropped before [`funnel`] runs the primitive, which is where the
    /// ledger takes its own lock. The two locks are never held together, so
    /// there is no acquisition order to keep.
    ///
    /// **A poisoned harness refuses.** Poison means another thread panicked
    /// while holding the harness, and what it left cannot be trusted: a
    /// `hook` call may have written some of its fields and not the rest, and
    /// a fast sequence the panicking holder opened stays open, so anything
    /// recorded next would be attributed to it. Recording into that would
    /// manufacture coverage evidence, and this is production code that may
    /// not panic, so the observer answers [`Injection::Error`] and the funnel
    /// returns a refusal naming the site and the phase instead of running or
    /// completing the primitive. The test driving the funnel fails there,
    /// which is the defined outcome a worker panic must have; the panic
    /// itself is reported by whoever joins the worker.
    fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        match self.harness.lock() {
            Ok(mut harness) => harness.hook(site, phase),
            Err(_poisoned) => Injection::Error,
        }
    }

    /// The same handle clone as [`Self::ledger`].
    fn durability_ledger(&self) -> DurabilityLedger {
        self.ledger.clone()
    }
}

/// Do what a hook answered.
///
/// [`Injection::Kill`] aborts, for the reason
/// [`crate::agent::proc`] already gives: the claim under test is what a
/// coordinator that dies **without running any cleanup** leaves durable, and
/// both `panic!` and `std::process::exit` run destructors.
///
/// [`Injection::Error`] refuses with the site and the phase that answered.
/// The refusal is an injected fault, so the variant is the one every funnel
/// family uses for it; a caller can only report it, and the message is the
/// whole of what there is to report.
pub(super) fn apply(
    injection: Injection,
    site: EffectSiteId,
    phase: HookPhase,
) -> Result<(), UpstrokeError> {
    match injection {
        Injection::Proceed => Ok(()),
        Injection::Kill => std::process::abort(),
        Injection::Error => Err(UpstrokeError::Refused {
            message: format!("the `{site}` funnel was made to fail at its `{phase}` phase"),
        }),
    }
}

/// `hook(Before, site) -> primitive -> hook(After, site)`, once.
///
/// Three early returns, each deliberate:
///
/// * **A refusal at `Before`** returns before the primitive runs, so the
///   effect never happens. The error names the site and the phase, and this
///   function has nothing to add.
/// * **An `Err` from the primitive** is returned as the parent made it. The
///   closure is the parent's, and its error already carries the path or the
///   Git message the parent chose at the source; wrapping it here would hide
///   the variant the parent's callers match on. `After` is then **not**
///   consulted, so the harness never records `After` for an effect that did
///   not complete.
/// * **A refusal at `After`** is returned after the primitive returned
///   `Ok`, and the result is dropped. That is all error-return mode
///   promises ([`crate::topology::effects::InjectionMode::ErrorReturn`]:
///   performed or partially performed, then `Err`). Whether anything became
///   durable is the primitive's business, not this function's: for a
///   read-only site such as `Worktree.Verify` nothing did, and the refusal
///   still fires there because a read-only site is still a site whose
///   phases are observed. The `?` stands because the caller can act on the
///   error exactly as it is. It names the site and the phase, and what the
///   caller does under this injection is treat the result as lost and
///   examine what the primitive left behind, which is the residue the fault
///   exists to produce; a funnel that reported "durable" would be claiming
///   something it cannot see. No funnel in the parent returns a guard, so
///   dropping the result undoes nothing.
pub(super) fn funnel<T, F>(
    hooks: &mut dyn EffectHooks,
    site: EffectSiteId,
    primitive: F,
) -> Result<T, UpstrokeError>
where
    F: FnOnce() -> Result<T, UpstrokeError>,
{
    apply(
        hooks.phase(site, HookPhase::Before),
        site,
        HookPhase::Before,
    )?;
    let value = primitive()?;
    apply(hooks.phase(site, HookPhase::After), site, HookPhase::After)?;
    Ok(value)
}

/// Consult a parent-side sub-effect point, in every mode the point declares.
///
/// The harness is keyed by `(site, point, mode)` because "a mode is executed
/// when its fault fired", so one funnel position consults it once per declared
/// mode and the first non-`Proceed` answer wins. Every mode is consulted even
/// after a decision, so the harness records the point as reached in each.
/// The decision is then applied at the coordinate that gave it, so a refusal
/// names the mode that fired rather than a fixed one.
/// [`SubEffectPoint::IdUnread`] declares `Kill` alone, so in practice this is
/// one call — but the loop is over [`SubEffectPoint::modes`] rather than over
/// a literal, so a point that gains a mode is consulted for it.
pub(super) fn point(
    hooks: &mut dyn EffectHooks,
    site: EffectSiteId,
    at: SubEffectPoint,
) -> Result<(), UpstrokeError> {
    let mut decision: Option<(Injection, HookPhase)> = None;
    for mode in at.modes() {
        let phase = HookPhase::Point {
            point: at,
            mode: *mode,
        };
        let answer = hooks.phase(site, phase);
        if decision.is_none() && answer != Injection::Proceed {
            decision = Some((answer, phase));
        }
    }
    match decision {
        Some((injection, phase)) => apply(injection, site, phase),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::topology::effects::{InjectionMode, WorktreeSite};
    use crate::util::DurableStep;

    /// Any site will do: the protocol under test does not read it, and the
    /// observer below asserts that the funnel passes it through unchanged.
    const SITE: EffectSiteId = EffectSiteId::Worktree(WorktreeSite::Add);

    /// A point that declares both modes, so the first-answer-wins rule has
    /// two coordinates to choose between. `IdUnread`, the one point these
    /// funnels reach in the parent, declares one and cannot exercise it.
    const TWO_MODES: SubEffectPoint = SubEffectPoint::Written;

    /// Answers `Injection::Error` at the scripted phases and `Proceed`
    /// everywhere else, and records every phase it was asked about, in order.
    struct Scripted {
        refuse_at: Vec<HookPhase>,
        asked: Vec<HookPhase>,
    }

    impl Scripted {
        fn proceeding() -> Self {
            Self::refusing_at_each(&[])
        }

        fn refusing_at(phase: HookPhase) -> Self {
            Self::refusing_at_each(&[phase])
        }

        fn refusing_at_each(phases: &[HookPhase]) -> Self {
            Self {
                refuse_at: phases.to_vec(),
                asked: Vec::new(),
            }
        }
    }

    impl EffectHooks for Scripted {
        fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
            assert_eq!(site, SITE, "the funnel consulted a site it was not given");
            self.asked.push(phase);
            if self.refuse_at.contains(&phase) {
                Injection::Error
            } else {
                Injection::Proceed
            }
        }
    }

    fn refusal(outcome: Result<(), UpstrokeError>) -> String {
        match outcome {
            Ok(()) => panic!("the funnel proceeded where a refusal was scripted"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn a_refusal_at_before_runs_no_primitive_and_never_consults_after() {
        let mut hooks = Scripted::refusing_at(HookPhase::Before);
        let mut ran = false;
        let message = refusal(funnel(&mut hooks, SITE, || {
            ran = true;
            Ok(())
        }));
        assert!(!ran, "the primitive ran after Before refused");
        assert_eq!(hooks.asked, vec![HookPhase::Before]);
        assert!(
            message.contains("`before` phase"),
            "the refusal names the phase that answered: {message}"
        );
    }

    #[test]
    fn a_primitive_that_fails_returns_its_own_error_and_after_is_not_claimed() {
        let mut hooks = Scripted::proceeding();
        let outcome: Result<(), UpstrokeError> = funnel(&mut hooks, SITE, || {
            Err(UpstrokeError::Refused {
                message: "the primitive's own error".to_owned(),
            })
        });
        assert_eq!(
            refusal(outcome),
            "the primitive's own error",
            "the funnel returned the primitive's error unchanged"
        );
        assert_eq!(
            hooks.asked,
            vec![HookPhase::Before],
            "After was consulted for an effect that did not complete"
        );
    }

    /// The primitive here performs no effect, which is the `Worktree.Verify`
    /// shape: `Proceed` at `Before`, `Ok` from a read-only body, `Error` at
    /// `After`. The funnel refuses and drops the result all the same, and
    /// nothing about durability is claimed or checked, because nothing is
    /// there to check.
    #[test]
    fn a_refusal_at_after_is_returned_after_the_primitive_ran() {
        let mut hooks = Scripted::refusing_at(HookPhase::After);
        let mut ran = false;
        let message = refusal(funnel(&mut hooks, SITE, || {
            ran = true;
            Ok(())
        }));
        assert!(ran, "After refused before the primitive ran");
        assert_eq!(hooks.asked, vec![HookPhase::Before, HookPhase::After]);
        assert!(
            message.contains("`after` phase"),
            "the refusal names the phase that answered: {message}"
        );
    }

    #[test]
    fn a_proceeding_funnel_returns_the_primitives_value() {
        let mut hooks = Scripted::proceeding();
        let value = funnel(&mut hooks, SITE, || Ok(7)).expect("nothing refused");
        assert_eq!(value, 7);
        assert_eq!(hooks.asked, vec![HookPhase::Before, HookPhase::After]);
    }

    #[test]
    fn a_point_consults_every_mode_and_applies_the_refusal_at_the_mode_that_answered() {
        let kill = HookPhase::Point {
            point: TWO_MODES,
            mode: InjectionMode::Kill,
        };
        let error_return = HookPhase::Point {
            point: TWO_MODES,
            mode: InjectionMode::ErrorReturn,
        };
        assert_eq!(
            TWO_MODES.modes(),
            &[InjectionMode::Kill, InjectionMode::ErrorReturn],
            "the point under test no longer declares both modes in this order"
        );

        let mut hooks = Scripted::refusing_at(error_return);
        let message = refusal(point(&mut hooks, SITE, TWO_MODES));
        assert_eq!(
            hooks.asked,
            vec![kill, error_return],
            "every declared mode is consulted, in declaration order"
        );
        assert!(
            message.contains("/error-return` phase") && !message.contains("/kill"),
            "the refusal names the mode that answered, not a fixed one: {message}"
        );

        // Both modes answer `Error`: the first declared mode wins, and the
        // second is still consulted so the harness records it as reached. A
        // `point` that let the last answer win would name `/error-return`.
        let mut hooks = Scripted::refusing_at_each(&[kill, error_return]);
        let message = refusal(point(&mut hooks, SITE, TWO_MODES));
        assert_eq!(
            hooks.asked,
            vec![kill, error_return],
            "a decision does not stop the remaining modes being consulted"
        );
        assert!(
            message.contains("/kill` phase") && !message.contains("/error-return"),
            "the first non-Proceed answer wins, not the last: {message}"
        );

        let mut hooks = Scripted::proceeding();
        point(&mut hooks, SITE, TWO_MODES).expect("nothing armed");
        assert_eq!(hooks.asked, vec![kill, error_return]);
    }

    /// A worker panics while holding the harness, mid-way through opening a
    /// fast sequence. The observer must not record into what it left.
    #[test]
    fn a_poisoned_harness_refuses_rather_than_recording_into_it() {
        let shared = Arc::new(Mutex::new(HookHarness::new()));
        let poisoner = Arc::clone(&shared);
        let worker = std::thread::spawn(move || {
            let mut harness = poisoner.lock().expect("not yet poisoned");
            harness.begin_fast_sequence("abandoned");
            panic!("a worker dies while holding the harness");
        });
        assert!(
            worker.join().is_err(),
            "the worker's panic is its join error, and nothing else"
        );
        assert!(shared.is_poisoned(), "the harness is poisoned");

        let mut hooks = HarnessEffects::new(Arc::clone(&shared));
        assert_eq!(hooks.phase(SITE, HookPhase::Before), Injection::Error);
        let mut ran = false;
        let message = refusal(funnel(&mut hooks, SITE, || {
            ran = true;
            Ok(())
        }));
        assert!(!ran, "the primitive ran against a poisoned harness");
        assert!(
            message.contains("`before` phase"),
            "the refusal names the phase at which the harness was found poisoned: {message}"
        );
    }

    #[test]
    fn no_hooks_proceeds_everywhere_and_records_nothing() {
        let mut hooks = NoHooks;
        let value = funnel(&mut hooks, SITE, || Ok("value")).expect("nothing is armed");
        assert_eq!(value, "value");
        point(&mut hooks, SITE, TWO_MODES).expect("nothing is armed");
        assert!(!hooks.durability_ledger().is_recording());
    }

    /// The two §6 claims the type makes, each driven so that a `Clone` that
    /// produced an independent harness or ledger would fail it: the original
    /// observer is dropped before its clone records, what the original's
    /// ledger handle wrote is read through the clone, and the harness is read
    /// only after every observer is gone.
    #[test]
    fn a_clone_shares_the_harness_and_the_ledger_and_the_harness_outlives_every_observer() {
        let shared = Arc::new(Mutex::new(HookHarness::new()));
        let mut hooks = HarnessEffects::new(Arc::clone(&shared));
        funnel(&mut hooks, SITE, || Ok(())).expect("nothing is armed");
        assert!(
            !hooks.durability_ledger().is_recording(),
            "recording starts off"
        );

        let original = hooks.recording_durability();
        let mut clone = original.clone();
        let handle = original.durability_ledger();
        drop(original);
        funnel(&mut clone, SITE, || {
            handle.record(DurableStep::Renamed, Path::new("staged"), 0);
            Ok(())
        })
        .expect("nothing is armed");
        assert_eq!(
            clone.ledger().steps(),
            vec![DurableStep::Renamed],
            "what the original's handle recorded is read through the clone"
        );
        drop(clone);

        assert_eq!(
            Arc::strong_count(&shared),
            1,
            "every observer is gone and the test's Arc is the last holder"
        );
        let harness = shared.lock().expect("the harness outlives its observers");
        assert_eq!(
            harness.count(SITE, HookPhase::Before),
            2,
            "the original and the clone recorded into one harness"
        );
        assert_eq!(harness.count(SITE, HookPhase::After), 2);
    }
}
