//! The ST-07 bijection check: every effect site, in both hook phases and at
//! every parent-side sub-effect point, observed executed, entered for every
//! observable order, and evidenced.
//!
//! Split out of `topology::effects`; the parent re-exports every item here, so
//! `crate::topology::effects::check_bijection` and its siblings are unchanged
//! paths.
//!
//! # What a failure means, and what it takes to fail to check
//!
//! [`check_bijection`] performs no I/O and reads no document it was not handed,
//! so there is no inspection in this file that can fail and be mistaken for a
//! negative answer. Every failure it reports is a fact about the values it was
//! given. What it does have is *scope*: the answer is about the `inventory` and
//! the `host` it was handed and about nothing else, so an empty answer is "no
//! way the bijection fails was found among those". Both narrowings are stated
//! on [`check_bijection`], and the `host` one is the narrowing that has already
//! produced a false success once — see [`Host`] for the incident.
//!
//! Enumeration, not sampling, on the axis the name suggests: the orders a phase
//! is checked at are exactly `EffectSiteId::observable_orders`, which by
//! construction is one order or none. The one thing here that *is* sampled is
//! the kill-sampling record inside recovery-proven evidence, and this file
//! checks only that the record accounts for itself: its `n` is the registry's
//! own frozen number, so nothing here can tell whether it was met or moved.

use thiserror::Error;

use super::EffectSiteId;
use super::harness::{HookHarness, HookPhase};
use super::registry::{EntryPhase, Evidence, RegistryEntry, RegistryError, validate_entry};
use super::residue_authority::{ObjectResidue, ObservableOrder, ResidueElement};
use super::vocab::Host;

// ---------------------------------------------------------------------------
// The bijection check
// ---------------------------------------------------------------------------

/// One way the bijection is not a bijection.
///
/// The site and the phase every direction names are the typed values, not
/// their spellings. A failure is a coordinate of the claim, and a coordinate a
/// caller can only compare as text is one a test pins by substring: `phase`
/// held the rendering of [`EntryPhase`], so "the point `Synced` in
/// error-return mode" was asserted by looking for two words inside one string.
/// The two free-text fields left are the ones whose values really are text a
/// document supplied — a resume action in the fault matrix's words, and the
/// name a suite gave a fast sequence.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BijectionFailure {
    #[error("`{site}` was never observed executing its `{phase}` hook")]
    Unobserved {
        /// The site.
        site: EffectSiteId,
        /// The hook phase or point that never ran.
        phase: HookPhase,
    },

    #[error("`{site}` has no registry entry for `{phase}` in order {order:?}")]
    MissingEntry {
        /// The site.
        site: EffectSiteId,
        /// The phase.
        phase: EntryPhase,
        /// The order.
        order: Option<ObservableOrder>,
    },

    #[error("`{site}`'s `{phase}` entry has no passing evidence")]
    MissingEvidence {
        /// The site.
        site: EffectSiteId,
        /// The phase.
        phase: EntryPhase,
    },

    #[error(
        "`{site}`'s `{phase}` entry lists the `{element:?}` residue element and does not record \
         constructing it"
    )]
    ResidueElementNotConstructed {
        /// The site.
        site: EffectSiteId,
        /// The phase.
        phase: EntryPhase,
        /// The element with no construction.
        element: ResidueElement,
    },

    #[error("`{site}`'s `{phase}` entry's `{element:?}` residue element did not recover")]
    ResidueElementNotRecovered {
        /// The site.
        site: EffectSiteId,
        /// The phase.
        phase: EntryPhase,
        /// The element that did not recover.
        element: ResidueElement,
    },

    #[error(
        "`{site}`'s `{phase}` entry classified its `{element:?}` residue element as \
         {classified:?}, and `{phase}` is the class of {expected:?}"
    )]
    ResidueElementMisclassified {
        /// The site.
        site: EffectSiteId,
        /// The phase.
        phase: EntryPhase,
        /// The element.
        element: ResidueElement,
        /// What the record said the classifier answered.
        classified: ObjectResidue,
        /// What the entry's own class is the class of.
        expected: ObjectResidue,
    },

    #[error("`{site}`'s sampling record classified {count} residues into no class at all")]
    UnclassifiableResidue {
        /// The site.
        site: EffectSiteId,
        /// How many.
        count: u32,
    },

    #[error(
        "`{site}`'s sampling record covers {n} samples but its histogram accounts for {counted}"
    )]
    SamplingUnaccounted {
        /// The site.
        site: EffectSiteId,
        /// The frozen sample count.
        n: u32,
        /// What the histogram and the unclassified count add up to, summed
        /// without saturation; wider than `n` so that a histogram one sample
        /// over a full `u32` reports as one sample over.
        counted: u64,
    },

    #[error("`{site}` has a residue class but no sampling record: its frozen N is zero")]
    MissingSampling {
        /// The site.
        site: EffectSiteId,
    },

    #[error("`{site}`'s sampled residues did not all recover by their classified action")]
    UnrecoveredSampling {
        /// The site.
        site: EffectSiteId,
    },

    #[error("`{site}`'s residue-class entry claims executed-hook evidence")]
    ResidueClaimsExecution {
        /// The site.
        site: EffectSiteId,
    },

    #[error(
        "`{site}` carries a no-execution record and the suite exercised no fast sequence; an \
         empty harness is not evidence that a site a fast sequence skips was skipped"
    )]
    NoFastSequenceExercised {
        /// The site.
        site: EffectSiteId,
    },

    #[error(
        "`{site}`'s no-execution record says nothing about the exercised fast sequence \
         `{sequence}`, in which the harness {}",
        if *observed { "observed the site's hook" } else { "observed no hook of the site" }
    )]
    UnwitnessedFastSequence {
        /// The site.
        site: EffectSiteId,
        /// The sequence it says nothing about.
        sequence: String,
        /// Whether the harness recorded a hook of this site inside that
        /// sequence. An observation, not an execution: the harness knows only
        /// what called [`HookHarness::hook`], so `false` says no funnel of the
        /// site reported itself during the sequence, and cannot say the site
        /// did not run through a path that omits its hook. Carried beside the
        /// gap so a reader sees at once what the record would contradict if it
        /// named the sequence; it asserts nothing about whether an execution is
        /// allowed.
        observed: bool,
    },

    #[error("`{site}`'s no-execution record names `{sequence}`, which the harness never exercised")]
    UnknownFastSequence {
        /// The site.
        site: EffectSiteId,
        /// The sequence it named.
        sequence: String,
    },

    #[error("`{site}` executed during the fast sequence `{sequence}` its record says it skipped")]
    ExecutedInFastSequence {
        /// The site.
        site: EffectSiteId,
        /// The sequence its record names and it ran in.
        sequence: String,
    },

    #[error("the registry holds an entry for `{site}`, which the inventory under check does not")]
    EntryOutsideInventory {
        /// The site.
        site: EffectSiteId,
    },

    #[error(
        "`{site}` has {count} entries for `{phase}` in order {order:?}; a registry key is one \
         entry, and a checker that kept the first or the last would report whichever of two \
         disagreeing claims it happened to reach"
    )]
    DuplicateEntry {
        /// The site.
        site: EffectSiteId,
        /// The phase.
        phase: EntryPhase,
        /// The order.
        order: Option<ObservableOrder>,
        /// How many entries carried the key.
        count: usize,
    },

    #[error(
        "`{site}`'s `{phase}` entry resumes by `{found}` and its before-phase entry resumes by \
         `{expected}`; this phase's resume action is the before-phase action"
    )]
    ResumeActionNotBeforeAction {
        /// The site.
        site: EffectSiteId,
        /// The phase.
        phase: EntryPhase,
        /// What the entry said.
        found: String,
        /// The site's before-phase action.
        expected: String,
    },

    #[error("`{site}`'s `{phase}` entry is not a valid entry: {error}")]
    InvalidEntry {
        /// The site.
        site: EffectSiteId,
        /// The phase.
        phase: EntryPhase,
        /// Why, in the format's own words.
        error: RegistryError,
    },
}

/// The checked bijection over an inventory
/// (`fault_injection_registry.completeness_rule`).
///
/// Returns every way the claim fails; an empty answer is the claim holding
/// **over the inventory and the host it was handed**, which are two different
/// narrowings and both are the caller's to widen.
///
/// `inventory` is a parameter rather than [`EffectSiteId::all`] because the
/// framework has to be checkable long before every site exists: PR3 runs it
/// over the handful of sites its self-test drives, and PR10 runs it over
/// everything. A slice that narrows the inventory narrows its own claim, which
/// is why the self-test also runs the check over the *full* claimed inventory
/// and asserts that it fails. Nothing here refuses an empty slice: a check over
/// no sites requires nothing of the harness, every entry it is handed is then
/// outside the inventory and reported as such, and an empty inventory with an
/// empty entry slice reports nothing at all — it is the caller that has to know
/// which sites it meant.
///
/// `host` narrows the same way and is easier to miss, because it narrows
/// silently in the middle of a claim that otherwise reads as total. A
/// sub-effect point is required only where [`Platform::required_on`] says it
/// exists, so a Unix run of this check says nothing at all about the four
/// Windows containment points, and a green Unix suite is not evidence about
/// them. Running for [`Host::ALL`] rather than [`Host::current`] is what makes
/// the pair of runs total, and it is what the self-test does; [`Host`] carries
/// the incident that made the host a type instead of a `Platform`.
///
/// Legacy-scoped sites are skipped: `scope` says they are inventoried and
/// row-mapped and carry no fault-registry requirement.
///
/// [`Platform::required_on`]: super::vocab::Platform::required_on
#[must_use]
pub fn check_bijection(
    inventory: &[EffectSiteId],
    harness: &HookHarness,
    entries: &[RegistryEntry],
    host: Host,
) -> Vec<BijectionFailure> {
    let mut failures = Vec::new();

    // `FaultRegistry::insert` refuses a duplicate key, but this function is
    // documented to take a bare slice precisely because a registry.json that
    // was hand-edited between a gate and a review never went through `insert`.
    // `structure` keys entries by site x phase x order, so two entries at one
    // key are two answers to one question — and `check_evidence` would silently
    // read the first of them. Restated here so the bare-slice path carries the
    // same invariant the constructor does.
    for (index, entry) in entries.iter().enumerate() {
        let key = entry.key();
        if entries.iter().take(index).any(|held| held.key() == key) {
            // Already reported at its first occurrence. `take(index)` rather
            // than `entries[..index]`: §7 denies a panicking slice in
            // production code, and a bound this loop's own `enumerate` makes
            // safe is still a bound a later edit can move.
            continue;
        }
        let count = entries.iter().filter(|held| held.key() == key).count();
        if count > 1 {
            failures.push(BijectionFailure::DuplicateEntry {
                site: entry.site,
                phase: entry.phase,
                order: entry.order,
                count,
            });
        }
    }

    for entry in entries {
        if let Err(error) = validate_entry(entry) {
            // Restated rather than folded into `InvalidEntry`, because ST-07
            // names this one direction explicitly and a reviewer looking for it
            // should find it under its own name.
            if matches!(error, RegistryError::ResidueClaimsExecution { .. }) {
                failures.push(BijectionFailure::ResidueClaimsExecution { site: entry.site });
            } else {
                failures.push(BijectionFailure::InvalidEntry {
                    site: entry.site,
                    phase: entry.phase,
                    error,
                });
            }
        }
        if !inventory.contains(&entry.site) {
            failures.push(BijectionFailure::EntryOutsideInventory { site: entry.site });
        }
        // The relation stated between two entries rather than inside one: the
        // phases `structure` gives "the before-phase action" have to name the
        // action this site's own before-phase entry names.
        //
        // `validate_entry` reaches the same verdict by a different route — it
        // holds every entry to `semantics`, and `semantics` tables
        // `ResumeAction::ResumeUnperformed` for a before phase, for `IdUnread`
        // and for a residue class alike — so on entries the format accepts
        // this can only fire if that authority stops agreeing with itself.
        // That is what it is for, and it is why both failures appear together
        // on a doctored entry rather than one standing in for the other.
        if entry.phase.resumes_as_before() {
            let before = entries
                .iter()
                .find(|held| held.site == entry.site && held.phase == EntryPhase::Before);
            match before {
                Some(before) if before.resume_action == entry.resume_action => {}
                Some(before) => failures.push(BijectionFailure::ResumeActionNotBeforeAction {
                    site: entry.site,
                    phase: entry.phase,
                    found: entry.resume_action.clone(),
                    expected: before.resume_action.clone(),
                }),
                None => failures.push(BijectionFailure::MissingEntry {
                    site: entry.site,
                    phase: EntryPhase::Before,
                    order: entry.order,
                }),
            }
        }
    }

    for site in inventory {
        let site = *site;
        if !site.scope().is_claimed() {
            continue;
        }

        // A no-execution record is *additional* evidence about the fast
        // traces, not an alternative to ordinary coverage. The three sites it
        // may be written for are Topology-scoped sites on the stale-candidate
        // path: a staging worktree is added, a proposal is cherry-picked and a
        // prepared pin is taken whenever the base is not exact, and
        // `completeness_rule` requires "every site x hook phase ... observed
        // executed at least once by the suite" of them like any other. What
        // `structure` says is narrower and is a statement about traces: "for a
        // fast sequence Worktree.AddStaging, Object.ProposalCherryPick, and
        // Ref.PinPrepared are asserted not executed".
        //
        // So this block adds requirements and removes none. It does not ask
        // whether the harness ever touched the site — a global `touched` test
        // rejects the valid evidence of a suite that exercised both paths, and
        // accepts nothing extra: execution inside a fast sequence is caught by
        // `ExecutedInFastSequence` below, where the claim actually lives. And
        // it does not `continue`, because skipping the phase and point
        // bijection is how a site excuses itself from coverage by declaring
        // that it did not run.
        //
        // The condition is `skipped_on_fast_path()` — a property of the site —
        // and emphatically not "does a no-execution entry exist for it". The
        // predecessor asked the second question, so deleting all three records
        // made the entire branch unreachable and `check_bijection` reported
        // nothing: a completeness oracle that derives *whether* a requirement
        // exists from the very entries it is checking cannot report a missing
        // one. `DESIGN.md` §26's bijection contract requires the record
        // itself — the three sites the fast path skips carry a no-execution
        // entry naming every exercised fast sequence — and any missing
        // link fails. The `check_evidence` call at the end of the block is
        // what reports the record's absence, and it is now reached whether
        // or not the record is there.
        //
        // Exactly one record, not at least one: `check_evidence` finds the
        // entry at the key `(site, NoExecution, None)` and the duplicate sweep
        // above refuses a second at the same key, so the two together admit one
        // and only one.
        if site.skipped_on_fast_path() {
            // `DESIGN.md` §26: the no-execution record names every fast
            // sequence the suite exercised and the harness observed no hook of
            // the site in any of them — so there has to *be* a fast sequence,
            // the record has to hold within every one the suite exercised, and
            // it may not name one that never happened.
            // substantiates the claim, which is the same false report as an
            // empty coverage table.
            if harness.fast_sequences().is_empty() {
                failures.push(BijectionFailure::NoFastSequenceExercised { site });
            }
            let claimed: Vec<&str> = entries
                .iter()
                .filter(|entry| entry.site == site && entry.phase == EntryPhase::NoExecution)
                .filter_map(|entry| match &entry.evidence {
                    Evidence::NotExecuted { sequences, .. } => Some(sequences),
                    _ => None,
                })
                .flatten()
                .map(String::as_str)
                .collect();
            // Two observations about each exercised sequence, both read from
            // the inputs: whether the record names it, and whether the harness
            // recorded a hook of the site inside it. A named sequence with a
            // recorded hook is a contradiction between the record and the
            // observation, and is `ExecutedInFastSequence`. An unnamed
            // sequence is a gap in the record whatever happened in it, and is
            // `UnwitnessedFastSequence` — carrying whether a hook was
            // observed, so a reader sees at once what the record would
            // contradict if it named the sequence, instead of learning it a
            // round later by naming it. The harness witnesses only what called
            // its hook (`DESIGN.md` §26, the bijection contract): an absence
            // of observation is not evidence of non-execution, which is why
            // the record's own names are the claim and the observations are
            // what the claim is held to.
            for sequence in harness.fast_sequences() {
                let observed = sequence.ran(site);
                if claimed.contains(&sequence.name()) {
                    if observed {
                        failures.push(BijectionFailure::ExecutedInFastSequence {
                            site,
                            sequence: sequence.name().to_owned(),
                        });
                    }
                } else {
                    failures.push(BijectionFailure::UnwitnessedFastSequence {
                        site,
                        sequence: sequence.name().to_owned(),
                        observed,
                    });
                }
            }
            for sequence in &claimed {
                if harness.fast_sequence(sequence).is_none() {
                    failures.push(BijectionFailure::UnknownFastSequence {
                        site,
                        sequence: (*sequence).to_owned(),
                    });
                }
            }
            check_evidence(&mut failures, entries, site, EntryPhase::NoExecution, None);
        }

        // Each required coordinate carries both of its spellings: the hook
        // phase the harness records under, and the entry phase the registry is
        // keyed by. Carried rather than recovered — `EntryPhase::hook_phase`
        // answers an `Option` because a residue class and a no-execution
        // record have no hook phase, and recovering the hook here meant
        // unwrapping an `Option` that this loop's own construction had already
        // made total.
        let mut required = vec![
            (HookPhase::Before, EntryPhase::Before),
            (HookPhase::After, EntryPhase::After),
        ];
        for point in site.sub_effects() {
            if !point.platform().required_on(host) {
                continue;
            }
            for mode in point.modes() {
                let (point, mode) = (*point, *mode);
                required.push((
                    HookPhase::Point { point, mode },
                    EntryPhase::Point { point, mode },
                ));
            }
        }

        for (hook, phase) in required {
            if !harness.observed(site, hook) {
                failures.push(BijectionFailure::Unobserved { site, phase: hook });
            }
            check_orders(&mut failures, entries, site, phase);
        }

        for class in site.residue_classes() {
            check_orders(
                &mut failures,
                entries,
                site,
                EntryPhase::Residue { class: *class },
            );
        }
    }

    failures
}

/// Check one phase at every order a fault at this site can leave observable,
/// or at `None` where it can leave none.
///
/// One spelling for every phase kind. The residue-class loop used to take
/// `observable_orders()[0]` while the hook loop iterated the slice; the two
/// agree at this head, because `observable_orders` answers one order or none
/// by construction, and agreeing by coincidence is not the same as agreeing.
fn check_orders(
    failures: &mut Vec<BijectionFailure>,
    entries: &[RegistryEntry],
    site: EffectSiteId,
    phase: EntryPhase,
) {
    let orders = site.observable_orders();
    if orders.is_empty() {
        check_evidence(failures, entries, site, phase, None);
    } else {
        for order in orders {
            check_evidence(failures, entries, site, phase, Some(*order));
        }
    }
}

/// Whether one required key has an entry, and whether that entry's evidence
/// says anything.
fn check_evidence(
    failures: &mut Vec<BijectionFailure>,
    entries: &[RegistryEntry],
    site: EffectSiteId,
    phase: EntryPhase,
    order: Option<ObservableOrder>,
) {
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.key() == (site, phase, order))
    else {
        failures.push(BijectionFailure::MissingEntry { site, phase, order });
        return;
    };

    match &entry.evidence {
        Evidence::Executed { passed, .. } | Evidence::NotExecuted { passed, .. } => {
            if !passed {
                failures.push(BijectionFailure::MissingEvidence { site, phase });
            }
        }
        Evidence::RecoveryProven {
            synthetic,
            sampling,
        } => {
            let Some(class) = phase.residue_class() else {
                // Recovery-proven evidence on a phase that is not about a
                // residue class. `validate_entry` refuses that on its own
                // (`RegistryError::HookClaimsRecoveryProof`); what the
                // bijection has to say about it is that this hook phase
                // carries nothing saying it executed. The synthetic and
                // sampling records are left unread rather than held to a class
                // the entry is not about, which would be reporting on a claim
                // nobody made.
                failures.push(BijectionFailure::MissingEvidence { site, phase });
                return;
            };
            // The class the entry is about answers what its records must have
            // classified as. Read from `ResidueClass::classified_as` rather
            // than written in as `ObjectResidue::Internal`: that authority is
            // the reason the class and the classifier's codomain are two
            // types, and a second class added to either would otherwise be
            // checked against this one's answer.
            let expected = class.classified_as();
            // Every element that failed, not the first: three predicates over
            // a list of up to seven elements were one `MissingEvidence`
            // naming neither which element nor which predicate, so a reader
            // had to diff the document to find out what the check had already
            // worked out.
            for record in synthetic {
                if !record.constructed {
                    failures.push(BijectionFailure::ResidueElementNotConstructed {
                        site,
                        phase,
                        element: record.element,
                    });
                }
                if !record.recovered {
                    failures.push(BijectionFailure::ResidueElementNotRecovered {
                        site,
                        phase,
                        element: record.element,
                    });
                }
                if record.classified != expected {
                    failures.push(BijectionFailure::ResidueElementMisclassified {
                        site,
                        phase,
                        element: record.element,
                        classified: record.classified,
                        expected,
                    });
                }
            }
            if sampling.n == 0 {
                failures.push(BijectionFailure::MissingSampling { site });
            }
            if sampling.unclassified > 0 {
                failures.push(BijectionFailure::UnclassifiableResidue {
                    site,
                    count: sampling.unclassified,
                });
            }
            // Summed in `u64`, where four `u32`s cannot overflow, and compared
            // with `n` widened. `ClassHistogram::total` saturates, and a
            // saturating sum agrees with an `n` of `u32::MAX` whatever the
            // histogram holds: `{ none: u32::MAX, internal: 1, after: 0 }`
            // accounts for one sample more than `n` and used to pass this
            // check. The checker reads the three fields itself rather than
            // `total()` so that the arithmetic it decides by is its own.
            let histogram = sampling.histogram;
            let counted = [
                histogram.none,
                histogram.internal,
                histogram.after,
                sampling.unclassified,
            ]
            .into_iter()
            .map(u64::from)
            .sum::<u64>();
            if counted != u64::from(sampling.n) {
                failures.push(BijectionFailure::SamplingUnaccounted {
                    site,
                    n: sampling.n,
                    counted,
                });
            }
            if !sampling.recovered {
                failures.push(BijectionFailure::UnrecoveredSampling { site });
            }
        }
    }
}
