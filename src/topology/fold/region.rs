//! Extended notes: `docs/internals/topology/fold/region.md`

use super::*;

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
