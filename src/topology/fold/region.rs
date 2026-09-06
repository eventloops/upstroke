//! Extended notes: `docs/internals/topology/fold/region.md`

use super::*;

pub(super) fn describe_region(paths: &PathSet) -> String {
    match paths.prefixes() {
        None => "the whole repository".to_owned(),
        Some([]) => "no path at all".to_owned(),
        Some(prefixes) => prefixes
            .iter()
            .map(|path| format!("`{path}`"))
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

fn disposition_name(disposition: LeaseDisposition) -> &'static str {
    match disposition {
        LeaseDisposition::PredictedReleased => "predicted-released",
        LeaseDisposition::PredictedRetained => "predicted-retained",
        LeaseDisposition::LineageHeld => "lineage-held",
    }
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
        recorded: disposition_name(recorded).to_owned(),
        owner: match lease {
            GenerationLease::Own => "leaseholding",
            GenerationLease::InheritedLineage { .. } => "lineage",
        },
        fate: "closes",
        expected: disposition_name(expected).to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::QuestionKind;

    const KIND: &str = "attempt_interrupted";
    const KEY: TaskKey = TaskKey(4);
    const ROOT: TaskKey = TaskKey(9);

    fn region(paths: &[&str]) -> PathSet {
        PathSet::Prefixes {
            paths: paths
                .iter()
                .map(|path| GitPath((*path).to_owned()))
                .collect(),
        }
    }

    fn question() -> FrozenQuestion {
        FrozenQuestion {
            id: QuestionId("q1".to_owned()),
            key: KEY,
            kind: QuestionKind::Unblock,
            context: "which adapter".to_owned(),
            options: vec!["codex".to_owned()],
        }
    }

    #[test]
    fn a_repo_wide_region_is_named_rather_than_listed() {
        assert_eq!(describe_region(&PathSet::RepoWide), "the whole repository");
    }

    #[test]
    fn an_empty_prefix_list_reads_as_a_region_and_not_as_an_unread_one() {
        assert_eq!(describe_region(&region(&[])), "no path at all");
        assert_ne!(
            describe_region(&region(&[])),
            describe_region(&PathSet::RepoWide),
            "an empty region and a repo-wide one are different answers"
        );
    }

    #[test]
    fn prefixes_are_quoted_and_listed_in_the_order_they_were_recorded() {
        assert_eq!(describe_region(&region(&["src/b"])), "`src/b`");
        assert_eq!(
            describe_region(&region(&["src/b", "src/a"])),
            "`src/b`, `src/a`"
        );
    }

    #[test]
    fn each_reason_a_candidate_is_ineligible_reads_as_its_own_clause() {
        assert_eq!(
            ineligible_detail(Ineligible::AwaitingInput),
            "its task is parked on a question"
        );
        assert_eq!(
            ineligible_detail(Ineligible::VerificationDeferred),
            "its verification is deferred until the backoff elapses"
        );
        assert_eq!(
            ineligible_detail(Ineligible::InsideLineage { root: ROOT }),
            "it overlaps the region lineage 9 holds"
        );
        assert_eq!(
            ineligible_detail(Ineligible::BehindOlderLineage { root: ROOT }),
            "it overlaps the region the older lineage 9 holds"
        );
    }

    #[test]
    fn the_two_lineage_reasons_are_told_apart_at_the_same_root() {
        assert_ne!(
            ineligible_detail(Ineligible::InsideLineage { root: ROOT }),
            ineligible_detail(Ineligible::BehindOlderLineage { root: ROOT }),
            "holding a lineage's region and standing behind an older one are different refusals"
        );
    }

    #[test]
    fn every_spawn_admission_has_a_name_of_its_own() {
        let names = [
            spawn_admission_name(&SpawnAdmission::Runnable),
            spawn_admission_name(&SpawnAdmission::HumanRequired {
                limit: 2,
                question: question(),
            }),
            spawn_admission_name(&SpawnAdmission::HumanBinding {
                options: vec!["codex".to_owned()],
                question: question(),
            }),
        ];
        assert_eq!(names, ["runnable", "human-required", "human-binding"]);
        assert_eq!(BTreeSet::from(names).len(), names.len());
    }

    #[test]
    fn every_entry_admission_has_a_name_of_its_own() {
        let names = [
            admission_name(&Admission::Runnable),
            admission_name(&Admission::HumanBinding {
                options: vec!["codex".to_owned()],
            }),
        ];
        assert_eq!(names, ["runnable", "human-binding"]);
        assert_eq!(BTreeSet::from(names).len(), names.len());
    }

    #[test]
    fn a_mismatched_admission_pair_is_named_by_two_different_words() {
        let binding = Admission::HumanBinding {
            options: vec!["codex".to_owned()],
        };
        let pairs = [
            (SpawnAdmission::Runnable, &binding),
            (
                SpawnAdmission::HumanRequired {
                    limit: 2,
                    question: question(),
                },
                &binding,
            ),
            (
                SpawnAdmission::HumanBinding {
                    options: vec!["codex".to_owned()],
                    question: question(),
                },
                &Admission::Runnable,
            ),
        ];
        for (event, entry) in pairs {
            assert_ne!(
                spawn_admission_name(&event),
                admission_name(entry),
                "a refusal that names both sides of a mismatch has to distinguish them"
            );
        }
    }

    #[test]
    fn a_lineage_position_renders_as_a_numbered_member() {
        assert_eq!(ordinal(0), "#0");
        assert_eq!(ordinal(3), "#3");
    }

    #[test]
    fn every_lease_disposition_has_a_name_of_its_own() {
        let names = [
            disposition_name(LeaseDisposition::PredictedReleased),
            disposition_name(LeaseDisposition::PredictedRetained),
            disposition_name(LeaseDisposition::LineageHeld),
        ];
        assert_eq!(
            names,
            ["predicted-released", "predicted-retained", "lineage-held"]
        );
        assert_eq!(BTreeSet::from(names).len(), names.len());
    }

    #[test]
    fn a_closing_generation_admits_exactly_the_disposition_its_holding_implies() {
        let lineage = GenerationLease::InheritedLineage { root: ROOT };
        let cases = [
            (
                GenerationLease::Own,
                LeaseDisposition::PredictedReleased,
                true,
            ),
            (
                GenerationLease::Own,
                LeaseDisposition::PredictedRetained,
                false,
            ),
            (GenerationLease::Own, LeaseDisposition::LineageHeld, false),
            (lineage, LeaseDisposition::LineageHeld, true),
            (lineage, LeaseDisposition::PredictedReleased, false),
            (lineage, LeaseDisposition::PredictedRetained, false),
        ];
        for (lease, recorded, admitted) in cases {
            assert_eq!(
                check_lease_disposition(KIND, KEY, lease, recorded).is_ok(),
                admitted,
                "{lease:?} against {recorded:?}"
            );
        }
    }

    #[test]
    fn a_refused_disposition_names_the_event_the_task_the_holding_and_both_values() {
        let refusal = check_lease_disposition(
            KIND,
            KEY,
            GenerationLease::Own,
            LeaseDisposition::LineageHeld,
        )
        .expect_err("a leaseholding generation that closes releases its predicted lease");
        assert!(
            matches!(
                &refusal,
                FoldError::InvalidLeaseDisposition {
                    kind: "attempt_interrupted",
                    key: 4,
                    recorded,
                    owner: "leaseholding",
                    fate: "closes",
                    expected,
                } if recorded == "lineage-held" && expected == "predicted-released"
            ),
            "{refusal:?}"
        );
        assert_eq!(
            refusal.to_string(),
            "`attempt_interrupted` for task 4 records the lease disposition `lineage-held`, and a \
             leaseholding generation that closes records `predicted-released`"
        );
    }

    #[test]
    fn a_lineage_generation_is_told_the_lineage_keeps_the_holding() {
        let refusal = check_lease_disposition(
            KIND,
            KEY,
            GenerationLease::InheritedLineage { root: ROOT },
            LeaseDisposition::PredictedReleased,
        )
        .expect_err("a lineage generation closes with the lineage still holding its region");
        assert_eq!(
            refusal.to_string(),
            "`attempt_interrupted` for task 4 records the lease disposition `predicted-released`, \
             and a lineage generation that closes records `lineage-held`"
        );
    }
}
