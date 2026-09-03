//! The fault-injection registry format: what an entry is about, what evidence
//! it carries, and the whole of the validity rule as one function.
//!
//! Split out of `topology::effects`; the parent re-exports every item here, so
//! `crate::topology::effects::FaultRegistry` and its siblings are unchanged
//! paths.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::EffectSiteId;
use super::harness::HookPhase;
use super::residue_authority::{
    EvidenceLabel, ObjectResidue, ObservableOrder, ResidueClass, ResidueElement,
};
use super::vocab::{FaultRow, InjectionMode, ResourceRow, SubEffectPoint};

// ---------------------------------------------------------------------------
// The registry format
// ---------------------------------------------------------------------------

/// What a registry entry is about.
///
/// The four kinds are different in kind, and keeping them apart at the type
/// level is what stops a residue class from being counted as a hook: a
/// [`Self::Residue`] entry cannot carry a [`HookPhase`], and a hook entry
/// cannot carry a [`ResidueClass`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryPhase {
    /// The hook before the primitive.
    Before,
    /// The hook after the primitive.
    After,
    /// A parent-side sub-effect point in one injection mode.
    Point {
        /// Which point.
        point: SubEffectPoint,
        /// Which mode.
        mode: InjectionMode,
    },
    /// A command-internal residue class. Never an executed hook.
    Residue {
        /// Which class.
        class: ResidueClass,
    },
    /// The record that this site did *not* execute — the fast integration
    /// path's assertion about staging, cherry-pick and prepared-pin sites.
    NoExecution,
}

impl EntryPhase {
    /// The hook phase this entry is about, where it is about one.
    pub const fn hook_phase(self) -> Option<HookPhase> {
        match self {
            Self::Before => Some(HookPhase::Before),
            Self::After => Some(HookPhase::After),
            Self::Point { point, mode } => Some(HookPhase::Point { point, mode }),
            Self::Residue { .. } | Self::NoExecution => None,
        }
    }

    /// The residue class this entry is about, where it is about one.
    pub const fn residue_class(self) -> Option<ResidueClass> {
        match self {
            Self::Residue { class } => Some(class),
            Self::Before | Self::After | Self::Point { .. } | Self::NoExecution => None,
        }
    }

    /// Whether `structure` gives this phase the site's *before-phase* resume
    /// action rather than an action of its own.
    ///
    /// Two phases, and the packet says so of both in the same words:
    /// `IdUnread` ("R27 object without a recorded id; resume action = the
    /// before-phase action") and the `Internal` residue class ("objects
    /// present and unreferenced, R27, with administrative residue ...; resume
    /// action equal to the before-phase action"). Both are prefixes in which
    /// nothing was published, so recovery is what recovery from *nothing*
    /// would have been — and an entry free to name a different action could
    /// table a resume that adopts a prefix no reader can authenticate.
    pub const fn resumes_as_before(self) -> bool {
        matches!(
            self,
            Self::Point {
                point: SubEffectPoint::IdUnread,
                ..
            } | Self::Residue { .. }
        )
    }

    /// The evidence label an entry in this phase must carry.
    pub const fn required_label(self) -> EvidenceLabel {
        match self {
            Self::Before | Self::After | Self::Point { .. } | Self::NoExecution => {
                EvidenceLabel::ExecutionObserved
            }
            Self::Residue { .. } => EvidenceLabel::RecoveryProven,
        }
    }
}

impl fmt::Display for EntryPhase {
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
            Self::Residue { class } => f.write_str(class.name()),
            Self::NoExecution => f.write_str("no-execution"),
        }
    }
}

/// What is left durable after a fault at this entry's point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedResidue {
    /// The ledger rows still holding something. Empty is a real answer — the
    /// before phase of a creation, and a Windows containment point, each leave
    /// no row holding anything — but it is not the *only* answer a before phase
    /// has: see [`BeforeState`](crate::topology::effects::BeforeState).
    pub rows: Vec<ResourceRow>,
    /// The concrete artifacts, in the fault matrix's own words.
    pub detail: String,
}

/// One residue element's synthetic-construction record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticRecord {
    /// Which element.
    pub element: ResidueElement,
    /// Whether it was constructed in a real temporary repository.
    pub constructed: bool,
    /// What the classifier answered for it.
    pub classified: ObjectResidue,
    /// Whether the tabled recovery converged.
    pub recovered: bool,
}

/// How many of each class a site's kill sampling observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassHistogram {
    /// Samples that classified `None`.
    pub none: u32,
    /// Samples that classified `Internal`. Zero is legal: hitting the internal
    /// window is recorded, never required.
    pub internal: u32,
    /// Samples that classified `After`.
    pub after: u32,
}

impl ClassHistogram {
    /// How many samples the histogram accounts for.
    pub const fn total(self) -> u32 {
        self.none
            .saturating_add(self.internal)
            .saturating_add(self.after)
    }
}

/// The real-command kill-sampling record for one site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamplingRecord {
    /// The frozen sample count for this site.
    pub n: u32,
    /// What the classifier answered, by class.
    pub histogram: ClassHistogram,
    /// Samples that classified into no class at all. Any is a failure: the run
    /// would have durable state no tabled action recovers.
    pub unclassified: u32,
    /// Whether every sampled residue recovered by its classified action.
    pub recovered: bool,
}

/// An entry's evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Evidence {
    /// A hook phase or point ran, and this test recorded it.
    Executed {
        /// The test that executed it.
        test: String,
        /// Its pass record.
        passed: bool,
    },
    /// Nothing executed: every listed residue element was constructed and
    /// recovered, and the site was kill-sampled.
    RecoveryProven {
        /// One record per element the site's class lists.
        synthetic: Vec<SyntheticRecord>,
        /// The sampling record.
        sampling: SamplingRecord,
    },
    /// This site was asserted *not* to have executed.
    NotExecuted {
        /// The test that asserted it.
        test: String,
        /// Its pass record.
        passed: bool,
        /// The exercised fast sequences the absence was proved within.
        ///
        /// "The fast-path no-execution record shows that no staging,
        /// cherry-pick, or prepared-pin site executed **for any fast
        /// sequence**": the claim is about traces, so the evidence names the
        /// traces. An entry naming none is a claim about a process that may
        /// never have run an integration at all.
        sequences: Vec<String>,
    },
}

impl Evidence {
    /// The label this evidence's shape implies.
    pub const fn label(&self) -> EvidenceLabel {
        match self {
            Self::Executed { .. } | Self::NotExecuted { .. } => EvidenceLabel::ExecutionObserved,
            Self::RecoveryProven { .. } => EvidenceLabel::RecoveryProven,
        }
    }

    /// Whether this evidence claims a hook was executed.
    pub const fn claims_execution(&self) -> bool {
        matches!(self, Self::Executed { .. })
    }
}

/// One entry of the fault-injection registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryEntry {
    /// The site.
    pub site: EffectSiteId,
    /// What the entry is about.
    pub phase: EntryPhase,
    /// Which durable order, where the site has one.
    pub order: Option<ObservableOrder>,
    /// The fault-matrix row. Must equal the site's own.
    pub fault_row: FaultRow,
    /// What is left durable.
    pub expected_residue: ExpectedResidue,
    /// What a resume does about it, in the matrix's words.
    pub resume_action: String,
    /// How the claim was obtained.
    pub label: EvidenceLabel,
    /// The evidence itself.
    pub evidence: Evidence,
}

impl RegistryEntry {
    /// This entry's key: site, phase, order.
    pub fn key(&self) -> (EffectSiteId, EntryPhase, Option<ObservableOrder>) {
        (self.site, self.phase, self.order)
    }
}

/// Why the registry format refused an entry.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error(
        "`{site}`'s entry for the residue class `{class}` carries executed-hook evidence. A \
         residue class is a prefix inside an external command that no parent hook can observe; \
         its evidence is recovery-proven, and an entry claiming otherwise would report coverage \
         the suite does not have."
    )]
    ResidueClaimsExecution {
        /// The site.
        site: String,
        /// The class.
        class: &'static str,
    },

    #[error(
        "`{site}`'s `{phase}` entry carries recovery-proven evidence, but a hook phase is \
         observed by execution; recovery-proven is the label for what no hook can reach"
    )]
    HookClaimsRecoveryProof {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
    },

    #[error("`{site}`'s `{phase}` entry is labelled {found:?} but its phase requires {required:?}")]
    MislabelledEntry {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// The label the entry carried.
        found: EvidenceLabel,
        /// The label its phase requires.
        required: EvidenceLabel,
    },

    #[error("`{site}` records fault row {found} but the site's row is {expected}")]
    WrongFaultRow {
        /// The site.
        site: String,
        /// What the entry said.
        found: FaultRow,
        /// What the site says.
        expected: FaultRow,
    },

    #[error(
        "`{site}`'s `{phase}` entry records order {found:?}, which is not an order a fault at \
         this site can leave observable"
    )]
    WrongOrder {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// What the entry said.
        found: Option<ObservableOrder>,
    },

    #[error("`{site}` exposes no `{point}` point in {mode:?} mode")]
    NoSuchPoint {
        /// The site.
        site: String,
        /// The point.
        point: SubEffectPoint,
        /// The mode.
        mode: InjectionMode,
    },

    #[error("`{site}` registers no residue class `{class}`")]
    NoSuchResidueClass {
        /// The site.
        site: String,
        /// The class.
        class: &'static str,
    },

    #[error(
        "`{site}`'s recovery-proven entry has no synthetic-construction record for the `{element:?}` \
         residue element its class lists"
    )]
    MissingSyntheticElement {
        /// The site.
        site: String,
        /// The element with no record.
        element: ResidueElement,
    },

    #[error(
        "`{site}`'s recovery-proven entry records a synthetic construction of `{element:?}`, which its class does not list"
    )]
    UnlistedSyntheticElement {
        /// The site.
        site: String,
        /// The element that does not belong.
        element: ResidueElement,
    },

    #[error("`{site}`'s `{phase}` entry names no test")]
    UnnamedTest {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
    },

    #[error(
        "`{site}` carries a no-execution record, but only the three sites a fast integration \
         sequence skips — Worktree.AddStaging, Object.ProposalCherryPick, Ref.PinPrepared — may \
         record that they did not run"
    )]
    NoExecutionNotSkipped {
        /// The site.
        site: String,
    },

    #[error(
        "`{site}`'s `{phase}` entry expects {found:?} to hold residue and this site's `{phase}` \
         leaves {expected:?}"
    )]
    WrongResidueRows {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// What the entry claimed.
        found: Vec<ResourceRow>,
        /// What the site's own semantics leave.
        expected: Vec<ResourceRow>,
    },

    #[error(
        "`{site}`'s no-execution record names no fast sequence it holds within. Absence is proved \
         inside an exercised trace or it is a claim about a process that ran no integration at all."
    )]
    UnwitnessedNoExecution {
        /// The site.
        site: String,
    },

    #[error("`{site}`'s `{phase}` entry names no resume action")]
    UnnamedResumeAction {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
    },

    #[error(
        "`{site}`'s `{phase}` entry describes its residue as `{found}` and this site's `{phase}` \
         leaves `{expected}`"
    )]
    WrongResidueDetail {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// What the entry claimed.
        found: String,
        /// What the site's own semantics leave.
        expected: &'static str,
    },

    #[error(
        "`{site}`'s `{phase}` entry tables the resume action `{found}` and the matrix tables \
         `{expected}` for this phase of this site"
    )]
    WrongResumeAction {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// What the entry claimed.
        found: String,
        /// What the site's own semantics table.
        expected: &'static str,
    },

    #[error("`{site}` already has an entry for `{phase}` in order {order:?}")]
    DuplicateEntry {
        /// The site.
        site: String,
        /// The phase.
        phase: String,
        /// The order.
        order: Option<ObservableOrder>,
    },
}

/// The fault-injection registry: entries, and the format that refuses a bad
/// one.
///
/// `insert` is the format. Everything it refuses is refused *before* the
/// bijection check runs, so a registry that exists at all is one whose entries
/// are internally consistent with the enums; the bijection is then only about
/// whether the entries and the executions cover the inventory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FaultRegistry {
    entries: Vec<RegistryEntry>,
}

impl FaultRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an entry, or say why it is not one.
    pub fn insert(&mut self, entry: RegistryEntry) -> Result<(), RegistryError> {
        validate_entry(&entry)?;
        if self.entries.iter().any(|held| held.key() == entry.key()) {
            return Err(RegistryError::DuplicateEntry {
                site: entry.site.name(),
                phase: entry.phase.to_string(),
                order: entry.order,
            });
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Every entry, in insertion order.
    pub fn entries(&self) -> &[RegistryEntry] {
        &self.entries
    }

    /// The entry for one key, if there is one.
    pub fn get(
        &self,
        site: EffectSiteId,
        phase: EntryPhase,
        order: Option<ObservableOrder>,
    ) -> Option<&RegistryEntry> {
        self.entries
            .iter()
            .find(|entry| entry.key() == (site, phase, order))
    }

    /// How many entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The whole of the format's validity rule, as one function.
///
/// Separate from [`FaultRegistry::insert`] so the bijection check can apply it
/// again to entries handed to it as a bare slice — a registry.json that was
/// hand-edited between a gate and a review never went through `insert`, and
/// "the bijection check fails on a residue-class entry claiming executed-hook
/// evidence" has to be true of that document too.
pub fn validate_entry(entry: &RegistryEntry) -> Result<(), RegistryError> {
    let site = entry.site;
    let name = site.name();

    if entry.fault_row != site.fault_row() {
        return Err(RegistryError::WrongFaultRow {
            site: name,
            found: entry.fault_row,
            expected: site.fault_row(),
        });
    }

    // A no-execution record is not about an order: nothing was performed, so
    // there is no effect to be durable before or after the append. Every other
    // phase carries the site's one order, or `None` where the site has none.
    let orders = site.observable_orders();
    let order_ok = match (entry.phase, entry.order) {
        (EntryPhase::NoExecution, order) => order.is_none(),
        (_, Some(order)) => orders.contains(&order),
        (_, None) => orders.is_empty(),
    };
    if !order_ok {
        return Err(RegistryError::WrongOrder {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.order,
        });
    }

    if entry.phase == EntryPhase::NoExecution && !site.skipped_on_fast_path() {
        return Err(RegistryError::NoExecutionNotSkipped { site: name });
    }

    // The expected residue and the tabled recovery are the site's own
    // semantics, not the entry's opinion of them. Without this an otherwise
    // complete entry can name an unrelated row — or none — describe residue
    // the site does not leave, and table a resume the matrix does not give it,
    // and the registry reads as evidence that a fault there was accounted for
    // when nothing checked any of the three.
    //
    // All three come from one call, so they cannot be checked against two
    // tables that disagree.
    let semantics = site.semantics(entry.phase);
    if entry.expected_residue.rows != semantics.rows {
        return Err(RegistryError::WrongResidueRows {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.expected_residue.rows.clone(),
            expected: semantics.rows,
        });
    }
    if entry.resume_action.trim().is_empty() {
        return Err(RegistryError::UnnamedResumeAction {
            site: name,
            phase: entry.phase.to_string(),
        });
    }
    if entry.expected_residue.detail != semantics.artifact.detail() {
        return Err(RegistryError::WrongResidueDetail {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.expected_residue.detail.clone(),
            expected: semantics.artifact.detail(),
        });
    }
    if entry.resume_action != semantics.action.text() {
        return Err(RegistryError::WrongResumeAction {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.resume_action.clone(),
            expected: semantics.action.text(),
        });
    }

    match entry.phase {
        EntryPhase::Point { point, mode } => {
            if !site.exposes(point, mode) {
                return Err(RegistryError::NoSuchPoint {
                    site: name,
                    point,
                    mode,
                });
            }
        }
        EntryPhase::Residue { class } => {
            if !site.registers(class) {
                return Err(RegistryError::NoSuchResidueClass {
                    site: name,
                    class: class.name(),
                });
            }
        }
        EntryPhase::Before | EntryPhase::After | EntryPhase::NoExecution => {}
    }

    // The load-bearing refusal, stated first and stated by itself: a residue
    // class is not a hook, and an entry that claims one executed is refused
    // whatever else about it is well-formed.
    if let Some(class) = entry.phase.residue_class() {
        if entry.evidence.claims_execution() || entry.label == EvidenceLabel::ExecutionObserved {
            return Err(RegistryError::ResidueClaimsExecution {
                site: name,
                class: class.name(),
            });
        }
    }
    if entry.phase.residue_class().is_none()
        && matches!(entry.evidence, Evidence::RecoveryProven { .. })
    {
        return Err(RegistryError::HookClaimsRecoveryProof {
            site: name,
            phase: entry.phase.to_string(),
        });
    }
    if entry.label != entry.phase.required_label() {
        return Err(RegistryError::MislabelledEntry {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.label,
            required: entry.phase.required_label(),
        });
    }
    if entry.label != entry.evidence.label() {
        return Err(RegistryError::MislabelledEntry {
            site: name,
            phase: entry.phase.to_string(),
            found: entry.label,
            required: entry.evidence.label(),
        });
    }

    // The two evidence shapes that are legal for a hook entry are legal only
    // for the phase kind that matches them: `NoExecution` records that nothing
    // ran, and a before/after/point entry records that something did.
    match (&entry.phase, &entry.evidence) {
        (EntryPhase::NoExecution, Evidence::Executed { .. }) => {
            return Err(RegistryError::MislabelledEntry {
                site: name,
                phase: entry.phase.to_string(),
                found: EvidenceLabel::ExecutionObserved,
                required: EvidenceLabel::ExecutionObserved,
            });
        }
        (
            EntryPhase::Before | EntryPhase::After | EntryPhase::Point { .. },
            Evidence::NotExecuted { .. },
        ) => {
            return Err(RegistryError::MislabelledEntry {
                site: name,
                phase: entry.phase.to_string(),
                found: EvidenceLabel::ExecutionObserved,
                required: EvidenceLabel::ExecutionObserved,
            });
        }
        _ => {}
    }

    match &entry.evidence {
        Evidence::Executed { test, .. } => {
            if test.trim().is_empty() {
                return Err(RegistryError::UnnamedTest {
                    site: name,
                    phase: entry.phase.to_string(),
                });
            }
        }
        Evidence::NotExecuted {
            test, sequences, ..
        } => {
            if test.trim().is_empty() {
                return Err(RegistryError::UnnamedTest {
                    site: name,
                    phase: entry.phase.to_string(),
                });
            }
            if sequences.is_empty() || sequences.iter().any(|name| name.trim().is_empty()) {
                return Err(RegistryError::UnwitnessedNoExecution { site: name });
            }
        }
        Evidence::RecoveryProven { synthetic, .. } => {
            for element in site.residue_elements() {
                if !synthetic.iter().any(|record| record.element == *element) {
                    return Err(RegistryError::MissingSyntheticElement {
                        site: name,
                        element: *element,
                    });
                }
            }
            for record in synthetic {
                if !site.residue_elements().contains(&record.element) {
                    return Err(RegistryError::UnlistedSyntheticElement {
                        site: name,
                        element: record.element,
                    });
                }
            }
        }
    }

    Ok(())
}
