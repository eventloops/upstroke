//! The hook harness: the phases a fault can be injected at, what the harness
//! records, and the fast-sequence declaration.
//!
//! Split out of `topology::effects`; the parent re-exports every item here, so
//! `crate::topology::effects::HookHarness` and its siblings are unchanged
//! paths.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::EffectSiteId;
use super::vocab::{InjectionMode, SubEffectPoint};

// ---------------------------------------------------------------------------
// The hook harness
// ---------------------------------------------------------------------------

/// A phase at which the parent executes a hook.
///
/// There is deliberately no residue-class variant. A residue class is not an
/// executed hook, and the type is the first of the two places this framework
/// says so — the second is [`FaultRegistry::insert`](crate::topology::effects::FaultRegistry::insert), which refuses an entry
/// that claims otherwise even though this type made the claim unsayable to the
/// harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HookPhase {
    /// Before the primitive.
    Before,
    /// After the primitive.
    After,
    /// At a parent-side sub-effect point, in one injection mode.
    Point {
        /// Which point.
        point: SubEffectPoint,
        /// Which mode the injection is armed in.
        mode: InjectionMode,
    },
}

impl HookPhase {
    /// The two hook phases every site has.
    pub const PHASES: &'static [Self] = &[Self::Before, Self::After];
}

impl fmt::Display for HookPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Before => f.write_str("before"),
            Self::After => f.write_str("after"),
            Self::Point { point, mode } => write!(
                f,
                "{point}/{}",
                match mode {
                    InjectionMode::Kill => "kill",
                    InjectionMode::ErrorReturn => "error-return",
                }
            ),
        }
    }
}

/// What a funnel must do when it returns from a hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Injection {
    /// Nothing is armed here: carry on.
    Proceed,
    /// Die at this point.
    Kill,
    /// Return `Err` from this point.
    Error,
}

/// One `(site, phase)` the harness saw executed, and how often.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// The site whose funnel called the hook.
    pub site: EffectSiteId,
    /// The phase it called it at.
    pub phase: HookPhase,
    /// How many times.
    pub count: u32,
}

/// Why the harness refused to arm an injection.
///
/// Every field is the typed value the caller passed, the site included.
/// `DESIGN.md` §26's "diagnostics are typed" reserves free text for a
/// document's own words — a resume action in the fault matrix's wording, the
/// name a suite gave a fast sequence, what a hand-edited entry wrote in the
/// field the format refused — and an arming is a Rust call with no document
/// in it. Rendering the site to a `String` here made the one field a caller
/// might match on the one field it would have to parse; the message text is
/// unchanged, because that rendering was [`EffectSiteId`]'s own `Display`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HarnessError {
    #[error(
        "`{site}` exposes no parent-side sub-effect point `{point}`; arming one would record an \
         execution of a point that does not exist"
    )]
    NoSuchPoint {
        /// The site.
        site: EffectSiteId,
        /// The point that was asked for.
        point: SubEffectPoint,
    },

    #[error("`{site}`'s `{point}` point does not support {mode:?} injection")]
    UnsupportedMode {
        /// The site.
        site: EffectSiteId,
        /// The point.
        point: SubEffectPoint,
        /// The mode that was asked for.
        mode: InjectionMode,
    },
}

/// Records what the funnels actually executed.
///
/// The whole value of this type is negative: it can only report an execution
/// that a funnel told it about by calling [`Self::hook`]. Arming an injection
/// records nothing, because an armed injection that never fired is exactly the
/// case a coverage report must not count. A harness that recorded at arming
/// time would report full coverage for a suite that never reached a single
/// site.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookHarness {
    armed: Vec<(EffectSiteId, SubEffectPoint, InjectionMode)>,
    /// What executed: both hook phases, and the injected modes that fired.
    observed: Vec<Observation>,
    /// What a funnel walked past at a point, whether or not anything fired.
    reached: Vec<Observation>,
    /// The fast integration sequences the suite exercised, in order.
    fast: Vec<FastSequence>,
    /// Whether the sequence begun last is still recording.
    ///
    /// The open sequence is always the last of `fast`: `begin_fast_sequence`
    /// closes the previous one and then pushes, and nothing else adds to the
    /// vector. A flag beside `last_mut()` says that structurally, where the
    /// index this used to hold had to be *kept* true — and an index that went
    /// stale would either record into the wrong sequence or be defended
    /// against on every hook, which is a thing a reader has to check rather
    /// than read.
    recording: bool,
}

/// One exercised fast integration sequence, and every site its funnels ran.
///
/// ST-07's no-execution claim is "no staging, cherry-pick, or prepared-pin
/// site executed **for any fast sequence**" — a statement about traces, not a
/// statement about a process. A harness that had run nothing satisfies "the
/// site was never touched" trivially, so the absence has to be proved *inside*
/// a sequence that demonstrably happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FastSequence {
    name: String,
    touched: Vec<EffectSiteId>,
}

impl FastSequence {
    /// What the suite called this sequence.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every site whose funnel ran during it, in first-execution order.
    pub fn touched(&self) -> &[EffectSiteId] {
        &self.touched
    }

    /// Whether `site` ran during this sequence.
    pub fn ran(&self, site: EffectSiteId) -> bool {
        self.touched.contains(&site)
    }

    /// Record that `site`'s funnel ran inside this sequence.
    ///
    /// Once per site, in first-execution order: a sequence answers *whether* a
    /// site ran in it ([`Self::ran`]), and how often it ran is the harness's
    /// count, not the trace's.
    fn touch(&mut self, site: EffectSiteId) {
        if !self.touched.contains(&site) {
            self.touched.push(site);
        }
    }
}

impl HookHarness {
    /// A harness that has armed nothing and seen nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Arm an injection at one point of one site.
    ///
    /// Refuses a point the site does not declare and a mode that point does
    /// not declare support for, so a suite cannot quietly arm a fault the
    /// inventory says no funnel of that site consults.
    ///
    /// Which host a point exists on is deliberately not one of the refusals.
    /// Arming is host-agnostic: [`SubEffectPoint::platform`] is read by
    /// [`check_bijection`](crate::topology::effects::check_bijection) against
    /// the host it is given, and a suite drives a platform's point contract
    /// through a funnel fake on whatever host it runs — as
    /// `every_family_of_the_harness_bundle_records_into_the_same_harness`
    /// (`src/engine/topology/seams.rs`) does, arming a Windows-only point and
    /// asserting the injection it returns, on every host.
    ///
    /// # Errors
    ///
    /// [`HarnessError::NoSuchPoint`] if `site` does not expose `point`, and
    /// [`HarnessError::UnsupportedMode`] if `point` does not support `mode`.
    pub fn arm(
        &mut self,
        site: EffectSiteId,
        point: SubEffectPoint,
        mode: InjectionMode,
    ) -> Result<(), HarnessError> {
        if !site.sub_effects().contains(&point) {
            return Err(HarnessError::NoSuchPoint { site, point });
        }
        if !point.supports(mode) {
            return Err(HarnessError::UnsupportedMode { site, point, mode });
        }
        if !self.armed.contains(&(site, point, mode)) {
            self.armed.push((site, point, mode));
        }
        Ok(())
    }

    /// Disarm every injection, keeping everything already observed.
    pub fn disarm(&mut self) {
        self.armed.clear();
    }

    /// The call a funnel makes. Answers what to do, and records an execution
    /// only of what actually happened.
    ///
    /// The two are not the same claim, and the difference is the whole reason
    /// this type exists.
    /// `fault_injection_registry.completeness_rule` requires every point to be
    /// "observed executed at least once by the suite **in every injection mode
    /// it supports**", and a mode is executed when its fault fired — not when
    /// a funnel walked past the place it would have fired. A harness that
    /// counted the walk-past would report both modes of every point covered
    /// for a suite that armed nothing, which is the same false report as
    /// counting at arming time, one step later.
    ///
    /// So: `Before` and `After` are reachability and are counted whenever the
    /// funnel calls them; a `Point` is counted only when that exact `(site,
    /// point, mode)` was armed and therefore returns its specified `Kill` or
    /// `Error`. Reachability of a point in the generic sense is
    /// [`Self::reached`], which is recorded separately and is never what the
    /// bijection reads.
    pub fn hook(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection {
        if let Some(sequence) = self.open_sequence() {
            sequence.touch(site);
        }
        let injection = match phase {
            HookPhase::Before | HookPhase::After => Injection::Proceed,
            HookPhase::Point { point, mode } => {
                if self.armed.contains(&(site, point, mode)) {
                    match mode {
                        InjectionMode::Kill => Injection::Kill,
                        InjectionMode::ErrorReturn => Injection::Error,
                    }
                } else {
                    Injection::Proceed
                }
            }
        };
        if matches!(phase, HookPhase::Point { .. }) {
            // The phase as given, not one rebuilt from its own fields: a
            // rebuild is a second spelling of the coordinate that can drift
            // from the one the injection was decided by.
            Self::record(&mut self.reached, site, phase);
            if injection == Injection::Proceed {
                // Reached, and nothing was injected. Recorded as reachability
                // and as nothing else.
                return injection;
            }
        }
        Self::record(&mut self.observed, site, phase);
        injection
    }

    /// The sequence still recording, if one is open.
    fn open_sequence(&mut self) -> Option<&mut FastSequence> {
        if self.recording {
            self.fast.last_mut()
        } else {
            None
        }
    }

    fn record(into: &mut Vec<Observation>, site: EffectSiteId, phase: HookPhase) {
        match into
            .iter_mut()
            .find(|seen| seen.site == site && seen.phase == phase)
        {
            Some(seen) => seen.count = seen.count.saturating_add(1),
            None => into.push(Observation {
                site,
                phase,
                count: 1,
            }),
        }
    }

    /// Begin recording an exact-base fast integration sequence under `name`.
    ///
    /// Everything a funnel hooks until [`Self::end_fast_sequence`] is recorded
    /// as having run inside this sequence, which is what a no-execution entry
    /// is measured against. A second `begin` closes the first.
    /// A name is a label the suite chose and not an identity: beginning one
    /// twice records two sequences, so a second run under a name that saw
    /// something is still held to having seen something of its own.
    pub fn begin_fast_sequence(&mut self, name: &str) {
        self.end_fast_sequence();
        self.fast.push(FastSequence {
            name: name.to_owned(),
            touched: Vec::new(),
        });
        self.recording = true;
    }

    /// Stop recording the open fast sequence, keeping what it saw.
    ///
    /// A no-op when none is open, and it does not end the *run*: a later
    /// [`Self::hook`] is recorded as coverage, in no sequence.
    pub fn end_fast_sequence(&mut self) {
        self.recording = false;
    }

    /// Every fast sequence the suite exercised.
    pub fn fast_sequences(&self) -> &[FastSequence] {
        &self.fast
    }

    /// The first fast sequence recorded under this name, if there is one.
    ///
    /// First, because a name is a label rather than an identity: a suite that
    /// begins one twice has two sequences and this answers about the earlier.
    /// Anything holding a claim *to* the traces reads [`Self::fast_sequences`]
    /// and holds it to every one of them, which is what
    /// [`check_bijection`](crate::topology::effects::check_bijection) does with
    /// a no-execution record; this is for a caller that has one name in hand.
    pub fn fast_sequence(&self, name: &str) -> Option<&FastSequence> {
        self.fast.iter().find(|sequence| sequence.name == name)
    }

    /// Every `(site, point-phase)` a funnel *reached*, armed or not.
    ///
    /// Kept apart from [`Self::coverage`] on purpose: reaching a point proves
    /// the hook is wired into the funnel, and injecting at it proves the mode
    /// does what the fault matrix says. Only the second is evidence of
    /// coverage, and only the first tells a suite author that an arming was
    /// mistargeted rather than the site unreached.
    pub fn reached(&self) -> &[Observation] {
        &self.reached
    }

    /// Whether a funnel reached this point at all, whatever was armed.
    pub fn reached_point(
        &self,
        site: EffectSiteId,
        point: SubEffectPoint,
        mode: InjectionMode,
    ) -> bool {
        self.reached
            .iter()
            .any(|seen| seen.site == site && seen.phase == HookPhase::Point { point, mode })
    }

    /// Every `(site, phase)` observed, in first-observation order.
    pub fn coverage(&self) -> &[Observation] {
        &self.observed
    }

    /// Whether this exact `(site, phase)` was executed at least once.
    pub fn observed(&self, site: EffectSiteId, phase: HookPhase) -> bool {
        self.count(site, phase) > 0
    }

    /// How many times this exact `(site, phase)` was executed.
    pub fn count(&self, site: EffectSiteId, phase: HookPhase) -> u32 {
        self.observed
            .iter()
            .find(|seen| seen.site == site && seen.phase == phase)
            .map_or(0, |seen| seen.count)
    }

    /// Whether the harness saw this site execute at all, in any phase.
    ///
    /// Deliberately *not* what a no-execution record is measured against. That
    /// claim is scoped to a trace — "no staging, cherry-pick, or prepared-pin
    /// site executed **for any fast sequence**" — and its negation is
    /// [`FastSequence::ran`], per sequence. A suite that exercises a stale
    /// integration and a fast one touches all three sites and is exactly the
    /// suite ST-07 asks for; reading this answer as the no-execution test
    /// would reject it.
    pub fn touched(&self, site: EffectSiteId) -> bool {
        self.observed.iter().any(|seen| seen.site == site)
            || self.reached.iter().any(|seen| seen.site == site)
    }

    /// How many executions in total. Zero for a harness nothing has run
    /// through, whatever it has armed.
    pub fn executions(&self) -> u32 {
        self.observed.iter().map(|seen| seen.count).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::effects::{EventSite, LockSite, ObjectSite};

    /// The two sites this module's tests drive: one with two points in two
    /// modes, and one with a single kill-only point.
    const APPEND: EffectSiteId = EffectSiteId::Event(EventSite::AppendFirst);
    const COMMIT_TREE: EffectSiteId = EffectSiteId::Object(ObjectSite::CandidateCommitTree);

    #[test]
    fn an_arming_refusal_names_the_site_it_refused_by_type() {
        let mut harness = HookHarness::new();

        let error = harness
            .arm(COMMIT_TREE, SubEffectPoint::Written, InjectionMode::Kill)
            .expect_err("`CandidateCommitTree` exposes only `IdUnread`");
        assert_eq!(
            error,
            HarnessError::NoSuchPoint {
                site: COMMIT_TREE,
                point: SubEffectPoint::Written,
            },
        );
        assert_eq!(
            error.to_string(),
            "`Object.CandidateCommitTree` exposes no parent-side sub-effect point `Written`; \
             arming one would record an execution of a point that does not exist",
        );

        let error = harness
            .arm(
                COMMIT_TREE,
                SubEffectPoint::IdUnread,
                InjectionMode::ErrorReturn,
            )
            .expect_err("`IdUnread` is kill-only");
        assert_eq!(
            error,
            HarnessError::UnsupportedMode {
                site: COMMIT_TREE,
                point: SubEffectPoint::IdUnread,
                mode: InjectionMode::ErrorReturn,
            },
        );
        assert_eq!(
            error.to_string(),
            "`Object.CandidateCommitTree`'s `IdUnread` point does not support ErrorReturn \
             injection",
        );

        // A refusal armed nothing, so the legal arming of the same point is
        // still the one that fires.
        harness
            .arm(COMMIT_TREE, SubEffectPoint::IdUnread, InjectionMode::Kill)
            .expect("the one point it has, in the one mode it supports");
        assert_eq!(
            harness.hook(
                COMMIT_TREE,
                HookPhase::Point {
                    point: SubEffectPoint::IdUnread,
                    mode: InjectionMode::Kill,
                },
            ),
            Injection::Kill,
        );
    }

    #[test]
    fn the_open_sequence_is_the_one_begun_last_and_stops_at_the_end() {
        let outside = EffectSiteId::Lock(LockSite::AcquireRun);
        let mut harness = HookHarness::new();

        harness.begin_fast_sequence("fast/one");
        harness.hook(APPEND, HookPhase::Before);
        // A second begin closes the first, and what follows belongs to the
        // second alone.
        harness.begin_fast_sequence("fast/two");
        harness.hook(COMMIT_TREE, HookPhase::Before);
        harness.end_fast_sequence();
        // Ended: this one is coverage, recorded in no sequence at all.
        harness.hook(outside, HookPhase::Before);

        let names: Vec<&str> = harness
            .fast_sequences()
            .iter()
            .map(FastSequence::name)
            .collect();
        assert_eq!(names, ["fast/one", "fast/two"]);

        let one = harness
            .fast_sequence("fast/one")
            .expect("the first sequence");
        assert_eq!(
            one.touched(),
            [APPEND],
            "the second begin went on recording into the first",
        );
        assert!(!one.ran(COMMIT_TREE));

        let two = harness
            .fast_sequence("fast/two")
            .expect("the second sequence");
        assert_eq!(two.touched(), [COMMIT_TREE]);
        assert!(
            !two.ran(outside),
            "a hook after `end_fast_sequence` joined the sequence it ended",
        );
        assert!(
            harness.touched(outside),
            "a hook outside every sequence is still an execution the harness saw",
        );
    }

    #[test]
    fn a_reused_sequence_name_is_a_second_sequence_and_not_a_return_to_the_first() {
        let mut harness = HookHarness::new();
        harness.begin_fast_sequence("fast");
        harness.hook(APPEND, HookPhase::Before);
        harness.begin_fast_sequence("fast");
        harness.end_fast_sequence();

        assert_eq!(
            harness.fast_sequences().len(),
            2,
            "a reused name folded two traces into one, so the second is held to the first's \
             observation",
        );
        let empty = harness
            .fast_sequences()
            .iter()
            .filter(|sequence| sequence.touched().is_empty())
            .count();
        assert_eq!(
            empty, 1,
            "the second run under the name inherited an observation it did not make",
        );
        assert_eq!(
            harness.fast_sequence("fast").map(FastSequence::touched),
            Some(&[APPEND][..]),
            "the accessor answered about a sequence other than the first of that name",
        );
    }
}
