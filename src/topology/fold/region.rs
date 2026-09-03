//! Rendering helpers a refusal message is written from.

use super::*;

/// A region as a refusal names it.
///
/// The empty prefix list is spelled out rather than printed as `[]`: an empty
/// region and an unread one are different answers — [`PathSet::prefixes`] says
/// so — and a refusal that rendered the first as an empty pair of brackets
/// would read like a formatting accident next to `the whole repository`.
pub(super) fn describe_region(paths: &PathSet) -> String {
    match paths.prefixes() {
        None => "the whole repository".to_owned(),
        Some([]) => "no path at all".to_owned(),
        Some(prefixes) => prefixes
            .iter()
            .map(|path| format!("`{}`", path.as_str()))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// A ref name, for a diagnostic that has to print an `Option<GitRef>`.
pub(super) trait GitRefName {
    fn name(&self) -> &str;
}

impl GitRefName for GitRef {
    fn name(&self) -> &str {
        self.as_str()
    }
}

pub(super) fn ineligible_detail(why: Ineligible) -> String {
    match why {
        Ineligible::AwaitingInput => "its task is parked on a question".to_owned(),
        Ineligible::VerificationDeferred => {
            "its verification is deferred until the backoff elapses".to_owned()
        }
        Ineligible::InsideLineage { root } => {
            format!("it overlaps the region lineage {root} holds")
        }
        Ineligible::BehindOlderLineage { root } => {
            format!("it overlaps the region the older lineage {root} holds")
        }
    }
}

pub(super) fn spawn_admission_name(admission: &SpawnAdmission) -> &'static str {
    match admission {
        SpawnAdmission::Runnable => "runnable",
        SpawnAdmission::HumanRequired { .. } => "human-required",
        SpawnAdmission::HumanBinding { .. } => "human-binding",
    }
}

pub(super) fn admission_name(admission: &Admission) -> &'static str {
    match admission {
        Admission::Runnable => "runnable",
        Admission::HumanBinding { .. } => "human-binding",
    }
}

pub(super) fn ordinal(index: u32) -> String {
    format!("#{index}")
}

/// refusals[14]: the disposition an event records must be the one this
/// generation's holding admits.
/// The recorded disposition against the one the holding implies.
///
/// **Every caller passes a closing generation, and since 2026-08-27 there is no
/// other kind.** This took a `survives: bool`, and exactly one caller ever
/// passed `true`: `attempt_finished{Succeeded}`, the settlement that left a
/// generation open to hand its region to a candidate. That event is no longer a
/// settlement this fold accepts — `candidate_prepared` is the sole successful
/// one — so the parameter had a single reachable value and a second value that
/// documented a rule nothing could exercise.
///
/// **The surviving case did not disappear, it moved.** A generation that keeps
/// its region hands it over through `CandidatePrepared::lease_effect`, which
/// [`TopologyFold::check_candidate_prepared`] matches against the entry's
/// lineage — the same decision, on the event that now makes it.
/// [`GenerationLease::expected`] keeps both arms and its own table test,
/// because it is the statement of the rule rather than a caller of it.
pub(super) fn check_lease_disposition(
    kind: &'static str,
    key: TaskKey,
    lease: GenerationLease,
    recorded: LeaseDisposition,
) -> Result<(), FoldError> {
    let expected = lease.expected(false);
    if recorded == expected {
        return Ok(());
    }
    Err(FoldError::InvalidLeaseDisposition {
        kind,
        key: key.0,
        recorded: format!("{recorded:?}"),
        owner: match lease {
            GenerationLease::Own => "leaseholding",
            GenerationLease::InheritedLineage { .. } => "lineage",
        },
        fate: "closes",
        expected: format!("{expected:?}"),
    })
}
