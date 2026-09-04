//! The ST-07 bijection check: every effect site, in both hook phases and at
//! every parent-side sub-effect point, observed executed, entered for every
//! observable order, and evidenced.
//!
//! Split out of `topology::effects`; the parent re-exports every item here, so
//! `crate::topology::effects::check_bijection` and its siblings are unchanged
//! paths.

use thiserror::Error;

use super::EffectSiteId;
use super::harness::HookHarness;
use super::registry::{EntryPhase, Evidence, RegistryEntry, RegistryError, validate_entry};
use super::residue_authority::{ObjectResidue, ObservableOrder};
use super::vocab::Host;

// ---------------------------------------------------------------------------
// The bijection check
// ---------------------------------------------------------------------------

/// One way the bijection is not a bijection.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BijectionFailure {
    #[error("`{site}` was never observed executing its `{phase}` hook")]
    Unobserved {
        /// The site.
        site: String,
        /// The phase or point that never ran.
        phase: String,
    },

    #[error("`{site}` has no registry entry for `{phase}` in order {order:?}")]
    MissingEntry {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// The order.
        order: Option<ObservableOrder>,
    },

    #[error("`{site}`'s `{phase}` entry has no passing evidence")]
    MissingEvidence {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
    },

    #[error("`{site}`'s sampling record classified {count} residues into no class at all")]
    UnclassifiableResidue {
        /// The site.
        site: String,
        /// How many.
        count: u32,
    },

    #[error(
        "`{site}`'s sampling record covers {n} samples but its histogram accounts for {counted}"
    )]
    SamplingUnaccounted {
        /// The site.
        site: String,
        /// The frozen sample count.
        n: u32,
        /// What the histogram and the unclassified count add up to.
        counted: u32,
    },

    #[error("`{site}` has a residue class but no sampling record: its frozen N is zero")]
    MissingSampling {
        /// The site.
        site: String,
    },

    #[error("`{site}`'s sampled residues did not all recover by their classified action")]
    UnrecoveredSampling {
        /// The site.
        site: String,
    },

    #[error("`{site}`'s residue-class entry claims executed-hook evidence")]
    ResidueClaimsExecution {
        /// The site.
        site: String,
    },

    #[error(
        "`{site}` carries a no-execution record and the suite exercised no fast sequence; an \
         empty harness is not evidence that a site a fast sequence skips was skipped"
    )]
    NoFastSequenceExercised {
        /// The site.
        site: String,
    },

    #[error(
        "`{site}`'s no-execution record does not hold within the exercised fast sequence \
         `{sequence}`"
    )]
    UnwitnessedFastSequence {
        /// The site.
        site: String,
        /// The sequence it says nothing about.
        sequence: String,
    },

    #[error("`{site}`'s no-execution record names `{sequence}`, which the harness never exercised")]
    UnknownFastSequence {
        /// The site.
        site: String,
        /// The sequence it named.
        sequence: String,
    },

    #[error("`{site}` executed during the fast sequence `{sequence}` its record says it skipped")]
    ExecutedInFastSequence {
        /// The site.
        site: String,
        /// The sequence it ran in.
        sequence: String,
    },

    #[error("the registry holds an entry for `{site}`, which the inventory under check does not")]
    EntryOutsideInventory {
        /// The site.
        site: String,
    },

    #[error(
        "`{site}` has {count} entries for `{phase}` in order {order:?}; a registry key is one \
         entry, and a checker that kept the first or the last would report whichever of two \
         disagreeing claims it happened to reach"
    )]
    DuplicateEntry {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
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
        site: String,
        /// The phase.
        phase: String,
        /// What the entry said.
        found: String,
        /// The site's before-phase action.
        expected: String,
    },

    #[error("`{site}`'s `{phase}` entry is not a valid entry: {reason}")]
    InvalidEntry {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// Why.
        reason: String,
    },
}

/// The checked bijection over an inventory
/// (`fault_injection_registry.completeness_rule`).
///
/// Returns every way the claim fails; an empty answer is the claim holding.
/// `inventory` is a parameter rather than [`EffectSiteId::all`] because the
/// framework has to be checkable long before every site exists: PR3 runs it
/// over the handful of sites its self-test drives, and PR10 runs it over
/// everything. A slice that narrows the inventory narrows its own claim, which
/// is why the self-test also runs the check over the *full* claimed inventory
/// and asserts that it fails.
///
/// Legacy-scoped sites are skipped: `scope` says they are inventoried and
/// row-mapped and carry no fault-registry requirement.
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
        if entries[..index].iter().any(|held| held.key() == key) {
            // Already reported at its first occurrence.
            continue;
        }
        let count = entries.iter().filter(|held| held.key() == key).count();
        if count > 1 {
            failures.push(BijectionFailure::DuplicateEntry {
                site: entry.site.name(),
                phase: entry.phase.to_string(),
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
                failures.push(BijectionFailure::ResidueClaimsExecution {
                    site: entry.site.name(),
                });
            } else {
                failures.push(BijectionFailure::InvalidEntry {
                    site: entry.site.name(),
                    phase: entry.phase.to_string(),
                    reason: error.to_string(),
                });
            }
        }
        if !inventory.contains(&entry.site) {
            failures.push(BijectionFailure::EntryOutsideInventory {
                site: entry.site.name(),
            });
        }
        // The relation `validate_entry` cannot make, because it sees one entry:
        // the phases `structure` gives "the before-phase action" have to name
        // the action this site's own before-phase entry names.
        if entry.phase.resumes_as_before() {
            let before = entries
                .iter()
                .find(|held| held.site == entry.site && held.phase == EntryPhase::Before);
            match before {
                Some(before) if before.resume_action == entry.resume_action => {}
                Some(before) => failures.push(BijectionFailure::ResumeActionNotBeforeAction {
                    site: entry.site.name(),
                    phase: entry.phase.to_string(),
                    found: entry.resume_action.clone(),
                    expected: before.resume_action.clone(),
                }),
                None => failures.push(BijectionFailure::MissingEntry {
                    site: entry.site.name(),
                    phase: EntryPhase::Before.to_string(),
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
        let name = site.name();

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
        // accepts nothing extra: execution inside a named fast sequence is
        // caught by `ExecutedInFastSequence` below, where the claim actually
        // lives. And it does not `continue`, because skipping the phase and
        // point bijection is how a site excuses itself from coverage by
        // declaring that it did not run.
        //
        // The condition is `skipped_on_fast_path()` — a property of the site —
        // and emphatically not "does a no-execution entry exist for it". The
        // predecessor asked the second question, so deleting all three records
        // made the entire branch unreachable and `check_bijection` reported
        // nothing: a completeness oracle that derives *whether* a requirement
        // exists from the very entries it is checking cannot report a missing
        // one. `completeness_rule` is explicit that "any missing link fails",
        // and ST-07 requires the record itself — "the fast-path no-execution
        // record shows that no staging, cherry-pick, or prepared-pin site
        // executed for any fast sequence". The `check_evidence` call at the end
        // of the block is what reports the record's absence, and it is now
        // reached whether or not the record is there.
        //
        // Exactly one record, not at least one: `check_evidence` finds the
        // entry at the key `(site, NoExecution, None)` and the duplicate sweep
        // above refuses a second at the same key, so the two together admit one
        // and only one.
        if site.skipped_on_fast_path() {
            // "The fast-path no-execution record shows that no staging,
            // cherry-pick, or prepared-pin site executed for any fast
            // sequence" — so there has to *be* a fast sequence, the record has
            // to hold within every one the suite exercised, and it may not
            // name one that never happened. Without all three an empty harness
            // substantiates the claim, which is the same false report as an
            // empty coverage table.
            if harness.fast_sequences().is_empty() {
                failures.push(BijectionFailure::NoFastSequenceExercised { site: name.clone() });
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
            for sequence in harness.fast_sequences() {
                if !claimed.contains(&sequence.name()) {
                    failures.push(BijectionFailure::UnwitnessedFastSequence {
                        site: name.clone(),
                        sequence: sequence.name().to_owned(),
                    });
                } else if sequence.ran(site) {
                    failures.push(BijectionFailure::ExecutedInFastSequence {
                        site: name.clone(),
                        sequence: sequence.name().to_owned(),
                    });
                }
            }
            for sequence in &claimed {
                if harness.fast_sequence(sequence).is_none() {
                    failures.push(BijectionFailure::UnknownFastSequence {
                        site: name.clone(),
                        sequence: (*sequence).to_owned(),
                    });
                }
            }
            check_evidence(&mut failures, entries, site, EntryPhase::NoExecution, None);
        }

        let mut required = vec![EntryPhase::Before, EntryPhase::After];
        for point in site.sub_effects() {
            if !point.platform().required_on(host) {
                continue;
            }
            for mode in point.modes() {
                required.push(EntryPhase::Point {
                    point: *point,
                    mode: *mode,
                });
            }
        }

        for phase in required {
            #[expect(
                clippy::expect_used,
                reason = "before, after and point phases all have a hook phase"
            )]
            let hook = phase
                .hook_phase()
                .expect("before, after and point phases all have a hook phase");
            if !harness.observed(site, hook) {
                failures.push(BijectionFailure::Unobserved {
                    site: name.clone(),
                    phase: phase.to_string(),
                });
            }
            let orders = site.observable_orders();
            if orders.is_empty() {
                check_evidence(&mut failures, entries, site, phase, None);
            } else {
                for order in orders {
                    check_evidence(&mut failures, entries, site, phase, Some(*order));
                }
            }
        }

        for class in site.residue_classes() {
            let phase = EntryPhase::Residue { class: *class };
            let orders = site.observable_orders();
            let order = if orders.is_empty() {
                None
            } else {
                Some(orders[0])
            };
            check_evidence(&mut failures, entries, site, phase, order);
        }
    }

    failures
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
        failures.push(BijectionFailure::MissingEntry {
            site: site.name(),
            phase: phase.to_string(),
            order,
        });
        return;
    };

    match &entry.evidence {
        Evidence::Executed { passed, .. } | Evidence::NotExecuted { passed, .. } => {
            if !passed {
                failures.push(BijectionFailure::MissingEvidence {
                    site: site.name(),
                    phase: phase.to_string(),
                });
            }
        }
        Evidence::RecoveryProven {
            synthetic,
            sampling,
        } => {
            for record in synthetic {
                if !record.constructed
                    || !record.recovered
                    || record.classified != ObjectResidue::Internal
                {
                    failures.push(BijectionFailure::MissingEvidence {
                        site: site.name(),
                        phase: phase.to_string(),
                    });
                    break;
                }
            }
            if sampling.n == 0 {
                failures.push(BijectionFailure::MissingSampling { site: site.name() });
            }
            if sampling.unclassified > 0 {
                failures.push(BijectionFailure::UnclassifiableResidue {
                    site: site.name(),
                    count: sampling.unclassified,
                });
            }
            let counted = sampling
                .histogram
                .total()
                .saturating_add(sampling.unclassified);
            if counted != sampling.n {
                failures.push(BijectionFailure::SamplingUnaccounted {
                    site: site.name(),
                    n: sampling.n,
                    counted,
                });
            }
            if !sampling.recovered {
                failures.push(BijectionFailure::UnrecoveredSampling { site: site.name() });
            }
        }
    }
}
