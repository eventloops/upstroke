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
/// says so — the second is [`FaultRegistry::insert`], which refuses an entry
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
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HarnessError {
    #[error(
        "`{site}` exposes no parent-side sub-effect point `{point}`; arming one would record an \
         execution of a point that does not exist"
    )]
    NoSuchPoint {
        /// The site.
        site: String,
        /// The point that was asked for.
        point: SubEffectPoint,
    },

    #[error("`{site}`'s `{point}` point does not support {mode:?} injection")]
    UnsupportedMode {
        /// The site.
        site: String,
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
    /// The one being recorded, if a sequence is open.
    open_fast: Option<usize>,
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
}

impl HookHarness {
    /// A harness that has armed nothing and seen nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Arm an injection at one point of one site.
    ///
    /// Refuses a point the site does not expose and a mode the point does not
    /// support, so a suite cannot quietly arm a fault that no funnel will ever
    /// consult.
    pub fn arm(
        &mut self,
        site: EffectSiteId,
        point: SubEffectPoint,
        mode: InjectionMode,
    ) -> Result<(), HarnessError> {
        if !site.sub_effects().contains(&point) {
            return Err(HarnessError::NoSuchPoint {
                site: site.name(),
                point,
            });
        }
        if !point.supports(mode) {
            return Err(HarnessError::UnsupportedMode {
                site: site.name(),
                point,
                mode,
            });
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
        if let Some(open) = self.open_fast {
            if let Some(sequence) = self.fast.get_mut(open) {
                if !sequence.touched.contains(&site) {
                    sequence.touched.push(site);
                }
            }
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
        if let HookPhase::Point { point, mode } = phase {
            Self::record(&mut self.reached, site, HookPhase::Point { point, mode });
            if injection == Injection::Proceed {
                // Reached, and nothing was injected. Recorded as reachability
                // and as nothing else.
                return injection;
            }
        }
        Self::record(&mut self.observed, site, phase);
        injection
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
    pub fn begin_fast_sequence(&mut self, name: &str) {
        self.end_fast_sequence();
        self.fast.push(FastSequence {
            name: name.to_owned(),
            touched: Vec::new(),
        });
        self.open_fast = Some(self.fast.len() - 1);
    }

    /// Stop recording the open fast sequence, keeping what it saw.
    pub fn end_fast_sequence(&mut self) {
        self.open_fast = None;
    }

    /// Every fast sequence the suite exercised.
    pub fn fast_sequences(&self) -> &[FastSequence] {
        &self.fast
    }

    /// The fast sequence of this name, if the suite exercised one.
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
