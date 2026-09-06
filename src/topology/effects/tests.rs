use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::topology::events::TOPOLOGY_EVENT_KINDS;

// -----------------------------------------------------------------------
// The independent tables
//
// Every expected value below is read off `decisions.effect_site_inventory`,
// `decisions.resource_accounting.rows` and `transaction_fault_matrix` and
// written here as a literal. Nothing in this module computes an expected
// value by calling the function under test: `row()` is never its own
// oracle, and neither is `fault_row()`, `scope()` or `adjacent()`.
//
// The tables are keyed by dotted name and asserted *total* over
// `EffectSiteId::all()`, so a site added without a table row fails rather
// than passing unchecked.
// -----------------------------------------------------------------------

/// `(site, row, fault row, scope, adjacency)` for every site in the
/// inventory.
///
/// One table rather than four, so that a site can only be missing from all
/// four at once — four separate lists would let one of them quietly lose a
/// row while the totality assertion on the others still passed.
#[allow(clippy::type_complexity)]
fn expected_attributes() -> Vec<(&'static str, ResourceRow, FaultRow, SiteScope, Adjacent)> {
    use Adjacent::{After, Before, None as NoAdjacent};
    use DurableEvent as E;
    use FaultRow as F;
    use ResourceRow as R;
    use SiteScope::{Legacy, Shared, Topology};
    vec![
        // Worktree: R9 task, R10 staging, R18 execution root.
        (
            "Worktree.CreateExecutionRoot",
            R::R18,
            F::TRunstart,
            Topology,
            Before(E::RunStarted),
        ),
        (
            "Worktree.RemoveExecutionRoot",
            R::R18,
            F::TFinalize,
            Topology,
            After(E::RunFinished),
        ),
        (
            "Worktree.WriteIntent",
            R::R9,
            F::TDispatch,
            Topology,
            After(E::TaskDispatched),
        ),
        (
            "Worktree.Add",
            R::R9,
            F::TDispatch,
            Topology,
            After(E::TaskDispatched),
        ),
        (
            "Worktree.Verify",
            R::R9,
            F::TRetry,
            Topology,
            Before(E::AttemptStarted),
        ),
        (
            "Worktree.Remove",
            R::R9,
            F::TScrub,
            Topology,
            After(E::TaskCandidateCreated),
        ),
        (
            "Worktree.RemoveIntent",
            R::R9,
            F::TScrub,
            Topology,
            After(E::TaskCandidateCreated),
        ),
        (
            "Worktree.WriteStagingIntent",
            R::R10,
            F::TProposal,
            Topology,
            Before(E::MergeVerificationStarted),
        ),
        (
            "Worktree.AddStaging",
            R::R10,
            F::TProposal,
            Topology,
            Before(E::MergeVerificationStarted),
        ),
        (
            "Worktree.RemoveStaging",
            R::R10,
            F::TProposal,
            Topology,
            After(E::TaskMerged),
        ),
        (
            "Worktree.RemoveStagingIntent",
            R::R10,
            F::TProposal,
            Topology,
            After(E::TaskMerged),
        ),
        // Snapshot: R24 throughout.
        (
            "Snapshot.WriteIntent",
            R::R24,
            F::TAttempt,
            Topology,
            After(E::AttemptStarted),
        ),
        (
            "Snapshot.Add",
            R::R24,
            F::TAttempt,
            Topology,
            After(E::AttemptStarted),
        ),
        (
            "Snapshot.Remove",
            R::R24,
            F::TScrub,
            Topology,
            Before(E::AttemptFinished),
        ),
        (
            "Snapshot.RemoveIntent",
            R::R24,
            F::TScrub,
            Topology,
            Before(E::AttemptFinished),
        ),
        // Ref: R11 candidates, R12 prepared pin, R23 candidate pin, R21 integration.
        (
            "Ref.CreateIntegration",
            R::R21,
            F::TRunstart,
            Topology,
            Before(E::RunStarted),
        ),
        (
            "Ref.CompareAndSwapIntegration",
            R::R21,
            F::TFast,
            Topology,
            Before(E::TaskMerged),
        ),
        (
            "Ref.CreateCandidates",
            R::R11,
            F::TCandRef,
            Topology,
            Before(E::TaskCandidateCreated),
        ),
        (
            "Ref.DeleteCandidatesRef",
            R::R11,
            F::TFinalize,
            Topology,
            After(E::RunFinished),
        ),
        (
            "Ref.PinCandidatePrepared",
            R::R23,
            F::TCandObj,
            Topology,
            Before(E::CandidatePrepared),
        ),
        (
            "Ref.DeleteCandidatePin",
            R::R23,
            F::TCandRef,
            Topology,
            After(E::TaskCandidateCreated),
        ),
        (
            "Ref.PinPrepared",
            R::R12,
            F::TProposal,
            Topology,
            Before(E::MergeVerificationStarted),
        ),
        (
            "Ref.DeletePreparedPin",
            R::R12,
            F::TFinalize,
            Topology,
            After(E::TaskMerged),
        ),
        // Object: the row that references the object immediately after the effect.
        (
            "Object.CandidateStage",
            R::R9,
            F::TAttempt,
            Topology,
            After(E::AttemptStarted),
        ),
        (
            "Object.CandidateWriteTree",
            R::R9,
            F::TAttempt,
            Topology,
            After(E::AttemptStarted),
        ),
        (
            "Object.SnapshotCommitTree",
            R::R27,
            F::TAttempt,
            Topology,
            After(E::AttemptStarted),
        ),
        (
            "Object.CandidateCommitTree",
            R::R27,
            F::TCandObj,
            Topology,
            Before(E::CandidatePrepared),
        ),
        (
            "Object.ProposalCherryPick",
            R::R10,
            F::TProposal,
            Topology,
            Before(E::MergeVerificationStarted),
        ),
        (
            "Object.RepairMaterialize",
            R::R9,
            F::TRepairDispatch,
            Topology,
            After(E::TaskDispatched),
        ),
        // RunDir: all R21, the packet says so in as many words.
        (
            "RunDir.CreatePublicDir",
            R::R21,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "RunDir.StageMarker",
            R::R21,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "RunDir.PublishMarker",
            R::R21,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "RunDir.RemoveMarker",
            R::R21,
            F::TRunstart,
            Shared,
            After(E::RunStarted),
        ),
        (
            "RunDir.CreatePrivateDir",
            R::R21,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "RunDir.StageOwnerRecord",
            R::R21,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "RunDir.PublishOwnerRecord",
            R::R21,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "RunDir.StageCommitRecord",
            R::R21,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "RunDir.PublishCommitRecord",
            R::R21,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "RunDir.WritePlan",
            R::R21,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "RunDir.WriteReport",
            R::R21,
            F::TFinalize,
            Shared,
            After(E::RunFinished),
        ),
        (
            "RunDir.WriteQuestionPayload",
            R::R21,
            F::TFailed,
            Shared,
            Before(E::QuestionRaised),
        ),
        (
            "RunDir.RemovePrivateHusk",
            R::R21,
            F::TRunstart,
            Shared,
            NoAdjacent,
        ),
        (
            "RunDir.RemovePublicHusk",
            R::R21,
            F::TRunstart,
            Shared,
            NoAdjacent,
        ),
        // Event: R21, T-APPEND, Shared but for the two legacy sites.
        ("Event.OpenLog", R::R21, F::TAppend, Shared, NoAdjacent),
        (
            "Event.ProvePrefixStable",
            R::R21,
            F::TAppend,
            Shared,
            NoAdjacent,
        ),
        ("Event.AppendFirst", R::R21, F::TAppend, Shared, NoAdjacent),
        ("Event.Append", R::R21, F::TAppend, Shared, NoAdjacent),
        (
            "Event.AppendInformational",
            R::R21,
            F::TAppend,
            Shared,
            NoAdjacent,
        ),
        (
            "Event.LegacyOpenLog",
            R::R21,
            F::TAppend,
            Legacy,
            NoAdjacent,
        ),
        ("Event.LegacyAppend", R::R21, F::TAppend, Legacy, NoAdjacent),
        // Answer: R21, T-ANSWER.
        (
            "Answer.StageWrite",
            R::R21,
            F::TAnswer,
            Shared,
            Before(E::QuestionAnswered),
        ),
        (
            "Answer.PublishRename",
            R::R21,
            F::TAnswer,
            Shared,
            Before(E::QuestionAnswered),
        ),
        (
            "Answer.Ingest",
            R::R21,
            F::TAnswer,
            Shared,
            Before(E::QuestionAnswered),
        ),
        // Lock: R17 holds, R25 the file, R28 the observed reaper hold.
        (
            "Lock.AcquireRun",
            R::R17,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "Lock.AcquireWorktree",
            R::R17,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "Lock.ProbeCleanupExclusive",
            R::R17,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "Lock.Release",
            R::R17,
            F::TFinalize,
            Shared,
            After(E::RunFinished),
        ),
        (
            "Lock.CreateWorktreeLockFile",
            R::R25,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        (
            "Lock.ObserveCleanupHold",
            R::R28,
            F::TRunstart,
            Shared,
            Before(E::RunStarted),
        ),
        // Report.
        (
            "Report.Write",
            R::R21,
            F::TFinalize,
            Shared,
            After(E::RunFinished),
        ),
        // Process: R22.
        (
            "Process.Spawn",
            R::R22,
            F::TAttempt,
            Topology,
            After(E::AttemptStarted),
        ),
        (
            "Process.Terminate",
            R::R22,
            F::TAttempt,
            Topology,
            Before(E::AttemptFinished),
        ),
        // Container: R19 the view, R26 the container.
        (
            "Container.WriteIntent",
            R::R26,
            F::TContainer,
            Topology,
            After(E::AttemptStarted),
        ),
        (
            "Container.Create",
            R::R26,
            F::TContainer,
            Topology,
            After(E::AttemptStarted),
        ),
        (
            "Container.Start",
            R::R26,
            F::TContainer,
            Topology,
            After(E::AttemptStarted),
        ),
        (
            "Container.MountGitView",
            R::R19,
            F::TContainer,
            Topology,
            After(E::AttemptStarted),
        ),
        (
            "Container.Stop",
            R::R26,
            F::TContainer,
            Topology,
            Before(E::AttemptFinished),
        ),
        (
            "Container.Remove",
            R::R26,
            F::TContainer,
            Topology,
            Before(E::AttemptFinished),
        ),
        (
            "Container.UnmountGitView",
            R::R19,
            F::TContainer,
            Topology,
            Before(E::AttemptFinished),
        ),
        (
            "Container.RemoveIntent",
            R::R26,
            F::TContainer,
            Topology,
            Before(E::AttemptFinished),
        ),
    ]
}

/// Every site the packet names in prose, by dotted name.
///
/// Kept separate from [`expected_attributes`] on purpose: that table is
/// this module's own inventory, and this list is the packet's. A site that
/// exists in the enums and is missing from the design would pass the first
/// and fail nothing; this list is what makes the second direction — the
/// design names it and the enums must have it — an assertion.
const NAMED_IN_THE_DESIGN: &[&str] = &[
    "RunDir.CreatePublicDir",
    "RunDir.StageMarker",
    "RunDir.PublishMarker",
    "RunDir.RemoveMarker",
    "RunDir.CreatePrivateDir",
    "RunDir.StageOwnerRecord",
    "RunDir.PublishOwnerRecord",
    "RunDir.StageCommitRecord",
    "RunDir.PublishCommitRecord",
    "RunDir.WritePlan",
    "RunDir.WriteReport",
    "RunDir.WriteQuestionPayload",
    "RunDir.RemovePrivateHusk",
    "RunDir.RemovePublicHusk",
    "Event.OpenLog",
    "Event.ProvePrefixStable",
    "Event.AppendFirst",
    "Event.Append",
    "Event.AppendInformational",
    "Answer.StageWrite",
    "Answer.PublishRename",
    "Answer.Ingest",
    "Lock.AcquireRun",
    "Lock.AcquireWorktree",
    "Lock.ProbeCleanupExclusive",
    "Lock.Release",
    "Lock.ObserveCleanupHold",
    "Object.CandidateStage",
    "Object.CandidateWriteTree",
    "Object.SnapshotCommitTree",
    "Object.CandidateCommitTree",
    "Object.ProposalCherryPick",
    "Object.RepairMaterialize",
    "Worktree.Verify",
    "Worktree.Remove",
    "Worktree.Add",
    "Worktree.AddStaging",
    "Snapshot.Add",
    "Snapshot.Remove",
    "Ref.PinCandidatePrepared",
    "Ref.PinPrepared",
    "Ref.DeleteCandidatesRef",
    "Ref.DeleteCandidatePin",
    "Ref.DeletePreparedPin",
    "Container.Create",
    "Container.Start",
    "Process.Spawn",
];

fn attribute_map() -> BTreeMap<&'static str, (ResourceRow, FaultRow, SiteScope, Adjacent)> {
    expected_attributes()
        .into_iter()
        .map(|(name, row, fault, scope, adjacent)| (name, (row, fault, scope, adjacent)))
        .collect()
}

// -----------------------------------------------------------------------
// The enums
// -----------------------------------------------------------------------

#[test]
fn every_group_enums_all_slice_lists_every_one_of_its_variants() {
    // Each block's match is exhaustive over its enum, so a new variant
    // fails to compile until it is listed here; the assertion then ties it
    // to a distinct slot of `ALL`, so a variant that compiles is one `ALL`
    // also lists. The length pins the count against a duplicate.
    macro_rules! tie {
        ($enum:ty, $count:expr, $slot:expr) => {{
            let all = <$enum>::ALL;
            assert_eq!(
                all.len(),
                $count,
                concat!(stringify!($enum), "::ALL length")
            );
            let mut seen = BTreeSet::new();
            for site in all {
                let position: usize = $slot(*site);
                assert_eq!(
                    all[position], *site,
                    concat!(stringify!($enum), " is not at the slot it claims")
                );
                assert!(seen.insert(position), "two variants claim one slot");
            }
            assert_eq!(seen.len(), $count);
        }};
    }

    tie!(WorktreeSite, 11, |site| match site {
        WorktreeSite::CreateExecutionRoot => 0,
        WorktreeSite::RemoveExecutionRoot => 1,
        WorktreeSite::WriteIntent => 2,
        WorktreeSite::Add => 3,
        WorktreeSite::Verify => 4,
        WorktreeSite::Remove => 5,
        WorktreeSite::RemoveIntent => 6,
        WorktreeSite::WriteStagingIntent => 7,
        WorktreeSite::AddStaging => 8,
        WorktreeSite::RemoveStaging => 9,
        WorktreeSite::RemoveStagingIntent => 10,
    });
    tie!(SnapshotSite, 4, |site| match site {
        SnapshotSite::WriteIntent => 0,
        SnapshotSite::Add => 1,
        SnapshotSite::Remove => 2,
        SnapshotSite::RemoveIntent => 3,
    });
    tie!(RefSite, 8, |site| match site {
        RefSite::CreateIntegration => 0,
        RefSite::CompareAndSwapIntegration => 1,
        RefSite::CreateCandidates => 2,
        RefSite::DeleteCandidatesRef => 3,
        RefSite::PinCandidatePrepared => 4,
        RefSite::DeleteCandidatePin => 5,
        RefSite::PinPrepared => 6,
        RefSite::DeletePreparedPin => 7,
    });
    tie!(ObjectSite, 6, |site| match site {
        ObjectSite::CandidateStage => 0,
        ObjectSite::CandidateWriteTree => 1,
        ObjectSite::SnapshotCommitTree => 2,
        ObjectSite::CandidateCommitTree => 3,
        ObjectSite::ProposalCherryPick => 4,
        ObjectSite::RepairMaterialize => 5,
    });
    tie!(RunDirSite, 14, |site| match site {
        RunDirSite::CreatePublicDir => 0,
        RunDirSite::StageMarker => 1,
        RunDirSite::PublishMarker => 2,
        RunDirSite::RemoveMarker => 3,
        RunDirSite::CreatePrivateDir => 4,
        RunDirSite::StageOwnerRecord => 5,
        RunDirSite::PublishOwnerRecord => 6,
        RunDirSite::StageCommitRecord => 7,
        RunDirSite::PublishCommitRecord => 8,
        RunDirSite::WritePlan => 9,
        RunDirSite::WriteReport => 10,
        RunDirSite::WriteQuestionPayload => 11,
        RunDirSite::RemovePrivateHusk => 12,
        RunDirSite::RemovePublicHusk => 13,
    });
    tie!(EventSite, 7, |site| match site {
        EventSite::OpenLog => 0,
        EventSite::ProvePrefixStable => 1,
        EventSite::AppendFirst => 2,
        EventSite::Append => 3,
        EventSite::AppendInformational => 4,
        EventSite::LegacyOpenLog => 5,
        EventSite::LegacyAppend => 6,
    });
    tie!(AnswerSite, 3, |site| match site {
        AnswerSite::StageWrite => 0,
        AnswerSite::PublishRename => 1,
        AnswerSite::Ingest => 2,
    });
    tie!(LockSite, 6, |site| match site {
        LockSite::AcquireRun => 0,
        LockSite::AcquireWorktree => 1,
        LockSite::ProbeCleanupExclusive => 2,
        LockSite::Release => 3,
        LockSite::CreateWorktreeLockFile => 4,
        LockSite::ObserveCleanupHold => 5,
    });
    tie!(ReportSite, 1, |site| match site {
        ReportSite::Write => 0,
    });
    tie!(ProcessSite, 2, |site| match site {
        ProcessSite::Spawn => 0,
        ProcessSite::Terminate => 1,
    });
    tie!(ContainerSite, 8, |site| match site {
        ContainerSite::WriteIntent => 0,
        ContainerSite::Create => 1,
        ContainerSite::Start => 2,
        ContainerSite::MountGitView => 3,
        ContainerSite::Stop => 4,
        ContainerSite::Remove => 5,
        ContainerSite::UnmountGitView => 6,
        ContainerSite::RemoveIntent => 7,
    });
}

#[test]
fn the_inventory_is_the_eleven_groups_and_every_one_of_them_has_sites() {
    assert_eq!(FunnelGroup::ALL.len(), 11);
    let sites = EffectSiteId::all();
    let mut by_group: BTreeMap<FunnelGroup, usize> = BTreeMap::new();
    for site in &sites {
        *by_group.entry(site.group()).or_default() += 1;
    }
    for group in FunnelGroup::ALL {
        assert!(
            by_group.get(group).copied().unwrap_or_default() > 0,
            "{group} declares no sites"
        );
    }
    assert_eq!(by_group.len(), 11, "a group with no sites at all");
    // Every dotted name is unique: two sites sharing one would make the
    // wire form ambiguous and `from_name` arbitrary.
    let names: BTreeSet<String> = sites.iter().map(|site| site.name()).collect();
    assert_eq!(names.len(), sites.len(), "two sites share a dotted name");
    // The site's group prefix is its group's own name, not a second copy.
    for site in &sites {
        assert!(
            site.name()
                .starts_with(&format!("{}.", site.group().name())),
            "{} is not named for its group",
            site.name()
        );
    }
}

#[test]
fn every_group_names_the_funnel_module_the_design_confines_it_to() {
    // From `mechanism`'s funnel-module list, written out here rather than
    // read back from `module()`.
    let expected = [
        (FunnelGroup::Worktree, "src/workspace_manager.rs"),
        (FunnelGroup::Snapshot, "src/workspace_manager.rs"),
        (FunnelGroup::Ref, "src/workspace_manager.rs"),
        (FunnelGroup::Object, "src/workspace_manager.rs"),
        (FunnelGroup::RunDir, "src/rundir.rs"),
        (FunnelGroup::Event, "src/events/log.rs"),
        (FunnelGroup::Answer, "src/interaction.rs"),
        (FunnelGroup::Lock, "src/rundir.rs"),
        (FunnelGroup::Report, "src/util.rs"),
        (FunnelGroup::Process, "src/runner/host.rs"),
        (FunnelGroup::Container, "src/runner/container.rs"),
    ];
    assert_eq!(expected.len(), FunnelGroup::ALL.len());
    for (group, module) in expected {
        assert_eq!(group.module(), module, "{group}");
    }
    // A site's module is its group's; nothing invents a per-site one.
    for site in EffectSiteId::all() {
        assert_eq!(site.module(), site.group().module(), "{site}");
    }
    // The legacy allowlist may never contain a topology module, so no
    // Topology-scoped site may name one of the modules PR5 freezes.
    for site in EffectSiteId::all() {
        if site.scope() == SiteScope::Topology {
            assert!(
                site.module().starts_with("src/topology/")
                    || site.module().starts_with("src/runner/")
                    || site.module() == "src/workspace_manager.rs",
                "{site} is Topology-scoped but lives in {}",
                site.module()
            );
        }
    }
}

// -----------------------------------------------------------------------
// Rows, fault rows, scope, adjacency
// -----------------------------------------------------------------------

#[test]
fn every_site_carries_the_row_fault_row_scope_and_adjacency_the_design_gives_it() {
    let table = attribute_map();
    let sites = EffectSiteId::all();
    assert_eq!(
        table.len(),
        sites.len(),
        "the expected-attribute table and the inventory are different sizes"
    );
    for site in &sites {
        let name = site.name();
        let (row, fault, scope, adjacent) = *table
            .get(name.as_str())
            .unwrap_or_else(|| panic!("{name} has no expected-attribute row"));
        assert_eq!(site.row(), row, "{name} row");
        assert_eq!(site.fault_row(), fault, "{name} fault row");
        assert_eq!(site.scope(), scope, "{name} scope");
        assert_eq!(site.adjacent(), adjacent, "{name} adjacency");
    }
    // Total in the other direction too: a table row naming a site that
    // does not exist is a table that stopped describing the enums.
    for name in table.keys() {
        EffectSiteId::from_name(name)
            .unwrap_or_else(|error| panic!("expected-attribute table: {error}"));
    }
}

#[test]
fn every_site_the_design_names_in_prose_exists_in_the_enums() {
    for name in NAMED_IN_THE_DESIGN {
        let site = EffectSiteId::from_name(name)
            .unwrap_or_else(|error| panic!("the design names {name}: {error}"));
        assert_eq!(&site.name(), name);
    }
    // The fourteen R21 run-directory sites the packet lists with "(all
    // R21)" after them, as one statement rather than fourteen.
    let rundir: Vec<EffectSiteId> = RunDirSite::ALL
        .iter()
        .copied()
        .map(EffectSiteId::RunDir)
        .collect();
    assert_eq!(rundir.len(), 14);
    for site in rundir {
        assert_eq!(site.row(), ResourceRow::R21, "{site}");
    }
}

#[test]
fn the_packets_group_level_row_statements_hold_over_whole_groups() {
    // Each of these is a sentence of `identity`, asserted over the whole
    // group rather than site by site — a per-site table can agree with a
    // mistake, a group-wide invariant cannot.
    let rows = |sites: Vec<EffectSiteId>| -> BTreeSet<ResourceRow> {
        sites.into_iter().map(EffectSiteId::row).collect()
    };
    let group = |wanted: FunnelGroup| -> Vec<EffectSiteId> {
        EffectSiteId::all()
            .into_iter()
            .filter(|site| site.group() == wanted)
            .collect()
    };

    // "Ref.* (R11/R12/R23/R21)"
    assert_eq!(
        rows(group(FunnelGroup::Ref)),
        BTreeSet::from([
            ResourceRow::R11,
            ResourceRow::R12,
            ResourceRow::R21,
            ResourceRow::R23
        ])
    );
    // "Worktree.*/Snapshot.* (R9/R10/R24/R18)"
    let mut worktree_and_snapshot = group(FunnelGroup::Worktree);
    worktree_and_snapshot.extend(group(FunnelGroup::Snapshot));
    assert_eq!(
        rows(worktree_and_snapshot),
        BTreeSet::from([
            ResourceRow::R9,
            ResourceRow::R10,
            ResourceRow::R18,
            ResourceRow::R24
        ])
    );
    // "Process.* (R22)"
    assert_eq!(
        rows(group(FunnelGroup::Process)),
        BTreeSet::from([ResourceRow::R22])
    );
    // "Container.* (R19/R26)"
    assert_eq!(
        rows(group(FunnelGroup::Container)),
        BTreeSet::from([ResourceRow::R19, ResourceRow::R26])
    );
    // "Lock.AcquireRun, ..., Lock.Release (R17; the worktree lock file
    // creation maps to R25; the reaper hold is observed through
    // Lock.ObserveCleanupHold, R28)"
    assert_eq!(
        rows(group(FunnelGroup::Lock)),
        BTreeSet::from([ResourceRow::R17, ResourceRow::R25, ResourceRow::R28])
    );
    assert_eq!(
        EffectSiteId::Lock(LockSite::CreateWorktreeLockFile).row(),
        ResourceRow::R25
    );
    assert_eq!(
        EffectSiteId::Lock(LockSite::ObserveCleanupHold).row(),
        ResourceRow::R28
    );
    // "Answer.StageWrite and Answer.PublishRename (... R21), Answer.Ingest"
    // and the whole Event and Report groups.
    for wanted in [FunnelGroup::Answer, FunnelGroup::Event, FunnelGroup::Report] {
        assert_eq!(
            rows(group(wanted)),
            BTreeSet::from([ResourceRow::R21]),
            "{wanted}"
        );
    }
    // "unreferenced, R27, until ..." for exactly the two commit-tree sites,
    // and no other Object site.
    let r27: BTreeSet<String> = group(FunnelGroup::Object)
        .into_iter()
        .filter(|site| site.row() == ResourceRow::R27)
        .map(|site| site.name())
        .collect();
    assert_eq!(
        r27,
        BTreeSet::from([
            "Object.SnapshotCommitTree".to_owned(),
            "Object.CandidateCommitTree".to_owned()
        ])
    );
}

#[test]
fn every_external_and_process_local_row_has_at_least_one_claimed_site() {
    // `outputs`: "every such row has at least one Topology/Shared site".
    let claimed: BTreeSet<ResourceRow> = EffectSiteId::claimed()
        .into_iter()
        .map(|s| s.row())
        .collect();
    assert_eq!(ResourceRow::ALL.len(), 15);
    for row in ResourceRow::ALL {
        assert!(claimed.contains(row), "{row} has no Topology/Shared site");
    }
    // And nothing outside the fifteen: the logical fold/broker rows take no
    // effect-site mapping, which is why they are not in the enum at all.
    for site in EffectSiteId::all() {
        assert!(ResourceRow::ALL.contains(&site.row()), "{site}");
    }
    // The domains, from `enforcement_domains`, written out independently.
    for (row, domain) in [
        (ResourceRow::R9, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R10, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R11, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R12, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R17, EnforcementDomain::ProcessLocalOs),
        (ResourceRow::R18, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R19, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R21, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R22, EnforcementDomain::ProcessLocalOs),
        (ResourceRow::R23, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R24, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R25, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R26, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R27, EnforcementDomain::ExternalPhysical),
        (ResourceRow::R28, EnforcementDomain::ProcessLocalOs),
    ] {
        assert_eq!(row.domain(), domain, "{row}");
    }
}

#[test]
fn every_fault_matrix_row_exists_and_the_ones_sites_use_are_used() {
    assert_eq!(FaultRow::ALL.len(), 21, "the matrix has twenty-one rows");
    let ids: BTreeSet<&str> = FaultRow::ALL.iter().map(|row| row.id()).collect();
    assert_eq!(ids.len(), 21, "two rows share an id");
    for id in [
        "T-RUNSTART",
        "T-DISPATCH",
        "T-ATTEMPT",
        "T-RETRY",
        "T-CAND-OBJ",
        "T-CAND-REF",
        "T-SCRUB",
        "T-FAILED",
        "T-RETAINED",
        "T-FAST",
        "T-PROPOSAL",
        "T-VERIFY",
        "T-PREPARED",
        "T-REJECT",
        "T-REPAIR-DISPATCH",
        "T-CONTAINER",
        "T-APPEND",
        "T-ANSWER",
        "T-FINISH",
        "T-FINALIZE",
        "T-RESUME",
    ] {
        assert!(ids.contains(id), "the matrix row {id} has no variant");
    }
    // The Object sites map to exactly the rows `structure` says they map
    // to: "T-ATTEMPT (b')/(b)/(c), T-CAND-OBJ (a), T-PROPOSAL (a')/(a),
    // T-REPAIR-DISPATCH, and T-DISPATCH" — the last of which is the
    // worktree the objects land behind, not an Object site itself.
    let object_rows: BTreeSet<FaultRow> = ObjectSite::ALL
        .iter()
        .map(|site| EffectSiteId::Object(*site).fault_row())
        .collect();
    assert_eq!(
        object_rows,
        BTreeSet::from([
            FaultRow::TAttempt,
            FaultRow::TCandObj,
            FaultRow::TProposal,
            FaultRow::TRepairDispatch
        ])
    );
    assert_eq!(
        EffectSiteId::Worktree(WorktreeSite::Add).fault_row(),
        FaultRow::TDispatch
    );
    // Every Event site is T-APPEND, as `structure` says in as many words.
    for site in EventSite::ALL {
        assert_eq!(
            EffectSiteId::Event(*site).fault_row(),
            FaultRow::TAppend,
            "{site:?}"
        );
    }
}

#[test]
fn the_adjacency_vocabulary_is_the_logs_vocabulary() {
    // A1 froze the twenty-four tags; this module mirrors them, and the
    // mirror is checked rather than assumed. A tag renamed in `events.rs`
    // has to break here.
    assert_eq!(DurableEvent::ALL.len(), TOPOLOGY_EVENT_KINDS.len());
    for (mine, theirs) in DurableEvent::ALL.iter().zip(TOPOLOGY_EVENT_KINDS.iter()) {
        assert_eq!(&mine.kind(), theirs, "the vocabularies diverged");
    }
    // Every adjacency names one of them.
    for site in EffectSiteId::all() {
        if let Some(kind) = site.adjacent().event() {
            assert!(
                TOPOLOGY_EVENT_KINDS.contains(&kind.kind()),
                "{site} is ordered against `{kind}`, which the log never writes"
            );
        }
    }
    // Exactly the Event group and the two husk-removal sites have no
    // adjacency: an append site *is* the event, and a census runs outside
    // any run's log.
    let unordered: BTreeSet<String> = EffectSiteId::all()
        .into_iter()
        .filter(|site| site.adjacent() == Adjacent::None)
        .map(|site| site.name())
        .collect();
    let mut expected: BTreeSet<String> = EventSite::ALL
        .iter()
        .map(|site| EffectSiteId::Event(*site).name())
        .collect();
    expected.insert("RunDir.RemovePrivateHusk".to_owned());
    expected.insert("RunDir.RemovePublicHusk".to_owned());
    assert_eq!(unordered, expected);
}

#[test]
fn an_adjacency_before_a_kind_is_not_the_adjacency_after_it() {
    // `effect_site_inventory.identity` requires adjacency to be "exactly
    // Before(kind), After(kind), or None" — so the direction is half the
    // value, and every test that compares adjacencies is trusting that
    // `PartialEq` reads it. Nothing said so: replace the derive with
    // equality over `event()` alone and `Before(run_finished)` becomes
    // equal to `After(run_finished)`, after which an opposite-direction
    // site satisfies every equality-based check in the module, including
    // the attribute table's.
    //
    // Written against the vocabulary rather than against a site, so it
    // holds independently of what any site's `adjacent()` happens to say.
    for kind in DurableEvent::ALL {
        let before = Adjacent::Before(*kind);
        let after = Adjacent::After(*kind);

        assert_ne!(before, after, "{kind}");
        assert!(!before.eq(&after), "{kind}");
        // They agree about the event and differ anyway: the event is the
        // part a direction-blind equality keeps, so a test that compared
        // only `event()` would pass under the mutation.
        assert_eq!(before.event(), after.event());
        assert_eq!(before.event(), Some(*kind));
        assert_ne!(before, Adjacent::None, "{kind}");
        assert_ne!(after, Adjacent::None, "{kind}");
        assert_eq!(before, Adjacent::Before(*kind));
        assert_eq!(after, Adjacent::After(*kind));

        // And the wire forms differ, so the direction survives a round
        // trip through the artifacts a gate reads rather than only through
        // the Rust value.
        let before_json = serde_json::to_string(&before).expect("adjacency serializes");
        let after_json = serde_json::to_string(&after).expect("adjacency serializes");
        assert_ne!(before_json, after_json, "{kind}");
        assert!(before_json.contains("before"), "{before_json}");
        assert!(after_json.contains("after"), "{after_json}");
        assert_eq!(
            serde_json::from_str::<Adjacent>(&before_json).expect("round trip"),
            before
        );
        assert_eq!(
            serde_json::from_str::<Adjacent>(&after_json).expect("round trip"),
            after
        );
        // The forged direction: the other form's JSON does not read back
        // as this one.
        assert_ne!(
            serde_json::from_str::<Adjacent>(&after_json).expect("round trip"),
            before
        );
    }

    // Two different kinds in the same direction are unequal too, so the
    // above is not passing on a `PartialEq` that reads only the direction.
    assert_ne!(
        Adjacent::Before(DurableEvent::RunStarted),
        Adjacent::Before(DurableEvent::RunFinished)
    );
    assert_ne!(
        Adjacent::After(DurableEvent::RunStarted),
        Adjacent::After(DurableEvent::RunFinished)
    );
    assert_eq!(Adjacent::None, Adjacent::None);
    assert_eq!(Adjacent::None.event(), None);

    // The consequence the framework actually depends on: the two
    // directions are the two observable orders, and they are different
    // orders.
    for kind in DurableEvent::ALL {
        assert_ne!(
            observable_orders_of(Adjacent::Before(*kind)),
            observable_orders_of(Adjacent::After(*kind)),
            "{kind}"
        );
    }
}

/// The orders an adjacency admits, read off the adjacency alone.
///
/// `EffectSiteId::observable_orders` is a method on a site; this is the
/// same rule applied to the adjacency by itself, so the test above does
/// not need a site that happens to carry the direction it is testing.
fn observable_orders_of(adjacent: Adjacent) -> &'static [ObservableOrder] {
    match adjacent {
        Adjacent::Before(_) => &[ObservableOrder::EffectBeforeEvent],
        Adjacent::After(_) => &[ObservableOrder::EventBeforeEffect],
        Adjacent::None => &[],
    }
}

#[test]
fn the_observable_orders_are_the_ones_the_adjacency_admits() {
    // Crossed over every site rather than sampled: the relation is
    // "`Before` admits effect-before-event, `After` admits
    // event-before-effect, no adjacency admits neither", and a site that
    // broke it would be one whose registry entry could carry an order the
    // design never produced.
    let mut before = 0;
    let mut after = 0;
    let mut neither = 0;
    for site in EffectSiteId::all() {
        match site.adjacent() {
            Adjacent::Before(_) => {
                assert_eq!(
                    site.observable_orders(),
                    &[ObservableOrder::EffectBeforeEvent],
                    "{site}"
                );
                before += 1;
            }
            Adjacent::After(_) => {
                assert_eq!(
                    site.observable_orders(),
                    &[ObservableOrder::EventBeforeEffect],
                    "{site}"
                );
                after += 1;
            }
            Adjacent::None => {
                assert!(site.observable_orders().is_empty(), "{site}");
                neither += 1;
            }
        }
    }
    // All three arms are populated, so the crossing is a crossing.
    assert!(
        before > 0 && after > 0 && neither > 0,
        "{before}/{after}/{neither}"
    );
}

#[test]
fn only_the_legacy_event_sites_are_outside_the_claim() {
    let unclaimed: BTreeSet<String> = EffectSiteId::all()
        .into_iter()
        .filter(|site| !site.scope().is_claimed())
        .map(|site| site.name())
        .collect();
    assert_eq!(
        unclaimed,
        BTreeSet::from([
            "Event.LegacyOpenLog".to_owned(),
            "Event.LegacyAppend".to_owned()
        ]),
        "`scope` puts only schema-1..3 callers of a shared funnel outside the claim"
    );
    assert!(SiteScope::Topology.is_claimed());
    assert!(SiteScope::Shared.is_claimed());
    assert!(!SiteScope::Legacy.is_claimed());
    // Both claimed scopes are populated, and each by more than one group.
    let topology: BTreeSet<FunnelGroup> = EffectSiteId::all()
        .into_iter()
        .filter(|site| site.scope() == SiteScope::Topology)
        .map(|site| site.group())
        .collect();
    let shared: BTreeSet<FunnelGroup> = EffectSiteId::all()
        .into_iter()
        .filter(|site| site.scope() == SiteScope::Shared)
        .map(|site| site.group())
        .collect();
    assert_eq!(
        topology,
        BTreeSet::from([
            FunnelGroup::Worktree,
            FunnelGroup::Snapshot,
            FunnelGroup::Ref,
            FunnelGroup::Object,
            FunnelGroup::Process,
            FunnelGroup::Container
        ])
    );
    assert_eq!(
        shared,
        BTreeSet::from([
            FunnelGroup::RunDir,
            FunnelGroup::Event,
            FunnelGroup::Answer,
            FunnelGroup::Lock,
            FunnelGroup::Report
        ])
    );
}

#[test]
fn the_read_only_sites_are_the_four_the_design_says_perform_no_effect() {
    let read_only: BTreeSet<String> = EffectSiteId::all()
        .into_iter()
        .filter(|site| site.is_read_only())
        .map(|site| site.name())
        .collect();
    assert_eq!(
        read_only,
        BTreeSet::from([
            "Worktree.Verify".to_owned(),
            "Event.ProvePrefixStable".to_owned(),
            "Answer.Ingest".to_owned(),
            "Lock.ObserveCleanupHold".to_owned(),
        ]),
        "a read-only observation performs no effect and an effect site is not one"
    );
    // A read-only site still has both hook phases — it is a funnel API and
    // the funnel calls the hooks — but it registers no residue class,
    // because there is nothing for a fault to leave behind.
    for name in &read_only {
        let site = EffectSiteId::from_name(name).expect("named above");
        assert!(site.residue_classes().is_empty(), "{name}");
    }
}

// -----------------------------------------------------------------------
// Sub-effect points
// -----------------------------------------------------------------------

#[test]
fn every_sub_effect_point_supports_the_modes_and_platform_the_design_gives_it() {
    use InjectionMode::{ErrorReturn, Kill};
    use Platform::{Any, Unix, Windows};
    // Written from `command_internal_sub_effects` and
    // `containment_sub_effects`, not read back from `modes()`.
    let expected: &[(SubEffectPoint, &[InjectionMode], Platform)] = &[
        (SubEffectPoint::IdUnread, &[Kill], Any),
        (SubEffectPoint::Written, &[Kill, ErrorReturn], Any),
        (SubEffectPoint::WrittenFull, &[ErrorReturn], Any),
        (SubEffectPoint::Synced, &[Kill, ErrorReturn], Any),
        (SubEffectPoint::Create, &[Kill, ErrorReturn], Any),
        (SubEffectPoint::TruncateTornTail, &[Kill, ErrorReturn], Any),
        (SubEffectPoint::SyncPrefix, &[Kill, ErrorReturn], Any),
        (
            SubEffectPoint::AmbientJobJoined,
            &[Kill, ErrorReturn],
            Windows,
        ),
        (SubEffectPoint::CreatedSuspended, &[Kill], Windows),
        (SubEffectPoint::PrivateJobAssigned, &[Kill], Windows),
        (SubEffectPoint::Resumed, &[Kill], Windows),
        (SubEffectPoint::ReaperStarted, &[Kill], Unix),
        (SubEffectPoint::PreExecPgidAndRegister, &[Kill], Unix),
        (SubEffectPoint::Exec, &[Kill], Unix),
        (SubEffectPoint::Registered, &[Kill], Unix),
    ];
    assert_eq!(expected.len(), SubEffectPoint::ALL.len());
    for (point, modes, platform) in expected {
        assert_eq!(point.modes(), *modes, "{point} modes");
        assert_eq!(point.platform(), *platform, "{point} platform");
        for mode in InjectionMode::ALL {
            assert_eq!(
                point.supports(*mode),
                modes.contains(mode),
                "{point} {mode:?}"
            );
        }
    }
    // Kill is all but universal: a coordinator can die anywhere, so a
    // point that did not support it would generally be one the framework
    // could not model a crash at. `WrittenFull` is the single exception,
    // and it is one because a kill there is already a required coordinate
    // under another point rather than because it cannot happen.
    // `structure` tables "kill entries for Written (torn ...;
    // complete-unsynced ...) and Synced" — two kill entries for an append
    // site — and "error-return entries for Written-partial-then-Err,
    // Written-full-then-flush-Err, and Synced-Err" — three. A kill at
    // `WrittenFull` leaves the complete-unsynced prefix `Written`'s kill
    // entry covers, so declaring the mode would require a third kill entry
    // the design does not table.
    for point in SubEffectPoint::ALL {
        if *point == SubEffectPoint::WrittenFull {
            assert_eq!(point.modes(), &[ErrorReturn]);
            continue;
        }
        assert!(point.supports(Kill), "{point}");
    }
    // Both modes and all three platforms are represented, so the two
    // tables above are crossings rather than constants.
    let modes: BTreeSet<InjectionMode> = SubEffectPoint::ALL
        .iter()
        .flat_map(|point| point.modes().iter().copied())
        .collect();
    assert_eq!(modes.len(), 2);
    let platforms: BTreeSet<Platform> = SubEffectPoint::ALL.iter().map(|p| p.platform()).collect();
    assert_eq!(platforms.len(), 3);
}

#[test]
fn the_sites_that_expose_points_are_the_ones_the_design_names() {
    let expected: &[(&str, &[SubEffectPoint])] = &[
        (
            "Event.OpenLog",
            &[
                SubEffectPoint::Create,
                SubEffectPoint::TruncateTornTail,
                SubEffectPoint::SyncPrefix,
            ],
        ),
        (
            "Event.AppendFirst",
            &[
                SubEffectPoint::Written,
                SubEffectPoint::WrittenFull,
                SubEffectPoint::Synced,
            ],
        ),
        (
            "Event.Append",
            &[
                SubEffectPoint::Written,
                SubEffectPoint::WrittenFull,
                SubEffectPoint::Synced,
            ],
        ),
        (
            "Event.AppendInformational",
            &[
                SubEffectPoint::Written,
                SubEffectPoint::WrittenFull,
                SubEffectPoint::Synced,
            ],
        ),
        ("Object.SnapshotCommitTree", &[SubEffectPoint::IdUnread]),
        ("Object.CandidateCommitTree", &[SubEffectPoint::IdUnread]),
        (
            "Process.Spawn",
            &[
                SubEffectPoint::AmbientJobJoined,
                SubEffectPoint::CreatedSuspended,
                SubEffectPoint::PrivateJobAssigned,
                SubEffectPoint::Resumed,
                SubEffectPoint::ReaperStarted,
                SubEffectPoint::PreExecPgidAndRegister,
                SubEffectPoint::Exec,
                SubEffectPoint::Registered,
            ],
        ),
    ];
    let named: BTreeSet<&str> = expected.iter().map(|(name, _)| *name).collect();
    for (name, points) in expected {
        let site = EffectSiteId::from_name(name).expect("a site the design names");
        assert_eq!(site.sub_effects(), *points, "{name}");
    }
    // Nothing else exposes one. `IdUnread` is "the two commit-tree sites"
    // and no other Object site, because the rest have no post-child prefix
    // the parent can stand in.
    for site in EffectSiteId::all() {
        if !named.contains(site.name().as_str()) {
            assert!(
                site.sub_effects().is_empty(),
                "{site} exposes points the design does not give it"
            );
        }
    }
    // The Legacy sites expose none: they carry no fault-registry
    // requirement, and a point would manufacture one.
    for site in [EventSite::LegacyOpenLog, EventSite::LegacyAppend] {
        assert!(EffectSiteId::Event(site).sub_effects().is_empty());
    }
    // Both injection modes are reachable through a real site, and so is a
    // kill-only point — the crossing the harness and registry range over.
    let append = EffectSiteId::Event(EventSite::AppendFirst);
    assert!(append.exposes(SubEffectPoint::Written, InjectionMode::Kill));
    assert!(append.exposes(SubEffectPoint::Written, InjectionMode::ErrorReturn));

    // `structure` names exactly two kill entries and three error-return
    // entries for an append site, and the three error-return cases are
    // distinct durable shapes: a partial line the next open truncates, a
    // complete unsynced line the barrier makes durable, and a synced line
    // whose sync reported failure. They are three *keys*, so a suite
    // cannot execute one and have the coordinate read as complete, and the
    // format cannot be handed both under one key without refusing the
    // second as a duplicate.
    for site in [
        EventSite::AppendFirst,
        EventSite::Append,
        EventSite::AppendInformational,
    ] {
        let site = EffectSiteId::Event(site);
        let kills: Vec<SubEffectPoint> = site
            .sub_effects()
            .iter()
            .copied()
            .filter(|point| point.supports(InjectionMode::Kill))
            .collect();
        let errors: Vec<SubEffectPoint> = site
            .sub_effects()
            .iter()
            .copied()
            .filter(|point| point.supports(InjectionMode::ErrorReturn))
            .collect();
        assert_eq!(
            kills,
            vec![SubEffectPoint::Written, SubEffectPoint::Synced],
            "{site} kill entries"
        );
        assert_eq!(
            errors,
            vec![
                SubEffectPoint::Written,
                SubEffectPoint::WrittenFull,
                SubEffectPoint::Synced,
            ],
            "{site} error-return entries"
        );
        // And the two written cases are different keys, so a registry
        // holding both is a registry rather than a duplicate.
        assert_ne!(
            EntryPhase::Point {
                point: SubEffectPoint::Written,
                mode: InjectionMode::ErrorReturn,
            },
            EntryPhase::Point {
                point: SubEffectPoint::WrittenFull,
                mode: InjectionMode::ErrorReturn,
            }
        );
    }
    let commit_tree = EffectSiteId::Object(ObjectSite::CandidateCommitTree);
    assert!(commit_tree.exposes(SubEffectPoint::IdUnread, InjectionMode::Kill));
    assert!(!commit_tree.exposes(SubEffectPoint::IdUnread, InjectionMode::ErrorReturn));
    assert!(!commit_tree.exposes(SubEffectPoint::Written, InjectionMode::Kill));
}

// -----------------------------------------------------------------------
// Residue classes
// -----------------------------------------------------------------------

#[test]
fn the_residue_class_is_registered_exactly_where_the_design_registers_it() {
    // "every Object site carries a registered residue class
    // ObjectResidue::Internal", and "the classifier is total over {None,
    // Internal, After} for every Object site and for
    // Worktree.Add/Snapshot.Add".
    let expected: BTreeSet<String> = ObjectSite::ALL
        .iter()
        .map(|site| EffectSiteId::Object(*site).name())
        .chain([
            EffectSiteId::Worktree(WorktreeSite::Add).name(),
            EffectSiteId::Worktree(WorktreeSite::AddStaging).name(),
            EffectSiteId::Snapshot(SnapshotSite::Add).name(),
        ])
        .collect();
    let actual: BTreeSet<String> = EffectSiteId::all()
        .into_iter()
        .filter(|site| !site.residue_classes().is_empty())
        .map(|site| site.name())
        .collect();
    assert_eq!(actual, expected);
    for name in &actual {
        let site = EffectSiteId::from_name(name).expect("listed above");
        assert_eq!(site.residue_classes(), &[ResidueClass::ObjectInternal]);
        assert!(site.registers(ResidueClass::ObjectInternal), "{name}");
        assert!(
            !site.residue_elements().is_empty(),
            "{name} registers a class with nothing to construct"
        );
    }
    // A class is never an executed hook: its label is fixed, and the
    // classifier outcome it stands for is `Internal` and not the two the
    // classifier can also answer.
    assert_eq!(ResidueClass::ALL.len(), 1);
    assert_eq!(
        ResidueClass::ObjectInternal.label(),
        EvidenceLabel::RecoveryProven
    );
    assert_eq!(
        ResidueClass::ObjectInternal.classified_as(),
        ObjectResidue::Internal
    );
    assert_eq!(
        ObjectResidue::ALL.len(),
        3,
        "the classifier is total over three"
    );
}

#[test]
fn each_site_lists_the_residue_elements_its_own_command_can_leave() {
    use ResidueElement as X;
    // From each transaction's own residue description in the fault matrix.
    // The lists differ by command on purpose: a killed `commit-tree`
    // touches no index, so an `index.lock` in its list would be a residue
    // element nothing could ever construct there.
    let expected: &[(&str, &[ResidueElement])] = &[
        (
            "Object.CandidateStage",
            &[X::UnreferencedObject, X::TemporaryObjectFile, X::IndexLock],
        ),
        (
            "Object.CandidateWriteTree",
            &[X::UnreferencedObject, X::TemporaryObjectFile, X::IndexLock],
        ),
        (
            "Object.SnapshotCommitTree",
            &[X::UnreferencedObject, X::TemporaryObjectFile],
        ),
        (
            "Object.CandidateCommitTree",
            &[X::UnreferencedObject, X::TemporaryObjectFile],
        ),
        (
            "Object.ProposalCherryPick",
            &[
                X::UnreferencedObject,
                X::TemporaryObjectFile,
                X::IndexLock,
                X::CherryPickHead,
                X::MergeHead,
                X::MergeMsg,
                X::SequencerState,
            ],
        ),
        (
            "Object.RepairMaterialize",
            &[
                X::UnreferencedObject,
                X::TemporaryObjectFile,
                X::IndexLock,
                X::CherryPickHead,
            ],
        ),
        ("Worktree.Add", &[X::RegisteredUnpopulatedWorktree]),
        ("Worktree.AddStaging", &[X::RegisteredUnpopulatedWorktree]),
        ("Snapshot.Add", &[X::RegisteredUnpopulatedWorktree]),
    ];
    for (name, elements) in expected {
        let site = EffectSiteId::from_name(name).expect("a site with a residue class");
        assert_eq!(site.residue_elements(), *elements, "{name}");
    }
    // The lists are genuinely different: five distinct list lengths across
    // nine sites, so a `residue_elements` that answered one constant would
    // fail rather than pass.
    let lengths: BTreeSet<usize> = expected
        .iter()
        .map(|(_, elements)| elements.len())
        .collect();
    assert_eq!(lengths, BTreeSet::from([1, 2, 3, 4, 7]), "{lengths:?}");
    // Every element classifies `Internal`; that is what makes the answer a
    // class rather than a list of files.
    assert_eq!(ResidueElement::ALL.len(), 9);
    for element in ResidueElement::ALL {
        assert_eq!(
            element.classifies_as(),
            ObjectResidue::Internal,
            "{element:?}"
        );
    }
    // `ORIG_HEAD` is classifiable and is on no site's construction list:
    // the classifier's definition names it, the synthetic-construction list
    // does not, and this framework does not invent an element for a site to
    // have to build.
    let constructed: BTreeSet<ResidueElement> = EffectSiteId::all()
        .into_iter()
        .flat_map(|site| site.residue_elements().iter().copied())
        .collect();
    assert!(!constructed.contains(&ResidueElement::OrigHead));
    assert_eq!(constructed.len(), 8);
}

#[test]
fn a_residue_class_is_not_a_hook_phase() {
    // The type says so: an `EntryPhase::Residue` has no hook phase, and a
    // hook phase has no residue class. This is the first of the two places
    // the framework refuses to count a class as an execution — the second
    // is the format, below, which refuses the claim even when it is made.
    let class = EntryPhase::Residue {
        class: ResidueClass::ObjectInternal,
    };
    assert_eq!(class.hook_phase(), None);
    assert_eq!(class.residue_class(), Some(ResidueClass::ObjectInternal));
    assert_eq!(class.required_label(), EvidenceLabel::RecoveryProven);
    for phase in [
        EntryPhase::Before,
        EntryPhase::After,
        EntryPhase::Point {
            point: SubEffectPoint::Synced,
            mode: InjectionMode::ErrorReturn,
        },
    ] {
        assert!(phase.hook_phase().is_some(), "{phase}");
        assert_eq!(phase.residue_class(), None, "{phase}");
        assert_eq!(phase.required_label(), EvidenceLabel::ExecutionObserved);
    }
    // A no-execution record is neither: nothing ran, and nothing was left.
    assert_eq!(EntryPhase::NoExecution.hook_phase(), None);
    assert_eq!(EntryPhase::NoExecution.residue_class(), None);
    assert_eq!(
        EntryPhase::NoExecution.required_label(),
        EvidenceLabel::ExecutionObserved
    );
}

#[test]
fn only_the_three_sites_a_fast_sequence_skips_may_record_that_they_did_not_run() {
    let skipped: BTreeSet<String> = EffectSiteId::all()
        .into_iter()
        .filter(|site| site.skipped_on_fast_path())
        .map(|site| site.name())
        .collect();
    assert_eq!(
        skipped,
        BTreeSet::from([
            "Worktree.AddStaging".to_owned(),
            "Object.ProposalCherryPick".to_owned(),
            "Ref.PinPrepared".to_owned(),
        ]),
        "`structure` names exactly these three as asserted-not-executed"
    );
}

#[test]
fn a_site_name_no_group_declares_is_refused() {
    for name in [
        "RunDir.NoSuchSite",
        "NoSuchGroup.CreatePublicDir",
        "CreatePublicDir",
        "RunDir.createpublicdir",
        "RunDir.CreatePublicDir ",
        "",
    ] {
        let error = EffectSiteId::from_name(name).expect_err("must be refused");
        assert_eq!(error.name, name);
        assert!(error.to_string().contains(name), "{error}");
    }
    // And the round trip holds for every real one.
    for site in EffectSiteId::all() {
        assert_eq!(EffectSiteId::from_name(&site.name()), Ok(site));
    }
}

// -----------------------------------------------------------------------
// ST-07: the framework self-test
//
// A synthetic exercise — nothing here performs an effect — of the whole
// loop: sites out of the enums, executions into the harness, entries into
// the registry format, and the bijection over the three.
//
// Fixture hostility, stated as counts rather than as a claim (§8.2 of this
// slice's contract). Across the thirty entries of `self_test_registry`:
//
//   order              3 distinct  (None, EffectBeforeEvent, EventBeforeEffect)
//   fault_row          5 distinct  (T-APPEND, T-CAND-OBJ, T-ATTEMPT,
//                                 T-REPAIR-DISPATCH, T-PROPOSAL)
//   expected_residue.rows  6 distinct row sets, incl. the empty one
//   expected_residue.detail  30 distinct (one per entry)
//   resume_action      30 distinct (one per entry)
//   label               2 distinct
//   evidence kind       3 distinct (Executed, RecoveryProven, NotExecuted)
//   evidence test name 28 distinct (the two residue entries name none)
//   sampling.n          2 distinct (61 and 23)
//   sampling histogram  2 distinct, one with internal == 0 and one > 0
//   synthetic element   4 distinct across the two residue entries
//
// The counts are asserted by `the_self_test_fixture_varies_every_field_it_reads`,
// so they cannot drift away from this comment silently.
// -----------------------------------------------------------------------

/// The sites the self-test drives.
///
/// Chosen to cover every shape the framework has: a site with three points
/// in two modes each (`Event.OpenLog`), one with two points in two modes
/// (`Event.AppendFirst`), one with a kill-only point *and* a residue class
/// (`Object.CandidateCommitTree`), one with a residue class and no points
/// (`Object.RepairMaterialize`), one whose points are platform-scoped
/// (`Process.Spawn`), one whose before phase finds its target already
/// durable (`Worktree.Remove`), the three a fast sequence skips, and a
/// Legacy site that must be exempt.
///
/// `Worktree.Remove` is here because a fixture in which a classification
/// never occurs cannot show that the format reads it: the entry a
/// packet-correct registry writes for `Worktree.Remove`'s before phase
/// carries `[R9]`, and the authority that predated PR3-ST07-011 refused
/// exactly that entry. All three [`BeforeState`] answers now occur here —
/// `Worktree.Remove` is [`BeforeState::Present`], `Worktree.AddStaging`
/// (already present as one of the three sites a fast sequence skips) is
/// [`BeforeState::PrecursorDurable`], and the rest are
/// [`BeforeState::Absent`] — and
/// `the_self_test_fixture_varies_every_field_it_reads` asserts that,
/// because the two non-empty answers name the same row and differ only in
/// the words the format checks.
fn self_test_inventory() -> Vec<EffectSiteId> {
    vec![
        EffectSiteId::Event(EventSite::OpenLog),
        EffectSiteId::Event(EventSite::AppendFirst),
        EffectSiteId::Event(EventSite::LegacyAppend),
        EffectSiteId::Object(ObjectSite::CandidateCommitTree),
        EffectSiteId::Object(ObjectSite::RepairMaterialize),
        EffectSiteId::Process(ProcessSite::Spawn),
        EffectSiteId::Worktree(WorktreeSite::Remove),
        EffectSiteId::Worktree(WorktreeSite::AddStaging),
        EffectSiteId::Object(ObjectSite::ProposalCherryPick),
        EffectSiteId::Ref(RefSite::PinPrepared),
    ]
}

/// The sites the self-test asserts did *not* run.
fn fast_path_skipped() -> Vec<EffectSiteId> {
    vec![
        EffectSiteId::Worktree(WorktreeSite::AddStaging),
        EffectSiteId::Object(ObjectSite::ProposalCherryPick),
        EffectSiteId::Ref(RefSite::PinPrepared),
    ]
}

/// The one order an entry for this site must carry, or `None`.
fn only_order(site: EffectSiteId) -> Option<ObservableOrder> {
    site.observable_orders().first().copied()
}

/// Run one site through both hook phases and every point required on
/// `host`, exactly as a funnel would.
fn drive(harness: &mut HookHarness, site: EffectSiteId, host: Host) {
    harness.hook(site, HookPhase::Before);
    for point in site.sub_effects() {
        if !point.platform().required_on(host) {
            continue;
        }
        for mode in point.modes() {
            harness
                .arm(site, *point, *mode)
                .expect("the site exposes this point in this mode");
            harness.hook(
                site,
                HookPhase::Point {
                    point: *point,
                    mode: *mode,
                },
            );
        }
    }
    harness.disarm();
    harness.hook(site, HookPhase::After);
}

/// A harness that has driven every site of the inventory that is supposed
/// to run, and none of the three a fast sequence skips.
fn self_test_harness(host: Host) -> HookHarness {
    let mut harness = HookHarness::new();
    // Every fast sequence the suite runs is recorded by name, and the
    // sites a fast publication skips run in none of them. A harness that
    // exercised no sequence would substantiate the no-execution record by
    // having done nothing, which is what this shape exists to prevent.
    for sequence in FAST_SEQUENCES {
        harness.begin_fast_sequence(sequence);
        for site in self_test_inventory() {
            if site.skipped_on_fast_path() || !site.scope().is_claimed() {
                continue;
            }
            drive(&mut harness, site, host);
        }
        harness.end_fast_sequence();
    }
    // And the stale-candidate path, outside every fast sequence: a staging
    // worktree is added, the proposal is cherry-picked and a prepared pin
    // is taken. These are ordinary claimed sites and ST-07 requires their
    // hook phases observed; the no-execution record says only that they do
    // not run *inside* a fast sequence. A suite that never drove them
    // would have no coverage of them at all, which is the report the
    // no-execution entry used to stand in for.
    harness.end_fast_sequence();
    for site in self_test_inventory() {
        if !site.skipped_on_fast_path() || !site.scope().is_claimed() {
            continue;
        }
        drive(&mut harness, site, host);
    }
    harness
}

/// The rows the fixture writes, which are the site's own semantics.
///
/// Deliberately the production authority rather than a second copy of it:
/// the fixture's job is to be a registry the format accepts, and a fixture
/// carrying its own table would only prove the two tables agree. What the
/// *values* are is asserted separately, against the packet's words, in
/// `the_expected_residue_of_a_phase_is_the_sites_own_semantics`.
fn residue_rows(site: EffectSiteId, phase: EntryPhase) -> Vec<ResourceRow> {
    site.expected_rows(phase)
}

/// The resume action an entry in this phase must name.
///
/// The production authority, for the same reason [`residue_rows`] is: the
/// fixture's job is to be a registry the format accepts. What the values
/// *are* is asserted against the packet's words by the independent oracle
/// in `the_residue_and_recovery_authority_is_exhaustive_and_says_what_the_packet_says`.
fn resume_action(site: EffectSiteId, phase: EntryPhase) -> String {
    site.semantics(phase).action.text().to_owned()
}

/// The residue detail an entry in this phase must carry.
fn residue_detail(site: EffectSiteId, phase: EntryPhase) -> String {
    site.semantics(phase).artifact.detail().to_owned()
}

fn hook_entry(site: EffectSiteId, phase: EntryPhase) -> RegistryEntry {
    let name = format!("{site}/{phase}");
    RegistryEntry {
        site,
        phase,
        order: only_order(site),
        fault_row: site.fault_row(),
        expected_residue: ExpectedResidue {
            rows: residue_rows(site, phase),
            detail: residue_detail(site, phase),
        },
        resume_action: resume_action(site, phase),
        label: EvidenceLabel::ExecutionObserved,
        evidence: Evidence::Executed {
            test: format!("st07::{name}"),
            passed: true,
        },
    }
}

/// A residue-class entry. `internal` is how many of the `n` samples landed
/// in the internal window — zero is legal and is one of the two cases the
/// self-test carries.
fn residue_entry(site: EffectSiteId, n: u32, internal: u32) -> RegistryEntry {
    let phase = EntryPhase::Residue {
        class: ResidueClass::ObjectInternal,
    };
    let none = (n - internal) / 2;
    let after = n - internal - none;
    RegistryEntry {
        site,
        phase,
        order: only_order(site),
        fault_row: site.fault_row(),
        expected_residue: ExpectedResidue {
            rows: residue_rows(site, phase),
            detail: residue_detail(site, phase),
        },
        resume_action: resume_action(site, phase),
        label: EvidenceLabel::RecoveryProven,
        evidence: Evidence::RecoveryProven {
            synthetic: site
                .residue_elements()
                .iter()
                .map(|element| SyntheticRecord {
                    element: *element,
                    constructed: true,
                    classified: ObjectResidue::Internal,
                    recovered: true,
                })
                .collect(),
            sampling: SamplingRecord {
                n,
                histogram: ClassHistogram {
                    none,
                    internal,
                    after,
                },
                unclassified: 0,
                recovered: true,
            },
        },
    }
}

/// The fast integration sequences the self-test drives.
///
/// Two, and named, because a no-execution record is measured against every
/// one the suite exercised: one sequence cannot show that the second did
/// not reach a site the first skipped.
const FAST_SEQUENCES: [&str; 2] = ["fast/seq-0", "fast/seq-1"];

fn no_execution_entry(site: EffectSiteId) -> RegistryEntry {
    RegistryEntry {
        site,
        phase: EntryPhase::NoExecution,
        order: None,
        fault_row: site.fault_row(),
        expected_residue: ExpectedResidue {
            rows: residue_rows(site, EntryPhase::NoExecution),
            detail: residue_detail(site, EntryPhase::NoExecution),
        },
        resume_action: resume_action(site, EntryPhase::NoExecution),
        label: EvidenceLabel::ExecutionObserved,
        evidence: Evidence::NotExecuted {
            test: format!("st07::fast-path::{site}"),
            passed: true,
            sequences: FAST_SEQUENCES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        },
    }
}

/// The frozen sample count and internal-window hit count for one site.
///
/// Distinct per site, because `N` is frozen per site and a fixture that
/// wrote one number for four sites could not show that anything reads it.
/// One of the four is deliberately zero: "hitting Internal is recorded but
/// never required".
fn sampling_for(site: EffectSiteId) -> (u32, u32) {
    match site {
        EffectSiteId::Object(ObjectSite::CandidateCommitTree) => (61, 0),
        EffectSiteId::Object(ObjectSite::RepairMaterialize) => (23, 4),
        EffectSiteId::Object(ObjectSite::ProposalCherryPick) => (37, 9),
        EffectSiteId::Worktree(WorktreeSite::AddStaging) => (19, 2),
        other => unreachable!("the fixture drives no other residue-class site: {other}"),
    }
}

/// Every entry the inventory needs, built through the format so that a
/// fixture the format would refuse cannot be the thing the bijection
/// passes on.
fn self_test_registry(host: Host) -> Vec<RegistryEntry> {
    let mut registry = FaultRegistry::new();
    for site in self_test_inventory() {
        if !site.scope().is_claimed() {
            continue;
        }
        if site.skipped_on_fast_path() {
            // Additive, not instead of: the no-execution record goes in
            // *and* the site carries the ordinary entries every claimed
            // site carries.
            registry
                .insert(no_execution_entry(site))
                .expect("a no-execution record for a site a fast sequence skips");
        }
        for phase in [EntryPhase::Before, EntryPhase::After] {
            registry
                .insert(hook_entry(site, phase))
                .expect("hook entry");
        }
        for point in site.sub_effects() {
            if !point.platform().required_on(host) {
                continue;
            }
            for mode in point.modes() {
                registry
                    .insert(hook_entry(
                        site,
                        EntryPhase::Point {
                            point: *point,
                            mode: *mode,
                        },
                    ))
                    .expect("point entry");
            }
        }
        // A frozen sample count per site — "N frozen per site in the
        // registry" — and one class deliberately never hit.
        for class in site.residue_classes() {
            assert_eq!(*class, ResidueClass::ObjectInternal);
            let (n, internal) = sampling_for(site);
            registry
                .insert(residue_entry(site, n, internal))
                .expect("residue entry");
        }
    }
    registry.entries().to_vec()
}

#[test]
fn the_framework_self_test_round_trips_through_enums_harness_and_registry() {
    // ST-07's proof test for this slice, in one place: a site set out of
    // the enums, driven through the harness in both injection modes, with
    // a residue-class entry, checked by the bijection.
    //
    // Run for both hosts and not only for [`Host::current`]. The fixture,
    // the harness and the check all take the host as a parameter, so a
    // Linux box can build and check the Windows shape; leaving that to the
    // Windows CI cell is how a self-test acquires a platform it has never
    // been run against.
    for host in Host::ALL.iter().copied() {
        let inventory = self_test_inventory();
        let harness = self_test_harness(host);
        let entries = self_test_registry(host);
        let failures = check_bijection(&inventory, &harness, &entries, host);
        assert!(failures.is_empty(), "{host}: {failures:#?}");
    }

    let host = Host::current();
    let inventory = self_test_inventory();
    let harness = self_test_harness(host);
    let entries = self_test_registry(host);

    let failures = check_bijection(&inventory, &harness, &entries, host);
    assert!(failures.is_empty(), "{failures:#?}");

    // The exercise was real: both injection modes were executed, the
    // kill-only point was executed in kill mode, and the sites a fast
    // sequence skips executed in neither.
    let append = EffectSiteId::Event(EventSite::AppendFirst);
    for mode in InjectionMode::ALL {
        for point in [SubEffectPoint::Written, SubEffectPoint::Synced] {
            assert!(
                harness.observed(append, HookPhase::Point { point, mode: *mode }),
                "{append} {point} {mode:?}"
            );
        }
    }
    let commit_tree = EffectSiteId::Object(ObjectSite::CandidateCommitTree);
    assert!(harness.observed(
        commit_tree,
        HookPhase::Point {
            point: SubEffectPoint::IdUnread,
            mode: InjectionMode::Kill,
        }
    ));
    assert!(!harness.observed(
        commit_tree,
        HookPhase::Point {
            point: SubEffectPoint::IdUnread,
            mode: InjectionMode::ErrorReturn,
        }
    ));
    // The three sites a fast publication skips ran on the stale-candidate
    // path and in none of the fast sequences: both halves, because the
    // no-execution record is a claim about the traces and not a claim that
    // the site never runs. A suite that had never driven them would
    // satisfy the second half by having no coverage at all.
    for site in fast_path_skipped() {
        assert!(
            harness.touched(site),
            "{site} was never driven, so its hook phases have no coverage"
        );
        for sequence in harness.fast_sequences() {
            assert!(
                !sequence.ran(site),
                "{site} ran inside the fast sequence {}",
                sequence.name()
            );
        }
        for phase in HookPhase::PHASES {
            assert!(harness.observed(site, *phase), "{site}/{phase}");
        }
    }
    // The Legacy site was never driven and never entered, and the check
    // passed anyway — `scope` says it carries no requirement.
    let legacy = EffectSiteId::Event(EventSite::LegacyAppend);
    assert!(!harness.touched(legacy));
    assert!(!entries.iter().any(|entry| entry.site == legacy));
}

/// The shape of the self-test fixture on one host: how many entries it
/// holds and how many distinct values each field the format reads takes
/// across them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureShape {
    entries: usize,
    order: usize,
    fault_row: usize,
    rows: usize,
    detail: usize,
    resume_action: usize,
    label: usize,
    evidence_kind: usize,
    test_name: usize,
    bound_to_before: usize,
    sampling: usize,
}

/// Measure one host's fixture. Takes the host as a parameter rather than
/// reading [`Host::current`], so the numbers below are checked for both
/// platforms wherever the suite runs.
fn fixture_shape(host: Host) -> FixtureShape {
    let entries = self_test_registry(host);
    let distinct = |mut values: Vec<String>| -> usize {
        values.sort();
        values.dedup();
        values.len()
    };
    FixtureShape {
        entries: entries.len(),
        order: distinct(entries.iter().map(|e| format!("{:?}", e.order)).collect()),
        fault_row: distinct(entries.iter().map(|e| e.fault_row.to_string()).collect()),
        rows: distinct(
            entries
                .iter()
                .map(|e| format!("{:?}", e.expected_residue.rows))
                .collect(),
        ),
        detail: distinct(
            entries
                .iter()
                .map(|e| e.expected_residue.detail.clone())
                .collect(),
        ),
        resume_action: distinct(entries.iter().map(|e| e.resume_action.clone()).collect()),
        label: distinct(entries.iter().map(|e| format!("{:?}", e.label)).collect()),
        evidence_kind: distinct(
            entries
                .iter()
                .map(|e| match &e.evidence {
                    Evidence::Executed { .. } => "executed".to_owned(),
                    Evidence::RecoveryProven { .. } => "recovery".to_owned(),
                    Evidence::NotExecuted { .. } => "not-executed".to_owned(),
                })
                .collect(),
        ),
        test_name: distinct(
            entries
                .iter()
                .filter_map(|e| match &e.evidence {
                    Evidence::Executed { test, .. } | Evidence::NotExecuted { test, .. } => {
                        Some(test.clone())
                    }
                    Evidence::RecoveryProven { .. } => None,
                })
                .collect(),
        ),
        bound_to_before: entries
            .iter()
            .filter(|e| e.phase.resumes_as_before())
            .count(),
        sampling: entries
            .iter()
            .filter(|e| matches!(e.evidence, Evidence::RecoveryProven { .. }))
            .count(),
    }
}

/// What each host's fixture has to look like.
///
/// Two literals, and both asserted on every host. PR3-ST07-013's second
/// half: the predecessor of this test read `Platform::host()` and asserted
/// a single hard-coded total of 39. That total is the *Unix* shape.
/// `Spawn.AmbientJobJoined` supports kill and error-return while the other
/// three Windows containment points and all four Unix ones are kill-only,
/// so a Windows fixture holds one entry more — and CLAUDE.md makes Windows
/// a first-class target whose CI cell runs this very suite. A number
/// measured on one platform and asserted on both is a red matrix cell that
/// the platform which produced it can never see.
///
/// So the shapes are computed for both hosts here, on whichever host is
/// running. A Linux box proves the Windows numbers and a Windows box proves
/// the Unix ones.
const FIXTURE_SHAPES: &[(Host, FixtureShape)] = &[
    (
        Host::Unix,
        FixtureShape {
            entries: 41,
            order: 3,
            fault_row: 6,
            rows: 10,
            detail: 17,
            resume_action: 8,
            label: 2,
            evidence_kind: 3,
            test_name: 37,
            bound_to_before: 5,
            sampling: 4,
        },
    ),
    (
        Host::Windows,
        FixtureShape {
            entries: 42,
            order: 3,
            fault_row: 6,
            rows: 9,
            detail: 18,
            resume_action: 9,
            label: 2,
            evidence_kind: 3,
            test_name: 38,
            bound_to_before: 5,
            sampling: 4,
        },
    ),
];

#[test]
fn the_self_test_fixture_varies_every_field_it_reads() {
    // Counts, not prose. A field with one distinct value across the
    // fixture cannot prove that anything reads that field, so each of
    // these is asserted to take more than one — and the ones that are
    // deliberately constant say so.
    assert_eq!(FIXTURE_SHAPES.len(), Host::ALL.len());
    for (host, expected) in FIXTURE_SHAPES.iter().copied() {
        assert_eq!(fixture_shape(host), expected, "the {host} fixture");
    }
    // Every count above is at least two except the ones named here, so a
    // table edited to a pile of ones would not pass by being a table.
    for (host, shape) in FIXTURE_SHAPES.iter().copied() {
        for (name, count) in [
            ("order", shape.order),
            ("fault_row", shape.fault_row),
            ("expected_residue.rows", shape.rows),
            ("expected_residue.detail", shape.detail),
            ("resume_action", shape.resume_action),
            ("label", shape.label),
            ("evidence kind", shape.evidence_kind),
        ] {
            assert!(count >= 2, "{host}: {name} takes {count} distinct values");
        }
    }
    // The two hosts differ in exactly the containment points: one more
    // entry on Windows, one more evidence test name with it, and the same
    // number of everything the platform does not touch.
    let unix = fixture_shape(Host::Unix);
    let windows = fixture_shape(Host::Windows);
    assert_eq!(windows.entries, unix.entries + 1);
    assert_eq!(windows.test_name, unix.test_name + 1);
    assert_eq!(windows.sampling, unix.sampling);
    assert_eq!(windows.bound_to_before, unix.bound_to_before);
    assert_eq!(windows.order, unix.order);
    assert_eq!(windows.fault_row, unix.fault_row);
    // One fewer distinct row-list on Windows, and it is the interesting
    // one: the Unix containment points leave `[R28]`, the Windows ones
    // leave `[]`, and `[]` is already the before phase of every creation.
    // Under the shipped authority both platforms answered `[R22]` and this
    // difference did not exist.
    assert_eq!(windows.rows, unix.rows - 1);

    // Every `BeforeState` answer occurs in the fixture, so each one is
    // carried through the format — `hook_entry`, `validate_entry`,
    // `check_bijection` — rather than only through the table. The two
    // non-empty answers name the *same* row and differ only in the words
    // the format compares, so a fixture holding one of them and not the
    // other could not show that the format tells them apart.
    let classified: BTreeSet<BeforeState> = self_test_inventory()
        .into_iter()
        .map(EffectSiteId::before_state)
        .collect();
    assert_eq!(
        classified,
        BTreeSet::from([
            BeforeState::Absent,
            BeforeState::PrecursorDurable,
            BeforeState::Present,
        ]),
        "the self-test fixture no longer exercises every before-phase answer"
    );
    assert_eq!(
        EffectSiteId::Worktree(WorktreeSite::Remove).before_state(),
        BeforeState::Present
    );
    assert_eq!(
        EffectSiteId::Worktree(WorktreeSite::AddStaging).before_state(),
        BeforeState::PrecursorDurable
    );

    // The rest is host-independent and is asserted on both fixtures.
    for host in Host::ALL.iter().copied() {
        let entries = self_test_registry(host);

        // The entries whose phase `structure` gives "the before-phase
        // action" carry their site's own before-phase action and are not
        // free to vary.
        for entry in entries.iter().filter(|e| e.phase.resumes_as_before()) {
            assert_eq!(
                entry.resume_action,
                entry.site.semantics(EntryPhase::Before).action.text(),
                "{host}: {}/{}",
                entry.site,
                entry.phase
            );
        }

        // Both before-phase classifications occur in the fixture, so the
        // registry the bijection passes on is one that exercises each. The
        // `[R9]` entry is the one the shipped authority refused.
        let before: Vec<&RegistryEntry> = entries
            .iter()
            .filter(|e| e.phase == EntryPhase::Before)
            .collect();
        assert!(
            before
                .iter()
                .any(|e| e.expected_residue.rows == vec![ResourceRow::R9]
                    && e.site == EffectSiteId::Worktree(WorktreeSite::Remove)),
            "{host}: no before-phase entry carries a target that is already durable"
        );
        assert!(
            before.iter().any(|e| e.expected_residue.rows.is_empty()),
            "{host}: no before-phase entry carries an absent target"
        );

        // The residue entries differ in every field a checker reads: the
        // frozen N, the histogram, and whether the internal window was hit.
        let sampling: Vec<SamplingRecord> = entries
            .iter()
            .filter_map(|e| match &e.evidence {
                Evidence::RecoveryProven { sampling, .. } => Some(*sampling),
                _ => None,
            })
            .collect();
        let distinct = |mut values: Vec<String>| -> usize {
            values.sort();
            values.dedup();
            values.len()
        };
        assert_eq!(sampling.len(), 4);
        assert_eq!(
            distinct(sampling.iter().map(|s| s.n.to_string()).collect()),
            sampling.len(),
            "{host}: frozen N is per site"
        );
        assert_eq!(
            distinct(
                sampling
                    .iter()
                    .map(|s| format!("{:?}", s.histogram))
                    .collect()
            ),
            sampling.len(),
            "{host}: the histogram is per site"
        );
        let hit: Vec<u32> = sampling.iter().map(|s| s.histogram.internal).collect();
        assert!(
            hit.contains(&0) && hit.iter().any(|count| *count > 0),
            "{host}: one class never hit and the rest hit: {hit:?}"
        );
        // Every residue element the inventory can construct is constructed
        // across the four.
        let elements: BTreeSet<ResidueElement> = entries
            .iter()
            .filter_map(|e| match &e.evidence {
                Evidence::RecoveryProven { synthetic, .. } => Some(synthetic.clone()),
                _ => None,
            })
            .flatten()
            .map(|record| record.element)
            .collect();
        assert_eq!(elements.len(), 8, "{host}: {elements:?}");
    }
}

#[test]
fn the_fixture_shape_table_is_measured_and_not_asserted_into_being() {
    // The table above is a literal, so this prints what the fixture
    // actually is when it disagrees — a bare `assert_eq!` on a struct of
    // eleven numbers is otherwise a puzzle to re-derive by hand.
    for (host, expected) in FIXTURE_SHAPES.iter().copied() {
        let measured = fixture_shape(host);
        assert_eq!(
            measured, expected,
            "{host} fixture measured {measured:?}, table says {expected:?}"
        );
    }
}

#[test]
fn the_harness_reports_no_execution_that_did_not_happen() {
    // The §7 empty-coverage proof, and the whole reason the harness exists
    // as a type rather than as a boolean per site.
    let mut harness = HookHarness::new();
    assert!(harness.coverage().is_empty());
    assert_eq!(harness.executions(), 0);

    // Arm every injection the whole inventory admits, and fire none.
    let mut armed = 0;
    for site in EffectSiteId::all() {
        for point in site.sub_effects() {
            for mode in point.modes() {
                harness.arm(site, *point, *mode).expect("a legal arming");
                armed += 1;
            }
        }
    }
    assert!(armed > 20, "the arming was not vacuous: {armed}");
    assert!(
        harness.coverage().is_empty(),
        "arming an injection recorded an execution: {:?}",
        harness.coverage()
    );
    assert_eq!(harness.executions(), 0);
    for site in EffectSiteId::all() {
        assert!(!harness.touched(site), "{site}");
        for phase in HookPhase::PHASES {
            assert!(!harness.observed(site, *phase), "{site}");
        }
    }
    // And a bijection over a site nothing ran through fails, rather than
    // passing because the coverage report was empty.
    let site = EffectSiteId::Event(EventSite::AppendFirst);
    let failures = check_bijection(&[site], &harness, &[], Host::current());
    assert!(
        failures
            .iter()
            .any(|failure| matches!(failure, BijectionFailure::Unobserved { .. })),
        "{failures:#?}"
    );
}

#[test]
fn no_execution_evidence_holds_inside_an_exercised_fast_sequence_or_it_holds_nothing() {
    // ST-07: "the fast-path no-execution record shows that no staging,
    // cherry-pick, or prepared-pin site executed **for any fast
    // sequence**". A fresh harness has touched nothing, so `!touched(site)`
    // is true of it — and of a process that never ran an integration, or
    // ran one and forgot to hook it. The record has to hold *within* a
    // sequence that demonstrably happened, so every direction below is a
    // way it can fail to.
    let host = Host::current();
    let inventory = self_test_inventory();
    let entries = self_test_registry(host);
    let skipped = fast_path_skipped();
    assert_eq!(skipped.len(), 3, "the three sites a fast sequence skips");

    // The shape that passes, for contrast.
    assert!(check_bijection(&inventory, &self_test_harness(host), &entries, host).is_empty());

    // (a) An empty harness. This is the withheld mutation exactly: "treat
    // an empty harness as sufficient no-execution evidence without an
    // explicit entry bound to an exercised trace."
    let empty = HookHarness::new();
    assert!(empty.fast_sequences().is_empty());
    let failures = check_bijection(&inventory, &empty, &entries, host);
    for site in &skipped {
        assert!(
            failures.iter().any(|failure| matches!(
                failure,
                BijectionFailure::NoFastSequenceExercised { site: named } if named == site
            )),
            "{site}'s absence was substantiated by a harness that ran nothing: {failures:#?}"
        );
    }

    // (b) A harness that ran the fast sites but recorded no sequence: the
    // executions happened and nothing says they were a fast integration,
    // so there is still no trace the absence is measured inside.
    let mut unrecorded = HookHarness::new();
    for site in self_test_inventory() {
        if site.skipped_on_fast_path() || !site.scope().is_claimed() {
            continue;
        }
        drive(&mut unrecorded, site, host);
    }
    assert!(unrecorded.executions() > 0);
    assert!(unrecorded.fast_sequences().is_empty());
    assert!(
        check_bijection(&inventory, &unrecorded, &entries, host)
            .iter()
            .any(|failure| matches!(failure, BijectionFailure::NoFastSequenceExercised { .. }))
    );

    // (c) A second fast sequence the record says nothing about. One
    // sequence cannot witness another, so a record naming only the first
    // is silent about whether the second cherry-picked anything.
    let mut extra = self_test_harness(host);
    extra.begin_fast_sequence("fast/seq-2");
    drive(
        &mut extra,
        EffectSiteId::Event(EventSite::AppendFirst),
        host,
    );
    extra.end_fast_sequence();
    let failures = check_bijection(&inventory, &extra, &entries, host);
    for site in &skipped {
        assert!(
            failures.iter().any(|failure| matches!(
                failure,
                BijectionFailure::UnwitnessedFastSequence { site: named, sequence, observed: false }
                    if named == site && sequence == "fast/seq-2"
            )),
            "{site} said nothing about a fast sequence the suite ran: {failures:#?}"
        );
    }

    // (d) A record naming a sequence the harness never exercised — the
    // forgery direction, where the evidence is invented rather than
    // missing.
    let mut invented = entries.clone();
    for entry in &mut invented {
        if let Evidence::NotExecuted { sequences, .. } = &mut entry.evidence {
            sequences.push("fast/seq-that-never-ran".to_owned());
        }
    }
    let failures = check_bijection(&inventory, &self_test_harness(host), &invented, host);
    assert!(
        failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::UnknownFastSequence { sequence, .. }
                if sequence == "fast/seq-that-never-ran"
        )),
        "{failures:#?}"
    );

    // (e) A site that actually ran inside a recorded fast sequence. The
    // exact-base decision is made before any staging effect, so a
    // cherry-pick inside a fast sequence is INV-09 broken, and the record
    // that says it did not happen is the thing that must fail.
    let mut cherry_picked = HookHarness::new();
    for sequence in FAST_SEQUENCES {
        cherry_picked.begin_fast_sequence(sequence);
        for site in self_test_inventory() {
            if !site.scope().is_claimed() {
                continue;
            }
            if site.skipped_on_fast_path()
                && site != EffectSiteId::Object(ObjectSite::ProposalCherryPick)
            {
                continue;
            }
            drive(&mut cherry_picked, site, host);
        }
        cherry_picked.end_fast_sequence();
    }
    let failures = check_bijection(&inventory, &cherry_picked, &entries, host);
    let cherry = EffectSiteId::Object(ObjectSite::ProposalCherryPick);
    assert!(
        failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::ExecutedInFastSequence { site: named, .. }
                if *named == cherry
        )),
        "a cherry-pick inside a fast sequence passed its own no-execution record: \
             {failures:#?}"
    );
    // And the other two, which did not run, are still clean of that
    // particular failure — so the report names what happened rather than
    // failing everything at once.
    for site in &skipped {
        if *site == cherry {
            continue;
        }
        assert!(
            !failures.iter().any(|failure| matches!(
                failure,
                BijectionFailure::ExecutedInFastSequence { site: named, .. }
                    if named == site
            )),
            "{site} was reported as executing and it did not"
        );
    }

    // (f) A site whose hook the harness recorded inside an exercised fast
    // sequence the record does not name. Two observations, both from the
    // inputs: the record says nothing about `fast/seq-2`, and the harness
    // recorded the site's hook in it. The report carries both in one failure,
    // so a reader learns of the observation now rather than a round later by
    // adding the sequence to the record and reading `ExecutedInFastSequence`
    // then. Until `ffe26ca` the `else` placement reported the gap with the
    // observation withheld; between `ffe26ca` and `c2b6b6c` the observation
    // was reported as a failure of its own asserting a rule `DESIGN.md` did
    // not then contain (pass 3 on `c2b6b6c`); at `4fb81b3` the field was
    // called `executed`, which claims more than a harness that sees only its
    // own hook can know (pass 4 on `4fb81b3`).
    let mut ran_unnamed = self_test_harness(host);
    ran_unnamed.begin_fast_sequence("fast/seq-2");
    drive(&mut ran_unnamed, cherry, host);
    ran_unnamed.end_fast_sequence();
    let failures = check_bijection(&inventory, &ran_unnamed, &entries, host);
    assert!(
        failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::UnwitnessedFastSequence { site: named, sequence, observed: true }
                if *named == cherry && sequence == "fast/seq-2"
        )),
        "a cherry-pick hook inside an unnamed fast sequence was reported as a gap with the \
             observation withheld: {failures:#?}"
    );
    // And no contradiction is reported, because the record made no claim
    // about `fast/seq-2` to contradict.
    assert!(
        !failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::ExecutedInFastSequence { site: named, sequence }
                if *named == cherry && sequence == "fast/seq-2"
        )),
        "an unnamed sequence was reported as a contradiction of the record: {failures:#?}"
    );

    // (g) The format refuses a record that names no sequence at all,
    // before the bijection is reached.
    let mut unwitnessed = no_execution_entry(skipped[0]);
    if let Evidence::NotExecuted { sequences, .. } = &mut unwitnessed.evidence {
        sequences.clear();
    }
    assert!(matches!(
        validate_entry(&unwitnessed),
        Err(RegistryError::UnwitnessedNoExecution { .. })
    ));
    if let Evidence::NotExecuted { sequences, .. } = &mut unwitnessed.evidence {
        *sequences = vec!["   ".to_owned()];
    }
    assert!(matches!(
        validate_entry(&unwitnessed),
        Err(RegistryError::UnwitnessedNoExecution { .. })
    ));
}

#[test]
fn a_fast_path_record_that_is_simply_absent_fails_like_any_other_missing_entry() {
    // PR3-ST07-012, the omission direction — the one the malformed-record
    // cases above cannot reach.
    //
    // The predecessor of this branch opened with
    //
    //     let no_execution = entries.iter().any(|e| e.site == site
    //         && e.phase == EntryPhase::NoExecution);
    //     if no_execution { ... }
    //
    // so *whether* the fast-path requirement existed was read off the
    // entries being checked. Delete all three records and every check of
    // them is skipped: `check_bijection` returns no failure for a registry
    // that contains no fast-path absence proof at all. A completeness
    // oracle that asks the artifact whether it is required is not a
    // completeness oracle. `completeness_rule` — "any missing link fails";
    // ST-07 — "the fast-path no-execution record shows that no staging,
    // cherry-pick, or prepared-pin site executed for any fast sequence".
    //
    // The requirement now comes from `skipped_on_fast_path()`, a property
    // of the site, and this test is that mutation.
    for host in Host::ALL.iter().copied() {
        let inventory = self_test_inventory();
        let harness = self_test_harness(host);
        let entries = self_test_registry(host);
        let skipped = fast_path_skipped();
        assert!(check_bijection(&inventory, &harness, &entries, host).is_empty());

        // Delete all three, leaving every ordinary entry and every
        // observation exactly as it was.
        let stripped: Vec<RegistryEntry> = entries
            .iter()
            .filter(|entry| entry.phase != EntryPhase::NoExecution)
            .cloned()
            .collect();
        assert_eq!(
            entries.len() - stripped.len(),
            3,
            "the fixture did not carry three records to delete"
        );
        let failures = check_bijection(&inventory, &harness, &stripped, host);
        for site in &skipped {
            assert!(
                failures.iter().any(|failure| matches!(
                    failure,
                    BijectionFailure::MissingEntry { site: named, phase, .. }
                        if named == site && *phase == EntryPhase::NoExecution
                )),
                "{host}: {site} has no fast-path record and the bijection did not say so: \
                     {failures:#?}"
            );
        }
        // Every ordinary requirement still holds, so the report is about
        // the three missing records and not about a fixture that fell
        // apart around them.
        assert!(
            !failures
                .iter()
                .any(|failure| matches!(failure, BijectionFailure::Unobserved { .. })),
            "{host}: {failures:#?}"
        );

        // And deleting one of the three is caught too, so the check is per
        // site rather than "at least one record exists somewhere".
        for site in &skipped {
            let one_gone: Vec<RegistryEntry> = entries
                .iter()
                .filter(|entry| !(entry.site == *site && entry.phase == EntryPhase::NoExecution))
                .cloned()
                .collect();
            assert_eq!(one_gone.len(), entries.len() - 1);
            let failures = check_bijection(&inventory, &harness, &one_gone, host);
            assert!(
                failures.iter().any(|failure| matches!(
                    failure,
                    BijectionFailure::MissingEntry { site: named, phase, .. }
                        if named == site && *phase == EntryPhase::NoExecution
                )),
                "{host}: {site}'s record could be dropped alone: {failures:#?}"
            );
            // Precisely that site, and precisely that claim. A record that
            // is absent also says nothing about each sequence the harness
            // ran, which is a true report and the only other one: one
            // `MissingEntry` plus one `UnwitnessedFastSequence` per
            // exercised sequence, and nothing about the other two sites or
            // about any ordinary coverage.
            assert_eq!(
                failures.len(),
                1 + harness.fast_sequences().len(),
                "{host}: dropping {site}'s record reported more than its absence: \
                     {failures:#?}"
            );
            for failure in &failures {
                let named = match failure {
                    BijectionFailure::MissingEntry { site, .. }
                    | BijectionFailure::UnwitnessedFastSequence { site, .. } => *site,
                    other => panic!("{host}: unexpected failure {other:?}"),
                };
                assert_eq!(named, *site, "{host}: {failures:#?}");
            }
        }
    }
}

#[test]
fn exactly_one_fast_path_record_is_required_and_a_second_is_refused() {
    // "Exactly one valid record", from both sides. The missing direction is
    // above; this is the duplicate direction, which matters because a
    // checker that accepted two would read whichever it reached first and
    // report one of two disagreeing claims.
    let host = Host::current();
    let inventory = self_test_inventory();
    let harness = self_test_harness(host);
    let entries = self_test_registry(host);
    let site = EffectSiteId::Worktree(WorktreeSite::AddStaging);

    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.site == site && entry.phase == EntryPhase::NoExecution)
            .count(),
        1
    );
    // The constructor refuses the second at the same key...
    let mut registry = FaultRegistry::new();
    for entry in &entries {
        registry
            .insert(entry.clone())
            .expect("the fixture is sound");
    }
    assert!(registry.insert(no_execution_entry(site)).is_err());
    // ...and so does the bare-slice path a hand-edited registry.json takes.
    let mut doubled = entries.clone();
    doubled.push(no_execution_entry(site));
    let failures = check_bijection(&inventory, &harness, &doubled, host);
    assert!(
        failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::DuplicateEntry { site: named, phase, .. }
                if *named == site && *phase == EntryPhase::NoExecution
        )),
        "{failures:#?}"
    );
}

#[test]
fn a_destructive_sites_before_entry_names_the_target_it_is_about_to_destroy() {
    // PR3-ST07-011, as the inversion it actually was rather than as a table
    // difference.
    //
    // `transaction_fault_matrix[T-SCRUB]` is live and binding, and its
    // boundary is "task_candidate_created appended; worktree, its intent,
    // or snapshots not yet removed". So the packet-correct registry entry
    // for `Worktree.Remove`'s before hook carries `[R9]`. Under the shipped
    // authority — `EntryPhase::Before => rows: Vec::new()` for every site —
    // `validate_entry` *refused* that entry with `WrongResidueRows` and
    // *accepted* an entry claiming the worktree was already gone. Both
    // directions are asserted here, because either one alone is satisfied
    // by a table that simply says something different.
    let scrub = EffectSiteId::Worktree(WorktreeSite::Remove);
    let sound = hook_entry(scrub, EntryPhase::Before);
    assert_eq!(
        sound.expected_residue.rows,
        vec![ResourceRow::R9],
        "the fixture writes the site's own semantics, which is what the packet says"
    );
    validate_entry(&sound).expect("a packet-correct before entry must be accepted");

    let mut empty = sound.clone();
    empty.expected_residue.rows = Vec::new();
    assert!(
        matches!(
            validate_entry(&empty),
            Err(RegistryError::WrongResidueRows { .. })
        ),
        "an entry claiming the worktree is already gone before its removal was accepted"
    );
    // The detail moves with the rows, so a mutation that changed only one
    // of the two cannot pass by leaving the other alone.
    let mut wrong_detail = sound.clone();
    wrong_detail.expected_residue.detail = ResidueArtifact::Nothing.detail().to_owned();
    assert!(matches!(
        validate_entry(&wrong_detail),
        Err(RegistryError::WrongResidueDetail { .. })
    ));

    // The mirror: a one-step creation's before entry is empty, and naming
    // its row is refused. Without this, an authority that answered
    // `[row()]` for every before phase would pass the assertions above.
    // `Worktree.WriteIntent` is the mirror rather than `Worktree.Add`,
    // which is not a one-step creation — see the third case below.
    let intent = EffectSiteId::Worktree(WorktreeSite::WriteIntent);
    let sound_intent = hook_entry(intent, EntryPhase::Before);
    assert!(sound_intent.expected_residue.rows.is_empty());
    validate_entry(&sound_intent).expect("a creation's before phase names no row");
    let mut invented = sound_intent.clone();
    invented.expected_residue.rows = vec![ResourceRow::R9];
    assert!(matches!(
        validate_entry(&invented),
        Err(RegistryError::WrongResidueRows { .. })
    ));
    // The two sites share a row and a group and differ only in the
    // authority, which is what makes this a per-site claim.
    assert_eq!(scrub.row(), intent.row());
    assert_eq!(scrub.group(), intent.group());

    // PR3-ST07-014, and the third answer the row alone cannot give.
    // `transaction_fault_matrix[T-DISPATCH]`'s boundary is "worktree
    // **intent** or worktree not yet created", its resume is "recreate it
    // (**intent then add**)", and R9 is "Task worktree **+ its durable
    // synced intent**". So a kill at `Worktree.Add`'s before hook leaves
    // R9 holding that intent — and the predecessor answered `[]` here,
    // refusing the packet-correct entry and accepting one that said a
    // durable synced intent was not there.
    //
    // All three directions, because any two of them are satisfied by a
    // table that merely says something different: the rows accepted, the
    // empty rows refused, and the *other* non-empty answer's words —
    // `Worktree.Remove`'s, over the identical row — refused too. That last
    // one is what stops `PrecursorDurable` from being `Present` under
    // another name: an add whose entry claimed the worktree was present
    // and unchanged would be as false as one claiming nothing was there.
    let add = EffectSiteId::Worktree(WorktreeSite::Add);
    assert_eq!(add.row(), scrub.row());
    assert_eq!(add.group(), scrub.group());
    let sound_add = hook_entry(add, EntryPhase::Before);
    assert_eq!(sound_add.expected_residue.rows, vec![ResourceRow::R9]);
    assert_eq!(
        sound_add.expected_residue.detail,
        ResidueArtifact::PrecursorDurable.detail()
    );
    validate_entry(&sound_add).expect("the intent an add follows is durable and R9 holds it");

    let mut denied = sound_add.clone();
    denied.expected_residue.rows = Vec::new();
    denied.expected_residue.detail = ResidueArtifact::Nothing.detail().to_owned();
    assert!(
        matches!(
            validate_entry(&denied),
            Err(RegistryError::WrongResidueRows { .. })
        ),
        "an entry claiming the synced intent is not there before the add was accepted"
    );

    let mut as_if_intact = sound_add.clone();
    as_if_intact.expected_residue.detail = ResidueArtifact::TargetIntact.detail().to_owned();
    assert!(
        matches!(
            validate_entry(&as_if_intact),
            Err(RegistryError::WrongResidueDetail { .. })
        ),
        "an add's before entry claimed the worktree it is about to create is already intact"
    );
    // ...and the same words in the other direction, over the same row: the
    // removal may not borrow the add's.
    let mut as_if_precursor = sound.clone();
    as_if_precursor.expected_residue.detail = ResidueArtifact::PrecursorDurable.detail().to_owned();
    assert!(matches!(
        validate_entry(&as_if_precursor),
        Err(RegistryError::WrongResidueDetail { .. })
    ));

    // And the containment half, through the format rather than through the
    // table: a `Process.Spawn` kill at a Unix point that claimed the R22
    // handle — the shipped answer — is refused, and the R28 hold its own
    // detail names is accepted.
    let spawn = EffectSiteId::Process(ProcessSite::Spawn);
    for (point, packet_rows) in [
        (SubEffectPoint::ReaperStarted, vec![ResourceRow::R28]),
        (SubEffectPoint::Registered, vec![ResourceRow::R28]),
        (SubEffectPoint::CreatedSuspended, Vec::new()),
        (SubEffectPoint::Resumed, Vec::new()),
    ] {
        let phase = EntryPhase::Point {
            point,
            mode: InjectionMode::Kill,
        };
        let sound = hook_entry(spawn, phase);
        assert_eq!(sound.expected_residue.rows, packet_rows, "{point}");
        validate_entry(&sound).unwrap_or_else(|e| panic!("{point}: {e}"));
        let mut r22 = sound.clone();
        r22.expected_residue.rows = vec![ResourceRow::R22];
        assert!(
            matches!(
                validate_entry(&r22),
                Err(RegistryError::WrongResidueRows { .. })
            ),
            "{point} still accepts the host-process handle its own detail denies"
        );
    }
}

#[test]
fn the_harness_counts_what_the_funnel_told_it_and_answers_what_is_armed() {
    let site = EffectSiteId::Event(EventSite::AppendFirst);
    let written = |mode| HookPhase::Point {
        point: SubEffectPoint::Written,
        mode,
    };
    let mut harness = HookHarness::new();

    // Unarmed: every phase proceeds. The two hook phases are reachability
    // and are counted; a point that fired nothing is *reached* and is not
    // coverage. `completeness_rule` asks for each mode "observed executed",
    // and a funnel walking past an unarmed point executed no mode.
    assert_eq!(harness.hook(site, HookPhase::Before), Injection::Proceed);
    assert_eq!(harness.hook(site, HookPhase::Before), Injection::Proceed);
    assert_eq!(
        harness.hook(site, written(InjectionMode::Kill)),
        Injection::Proceed
    );
    assert_eq!(harness.count(site, HookPhase::Before), 2);
    assert_eq!(harness.count(site, HookPhase::After), 0);
    assert_eq!(
        harness.count(site, written(InjectionMode::Kill)),
        0,
        "an unarmed point recorded a mode as executed"
    );
    assert!(
        !harness.observed(site, written(InjectionMode::Kill)),
        "an unarmed point reported coverage of its mode"
    );
    assert!(
        harness.reached_point(site, SubEffectPoint::Written, InjectionMode::Kill),
        "the funnel reached the point and the harness did not record that it had"
    );
    assert_eq!(harness.executions(), 2);

    // Armed: the injection is the mode that was armed, and only for the
    // exact (site, point, mode) triple — and *that* is the execution the
    // bijection reads.
    harness
        .arm(site, SubEffectPoint::Written, InjectionMode::Kill)
        .expect("armable");
    assert_eq!(
        harness.hook(site, written(InjectionMode::Kill)),
        Injection::Kill
    );
    assert_eq!(harness.count(site, written(InjectionMode::Kill)), 1);
    assert_eq!(
        harness.hook(site, written(InjectionMode::ErrorReturn)),
        Injection::Proceed,
        "arming kill must not arm error-return"
    );
    assert_eq!(
        harness.count(site, written(InjectionMode::ErrorReturn)),
        0,
        "arming one mode reported coverage of the other"
    );
    harness
        .arm(site, SubEffectPoint::Synced, InjectionMode::ErrorReturn)
        .expect("armable");
    assert_eq!(
        harness.hook(
            site,
            HookPhase::Point {
                point: SubEffectPoint::Synced,
                mode: InjectionMode::ErrorReturn
            }
        ),
        Injection::Error
    );
    assert_eq!(
        harness.count(
            site,
            HookPhase::Point {
                point: SubEffectPoint::Synced,
                mode: InjectionMode::ErrorReturn
            }
        ),
        1
    );
    // A different site's identical point is not armed, and not covered.
    let other = EffectSiteId::Event(EventSite::Append);
    assert_eq!(
        harness.hook(other, written(InjectionMode::Kill)),
        Injection::Proceed,
        "arming one site must not arm another"
    );
    assert_eq!(harness.count(other, written(InjectionMode::Kill)), 0);
    assert!(harness.reached_point(other, SubEffectPoint::Written, InjectionMode::Kill));
    // A hook phase never injects, whatever is armed.
    assert_eq!(harness.hook(site, HookPhase::After), Injection::Proceed);
    // Disarming keeps what was seen and stops what would be injected —
    // and a call after disarming adds reachability, not coverage.
    harness.disarm();
    assert_eq!(
        harness.hook(site, written(InjectionMode::Kill)),
        Injection::Proceed
    );
    assert!(harness.observed(site, HookPhase::After));
    assert_eq!(
        harness.count(site, written(InjectionMode::Kill)),
        1,
        "a disarmed point went on counting the mode it no longer injects"
    );
}

#[test]
fn a_point_and_a_mode_are_one_coverage_coordinate_and_not_two_axes() {
    // `completeness_rule` requires "every parent-side sub-effect point (in
    // every injection mode it supports) ... observed executed at least
    // once". The unit of coverage is the pair, and the suite only ever
    // recorded matrices in which the pair happened to be recoverable from
    // its halves: drive both points in both modes and a harness that
    // reports the Cartesian product of the points it saw and the modes it
    // saw is indistinguishable from one that keeps them together, and a
    // harness whose `Synced` queries silently answer for `Written` is
    // indistinguishable too.
    //
    // Both mutations die on an *asymmetric* matrix: one where the set of
    // observed points and the set of observed modes have a product
    // strictly larger than what was observed.
    let site = EffectSiteId::Event(EventSite::AppendFirst);
    let written = SubEffectPoint::Written;
    let synced = SubEffectPoint::Synced;
    let kill = InjectionMode::Kill;
    let err = InjectionMode::ErrorReturn;
    let at = |point, mode| HookPhase::Point { point, mode };
    let fire = |harness: &mut HookHarness, point, mode| {
        harness.arm(site, point, mode).expect("a legal arming");
        harness.hook(site, at(point, mode));
        harness.disarm();
    };

    // (a) One cell. Its own row and its own column stay empty, which is
    //     what a per-point or per-mode record cannot say.
    let mut one = HookHarness::new();
    fire(&mut one, written, kill);
    assert!(one.observed(site, at(written, kill)));
    assert_eq!(one.count(site, at(written, kill)), 1);
    for (point, mode) in [(written, err), (synced, kill), (synced, err)] {
        assert!(
            !one.observed(site, at(point, mode)),
            "{point}/{mode:?} was reported present after only Written/Kill ran"
        );
        assert_eq!(one.count(site, at(point, mode)), 0, "{point}/{mode:?}");
    }
    assert_eq!(one.coverage().len(), 1, "{:?}", one.coverage());
    assert_eq!(one.executions(), 1);

    // (b) The anti-diagonal. Points {Written, Synced} and modes {Kill,
    //     ErrorReturn} were each observed, and the two cells that were not
    //     observed are exactly the ones their product invents.
    let mut diagonal = HookHarness::new();
    fire(&mut diagonal, written, kill);
    fire(&mut diagonal, synced, err);
    assert!(diagonal.observed(site, at(written, kill)));
    assert!(
        diagonal.observed(site, at(synced, err)),
        "a Synced query answered about Written"
    );
    assert!(
        !diagonal.observed(site, at(written, err)),
        "an unobserved cell was reported present by the product of its axes"
    );
    assert!(
        !diagonal.observed(site, at(synced, kill)),
        "an unobserved cell was reported present by the product of its axes"
    );
    assert_eq!(diagonal.coverage().len(), 2, "{:?}", diagonal.coverage());
    let recorded: BTreeSet<String> = diagonal
        .coverage()
        .iter()
        .map(|seen| seen.phase.to_string())
        .collect();
    assert_eq!(
        recorded,
        BTreeSet::from(["Written/kill".to_owned(), "Synced/error-return".to_owned()])
    );

    // (c) The other anti-diagonal, so neither cell of the pair is the one
    //     that happens to be reachable by a mutation's fallback.
    let mut mirrored = HookHarness::new();
    fire(&mut mirrored, written, err);
    fire(&mut mirrored, synced, kill);
    assert!(mirrored.observed(site, at(written, err)));
    assert!(mirrored.observed(site, at(synced, kill)));
    assert!(!mirrored.observed(site, at(written, kill)));
    assert!(!mirrored.observed(site, at(synced, err)));

    // (d) Reachability is the same coordinate and the same claim: walking
    //     past Synced in one mode does not report the other mode reached.
    let mut reached = HookHarness::new();
    reached.hook(site, at(synced, kill));
    assert!(reached.reached_point(site, synced, kill));
    assert!(!reached.reached_point(site, synced, err));
    assert!(!reached.reached_point(site, written, kill));

    // (e) And the bijection reads the coordinate, not the axes: with a
    //     complete registry and the anti-diagonal harness, the two absent
    //     cells are the two reported unobserved — by name, so a checker
    //     that reported the wrong pair fails here too.
    let host = Host::current();
    let inventory = vec![site];
    let entries: Vec<RegistryEntry> = [
        EntryPhase::Before,
        EntryPhase::After,
        EntryPhase::Point {
            point: written,
            mode: kill,
        },
        EntryPhase::Point {
            point: written,
            mode: err,
        },
        EntryPhase::Point {
            point: SubEffectPoint::WrittenFull,
            mode: err,
        },
        EntryPhase::Point {
            point: synced,
            mode: kill,
        },
        EntryPhase::Point {
            point: synced,
            mode: err,
        },
    ]
    .into_iter()
    .map(|phase| hook_entry(site, phase))
    .collect();

    let mut harness = HookHarness::new();
    harness.hook(site, HookPhase::Before);
    harness.hook(site, HookPhase::After);
    fire(&mut harness, written, kill);
    fire(&mut harness, synced, err);
    fire(&mut harness, SubEffectPoint::WrittenFull, err);
    let unobserved: BTreeSet<HookPhase> = check_bijection(&inventory, &harness, &entries, host)
        .into_iter()
        .filter_map(|failure| match failure {
            BijectionFailure::Unobserved { phase, .. } => Some(phase),
            _ => None,
        })
        .collect();
    assert_eq!(
        unobserved,
        BTreeSet::from([
            HookPhase::Point {
                point: written,
                mode: err,
            },
            HookPhase::Point {
                point: synced,
                mode: kill,
            },
        ]),
        "the bijection did not report exactly the two coordinates that did not run"
    );
}

#[test]
fn an_unarmed_harness_substantiates_no_mode_however_far_the_funnels_run() {
    // The withheld mutation this is against, stated: "increment coverage
    // before checking whether the injector is enabled or matches the
    // reached coordinate, then inject nothing". A suite whose funnels all
    // run but whose injector is never armed — a mistargeted arming, a
    // harness reset, a feature flag off — would report every mode of every
    // point covered, which is the empty-coverage failure one level up from
    // the one §7 already guards.
    let host = Host::current();
    let mut harness = HookHarness::new();
    let mut points = 0;
    for site in self_test_inventory() {
        if !site.scope().is_claimed() {
            continue;
        }
        harness.hook(site, HookPhase::Before);
        for point in site.sub_effects() {
            if !point.platform().required_on(host) {
                continue;
            }
            for mode in point.modes() {
                // Reached, deliberately unarmed.
                assert_eq!(
                    harness.hook(
                        site,
                        HookPhase::Point {
                            point: *point,
                            mode: *mode
                        }
                    ),
                    Injection::Proceed
                );
                points += 1;
            }
        }
        harness.hook(site, HookPhase::After);
    }
    assert!(points > 5, "the sweep was not vacuous: {points}");

    // Every point was reached and no mode was executed.
    for observation in harness.reached() {
        assert!(matches!(observation.phase, HookPhase::Point { .. }));
    }
    assert_eq!(harness.reached().len(), points);
    assert!(
        harness
            .coverage()
            .iter()
            .all(|seen| matches!(seen.phase, HookPhase::Before | HookPhase::After)),
        "an unarmed run reported an injected mode: {:?}",
        harness.coverage()
    );

    // And the bijection says so, rather than passing on the strength of a
    // full-looking coverage report.
    let failures = check_bijection(
        &self_test_inventory(),
        &harness,
        &self_test_registry(host),
        host,
    );
    let unobserved: Vec<&BijectionFailure> = failures
        .iter()
        .filter(|failure| matches!(failure, BijectionFailure::Unobserved { .. }))
        .collect();
    assert_eq!(
        unobserved.len(),
        points,
        "an unarmed harness substantiated {} of {points} mode(s): {failures:#?}",
        points - unobserved.len()
    );
}

#[test]
fn the_harness_refuses_to_arm_a_point_or_mode_the_site_does_not_have() {
    let mut harness = HookHarness::new();
    let commit_tree = EffectSiteId::Object(ObjectSite::CandidateCommitTree);
    let append = EffectSiteId::Event(EventSite::AppendFirst);

    // A point the site does not expose.
    let error = harness
        .arm(commit_tree, SubEffectPoint::Written, InjectionMode::Kill)
        .expect_err("CandidateCommitTree exposes only IdUnread");
    assert!(
        matches!(error, HarnessError::NoSuchPoint { ref point, .. } if *point == SubEffectPoint::Written),
        "{error}"
    );
    // A mode the point does not support.
    let error = harness
        .arm(
            commit_tree,
            SubEffectPoint::IdUnread,
            InjectionMode::ErrorReturn,
        )
        .expect_err("IdUnread is kill-only");
    assert!(
        matches!(error, HarnessError::UnsupportedMode { mode, .. } if mode == InjectionMode::ErrorReturn),
        "{error}"
    );
    // A site with no points at all.
    let lock = EffectSiteId::Lock(LockSite::AcquireRun);
    assert!(
        harness
            .arm(lock, SubEffectPoint::Synced, InjectionMode::Kill)
            .is_err()
    );
    // And the legal ones are legal.
    harness
        .arm(commit_tree, SubEffectPoint::IdUnread, InjectionMode::Kill)
        .expect("the one point it has, in the one mode it supports");
    for mode in InjectionMode::ALL {
        harness
            .arm(append, SubEffectPoint::Written, *mode)
            .expect("an append point supports both modes");
    }
    // A refused arming armed nothing and recorded nothing.
    assert!(harness.coverage().is_empty());
}

// -----------------------------------------------------------------------
// The registry format
// -----------------------------------------------------------------------

#[test]
fn a_residue_class_entry_with_an_executed_hook_claim_is_refused() {
    // ST-07's load-bearing clause, on its own, in both the ways the claim
    // can be made: through the evidence and through the label.
    let site = EffectSiteId::Object(ObjectSite::CandidateCommitTree);
    let sound = residue_entry(site, 61, 0);
    FaultRegistry::new()
        .insert(sound.clone())
        .expect("a well-formed recovery-proven entry");

    let mut claims_by_evidence = sound.clone();
    claims_by_evidence.evidence = Evidence::Executed {
        test: "st07::pretending_the_internal_point_ran".to_owned(),
        passed: true,
    };
    let error = FaultRegistry::new()
        .insert(claims_by_evidence.clone())
        .expect_err("executed-hook evidence for a residue class");
    assert!(
        matches!(error, RegistryError::ResidueClaimsExecution { .. }),
        "{error}"
    );

    let mut claims_by_label = sound.clone();
    claims_by_label.label = EvidenceLabel::ExecutionObserved;
    let error = FaultRegistry::new()
        .insert(claims_by_label.clone())
        .expect_err("an execution-observed label on a residue class");
    assert!(
        matches!(error, RegistryError::ResidueClaimsExecution { .. }),
        "{error}"
    );

    // And the bijection refuses the same document, because a registry.json
    // handed to a reviewer never went through `insert`.
    let host = Host::current();
    for bad in [claims_by_evidence, claims_by_label] {
        let mut entries = self_test_registry(host);
        let slot = entries
            .iter()
            .position(|entry| entry.key() == bad.key())
            .expect("the self-test registry holds this key");
        entries[slot] = bad;
        let failures = check_bijection(
            &self_test_inventory(),
            &self_test_harness(host),
            &entries,
            host,
        );
        assert!(
            failures
                .iter()
                .any(|f| matches!(f, BijectionFailure::ResidueClaimsExecution { .. })),
            "{failures:#?}"
        );
    }

    // The converse is refused too: a hook entry cannot borrow the label
    // that exists for what no hook can reach.
    let mut hook = hook_entry(site, EntryPhase::Before);
    hook.label = EvidenceLabel::RecoveryProven;
    // An assertion naming the premise rather than an `unreachable!` arm: §7
    // denies `unreachable!` in tests too, having no Clippy allowance to take,
    // and a test that fails its own setup says which premise failed.
    assert!(
        matches!(sound.evidence, Evidence::RecoveryProven { .. }),
        "the sound entry is recovery-proven"
    );
    hook.evidence = sound.evidence.clone();
    let error = FaultRegistry::new()
        .insert(hook.clone())
        .expect_err("a before-phase claiming recovery-proven evidence");
    assert!(
        matches!(error, RegistryError::HookClaimsRecoveryProof { .. }),
        "{error}"
    );

    // And what the bijection says about the same hand-edited document. The
    // format's refusal is one answer; the other is that this hook phase
    // carries nothing saying it executed, which is the question the
    // bijection is asking. The synthetic and sampling records inside it are
    // left unread rather than held to a residue class the entry is not
    // about — reporting on them would be reporting on a claim nobody made,
    // and there is no class here to say what they should have classified
    // as.
    let mut entries = self_test_registry(host);
    let slot = entries
        .iter()
        .position(|entry| entry.key() == hook.key())
        .expect("the self-test registry holds this site's before phase");
    entries[slot] = hook;
    let failures = check_bijection(
        &self_test_inventory(),
        &self_test_harness(host),
        &entries,
        host,
    );
    assert!(
        failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::InvalidEntry {
                error: RegistryError::HookClaimsRecoveryProof { .. },
                ..
            }
        )),
        "{failures:#?}"
    );
    assert!(
        failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::MissingEvidence { site: named, phase: EntryPhase::Before }
                if *named == site
        )),
        "a before phase carrying recovery-proven evidence was read as evidenced: {failures:#?}"
    );
    assert!(
        !failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::ResidueElementNotConstructed { .. }
                | BijectionFailure::ResidueElementNotRecovered { .. }
                | BijectionFailure::ResidueElementMisclassified { .. }
        )),
        "the records of a class the entry is not about were read anyway: {failures:#?}"
    );
}

#[test]
fn the_format_admits_exactly_one_evidence_shape_and_label_per_phase_kind() {
    // The crossed grid: five phase kinds x three evidence shapes x two
    // labels. Five cells are legal and twenty-five are refused; the whole
    // thirty are enumerated rather than sampled, because any smaller set
    // is satisfiable by a rule that happens to agree on the cases tried.
    let commit_tree = EffectSiteId::Object(ObjectSite::CandidateCommitTree);
    let skipped = EffectSiteId::Object(ObjectSite::ProposalCherryPick);
    let phases = [
        (commit_tree, EntryPhase::Before),
        (commit_tree, EntryPhase::After),
        (
            commit_tree,
            EntryPhase::Point {
                point: SubEffectPoint::IdUnread,
                mode: InjectionMode::Kill,
            },
        ),
        (
            commit_tree,
            EntryPhase::Residue {
                class: ResidueClass::ObjectInternal,
            },
        ),
        (skipped, EntryPhase::NoExecution),
    ];
    let executed = Evidence::Executed {
        test: "grid::executed".to_owned(),
        passed: true,
    };
    let not_executed = Evidence::NotExecuted {
        test: "grid::not-executed".to_owned(),
        passed: true,
        sequences: FAST_SEQUENCES
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
    };
    let recovery = match residue_entry(commit_tree, 61, 0).evidence {
        evidence @ Evidence::RecoveryProven { .. } => evidence,
        _ => unreachable!(),
    };
    let recovery_for_skipped = match residue_entry(skipped, 23, 4).evidence {
        evidence @ Evidence::RecoveryProven { .. } => evidence,
        _ => unreachable!(),
    };

    let mut legal = 0;
    let mut refused = 0;
    for (site, phase) in phases {
        for evidence_kind in 0..3 {
            for label in [
                EvidenceLabel::ExecutionObserved,
                EvidenceLabel::RecoveryProven,
            ] {
                let evidence = match evidence_kind {
                    0 => executed.clone(),
                    1 => not_executed.clone(),
                    _ if site == skipped => recovery_for_skipped.clone(),
                    _ => recovery.clone(),
                };
                let mut entry = hook_entry(site, phase);
                if phase == EntryPhase::NoExecution {
                    entry.order = None;
                }
                entry.evidence = evidence;
                entry.label = label;

                let expected_legal = matches!(
                    (phase, evidence_kind, label),
                    (
                        EntryPhase::Before | EntryPhase::After | EntryPhase::Point { .. },
                        0,
                        EvidenceLabel::ExecutionObserved
                    ) | (EntryPhase::NoExecution, 1, EvidenceLabel::ExecutionObserved)
                        | (EntryPhase::Residue { .. }, 2, EvidenceLabel::RecoveryProven)
                );
                let result = FaultRegistry::new().insert(entry);
                assert_eq!(
                    result.is_ok(),
                    expected_legal,
                    "phase {phase}, evidence {evidence_kind}, label {label:?}: {result:?}"
                );
                if expected_legal {
                    legal += 1;
                } else {
                    refused += 1;
                }
            }
        }
    }
    assert_eq!((legal, refused), (5, 25));
}

#[test]
fn the_format_refuses_an_entry_that_disagrees_with_the_site_it_names() {
    let commit_tree = EffectSiteId::Object(ObjectSite::CandidateCommitTree);
    let append = EffectSiteId::Event(EventSite::AppendFirst);

    // A fault row that is not the site's.
    let mut entry = hook_entry(commit_tree, EntryPhase::Before);
    entry.fault_row = FaultRow::TFinish;
    assert!(matches!(
        FaultRegistry::new().insert(entry).expect_err("wrong row"),
        RegistryError::WrongFaultRow { .. }
    ));

    // An order the site cannot leave observable, in both directions: a
    // site with one order carrying the other or none, and a site with no
    // order carrying one.
    let mut entry = hook_entry(commit_tree, EntryPhase::Before);
    entry.order = Some(ObservableOrder::EventBeforeEffect);
    assert!(matches!(
        FaultRegistry::new().insert(entry).expect_err("wrong order"),
        RegistryError::WrongOrder { .. }
    ));
    let mut entry = hook_entry(commit_tree, EntryPhase::Before);
    entry.order = None;
    assert!(matches!(
        FaultRegistry::new()
            .insert(entry)
            .expect_err("missing order"),
        RegistryError::WrongOrder { .. }
    ));
    let mut entry = hook_entry(append, EntryPhase::Before);
    entry.order = Some(ObservableOrder::EffectBeforeEvent);
    assert!(matches!(
        FaultRegistry::new()
            .insert(entry)
            .expect_err("an append has no order"),
        RegistryError::WrongOrder { .. }
    ));

    // A point the site does not expose, and a mode it does not support.
    let entry = hook_entry(
        commit_tree,
        EntryPhase::Point {
            point: SubEffectPoint::Written,
            mode: InjectionMode::Kill,
        },
    );
    assert!(matches!(
        FaultRegistry::new()
            .insert(entry)
            .expect_err("no such point"),
        RegistryError::NoSuchPoint { .. }
    ));
    let entry = hook_entry(
        commit_tree,
        EntryPhase::Point {
            point: SubEffectPoint::IdUnread,
            mode: InjectionMode::ErrorReturn,
        },
    );
    assert!(matches!(
        FaultRegistry::new()
            .insert(entry)
            .expect_err("no such mode"),
        RegistryError::NoSuchPoint { .. }
    ));

    // A residue class the site does not register.
    let entry = hook_entry(
        append,
        EntryPhase::Residue {
            class: ResidueClass::ObjectInternal,
        },
    );
    assert!(matches!(
        FaultRegistry::new()
            .insert(entry)
            .expect_err("no such class"),
        RegistryError::NoSuchResidueClass { .. }
    ));

    // A no-execution record for a site a fast sequence does not skip.
    let entry = no_execution_entry(append);
    assert!(matches!(
        FaultRegistry::new()
            .insert(entry)
            .expect_err("only three sites may claim they did not run"),
        RegistryError::NoExecutionNotSkipped { .. }
    ));
    for site in fast_path_skipped() {
        FaultRegistry::new()
            .insert(no_execution_entry(site))
            .expect("the three the design names");
    }
}

#[test]
fn the_format_refuses_an_incomplete_or_invented_synthetic_record() {
    let site = EffectSiteId::Object(ObjectSite::ProposalCherryPick);
    assert_eq!(site.residue_elements().len(), 7);

    // Every element removed in turn: a class whose evidence skipped one is
    // a class whose recovery was never shown for that element.
    for absent in site.residue_elements() {
        let mut entry = residue_entry(site, 23, 4);
        if let Evidence::RecoveryProven { synthetic, .. } = &mut entry.evidence {
            synthetic.retain(|record| record.element != *absent);
        }
        let error = FaultRegistry::new()
            .insert(entry)
            .expect_err("a missing element");
        assert!(
            matches!(error, RegistryError::MissingSyntheticElement { element, .. } if element == *absent),
            "{error}"
        );
    }

    // And an element the site's command cannot leave: a `MERGE_HEAD` in a
    // repair worktree that only ever cherry-picks one commit is evidence
    // about something that never happens there.
    let repair = EffectSiteId::Object(ObjectSite::RepairMaterialize);
    let mut entry = residue_entry(repair, 23, 4);
    if let Evidence::RecoveryProven { synthetic, .. } = &mut entry.evidence {
        synthetic.push(SyntheticRecord {
            element: ResidueElement::MergeHead,
            constructed: true,
            classified: ObjectResidue::Internal,
            recovered: true,
        });
    }
    let error = FaultRegistry::new()
        .insert(entry)
        .expect_err("an unlisted element");
    assert!(
        matches!(error, RegistryError::UnlistedSyntheticElement { element, .. } if element == ResidueElement::MergeHead),
        "{error}"
    );
}

#[test]
fn the_expected_residue_of_a_phase_is_the_sites_own_semantics() {
    // The values, written from `fault_injection_registry.structure` rather
    // than read back from `expected_rows`, so this pins the table and not
    // merely today's output of it.
    let commit_tree = EffectSiteId::Object(ObjectSite::CandidateCommitTree);
    let repair = EffectSiteId::Object(ObjectSite::RepairMaterialize);
    let append = EffectSiteId::Event(EventSite::AppendFirst);
    let staging = EffectSiteId::Worktree(WorktreeSite::AddStaging);
    let internal = EntryPhase::Residue {
        class: ResidueClass::ObjectInternal,
    };
    let id_unread = EntryPhase::Point {
        point: SubEffectPoint::IdUnread,
        mode: InjectionMode::Kill,
    };

    // The before phase, from the packet and not from the code.
    //
    // "Object sites carry entries — before: no object (hook)" — the whole
    // group, by name.
    for site in EffectSiteId::all() {
        if site.group() == FunnelGroup::Object {
            assert!(site.expected_rows(EntryPhase::Before).is_empty(), "{site}");
        }
        assert!(
            site.expected_rows(EntryPhase::NoExecution).is_empty(),
            "{site}"
        );
    }
    // `transaction_fault_matrix[T-SCRUB]` — live and binding — boundary:
    // "task_candidate_created appended; worktree, its intent, or snapshots
    // not yet removed". The worktree is still there, and R9 is the row that
    // holds "task worktree + its durable synced intent, and the objects its
    // index or HEAD references".
    //
    // This is PR3-ST07-011's witness. Under the shipped authority the
    // literal `[R9]` below was the value `validate_entry` *refused*, and an
    // entry claiming an empty before phase was the one it accepted.
    let scrub = EffectSiteId::Worktree(WorktreeSite::Remove);
    assert_eq!(scrub.fault_row(), FaultRow::TScrub);
    assert_eq!(scrub.row(), ResourceRow::R9);
    assert_eq!(
        scrub.expected_rows(EntryPhase::Before),
        vec![ResourceRow::R9]
    );
    assert_eq!(
        scrub.semantics(EntryPhase::Before).artifact,
        ResidueArtifact::TargetIntact
    );
    // The same matrix row covers the intent and the snapshots it names.
    for site in [
        EffectSiteId::Worktree(WorktreeSite::RemoveIntent),
        EffectSiteId::Snapshot(SnapshotSite::Remove),
        EffectSiteId::Snapshot(SnapshotSite::RemoveIntent),
    ] {
        assert_eq!(site.fault_row(), FaultRow::TScrub, "{site}");
        assert_eq!(site.expected_rows(EntryPhase::Before), vec![site.row()]);
    }
    // T-FAST: "assert_publishable read the integration ref head H ==
    // candidate.base_sha" — the CAS replaces a head that is there.
    let cas = EffectSiteId::Ref(RefSite::CompareAndSwapIntegration);
    assert_eq!(cas.fault_row(), FaultRow::TFast);
    assert_eq!(
        cas.expected_rows(EntryPhase::Before),
        vec![ResourceRow::R21]
    );
    // T-RUNSTART: "no ref until P8" — the creation of the same ref, in the
    // same row, finds nothing. The two differ in the authority and not in
    // anything a generic rule over R21 could see.
    let create_ref = EffectSiteId::Ref(RefSite::CreateIntegration);
    assert_eq!(create_ref.row(), cas.row());
    assert!(create_ref.expected_rows(EntryPhase::Before).is_empty());
    // T-CAND-OBJ (b): "the object and the candidate-prepared pin (R23)
    // exist" — so the deletion of that pin finds it, and the pinning does
    // not: "(a) ... and no pin exists".
    assert_eq!(
        EffectSiteId::Ref(RefSite::DeleteCandidatePin).expected_rows(EntryPhase::Before),
        vec![ResourceRow::R23]
    );
    assert!(
        EffectSiteId::Ref(RefSite::PinCandidatePrepared)
            .expected_rows(EntryPhase::Before)
            .is_empty()
    );
    // T-RUNSTART again: "P6 run_started durable ..., marker still present;
    // P7 marker removed".
    assert_eq!(
        EffectSiteId::RunDir(RunDirSite::RemoveMarker).expected_rows(EntryPhase::Before),
        vec![ResourceRow::R21]
    );
    // ...and "P1 marker **staged (.creating.tmp)** or published
    // (.creating ...)": the publication renames a temporary its own
    // staging site made durable, so R21 holds something at its before hook
    // — and what it holds is not the marker, so the words differ from
    // `RemoveMarker`'s even though the row does not.
    assert_eq!(
        EffectSiteId::RunDir(RunDirSite::PublishMarker).expected_rows(EntryPhase::Before),
        vec![ResourceRow::R21]
    );
    assert_eq!(
        EffectSiteId::RunDir(RunDirSite::PublishMarker)
            .semantics(EntryPhase::Before)
            .artifact,
        ResidueArtifact::PrecursorDurable
    );
    assert!(
        EffectSiteId::RunDir(RunDirSite::StageMarker)
            .expected_rows(EntryPhase::Before)
            .is_empty(),
        "the staging is the first half of the pair and finds nothing"
    );
    // PR3-ST07-014. `transaction_fault_matrix[T-DISPATCH]` — live and
    // binding — puts the boundary at "worktree **intent** or worktree not
    // yet created" and tables the resume as "recreate it (**intent then
    // add**)"; R9 is "Task worktree **+ its durable synced intent**". So a
    // kill at `Worktree.Add`'s before hook leaves R9 holding that intent.
    // The predecessor answered `[]` here and refused exactly the literal
    // below.
    let add = EffectSiteId::Worktree(WorktreeSite::Add);
    assert_eq!(add.row(), ResourceRow::R9);
    assert_eq!(add.fault_row(), FaultRow::TDispatch);
    assert_eq!(add.expected_rows(EntryPhase::Before), vec![ResourceRow::R9]);
    assert_eq!(
        add.semantics(EntryPhase::Before).artifact,
        ResidueArtifact::PrecursorDurable
    );
    // And the row is the same one `Worktree.Remove` names while the words
    // are not: R9 holds the intent here and the worktree there, and an
    // entry that said "the artifact this site acts on is present and
    // unchanged" of an add would be false.
    let scrub_words = EffectSiteId::Worktree(WorktreeSite::Remove)
        .semantics(EntryPhase::Before)
        .artifact;
    assert_eq!(scrub_words, ResidueArtifact::TargetIntact);
    assert_ne!(
        add.semantics(EntryPhase::Before).artifact.detail(),
        scrub_words.detail()
    );
    // The intent that add follows is itself a creation, and finds nothing.
    assert!(
        EffectSiteId::Worktree(WorktreeSite::WriteIntent)
            .expected_rows(EntryPhase::Before)
            .is_empty(),
        "the first half of the pair creates the intent it is about to write"
    );
    // WHERE THIS CLASSIFICATION STOPS, as an assertion rather than a
    // paragraph. `RunDir.CreatePrivateDir` runs at T-RUNSTART's P3a, after
    // "P0 public directory created" and "P1 marker ... published" — both
    // durable, both accounted for by R21, the same row this site names —
    // and its before phase still names nothing, because neither is an
    // earlier state of the private directory. A before phase names this
    // site's own artifact, not the durable prefix of its transaction.
    assert_eq!(
        EffectSiteId::RunDir(RunDirSite::CreatePrivateDir).row(),
        ResourceRow::R21
    );
    assert!(
        EffectSiteId::RunDir(RunDirSite::CreatePrivateDir)
            .expected_rows(EntryPhase::Before)
            .is_empty(),
        "the public half is durable at P3a and this entry does not claim it"
    );
    assert!(
        EffectSiteId::Event(EventSite::Append)
            .expected_rows(EntryPhase::Before)
            .is_empty(),
        "an append names the line it appends, not the log the open created"
    );
    // The containment rows, from `containment_sub_effects`: Windows kills
    // leave "no host process", Unix kills leave a group "the reaper settles
    // while holding R28". Both were R22 — the host-process handle row — and
    // both contradicted their own entry's detail.
    let spawn = EffectSiteId::Process(ProcessSite::Spawn);
    assert_eq!(spawn.row(), ResourceRow::R22);
    for point in [
        SubEffectPoint::AmbientJobJoined,
        SubEffectPoint::CreatedSuspended,
        SubEffectPoint::PrivateJobAssigned,
        SubEffectPoint::Resumed,
    ] {
        let phase = EntryPhase::Point {
            point,
            mode: InjectionMode::Kill,
        };
        assert!(spawn.expected_rows(phase).is_empty(), "{point}");
        assert_eq!(
            spawn.semantics(phase).artifact,
            ResidueArtifact::NoHostProcess
        );
    }
    for point in [
        SubEffectPoint::ReaperStarted,
        SubEffectPoint::PreExecPgidAndRegister,
        SubEffectPoint::Exec,
        SubEffectPoint::Registered,
    ] {
        let phase = EntryPhase::Point {
            point,
            mode: InjectionMode::Kill,
        };
        assert_eq!(
            spawn.expected_rows(phase),
            vec![ResourceRow::R28],
            "{point}"
        );
        assert_eq!(
            spawn.semantics(phase).artifact,
            ResidueArtifact::ReaperHeldGroup
        );
    }
    // And R22 is not left unused by the repair: the site's own after phase
    // is still the handle row, which is what makes the point rows a
    // statement about the points rather than a blanket change.
    assert_eq!(
        spawn.expected_rows(EntryPhase::After),
        vec![ResourceRow::R22]
    );
    // "after: the object present and referenced by the row named by row(),
    // or unreferenced R27 for the commit-tree sites"
    assert_eq!(
        commit_tree.expected_rows(EntryPhase::After),
        vec![ResourceRow::R27]
    );
    assert_eq!(commit_tree.row(), ResourceRow::R27);
    assert_eq!(
        append.expected_rows(EntryPhase::After),
        vec![ResourceRow::R21]
    );
    assert_eq!(
        staging.expected_rows(EntryPhase::After),
        vec![staging.row()]
    );
    // "IdUnread ... R27 object without a recorded id"
    assert_eq!(commit_tree.expected_rows(id_unread), vec![ResourceRow::R27]);
    // "Internal residue class ... objects present and unreferenced, R27,
    // with administrative residue in the owning worktree"
    assert_eq!(commit_tree.expected_rows(internal), vec![ResourceRow::R27]);
    assert_eq!(
        repair.expected_rows(internal),
        vec![ResourceRow::R27, repair.row()]
    );
    assert_ne!(
        repair.row(),
        ResourceRow::R27,
        "the two-row case has to be a site whose own row is not R27, or it is the one-row case"
    );

    // And the two phases `structure` binds to the before-phase action, and
    // no others.
    assert!(id_unread.resumes_as_before());
    assert!(internal.resumes_as_before());
    for phase in [
        EntryPhase::Before,
        EntryPhase::After,
        EntryPhase::NoExecution,
        EntryPhase::Point {
            point: SubEffectPoint::Written,
            mode: InjectionMode::Kill,
        },
    ] {
        assert!(!phase.resumes_as_before(), "{phase}");
    }
}

#[test]
fn the_format_refuses_residue_and_resume_semantics_the_site_does_not_have() {
    // A2's and A3's shared blind spot: an entry can be complete, correctly
    // keyed, correctly labelled and carry passing evidence while claiming
    // that a fault at its point leaves an unrelated ledger row and that a
    // resume does something the fault matrix does not say. Neither field
    // was consulted, so a unique garbage string in either satisfied the
    // fixture's own diversity counts as well as a right answer would.
    let host = Host::current();
    let commit_tree = EffectSiteId::Object(ObjectSite::CandidateCommitTree);
    let internal = EntryPhase::Residue {
        class: ResidueClass::ObjectInternal,
    };

    // Wrong rows, one wrong way at a time, for a residue class whose
    // authority is R27 alone.
    for (label, rows) in [
        ("an unrelated row", vec![ResourceRow::R9]),
        ("no rows at all", Vec::new()),
        (
            "R27 plus a row this site does not hold",
            vec![ResourceRow::R27, ResourceRow::R24],
        ),
        (
            "the right row twice",
            vec![ResourceRow::R27, ResourceRow::R27],
        ),
    ] {
        let mut entry = residue_entry(commit_tree, 61, 0);
        entry.expected_residue.rows = rows.clone();
        let error =
            validate_entry(&entry).expect_err("an entry claiming residue the site does not leave");
        assert!(
            matches!(error, RegistryError::WrongResidueRows { .. }),
            "{label} was accepted: {error}"
        );
        assert_eq!(
            FaultRegistry::new().insert(entry.clone()),
            Err(match validate_entry(&entry) {
                Err(error) => error,
                Ok(()) => unreachable!("just refused"),
            }),
            "{label} was accepted by the constructor"
        );
    }

    // An after-phase entry that claims the before-phase's empty residue,
    // and a before-phase entry that claims the after-phase's row: the two
    // directions of the same relation.
    let mut empty_after = hook_entry(commit_tree, EntryPhase::After);
    empty_after.expected_residue.rows = Vec::new();
    assert!(matches!(
        validate_entry(&empty_after),
        Err(RegistryError::WrongResidueRows { .. })
    ));
    let mut full_before = hook_entry(commit_tree, EntryPhase::Before);
    full_before.expected_residue.rows = vec![ResourceRow::R27];
    assert!(matches!(
        validate_entry(&full_before),
        Err(RegistryError::WrongResidueRows { .. })
    ));

    // A resume action that is not a resume action at all.
    let mut unnamed = hook_entry(commit_tree, EntryPhase::Before);
    unnamed.resume_action = "   ".to_owned();
    assert!(matches!(
        validate_entry(&unnamed),
        Err(RegistryError::UnnamedResumeAction { .. })
    ));

    // A resume action that is a resume action and is not this one. The
    // blank check above was the whole of what the format asked of the
    // field for every phase but two, so a unique, plausible, false claim
    // passed — and passed the fixture's own diversity counts while it did.
    for phase in [
        EntryPhase::Before,
        EntryPhase::After,
        EntryPhase::Point {
            point: SubEffectPoint::IdUnread,
            mode: InjectionMode::Kill,
        },
        internal,
    ] {
        let mut entry = if phase == internal {
            residue_entry(commit_tree, 61, 0)
        } else {
            hook_entry(commit_tree, phase)
        };
        entry.resume_action = "retry the command from the start".to_owned();
        let error = validate_entry(&entry).expect_err("a resume the matrix does not table");
        assert!(
            matches!(error, RegistryError::WrongResumeAction { .. }),
            "{phase}: {error}"
        );

        let mut entry = if phase == internal {
            residue_entry(commit_tree, 61, 0)
        } else {
            hook_entry(commit_tree, phase)
        };
        entry.expected_residue.detail = "some durable state or other".to_owned();
        let error = validate_entry(&entry).expect_err("residue the site does not leave");
        assert!(
            matches!(error, RegistryError::WrongResidueDetail { .. }),
            "{phase}: {error}"
        );
    }

    // Swapping two real answers is the sharper direction: both strings are
    // the matrix's own words, and neither belongs to this coordinate.
    let mut swapped = hook_entry(commit_tree, EntryPhase::After);
    swapped.expected_residue.detail = commit_tree
        .semantics(EntryPhase::Before)
        .artifact
        .detail()
        .to_owned();
    assert!(matches!(
        validate_entry(&swapped),
        Err(RegistryError::WrongResidueDetail { .. })
    ));
    let mut swapped = hook_entry(commit_tree, EntryPhase::After);
    swapped.resume_action = commit_tree
        .semantics(EntryPhase::Before)
        .action
        .text()
        .to_owned();
    assert!(matches!(
        validate_entry(&swapped),
        Err(RegistryError::WrongResumeAction { .. })
    ));

    // And the relation that needs the whole slice: `IdUnread` and the
    // `Internal` class resume by the site's *before-phase* action. The
    // format now refuses the entry on its own — the authority tables one
    // action for the coordinate — and `check_bijection`, which collects
    // failures rather than stopping at the first, still names the relation
    // it breaks as well as the entry it invalidates.
    for phase in [
        internal,
        EntryPhase::Point {
            point: SubEffectPoint::IdUnread,
            mode: InjectionMode::Kill,
        },
    ] {
        let mut entries = self_test_registry(host);
        let position = entries
            .iter()
            .position(|entry| entry.site == commit_tree && entry.phase == phase)
            .expect("the fixture carries this entry");
        entries[position].resume_action = "do nothing".to_owned();
        let failures = check_bijection(
            &self_test_inventory(),
            &self_test_harness(host),
            &entries,
            host,
        );
        assert!(
            failures.iter().any(|failure| matches!(
                failure,
                BijectionFailure::ResumeActionNotBeforeAction { .. }
            )),
            "a {phase} entry resuming by something other than the before-phase action passed: \
                 {failures:#?}"
        );
        assert!(
            failures
                .iter()
                .any(|failure| matches!(failure, BijectionFailure::InvalidEntry { .. })),
            "{failures:#?}"
        );
    }
}

#[test]
fn a_pruning_sites_after_phase_records_the_objects_it_released() {
    // `structure`: "the pruning sites' after-phase entries record the
    // released objects as R27 residue". The framework shipped with one
    // `After | Point => vec![self.row()]` arm, so the packet-required entry
    // was *refused* and the wrong one — a removed worktree still held by
    // R9 — was the only one the format would take.
    let removed = EffectSiteId::Worktree(WorktreeSite::Remove);
    assert_eq!(removed.row(), ResourceRow::R9, "the row it accounted for");
    assert_eq!(
        removed.expected_rows(EntryPhase::After),
        vec![ResourceRow::R27],
        "a forced removal releases its index-referenced objects and keeps nothing"
    );

    let mut entry = hook_entry(removed, EntryPhase::After);
    validate_entry(&entry).expect("the packet-required released-object entry");
    entry.expected_residue.rows = vec![removed.row()];
    assert!(
        matches!(
            validate_entry(&entry),
            Err(RegistryError::WrongResidueRows { .. })
        ),
        "a removed worktree is still accounted for by the row it no longer occupies"
    );

    // Every pruning site the packet names, and only those: `Worktree.Remove`
    // and `Worktree.RemoveStaging`, `Snapshot.Remove`, and `Ref.Delete*`.
    let released: BTreeSet<String> = EffectSiteId::all()
        .into_iter()
        .filter(|site| site.after_effect() == AfterEffect::Released)
        .map(|site| site.name())
        .collect();
    assert_eq!(
        released,
        BTreeSet::from([
            "Worktree.Remove".to_owned(),
            "Worktree.RemoveStaging".to_owned(),
            "Snapshot.Remove".to_owned(),
            "Ref.DeleteCandidatesRef".to_owned(),
            "Ref.DeleteCandidatePin".to_owned(),
            "Ref.DeletePreparedPin".to_owned(),
        ])
    );
    for site in EffectSiteId::all() {
        if site.after_effect() != AfterEffect::Released {
            continue;
        }
        assert_eq!(
            site.expected_rows(EntryPhase::After),
            vec![ResourceRow::R27],
            "{site}"
        );
        assert_eq!(
            site.semantics(EntryPhase::After).artifact,
            ResidueArtifact::Released,
            "{site}"
        );
    }
}

#[test]
fn both_commit_tree_sites_leave_an_unrecorded_id_at_r27() {
    // "IdUnread for the two commit-tree sites (hook; R27 object without a
    // recorded id)". Both, and stated rather than inherited from `row()`:
    // `row()` is R27 for both today, which is exactly why moving one of
    // them to R24 survived the suite.
    let id_unread = EntryPhase::Point {
        point: SubEffectPoint::IdUnread,
        mode: InjectionMode::Kill,
    };
    let sites: Vec<EffectSiteId> = EffectSiteId::all()
        .into_iter()
        .filter(|site| site.sub_effects().contains(&SubEffectPoint::IdUnread))
        .collect();
    assert_eq!(
        sites,
        vec![
            EffectSiteId::Object(ObjectSite::SnapshotCommitTree),
            EffectSiteId::Object(ObjectSite::CandidateCommitTree),
        ],
        "the two the packet names, in inventory order"
    );
    for site in sites {
        assert_eq!(
            site.expected_rows(id_unread),
            vec![ResourceRow::R27],
            "{site}"
        );
        let semantics = site.semantics(id_unread);
        assert_eq!(semantics.artifact, ResidueArtifact::IdNotRecorded, "{site}");
        assert_eq!(
            semantics.action,
            site.semantics(EntryPhase::Before).action,
            "{site}: \"resume action = the before-phase action\""
        );
        let mut entry = hook_entry(site, id_unread);
        validate_entry(&entry).expect("the packet's own entry");
        entry.expected_residue.rows = vec![ResourceRow::R24];
        assert!(matches!(
            validate_entry(&entry),
            Err(RegistryError::WrongResidueRows { .. })
        ));
    }
}

/// Every site of the inventory and what its *after* phase leaves, written
/// out by dotted name from the packet's own words rather than derived from
/// the enums.
///
/// Independent of the production table in the way that matters: production
/// classifies by variant pattern, in eleven grouped matches, and this is a
/// flat list keyed by the name the wire format uses. A production arm that
/// merges two variants, moves one between buckets, or acquires a default
/// disagrees with a row here; a site added to a group and forgotten here
/// fails the totality assertion below rather than passing unclassified.
///
/// Reading:
///
/// * `Referenced` — the site publishes something and its own `row()`
///   references it afterwards.
/// * `Unreferenced` — "unreferenced R27 for the commit-tree sites".
/// * `Released` — a pruning site: "the release of objects to R27 is never
///   a separate effect but the after-phase residue of the pruning sites
///   (Worktree.Remove, Snapshot.Remove, Ref.Delete*), whose entries record
///   it", and `effect_phases_covered`'s "worktree/staging/snapshot ...
///   removals (forced; with the objects they referenced released to R27
///   and administrative residue removed)".
/// * `Removed` — a removal with no objects to release: the row that
///   accounted for what it removed holds nothing afterwards.
/// * `NoEffect` — the four sites the design says perform no effect.
const AFTER_EFFECT_ORACLE: &[(&str, AfterEffect)] = &[
    ("Worktree.CreateExecutionRoot", AfterEffect::Referenced),
    ("Worktree.RemoveExecutionRoot", AfterEffect::Removed),
    ("Worktree.WriteIntent", AfterEffect::Referenced),
    ("Worktree.Add", AfterEffect::Referenced),
    ("Worktree.Verify", AfterEffect::NoEffect),
    ("Worktree.Remove", AfterEffect::Released),
    ("Worktree.RemoveIntent", AfterEffect::Removed),
    ("Worktree.WriteStagingIntent", AfterEffect::Referenced),
    ("Worktree.AddStaging", AfterEffect::Referenced),
    ("Worktree.RemoveStaging", AfterEffect::Released),
    ("Worktree.RemoveStagingIntent", AfterEffect::Removed),
    ("Snapshot.WriteIntent", AfterEffect::Referenced),
    ("Snapshot.Add", AfterEffect::Referenced),
    ("Snapshot.Remove", AfterEffect::Released),
    ("Snapshot.RemoveIntent", AfterEffect::Removed),
    ("Ref.CreateIntegration", AfterEffect::Referenced),
    ("Ref.CompareAndSwapIntegration", AfterEffect::Referenced),
    ("Ref.CreateCandidates", AfterEffect::Referenced),
    ("Ref.DeleteCandidatesRef", AfterEffect::Released),
    ("Ref.PinCandidatePrepared", AfterEffect::Referenced),
    ("Ref.DeleteCandidatePin", AfterEffect::Released),
    ("Ref.PinPrepared", AfterEffect::Referenced),
    ("Ref.DeletePreparedPin", AfterEffect::Released),
    ("Object.CandidateStage", AfterEffect::Referenced),
    ("Object.CandidateWriteTree", AfterEffect::Referenced),
    ("Object.SnapshotCommitTree", AfterEffect::Unreferenced),
    ("Object.CandidateCommitTree", AfterEffect::Unreferenced),
    ("Object.ProposalCherryPick", AfterEffect::Referenced),
    ("Object.RepairMaterialize", AfterEffect::Referenced),
    ("RunDir.CreatePublicDir", AfterEffect::Referenced),
    ("RunDir.StageMarker", AfterEffect::Referenced),
    ("RunDir.PublishMarker", AfterEffect::Referenced),
    ("RunDir.RemoveMarker", AfterEffect::Removed),
    ("RunDir.CreatePrivateDir", AfterEffect::Referenced),
    ("RunDir.StageOwnerRecord", AfterEffect::Referenced),
    ("RunDir.PublishOwnerRecord", AfterEffect::Referenced),
    ("RunDir.StageCommitRecord", AfterEffect::Referenced),
    ("RunDir.PublishCommitRecord", AfterEffect::Referenced),
    ("RunDir.WritePlan", AfterEffect::Referenced),
    ("RunDir.WriteReport", AfterEffect::Referenced),
    ("RunDir.WriteQuestionPayload", AfterEffect::Referenced),
    ("RunDir.RemovePrivateHusk", AfterEffect::Removed),
    ("RunDir.RemovePublicHusk", AfterEffect::Removed),
    ("Event.OpenLog", AfterEffect::Referenced),
    ("Event.ProvePrefixStable", AfterEffect::NoEffect),
    ("Event.AppendFirst", AfterEffect::Referenced),
    ("Event.Append", AfterEffect::Referenced),
    ("Event.AppendInformational", AfterEffect::Referenced),
    ("Event.LegacyOpenLog", AfterEffect::Referenced),
    ("Event.LegacyAppend", AfterEffect::Referenced),
    ("Answer.StageWrite", AfterEffect::Referenced),
    ("Answer.PublishRename", AfterEffect::Referenced),
    ("Answer.Ingest", AfterEffect::NoEffect),
    ("Lock.AcquireRun", AfterEffect::Referenced),
    ("Lock.AcquireWorktree", AfterEffect::Referenced),
    ("Lock.ProbeCleanupExclusive", AfterEffect::Referenced),
    ("Lock.Release", AfterEffect::Removed),
    ("Lock.CreateWorktreeLockFile", AfterEffect::Referenced),
    ("Lock.ObserveCleanupHold", AfterEffect::NoEffect),
    ("Report.Write", AfterEffect::Referenced),
    ("Process.Spawn", AfterEffect::Referenced),
    ("Process.Terminate", AfterEffect::Removed),
    ("Container.WriteIntent", AfterEffect::Referenced),
    ("Container.Create", AfterEffect::Referenced),
    ("Container.Start", AfterEffect::Referenced),
    ("Container.MountGitView", AfterEffect::Referenced),
    ("Container.Stop", AfterEffect::Referenced),
    ("Container.Remove", AfterEffect::Removed),
    ("Container.UnmountGitView", AfterEffect::Removed),
    ("Container.RemoveIntent", AfterEffect::Removed),
];

/// Every site and the before-phase state the packet gives it, written by
/// name.
///
/// PR3-ST07-011's witness table. A second, independent statement of
/// `before_state()` — not a derivation from it and not a derivation from
/// `after_effect()` either, which is why it is a literal list of seventy
/// names rather than a rule. The shipped authority answered "nothing is
/// durable" for all seventy, which is a rule that is right for forty-nine
/// of them and inverts the registry's verdict on the other twenty-one.
///
/// The twenty-one are every removal and release, plus the two in-place
/// replacements the fault matrix puts after an artifact that exists:
/// `Ref.CompareAndSwapIntegration` (T-FAST reads the head H before the CAS)
/// and `Container.Start`/`Container.Stop` (T-CONTAINER: "container created
/// ... and verified; docker start issued; ... the invocation running").
const BEFORE_STATE_ORACLE: &[(&str, BeforeState)] = &[
    ("Worktree.CreateExecutionRoot", BeforeState::Absent),
    ("Worktree.RemoveExecutionRoot", BeforeState::Present),
    ("Worktree.WriteIntent", BeforeState::Absent),
    ("Worktree.Add", BeforeState::PrecursorDurable),
    ("Worktree.Verify", BeforeState::Absent),
    ("Worktree.Remove", BeforeState::Present),
    ("Worktree.RemoveIntent", BeforeState::Present),
    ("Worktree.WriteStagingIntent", BeforeState::Absent),
    ("Worktree.AddStaging", BeforeState::PrecursorDurable),
    ("Worktree.RemoveStaging", BeforeState::Present),
    ("Worktree.RemoveStagingIntent", BeforeState::Present),
    ("Snapshot.WriteIntent", BeforeState::Absent),
    ("Snapshot.Add", BeforeState::PrecursorDurable),
    ("Snapshot.Remove", BeforeState::Present),
    ("Snapshot.RemoveIntent", BeforeState::Present),
    ("Ref.CreateIntegration", BeforeState::Absent),
    ("Ref.CompareAndSwapIntegration", BeforeState::Present),
    ("Ref.CreateCandidates", BeforeState::Absent),
    ("Ref.DeleteCandidatesRef", BeforeState::Present),
    ("Ref.PinCandidatePrepared", BeforeState::Absent),
    ("Ref.DeleteCandidatePin", BeforeState::Present),
    ("Ref.PinPrepared", BeforeState::Absent),
    ("Ref.DeletePreparedPin", BeforeState::Present),
    ("Object.CandidateStage", BeforeState::Absent),
    ("Object.CandidateWriteTree", BeforeState::Absent),
    ("Object.SnapshotCommitTree", BeforeState::Absent),
    ("Object.CandidateCommitTree", BeforeState::Absent),
    ("Object.ProposalCherryPick", BeforeState::Absent),
    ("Object.RepairMaterialize", BeforeState::Absent),
    ("RunDir.CreatePublicDir", BeforeState::Absent),
    ("RunDir.StageMarker", BeforeState::Absent),
    ("RunDir.PublishMarker", BeforeState::PrecursorDurable),
    ("RunDir.RemoveMarker", BeforeState::Present),
    ("RunDir.CreatePrivateDir", BeforeState::Absent),
    ("RunDir.StageOwnerRecord", BeforeState::Absent),
    ("RunDir.PublishOwnerRecord", BeforeState::PrecursorDurable),
    ("RunDir.StageCommitRecord", BeforeState::Absent),
    ("RunDir.PublishCommitRecord", BeforeState::PrecursorDurable),
    ("RunDir.WritePlan", BeforeState::Absent),
    ("RunDir.WriteReport", BeforeState::Absent),
    ("RunDir.WriteQuestionPayload", BeforeState::Absent),
    ("RunDir.RemovePrivateHusk", BeforeState::Present),
    ("RunDir.RemovePublicHusk", BeforeState::Present),
    ("Event.OpenLog", BeforeState::Absent),
    ("Event.ProvePrefixStable", BeforeState::Absent),
    ("Event.AppendFirst", BeforeState::Absent),
    ("Event.Append", BeforeState::Absent),
    ("Event.AppendInformational", BeforeState::Absent),
    ("Event.LegacyOpenLog", BeforeState::Absent),
    ("Event.LegacyAppend", BeforeState::Absent),
    ("Answer.StageWrite", BeforeState::Absent),
    ("Answer.PublishRename", BeforeState::PrecursorDurable),
    ("Answer.Ingest", BeforeState::Absent),
    ("Lock.AcquireRun", BeforeState::Absent),
    ("Lock.AcquireWorktree", BeforeState::Absent),
    ("Lock.ProbeCleanupExclusive", BeforeState::Absent),
    ("Lock.Release", BeforeState::Present),
    ("Lock.CreateWorktreeLockFile", BeforeState::Absent),
    ("Lock.ObserveCleanupHold", BeforeState::Absent),
    ("Report.Write", BeforeState::Absent),
    ("Process.Spawn", BeforeState::Absent),
    ("Process.Terminate", BeforeState::Present),
    ("Container.WriteIntent", BeforeState::Absent),
    ("Container.Create", BeforeState::PrecursorDurable),
    ("Container.Start", BeforeState::Present),
    ("Container.MountGitView", BeforeState::Absent),
    ("Container.Stop", BeforeState::Present),
    ("Container.Remove", BeforeState::Present),
    ("Container.UnmountGitView", BeforeState::Present),
    ("Container.RemoveIntent", BeforeState::Present),
];

/// The rows a fault at a point leaves holding something, as the packet
/// states them and not as `residue_rows` computes them.
///
/// Four answers, which is the point: the predecessor of `residue_rows`
/// returned the site's own row for thirteen of the fifteen points, and the
/// oracle that checked it recorded only a `bool` for "R27 or the site row"
/// — so it could not have expressed, let alone caught, a containment point
/// claiming R22 while its own artifact said the coordinator left no host
/// process at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OracleRows {
    /// The row the site's own `row()` names.
    SiteRow,
    /// R27 and nothing else.
    R27,
    /// R28 and nothing else — a surviving Unix reaper's cleanup hold.
    R28,
    /// No row at all.
    NoRow,
}

impl OracleRows {
    /// The rows this answer names at a site whose own row is `site_row`.
    fn rows(self, site_row: ResourceRow) -> Vec<ResourceRow> {
        match self {
            Self::SiteRow => vec![site_row],
            Self::R27 => vec![ResourceRow::R27],
            Self::R28 => vec![ResourceRow::R28],
            Self::NoRow => Vec::new(),
        }
    }
}

/// Every `(point, mode)` and the semantics the packet gives it: the rows
/// the residue sits at, the artifact, and the tabled recovery.
///
/// Total over the whole product, not only the pairs
/// [`SubEffectPoint::modes`] admits, because
/// [`SubEffectPoint::resume_action`] is.
const POINT_ORACLE: &[(
    SubEffectPoint,
    InjectionMode,
    OracleRows,
    ResidueArtifact,
    ResumeAction,
)] = &[
    (
        SubEffectPoint::IdUnread,
        InjectionMode::Kill,
        OracleRows::R27,
        ResidueArtifact::IdNotRecorded,
        ResumeAction::ResumeUnperformed,
    ),
    (
        SubEffectPoint::IdUnread,
        InjectionMode::ErrorReturn,
        OracleRows::R27,
        ResidueArtifact::IdNotRecorded,
        ResumeAction::ResumeUnperformed,
    ),
    (
        SubEffectPoint::Written,
        InjectionMode::Kill,
        OracleRows::SiteRow,
        ResidueArtifact::UnsyncedBytes,
        ResumeAction::NextOpenConverges,
    ),
    (
        SubEffectPoint::Written,
        InjectionMode::ErrorReturn,
        OracleRows::SiteRow,
        ResidueArtifact::UnsyncedBytes,
        ResumeAction::AppendErrorProtocol,
    ),
    (
        SubEffectPoint::WrittenFull,
        InjectionMode::Kill,
        OracleRows::SiteRow,
        ResidueArtifact::UnsyncedLine,
        ResumeAction::NextOpenConverges,
    ),
    (
        SubEffectPoint::WrittenFull,
        InjectionMode::ErrorReturn,
        OracleRows::SiteRow,
        ResidueArtifact::UnsyncedLine,
        ResumeAction::AppendErrorProtocol,
    ),
    (
        SubEffectPoint::Synced,
        InjectionMode::Kill,
        OracleRows::SiteRow,
        ResidueArtifact::SyncedLine,
        ResumeAction::NextOpenConverges,
    ),
    (
        SubEffectPoint::Synced,
        InjectionMode::ErrorReturn,
        OracleRows::SiteRow,
        ResidueArtifact::SyncedLine,
        ResumeAction::AppendErrorProtocol,
    ),
    (
        SubEffectPoint::Create,
        InjectionMode::Kill,
        OracleRows::SiteRow,
        ResidueArtifact::LogCreated,
        ResumeAction::NextOpenConverges,
    ),
    (
        SubEffectPoint::Create,
        InjectionMode::ErrorReturn,
        OracleRows::SiteRow,
        ResidueArtifact::LogCreated,
        ResumeAction::RefuseResumably,
    ),
    (
        SubEffectPoint::TruncateTornTail,
        InjectionMode::Kill,
        OracleRows::SiteRow,
        ResidueArtifact::TornTailTruncated,
        ResumeAction::NextOpenConverges,
    ),
    (
        SubEffectPoint::TruncateTornTail,
        InjectionMode::ErrorReturn,
        OracleRows::SiteRow,
        ResidueArtifact::TornTailTruncated,
        ResumeAction::RefuseResumably,
    ),
    (
        SubEffectPoint::SyncPrefix,
        InjectionMode::Kill,
        OracleRows::SiteRow,
        ResidueArtifact::PrefixPossiblyNonDurable,
        ResumeAction::RefuseResumably,
    ),
    (
        SubEffectPoint::SyncPrefix,
        InjectionMode::ErrorReturn,
        OracleRows::SiteRow,
        ResidueArtifact::PrefixPossiblyNonDurable,
        ResumeAction::RefuseResumably,
    ),
    (
        SubEffectPoint::AmbientJobJoined,
        InjectionMode::Kill,
        OracleRows::NoRow,
        ResidueArtifact::NoHostProcess,
        ResumeAction::AmbientHandleTerminates,
    ),
    (
        SubEffectPoint::AmbientJobJoined,
        InjectionMode::ErrorReturn,
        OracleRows::NoRow,
        ResidueArtifact::NoProcessSpawned,
        ResumeAction::RefuseUnspawned,
    ),
    (
        SubEffectPoint::CreatedSuspended,
        InjectionMode::Kill,
        OracleRows::NoRow,
        ResidueArtifact::NoHostProcess,
        ResumeAction::AmbientHandleTerminates,
    ),
    (
        SubEffectPoint::CreatedSuspended,
        InjectionMode::ErrorReturn,
        OracleRows::NoRow,
        ResidueArtifact::NoHostProcess,
        ResumeAction::AmbientHandleTerminates,
    ),
    (
        SubEffectPoint::PrivateJobAssigned,
        InjectionMode::Kill,
        OracleRows::NoRow,
        ResidueArtifact::NoHostProcess,
        ResumeAction::AmbientHandleTerminates,
    ),
    (
        SubEffectPoint::PrivateJobAssigned,
        InjectionMode::ErrorReturn,
        OracleRows::NoRow,
        ResidueArtifact::NoHostProcess,
        ResumeAction::AmbientHandleTerminates,
    ),
    (
        SubEffectPoint::Resumed,
        InjectionMode::Kill,
        OracleRows::NoRow,
        ResidueArtifact::NoHostProcess,
        ResumeAction::AmbientHandleTerminates,
    ),
    (
        SubEffectPoint::Resumed,
        InjectionMode::ErrorReturn,
        OracleRows::NoRow,
        ResidueArtifact::NoHostProcess,
        ResumeAction::AmbientHandleTerminates,
    ),
    (
        SubEffectPoint::ReaperStarted,
        InjectionMode::Kill,
        OracleRows::R28,
        ResidueArtifact::ReaperHeldGroup,
        ResumeAction::ReaperSettlesGroup,
    ),
    (
        SubEffectPoint::ReaperStarted,
        InjectionMode::ErrorReturn,
        OracleRows::R28,
        ResidueArtifact::ReaperHeldGroup,
        ResumeAction::ReaperSettlesGroup,
    ),
    (
        SubEffectPoint::PreExecPgidAndRegister,
        InjectionMode::Kill,
        OracleRows::R28,
        ResidueArtifact::ReaperHeldGroup,
        ResumeAction::ReaperSettlesGroup,
    ),
    (
        SubEffectPoint::PreExecPgidAndRegister,
        InjectionMode::ErrorReturn,
        OracleRows::R28,
        ResidueArtifact::ReaperHeldGroup,
        ResumeAction::ReaperSettlesGroup,
    ),
    (
        SubEffectPoint::Exec,
        InjectionMode::Kill,
        OracleRows::R28,
        ResidueArtifact::ReaperHeldGroup,
        ResumeAction::ReaperSettlesGroup,
    ),
    (
        SubEffectPoint::Exec,
        InjectionMode::ErrorReturn,
        OracleRows::R28,
        ResidueArtifact::ReaperHeldGroup,
        ResumeAction::ReaperSettlesGroup,
    ),
    (
        SubEffectPoint::Registered,
        InjectionMode::Kill,
        OracleRows::R28,
        ResidueArtifact::ReaperHeldGroup,
        ResumeAction::ReaperSettlesGroup,
    ),
    (
        SubEffectPoint::Registered,
        InjectionMode::ErrorReturn,
        OracleRows::R28,
        ResidueArtifact::ReaperHeldGroup,
        ResumeAction::ReaperSettlesGroup,
    ),
];

#[test]
fn the_residue_and_recovery_authority_is_exhaustive_and_says_what_the_packet_says() {
    // The class this test is against, not the three symptoms of it: before
    // the typed authority, `expected_rows` answered `vec![self.row()]` for
    // the after phase and every point of every site, `expected_residue.detail`
    // was read by nothing at all, and `resume_action` had to be non-blank
    // and, for two phases out of five, equal to another entry's string.
    // Three fields, one of them partly wrong and two of them unchecked, and
    // no per-site statement anywhere the compiler could see.
    //
    // So: every site, every phase, against a table written by name.

    // (1) The after-phase oracle is total over the inventory and agrees
    //     with the enums. Totality first: an oracle that silently omitted
    //     a site would let production answer for it unchallenged.
    let inventory: BTreeSet<String> = EffectSiteId::all()
        .into_iter()
        .map(|site| site.name())
        .collect();
    let oracle: BTreeMap<&str, AfterEffect> = AFTER_EFFECT_ORACLE.iter().copied().collect();
    assert_eq!(
        oracle.len(),
        AFTER_EFFECT_ORACLE.len(),
        "the oracle names a site twice"
    );
    assert_eq!(
        oracle
            .keys()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<String>>(),
        inventory,
        "the oracle and the enums disagree about what the inventory is"
    );
    assert_eq!(oracle.len(), INVENTORY_SIZE);
    for site in EffectSiteId::all() {
        assert_eq!(
            site.after_effect(),
            oracle[site.name().as_str()],
            "{site}'s after phase"
        );
    }

    // (1b) The before-phase oracle, the same way and to the same standard.
    let before_oracle: BTreeMap<&str, BeforeState> = BEFORE_STATE_ORACLE.iter().copied().collect();
    assert_eq!(
        before_oracle.len(),
        BEFORE_STATE_ORACLE.len(),
        "the before-state oracle names a site twice"
    );
    assert_eq!(
        before_oracle
            .keys()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<String>>(),
        inventory,
        "the before-state oracle and the enums disagree about what the inventory is"
    );
    assert_eq!(before_oracle.len(), INVENTORY_SIZE);
    for site in EffectSiteId::all() {
        assert_eq!(
            site.before_state(),
            before_oracle[site.name().as_str()],
            "{site}'s before phase"
        );
    }
    // All three classifications occur, and none is a rounding error: an
    // oracle that said `Absent` for sixty-nine sites would restate the
    // defect and pass, and one that collapsed the two non-empty answers
    // would restate PR3-ST07-014's.
    let count = |state: BeforeState| {
        EffectSiteId::all()
            .into_iter()
            .filter(|site| site.before_state() == state)
            .count()
    };
    let (absent, precursor, present) = (
        count(BeforeState::Absent),
        count(BeforeState::PrecursorDurable),
        count(BeforeState::Present),
    );
    assert_eq!(
        present, 21,
        "the sites whose primitive acts on something already durable"
    );
    assert_eq!(
        precursor, 8,
        "the second halves of the two-step pairs the packet names as pairs"
    );
    assert_eq!(absent, 41);
    assert_eq!(absent + precursor + present, EffectSiteId::all().len());
    // The two non-empty classifications name the same row and must not
    // carry the same words, or the third classification is decoration.
    assert_ne!(
        ResidueArtifact::PrecursorDurable.detail(),
        ResidueArtifact::TargetIntact.detail()
    );
    assert_ne!(
        ResidueArtifact::PrecursorDurable.detail(),
        ResidueArtifact::Nothing.detail()
    );
    // And every site classified `PrecursorDurable` is the second half of a
    // pair whose first half is a site of the same group and the same row,
    // classified `Absent`. A site with no such partner is not a two-step
    // protocol and does not belong here.
    for site in EffectSiteId::all() {
        if site.before_state() != BeforeState::PrecursorDurable {
            continue;
        }
        assert!(
            EffectSiteId::all().into_iter().any(|other| {
                other != site
                    && other.group() == site.group()
                    && other.row() == site.row()
                    && other.before_state() == BeforeState::Absent
                    && other.after_effect() == AfterEffect::Referenced
            }),
            "{site} claims a durable precursor and no site of its group and row makes one"
        );
    }
    // And it is *not* `after_effect` wearing a second name. If it were, a
    // mutation of one table would move the other and no test between them
    // could see it. Two sites separate them in each direction.
    let cas = EffectSiteId::Ref(RefSite::CompareAndSwapIntegration);
    assert_eq!(cas.after_effect(), AfterEffect::Referenced);
    assert_eq!(cas.before_state(), BeforeState::Present);
    let add = EffectSiteId::Worktree(WorktreeSite::Add);
    assert_eq!(add.after_effect(), AfterEffect::Referenced);
    assert_eq!(add.before_state(), BeforeState::PrecursorDurable);
    let intent = EffectSiteId::Worktree(WorktreeSite::WriteIntent);
    assert_eq!(intent.after_effect(), AfterEffect::Referenced);
    assert_eq!(intent.before_state(), BeforeState::Absent);
    let verify = EffectSiteId::Worktree(WorktreeSite::Verify);
    assert_eq!(verify.after_effect(), AfterEffect::NoEffect);
    assert_eq!(
        verify.before_state(),
        BeforeState::Absent,
        "a read-only observation performs nothing at either phase"
    );
    for state in [
        BeforeState::Absent,
        BeforeState::PrecursorDurable,
        BeforeState::Present,
    ] {
        assert!(
            EffectSiteId::all().into_iter().any(|site| {
                site.after_effect() == AfterEffect::Referenced && site.before_state() == state
            }),
            "the `Referenced` after-effect class does not reach {state:?}, so one table could \
                 be determining the other"
        );
    }
    // Every class is exercised by some site, so no arm of the enum is
    // asserted only against itself.
    for effect in [
        AfterEffect::NoEffect,
        AfterEffect::Referenced,
        AfterEffect::Unreferenced,
        AfterEffect::Released,
        AfterEffect::Removed,
    ] {
        assert!(
            EffectSiteId::all()
                .into_iter()
                .any(|site| site.after_effect() == effect),
            "{effect:?} classifies no site"
        );
    }
    // `NoEffect` is exactly the read-only claim, from the other direction.
    for site in EffectSiteId::all() {
        assert_eq!(
            site.after_effect() == AfterEffect::NoEffect,
            site.is_read_only(),
            "{site}"
        );
    }

    // (2) The point oracle, over the whole (point, mode) product. Two
    //     probe rows, not one: a table that answered `vec![site_row]`
    //     everywhere would satisfy a single-probe check for every point
    //     whose oracle answer happens to be `SiteRow`, and the containment
    //     answers are exactly the ones that must *not* move with the site.
    assert_eq!(
        POINT_ORACLE.len(),
        SubEffectPoint::ALL.len() * InjectionMode::ALL.len(),
        "the point oracle is not total over the product"
    );
    for (point, mode, rows, artifact, action) in POINT_ORACLE.iter().copied() {
        for probe_row in [ResourceRow::R21, ResourceRow::R22] {
            assert_eq!(
                point.residue_rows(probe_row),
                rows.rows(probe_row),
                "{point}'s residue rows at a site whose row is {probe_row}"
            );
        }
        assert_eq!(
            point.residue_artifact(mode),
            artifact,
            "{point}/{mode:?}'s artifact"
        );
        assert_eq!(
            point.resume_action(mode),
            action,
            "{point}/{mode:?}'s resume action"
        );
    }
    // The four answers all occur, so no arm of the oracle is asserted only
    // against itself, and each occurs where the packet puts it.
    let probe_row = ResourceRow::R21;
    let answering = |rows: Vec<ResourceRow>| -> Vec<SubEffectPoint> {
        SubEffectPoint::ALL
            .iter()
            .copied()
            .filter(|point| point.residue_rows(probe_row) == rows)
            .collect()
    };
    assert_eq!(
        answering(vec![ResourceRow::R27]),
        vec![SubEffectPoint::IdUnread],
        "\"IdUnread ... R27 object without a recorded id\""
    );
    // `containment_sub_effects`, Windows: "a coordinator kill after any of
    // these leaves no host process".
    assert_eq!(
        answering(Vec::new()),
        vec![
            SubEffectPoint::AmbientJobJoined,
            SubEffectPoint::CreatedSuspended,
            SubEffectPoint::PrivateJobAssigned,
            SubEffectPoint::Resumed,
        ],
        "the Windows containment points"
    );
    // Unix: "leaves a group the reaper settles while holding R28".
    assert_eq!(
        answering(vec![ResourceRow::R28]),
        vec![
            SubEffectPoint::ReaperStarted,
            SubEffectPoint::PreExecPgidAndRegister,
            SubEffectPoint::Exec,
            SubEffectPoint::Registered,
        ],
        "the Unix containment points"
    );
    assert_eq!(
        answering(vec![probe_row]).len(),
        SubEffectPoint::ALL.len() - 9,
        "the append and open points"
    );
    // The platform half is not an accident of which points were listed:
    // every point that leaves no row is a Windows point and every point
    // that leaves R28 is a Unix one, stated from `platform()`.
    for point in SubEffectPoint::ALL.iter().copied() {
        match point.platform() {
            Platform::Windows => assert!(
                point.residue_rows(probe_row).is_empty(),
                "{point} is a Windows containment point and must leave no row"
            ),
            Platform::Unix => assert_eq!(
                point.residue_rows(probe_row),
                vec![ResourceRow::R28],
                "{point} is a Unix containment point and must leave the reaper's hold"
            ),
            Platform::Any => assert!(
                !point.residue_rows(probe_row).is_empty()
                    && point.residue_rows(probe_row) != vec![ResourceRow::R28],
                "{point}"
            ),
        }
    }
    // R22 is the row for a host-process handle, and no containment point
    // leaves one: that was the shipped answer for all eight of them.
    for point in SubEffectPoint::ALL.iter().copied() {
        if point.platform() == Platform::Any {
            continue;
        }
        assert!(
            !point
                .residue_rows(ResourceRow::R22)
                .contains(&ResourceRow::R22),
            "{point} still claims the R22 handle the dying coordinator does not have"
        );
    }
    assert!(
        SubEffectPoint::ALL
            .iter()
            .any(|point| point.resume_action(InjectionMode::Kill)
                != point.resume_action(InjectionMode::ErrorReturn)),
        "no point's recovery reads the mode, so the mode is not part of the coordinate"
    );

    // (3) The phase-uniform half, over every site — stated once here
    //     because `structure` states it once.
    for site in EffectSiteId::all() {
        let before = site.semantics(EntryPhase::Before);
        match site.before_state() {
            BeforeState::Absent => {
                assert!(before.rows.is_empty(), "{site}");
                assert_eq!(before.artifact, ResidueArtifact::Nothing, "{site}");
            }
            // The two classifications that name the same row and must not
            // carry the same words: the row holds the intent, or it holds
            // the target — and only one of the two is the thing this
            // site's primitive is about to act on.
            BeforeState::PrecursorDurable => {
                assert_eq!(before.rows, vec![site.row()], "{site}");
                assert_eq!(before.artifact, ResidueArtifact::PrecursorDurable, "{site}");
            }
            BeforeState::Present => {
                assert_eq!(before.rows, vec![site.row()], "{site}");
                assert_eq!(before.artifact, ResidueArtifact::TargetIntact, "{site}");
            }
        }
        // The action is the one thing the before phase *is* uniform in,
        // and it has to stay that way: `resumes_as_before` binds two other
        // phases to it.
        assert_eq!(before.action, ResumeAction::ResumeUnperformed, "{site}");

        let none = site.semantics(EntryPhase::NoExecution);
        assert!(none.rows.is_empty(), "{site}");
        assert_eq!(none.artifact, ResidueArtifact::NotReached, "{site}");
        assert_eq!(none.action, ResumeAction::NotExecuted, "{site}");

        let after = site.semantics(EntryPhase::After);
        match site.after_effect() {
            AfterEffect::NoEffect => {
                assert!(after.rows.is_empty(), "{site}");
                assert_eq!(after.artifact, ResidueArtifact::NoEffectPerformed, "{site}");
                assert_eq!(after.action, ResumeAction::RepeatObservation, "{site}");
            }
            AfterEffect::Referenced => {
                assert_eq!(after.rows, vec![site.row()], "{site}");
                assert_eq!(after.artifact, ResidueArtifact::Referenced, "{site}");
                assert_eq!(after.action, ResumeAction::AdoptPerformed, "{site}");
            }
            AfterEffect::Unreferenced => {
                assert_eq!(after.rows, vec![ResourceRow::R27], "{site}");
                assert_eq!(after.artifact, ResidueArtifact::Unreferenced, "{site}");
                assert_eq!(after.action, ResumeAction::AdoptPerformed, "{site}");
            }
            AfterEffect::Released => {
                assert_eq!(after.rows, vec![ResourceRow::R27], "{site}");
                assert_eq!(after.artifact, ResidueArtifact::Released, "{site}");
                assert_eq!(after.action, ResumeAction::ReclaimReleased, "{site}");
            }
            AfterEffect::Removed => {
                assert!(after.rows.is_empty(), "{site}");
                assert_eq!(after.artifact, ResidueArtifact::Removed, "{site}");
                assert_eq!(after.action, ResumeAction::AdoptPerformed, "{site}");
            }
        }

        for class in site.residue_classes() {
            let phase = EntryPhase::Residue { class: *class };
            let semantics = site.semantics(phase);
            if site.row() == ResourceRow::R27 {
                assert_eq!(semantics.rows, vec![ResourceRow::R27], "{site}");
                assert_eq!(
                    semantics.artifact,
                    ResidueArtifact::ObjectsUnreferenced,
                    "{site}"
                );
            } else {
                assert_eq!(semantics.rows, vec![ResourceRow::R27, site.row()], "{site}");
                assert_eq!(
                    semantics.artifact,
                    ResidueArtifact::ObjectsAndAdministrativeResidue,
                    "{site}"
                );
            }
            assert_eq!(semantics.action, ResumeAction::ResumeUnperformed, "{site}");
        }

        for point in site.sub_effects() {
            for mode in InjectionMode::ALL {
                let phase = EntryPhase::Point {
                    point: *point,
                    mode: *mode,
                };
                let semantics = site.semantics(phase);
                assert_eq!(
                    semantics.rows,
                    point.residue_rows(site.row()),
                    "{site}/{phase}"
                );
                assert_eq!(
                    semantics.artifact,
                    point.residue_artifact(*mode),
                    "{site}/{phase}"
                );
                assert_eq!(
                    semantics.action,
                    point.resume_action(*mode),
                    "{site}/{phase}"
                );
            }
        }
    }

    // (4) `resumes_as_before` is the authority's own answer and not a
    //     second opinion beside it: for every phase but the before phase
    //     itself, the phase is bound to the before-phase action exactly
    //     when the authority tables that action for it.
    for site in EffectSiteId::all() {
        let before = site.semantics(EntryPhase::Before).action;
        let mut phases = vec![EntryPhase::After, EntryPhase::NoExecution];
        for class in site.residue_classes() {
            phases.push(EntryPhase::Residue { class: *class });
        }
        for point in site.sub_effects() {
            for mode in InjectionMode::ALL {
                phases.push(EntryPhase::Point {
                    point: *point,
                    mode: *mode,
                });
            }
        }
        for phase in phases {
            assert_eq!(
                phase.resumes_as_before(),
                site.semantics(phase).action == before,
                "{site}/{phase}"
            );
        }
    }

    // (5) The words are distinguishable. Validation is by string equality,
    //     so two artifacts or two actions sharing a phrase would be one
    //     claim wearing two names, and a blank one would be no claim.
    let details: BTreeSet<&str> = ResidueArtifact::ALL
        .iter()
        .map(|artifact| artifact.detail())
        .collect();
    assert_eq!(details.len(), ResidueArtifact::ALL.len());
    assert!(details.iter().all(|detail| !detail.trim().is_empty()));
    let actions: BTreeSet<&str> = ResumeAction::ALL
        .iter()
        .map(|action| action.text())
        .collect();
    assert_eq!(actions.len(), ResumeAction::ALL.len());
    assert!(actions.iter().all(|action| !action.trim().is_empty()));
    // And `ALL` is every variant of each, checked by a match rather than a
    // count that a new variant would leave behind.
    for artifact in ResidueArtifact::ALL {
        match artifact {
            ResidueArtifact::Nothing
            | ResidueArtifact::TargetIntact
            | ResidueArtifact::PrecursorDurable
            | ResidueArtifact::NotReached
            | ResidueArtifact::NoEffectPerformed
            | ResidueArtifact::Referenced
            | ResidueArtifact::Unreferenced
            | ResidueArtifact::Released
            | ResidueArtifact::Removed
            | ResidueArtifact::IdNotRecorded
            | ResidueArtifact::ObjectsUnreferenced
            | ResidueArtifact::ObjectsAndAdministrativeResidue
            | ResidueArtifact::UnsyncedBytes
            | ResidueArtifact::UnsyncedLine
            | ResidueArtifact::SyncedLine
            | ResidueArtifact::LogCreated
            | ResidueArtifact::TornTailTruncated
            | ResidueArtifact::PrefixPossiblyNonDurable
            | ResidueArtifact::NoHostProcess
            | ResidueArtifact::NoProcessSpawned
            | ResidueArtifact::ReaperHeldGroup => {}
        }
    }
    assert_eq!(
        ResidueArtifact::ALL
            .iter()
            .copied()
            .collect::<BTreeSet<ResidueArtifact>>()
            .len(),
        ResidueArtifact::ALL.len()
    );
    for action in ResumeAction::ALL {
        match action {
            ResumeAction::ResumeUnperformed
            | ResumeAction::NotExecuted
            | ResumeAction::AdoptPerformed
            | ResumeAction::ReclaimReleased
            | ResumeAction::RepeatObservation
            | ResumeAction::AppendErrorProtocol
            | ResumeAction::NextOpenConverges
            | ResumeAction::RefuseResumably
            | ResumeAction::AmbientHandleTerminates
            | ResumeAction::ReaperSettlesGroup
            | ResumeAction::RefuseUnspawned => {}
        }
    }
    assert_eq!(
        ResumeAction::ALL
            .iter()
            .copied()
            .collect::<BTreeSet<ResumeAction>>()
            .len(),
        ResumeAction::ALL.len()
    );
}

#[test]
fn the_format_reads_the_detail_and_the_action_of_every_claimed_coordinate() {
    // Exhaustiveness of the *check*, not only of the table: for every
    // claimed site and every phase that site admits, a wrong detail and a
    // wrong action are each refused. A typed authority nothing consults at
    // some coordinate is the same gap in a nicer shape.
    let mut coordinates = 0;
    for site in EffectSiteId::claimed() {
        let mut phases = vec![EntryPhase::Before, EntryPhase::After];
        if site.skipped_on_fast_path() {
            phases.push(EntryPhase::NoExecution);
        }
        for class in site.residue_classes() {
            phases.push(EntryPhase::Residue { class: *class });
        }
        for point in site.sub_effects() {
            for mode in point.modes() {
                phases.push(EntryPhase::Point {
                    point: *point,
                    mode: *mode,
                });
            }
        }
        for phase in phases {
            let semantics = site.semantics(phase);
            let sound = RegistryEntry {
                site,
                phase,
                order: if phase == EntryPhase::NoExecution {
                    None
                } else {
                    only_order(site)
                },
                fault_row: site.fault_row(),
                expected_residue: ExpectedResidue {
                    rows: semantics.rows.clone(),
                    detail: semantics.artifact.detail().to_owned(),
                },
                resume_action: semantics.action.text().to_owned(),
                label: phase.required_label(),
                evidence: match phase {
                    EntryPhase::NoExecution => Evidence::NotExecuted {
                        test: "st07::oracle".to_owned(),
                        passed: true,
                        sequences: vec!["fast/seq-0".to_owned()],
                    },
                    EntryPhase::Residue { .. } => Evidence::RecoveryProven {
                        synthetic: site
                            .residue_elements()
                            .iter()
                            .map(|element| SyntheticRecord {
                                element: *element,
                                constructed: true,
                                classified: ObjectResidue::Internal,
                                recovered: true,
                            })
                            .collect(),
                        sampling: SamplingRecord {
                            n: 7,
                            histogram: ClassHistogram {
                                none: 7,
                                internal: 0,
                                after: 0,
                            },
                            unclassified: 0,
                            recovered: true,
                        },
                    },
                    EntryPhase::Before | EntryPhase::After | EntryPhase::Point { .. } => {
                        Evidence::Executed {
                            test: "st07::oracle".to_owned(),
                            passed: true,
                        }
                    }
                },
            };
            validate_entry(&sound).unwrap_or_else(|error| {
                panic!("{site}/{phase} is not a well-formed coordinate: {error}")
            });

            // The rows, at every coordinate and not only at the handful
            // the witness tests name. `structure`'s three fields are
            // checked by the same sweep or the weakest of them is checked
            // by nobody: before the per-site before-phase authority
            // existed, `rows` was the only one of the three that was read
            // at all, and it still answered one value for seventy sites.
            let mut wrong_rows = sound.clone();
            wrong_rows.expected_residue.rows = if semantics.rows.is_empty() {
                vec![ResourceRow::R27]
            } else {
                Vec::new()
            };
            assert!(
                matches!(
                    validate_entry(&wrong_rows),
                    Err(RegistryError::WrongResidueRows { .. })
                ),
                "{site}/{phase} accepted ledger rows it does not leave"
            );

            let mut wrong_detail = sound.clone();
            wrong_detail.expected_residue.detail = "durable state of some kind".to_owned();
            assert!(
                matches!(
                    validate_entry(&wrong_detail),
                    Err(RegistryError::WrongResidueDetail { .. })
                ),
                "{site}/{phase} accepted a residue description it does not have"
            );

            let mut wrong_action = sound.clone();
            wrong_action.resume_action = "resume somehow".to_owned();
            assert!(
                matches!(
                    validate_entry(&wrong_action),
                    Err(RegistryError::WrongResumeAction { .. })
                ),
                "{site}/{phase} accepted a resume action the matrix does not table"
            );

            coordinates += 1;
        }
    }
    assert!(
        coordinates > 150,
        "the sweep covered {coordinates} coordinates, which is not the inventory"
    );
}

#[test]
fn the_bijection_refuses_a_hand_edited_slice_that_keys_one_coordinate_twice() {
    // `check_bijection` is documented to revalidate a bare slice because a
    // registry.json hand-edited between a gate and a review never went
    // through `insert`. `structure` keys entries by site x phase x order,
    // so two entries at one key are two answers to one question — and
    // `check_evidence` reads whichever it reaches first. Both entries below
    // are individually valid and they disagree about the evidence, which is
    // the case a first-or-last policy decides silently.
    let host = Host::current();
    let commit_tree = EffectSiteId::Object(ObjectSite::CandidateCommitTree);
    let mut entries = self_test_registry(host);
    let position = entries
        .iter()
        .position(|entry| entry.site == commit_tree && entry.phase == EntryPhase::After)
        .expect("the fixture carries an after-phase entry");
    let mut second = entries[position].clone();
    second.evidence = Evidence::Executed {
        test: "st07::a-different-test".to_owned(),
        passed: false,
    };
    assert_eq!(second.key(), entries[position].key(), "the same key");
    assert_ne!(second, entries[position], "and a different claim");
    validate_entry(&second).expect("individually valid");
    entries.insert(position + 1, second.clone());

    let failures = check_bijection(
        &self_test_inventory(),
        &self_test_harness(host),
        &entries,
        host,
    );
    assert!(
        failures
            .iter()
            .any(|failure| matches!(failure, BijectionFailure::DuplicateEntry { count: 2, .. })),
        "a slice keying one coordinate twice passed: {failures:#?}"
    );

    // The order the two are written in must not decide the verdict: with
    // the failing entry first, the same duplicate is reported.
    let mut reversed = self_test_registry(host);
    reversed.insert(position, second);
    let failures = check_bijection(
        &self_test_inventory(),
        &self_test_harness(host),
        &reversed,
        host,
    );
    assert!(
        failures
            .iter()
            .any(|failure| matches!(failure, BijectionFailure::DuplicateEntry { count: 2, .. })),
        "the duplicate was reported only in one written order: {failures:#?}"
    );

    // The constructor refuses it too, so the two paths agree.
    let mut registry = FaultRegistry::new();
    for entry in self_test_registry(host) {
        registry.insert(entry).expect("the fixture inserts");
    }
    let held = registry
        .get(commit_tree, EntryPhase::After, only_order(commit_tree))
        .expect("the after-phase entry")
        .clone();
    assert!(matches!(
        registry.insert(held),
        Err(RegistryError::DuplicateEntry { .. })
    ));
}

#[test]
fn the_format_refuses_an_unnamed_test_and_a_duplicate_key() {
    let site = EffectSiteId::Event(EventSite::AppendFirst);
    for blank in ["", "   ", "\t\n"] {
        let mut entry = hook_entry(site, EntryPhase::Before);
        entry.evidence = Evidence::Executed {
            test: blank.to_owned(),
            passed: true,
        };
        assert!(matches!(
            FaultRegistry::new().insert(entry).expect_err("unnamed"),
            RegistryError::UnnamedTest { .. }
        ));
    }
    let mut registry = FaultRegistry::new();
    registry
        .insert(hook_entry(site, EntryPhase::Before))
        .expect("the first");
    let error = registry
        .insert(hook_entry(site, EntryPhase::Before))
        .expect_err("the second");
    assert!(
        matches!(error, RegistryError::DuplicateEntry { .. }),
        "{error}"
    );
    assert_eq!(registry.len(), 1, "a refused entry is not stored");

    // A different phase of the same site, and the same phase of a
    // different site, are different keys.
    registry
        .insert(hook_entry(site, EntryPhase::After))
        .expect("a different phase");
    registry
        .insert(hook_entry(
            EffectSiteId::Event(EventSite::Append),
            EntryPhase::Before,
        ))
        .expect("a different site");
    assert_eq!(registry.len(), 3);
    assert!(registry.get(site, EntryPhase::Before, None).is_some());
    assert!(registry.get(site, EntryPhase::NoExecution, None).is_none());
    assert!(!registry.is_empty());
}

// -----------------------------------------------------------------------
// The bijection's failure directions
// -----------------------------------------------------------------------

/// A mutilation of the passing self-test state, and the failure it must
/// produce.
struct Direction {
    name: &'static str,
    break_it: fn(&mut HookHarness, &mut Vec<RegistryEntry>),
    expect: fn(&BijectionFailure) -> bool,
}

#[test]
fn the_bijection_fails_on_every_missing_link() {
    // Each direction asserted positively — the checker must *reject*, and
    // reject with the failure that names what went wrong. A test that only
    // showed the checker accepting valid input would pass for a checker
    // that accepted everything.
    let host = Host::current();
    let commit_tree = EffectSiteId::Object(ObjectSite::CandidateCommitTree);
    let append = EffectSiteId::Event(EventSite::AppendFirst);

    let directions: &[Direction] = &[
        Direction {
            name: "an unobserved before-phase",
            break_it: |harness, _| {
                *harness = HookHarness::new();
                for site in self_test_inventory() {
                    if site.skipped_on_fast_path() || !site.scope().is_claimed() {
                        continue;
                    }
                    if site == EffectSiteId::Event(EventSite::AppendFirst) {
                        // Drive everything but the before phase.
                        for point in site.sub_effects() {
                            for mode in point.modes() {
                                harness.hook(
                                    site,
                                    HookPhase::Point {
                                        point: *point,
                                        mode: *mode,
                                    },
                                );
                            }
                        }
                        harness.hook(site, HookPhase::After);
                    } else {
                        drive(harness, site, Host::current());
                    }
                }
            },
            expect: |failure| {
                matches!(
                    failure,
                    BijectionFailure::Unobserved {
                        phase: HookPhase::Before,
                        ..
                    }
                )
            },
        },
        Direction {
            name: "an unobserved injection mode",
            break_it: |harness, _| {
                *harness = HookHarness::new();
                for site in self_test_inventory() {
                    if site.skipped_on_fast_path() || !site.scope().is_claimed() {
                        continue;
                    }
                    harness.hook(site, HookPhase::Before);
                    for point in site.sub_effects() {
                        if !point.platform().required_on(Host::current()) {
                            continue;
                        }
                        for mode in point.modes() {
                            // Every point in every mode but one: the
                            // error-return half of a sync.
                            if *point == SubEffectPoint::Synced
                                && *mode == InjectionMode::ErrorReturn
                            {
                                continue;
                            }
                            harness.hook(
                                site,
                                HookPhase::Point {
                                    point: *point,
                                    mode: *mode,
                                },
                            );
                        }
                    }
                    harness.hook(site, HookPhase::After);
                }
            },
            expect: |failure| {
                matches!(
                    failure,
                    BijectionFailure::Unobserved {
                        phase: HookPhase::Point {
                            point: SubEffectPoint::Synced,
                            mode: InjectionMode::ErrorReturn,
                        },
                        ..
                    }
                )
            },
        },
        Direction {
            name: "a missing entry",
            break_it: |_, entries| {
                entries.retain(|entry| {
                    entry.key()
                        != (
                            EffectSiteId::Event(EventSite::AppendFirst),
                            EntryPhase::After,
                            None,
                        )
                });
            },
            expect: |failure| matches!(failure, BijectionFailure::MissingEntry { .. }),
        },
        Direction {
            name: "evidence that did not pass",
            break_it: |_, entries| {
                for entry in entries.iter_mut() {
                    if let Evidence::Executed { passed, .. } = &mut entry.evidence {
                        *passed = false;
                        break;
                    }
                }
            },
            expect: |failure| matches!(failure, BijectionFailure::MissingEvidence { .. }),
        },
        Direction {
            name: "a residue element that was never constructed",
            break_it: |_, entries| {
                for entry in entries.iter_mut() {
                    if let Evidence::RecoveryProven { synthetic, .. } = &mut entry.evidence {
                        synthetic[0].constructed = false;
                        break;
                    }
                }
            },
            // The three predicates over a synthetic record are three
            // failures and not one, so each direction below is pinned by
            // the failure that names it. Under the one shared
            // `MissingEvidence` these three rows expected the same value
            // and a checker that read `constructed` three times would have
            // satisfied all of them.
            expect: |failure| {
                matches!(
                    failure,
                    BijectionFailure::ResidueElementNotConstructed { .. }
                )
            },
        },
        Direction {
            name: "a residue element that did not recover",
            break_it: |_, entries| {
                for entry in entries.iter_mut() {
                    if let Evidence::RecoveryProven { synthetic, .. } = &mut entry.evidence {
                        synthetic[0].recovered = false;
                        break;
                    }
                }
            },
            expect: |failure| {
                matches!(failure, BijectionFailure::ResidueElementNotRecovered { .. })
            },
        },
        Direction {
            name: "a residue element that classified as something else",
            break_it: |_, entries| {
                for entry in entries.iter_mut() {
                    if let Evidence::RecoveryProven { synthetic, .. } = &mut entry.evidence {
                        synthetic[0].classified = ObjectResidue::After;
                        break;
                    }
                }
            },
            expect: |failure| {
                matches!(
                    failure,
                    BijectionFailure::ResidueElementMisclassified {
                        classified: ObjectResidue::After,
                        expected: ObjectResidue::Internal,
                        ..
                    }
                )
            },
        },
        Direction {
            name: "an unclassifiable sampled residue",
            break_it: |_, entries| {
                for entry in entries.iter_mut() {
                    if let Evidence::RecoveryProven { sampling, .. } = &mut entry.evidence {
                        // Kept summing to N, so this is the unclassifiable
                        // failure and not the accounting one.
                        sampling.histogram.after -= 2;
                        sampling.unclassified = 2;
                        break;
                    }
                }
            },
            expect: |failure| matches!(failure, BijectionFailure::UnclassifiableResidue { .. }),
        },
        Direction {
            name: "a sampling record with no samples",
            break_it: |_, entries| {
                for entry in entries.iter_mut() {
                    if let Evidence::RecoveryProven { sampling, .. } = &mut entry.evidence {
                        sampling.n = 0;
                        sampling.histogram = ClassHistogram::default();
                        break;
                    }
                }
            },
            expect: |failure| matches!(failure, BijectionFailure::MissingSampling { .. }),
        },
        Direction {
            name: "a histogram that does not account for the samples",
            break_it: |_, entries| {
                for entry in entries.iter_mut() {
                    if let Evidence::RecoveryProven { sampling, .. } = &mut entry.evidence {
                        sampling.histogram.none += 1;
                        break;
                    }
                }
            },
            expect: |failure| matches!(failure, BijectionFailure::SamplingUnaccounted { .. }),
        },
        Direction {
            // The frontier reviewer's reproduction at `ffe26ca`. The check
            // summed with `saturating_add`, and a saturating sum agrees with
            // an `n` of `u32::MAX` whatever the histogram holds, so this
            // record — one sample more than `n` accounts for — produced no
            // `SamplingUnaccounted` and the document passed. `validate_entry`
            // does not read these counts, so the bijection is the only check
            // that can.
            name: "a histogram whose saturating total equals n",
            break_it: |_, entries| {
                for entry in entries.iter_mut() {
                    if let Evidence::RecoveryProven { sampling, .. } = &mut entry.evidence {
                        sampling.n = u32::MAX;
                        sampling.histogram = ClassHistogram {
                            none: u32::MAX,
                            internal: 1,
                            after: 0,
                        };
                        sampling.unclassified = 0;
                        sampling.recovered = true;
                        break;
                    }
                }
            },
            expect: |failure| {
                matches!(
                    failure,
                    BijectionFailure::SamplingUnaccounted {
                        n: u32::MAX,
                        counted,
                        ..
                    } if *counted == u64::from(u32::MAX) + 1
                )
            },
        },
        Direction {
            name: "a sampled residue that did not recover",
            break_it: |_, entries| {
                for entry in entries.iter_mut() {
                    if let Evidence::RecoveryProven { sampling, .. } = &mut entry.evidence {
                        sampling.recovered = false;
                        break;
                    }
                }
            },
            expect: |failure| matches!(failure, BijectionFailure::UnrecoveredSampling { .. }),
        },
        Direction {
            // A no-execution record is additional evidence, not a
            // substitute for the ordinary bijection. Drop one of the
            // skipped sites' ordinary entries and the check has to notice,
            // or "it did not run on the fast path" is a way to be excused
            // from coverage entirely.
            name: "a no-execution site missing its ordinary after entry",
            break_it: |_, entries| {
                let cherry = EffectSiteId::Object(ObjectSite::ProposalCherryPick);
                entries.retain(|entry| !(entry.site == cherry && entry.phase == EntryPhase::After));
            },
            expect: |failure| {
                matches!(
                    failure,
                    BijectionFailure::MissingEntry { site, phase, .. }
                        if *site == EffectSiteId::Object(ObjectSite::ProposalCherryPick)
                            && *phase == EntryPhase::After
                )
            },
        },
        Direction {
            // The same claim from the harness side: the record says
            // nothing about what happens off the fast path, so an
            // unobserved hook there is still an unobserved hook.
            name: "a no-execution site whose hooks were never observed",
            break_it: |harness, _| {
                let mut replacement = HookHarness::new();
                for sequence in FAST_SEQUENCES {
                    replacement.begin_fast_sequence(sequence);
                    replacement.end_fast_sequence();
                }
                *harness = replacement;
            },
            expect: |failure| {
                matches!(
                    failure,
                    BijectionFailure::Unobserved { site, .. }
                        if *site == EffectSiteId::Ref(RefSite::PinPrepared)
                )
            },
        },
        Direction {
            name: "an entry for a site outside the inventory",
            break_it: |_, entries| {
                entries.push(hook_entry(
                    EffectSiteId::Lock(LockSite::AcquireRun),
                    EntryPhase::Before,
                ));
            },
            expect: |failure| matches!(failure, BijectionFailure::EntryOutsideInventory { .. }),
        },
        Direction {
            name: "an entry the format would have refused",
            break_it: |_, entries| {
                entries[0].fault_row = FaultRow::TResume;
            },
            expect: |failure| matches!(failure, BijectionFailure::InvalidEntry { .. }),
        },
    ];

    for direction in directions {
        let mut harness = self_test_harness(host);
        let mut entries = self_test_registry(host);
        (direction.break_it)(&mut harness, &mut entries);
        let failures = check_bijection(&self_test_inventory(), &harness, &entries, host);
        assert!(
            failures.iter().any(direction.expect),
            "`{}` did not produce its failure: {failures:#?}",
            direction.name
        );
    }
    assert_eq!(directions.len(), 16, "every direction above is exercised");

    // The unbroken state passes, so each direction above is the *only*
    // difference between passing and failing.
    assert!(
        check_bijection(
            &self_test_inventory(),
            &self_test_harness(host),
            &self_test_registry(host),
            host
        )
        .is_empty()
    );
    let _ = (commit_tree, append);
}

#[test]
fn a_fast_sequence_the_harness_observed_nothing_in_is_not_an_exercised_one() {
    // Pass 5 on `da3204f`, finding 1 (P2). `begin_fast_sequence` records a
    // sequence the moment it is called, so a suite that begins and ends every
    // name with no funnel in between had "fast sequences", and a record
    // naming each of them held within all of them vacuously. With ordinary
    // coverage satisfied outside the sequences — which is what keeps
    // `Unobserved` from masking the hole, as it does in the Direction that
    // builds empty sequences — `check_bijection` answered an empty vector for
    // a run in which no exact-base integration happened. The same inference
    // pass 4 removed from the `observed` field, one level up: exercised was
    // being inferred from the map being non-empty.
    let host = Host::current();
    let inventory = self_test_inventory();
    let entries = self_test_registry(host);

    let mut hollow = HookHarness::new();
    for sequence in FAST_SEQUENCES {
        hollow.begin_fast_sequence(sequence);
        hollow.end_fast_sequence();
    }
    for site in &inventory {
        if !site.scope().is_claimed() {
            continue;
        }
        drive(&mut hollow, *site, host);
    }
    assert_eq!(hollow.fast_sequences().len(), FAST_SEQUENCES.len());
    assert!(
        hollow
            .fast_sequences()
            .iter()
            .all(|sequence| sequence.touched().is_empty()),
        "the fixture's sequences must be empty for this test to mean anything"
    );

    let failures = check_bijection(&inventory, &hollow, &entries, host);
    let mut empty: Vec<&str> = failures
        .iter()
        .filter_map(|failure| match failure {
            BijectionFailure::EmptyFastSequence { sequence } => Some(sequence.as_str()),
            _ => None,
        })
        .collect();
    empty.sort_unstable();
    let mut expected: Vec<&str> = FAST_SEQUENCES.to_vec();
    expected.sort_unstable();
    assert_eq!(
        empty, expected,
        "every empty sequence is reported once, by name: {failures:#?}"
    );
    // And nothing else: ordinary coverage, every record and every name are
    // satisfied, so the empty sequences are the only thing wrong with this
    // run — which is exactly why the check used to answer empty.
    assert_eq!(
        failures.len(),
        FAST_SEQUENCES.len(),
        "the hollow run reported something besides its empty sequences: {failures:#?}"
    );
}

#[test]
fn an_empty_inventory_still_checks_fast_traces_without_claiming_they_ended() {
    let host = Host::current();
    let mut harness = HookHarness::new();
    assert!(check_bijection(&[], &harness, &[], host).is_empty());

    // The sequence is still open. EmptyFastSequence says what was observed,
    // and must not claim that end_fast_sequence has been called.
    harness.begin_fast_sequence("fast/open");
    let failure = BijectionFailure::EmptyFastSequence {
        sequence: "fast/open".to_owned(),
    };
    let failures = check_bijection(&[], &harness, &[], host);
    assert_eq!(failures, [failure]);
    assert_eq!(
        failures
            .first()
            .expect("the open empty trace must report its missing observation")
            .to_string(),
        "the fast sequence `fast/open` has no hook observed inside it; a \
         trace the harness saw nothing in is not an exercised fast integration"
    );

    // A hook observation satisfies the trace check even before the sequence
    // closes. With no inventoried sites there is no ordinary coverage to add.
    harness.hook(
        EffectSiteId::Event(EventSite::AppendFirst),
        HookPhase::Before,
    );
    assert!(check_bijection(&[], &harness, &[], host).is_empty());
    harness.end_fast_sequence();
    assert!(check_bijection(&[], &harness, &[], host).is_empty());

    harness.begin_fast_sequence("fast/closed");
    harness.end_fast_sequence();
    assert_eq!(
        check_bijection(&[], &harness, &[], host),
        [BijectionFailure::EmptyFastSequence {
            sequence: "fast/closed".to_owned(),
        }]
    );
}

#[test]
fn every_failing_residue_element_is_reported_with_its_own_element_and_predicate() {
    // Pass 2 on `2421651`, finding 2: the three element Directions above each
    // break `synthetic[0]` alone and match the variant alone, so a checker
    // that reported the first bad element and stopped — the `break`
    // `SWEEP-BIJECTION-003` removed — or one that reported a constant element,
    // would pass all three. This is the witness those Directions are not:
    // several distinct elements failing at once, in different predicates,
    // one of them in two at once, and the exact set of `(variant, element)`
    // pairs asserted — every one present, nothing else of the family.
    let host = Host::current();
    let cherry = EffectSiteId::Object(ObjectSite::ProposalCherryPick);
    let elements = cherry.residue_elements();
    assert_eq!(elements.len(), 7, "the fixture site lists seven elements");
    let residue = EntryPhase::Residue {
        class: ResidueClass::ObjectInternal,
    };

    let mut entries = self_test_registry(host);
    let entry = entries
        .iter_mut()
        .find(|entry| entry.site == cherry && entry.phase == residue)
        .expect("the self-test registry carries the cherry-pick residue entry");
    let Evidence::RecoveryProven { synthetic, .. } = &mut entry.evidence else {
        panic!("a residue entry carries recovery-proven evidence");
    };
    // Four elements broken, in the middle and at the end of the list rather
    // than at index 0, so a checker that only ever reads the first record
    // has nothing to report.
    let mut broken = 0;
    for record in synthetic.iter_mut() {
        match record.element {
            ResidueElement::IndexLock => {
                record.constructed = false;
                broken += 1;
            }
            ResidueElement::MergeHead => {
                record.recovered = false;
                broken += 1;
            }
            ResidueElement::SequencerState => {
                record.classified = ObjectResidue::After;
                broken += 1;
            }
            // Two predicates on one element: both are reported, separately.
            ResidueElement::CherryPickHead => {
                record.constructed = false;
                record.recovered = false;
                broken += 1;
            }
            _ => {}
        }
    }
    assert_eq!(
        broken, 4,
        "the fixture carries the four elements this test breaks"
    );

    let failures = check_bijection(
        &self_test_inventory(),
        &self_test_harness(host),
        &entries,
        host,
    );
    let mut reported: Vec<(&str, ResidueElement)> = failures
        .iter()
        .filter_map(|failure| match failure {
            BijectionFailure::ResidueElementNotConstructed { site, element, .. }
                if *site == cherry =>
            {
                Some(("not-constructed", *element))
            }
            BijectionFailure::ResidueElementNotRecovered { site, element, .. }
                if *site == cherry =>
            {
                Some(("not-recovered", *element))
            }
            BijectionFailure::ResidueElementMisclassified {
                site,
                element,
                classified,
                expected,
                ..
            } if *site == cherry => {
                assert_eq!(*classified, ObjectResidue::After, "{failures:#?}");
                assert_eq!(*expected, ObjectResidue::Internal, "{failures:#?}");
                Some(("misclassified", *element))
            }
            _ => None,
        })
        .collect();
    reported.sort();
    let mut expected = vec![
        ("not-constructed", ResidueElement::IndexLock),
        ("not-recovered", ResidueElement::MergeHead),
        ("misclassified", ResidueElement::SequencerState),
        ("not-constructed", ResidueElement::CherryPickHead),
        ("not-recovered", ResidueElement::CherryPickHead),
    ];
    expected.sort();
    assert_eq!(
        reported, expected,
        "the report did not name every failing element with its own predicate: {failures:#?}"
    );
    // And nothing else of the family: the three healthy elements and the
    // sampling record produce no residue-element failure.
    assert!(
        !failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::MissingEvidence { site, .. } if *site == cherry
        )),
        "{failures:#?}"
    );
}

#[test]
fn a_never_hit_internal_class_passes_and_an_unclassifiable_one_does_not() {
    // Both directions of `completeness_rule`'s one explicit exemption:
    // "an unclassifiable residue fails; a never-hit Internal class does
    // not fail".
    let host = Host::current();
    let site = EffectSiteId::Object(ObjectSite::CandidateStage);
    let inventory = vec![site];
    let mut harness = HookHarness::new();
    drive(&mut harness, site, host);

    let entries = |n: u32, internal: u32, unclassified: u32| -> Vec<RegistryEntry> {
        let mut registry = FaultRegistry::new();
        for phase in [EntryPhase::Before, EntryPhase::After] {
            registry.insert(hook_entry(site, phase)).expect("hook");
        }
        let mut residue = residue_entry(site, n, internal);
        if unclassified > 0 {
            if let Evidence::RecoveryProven { sampling, .. } = &mut residue.evidence {
                sampling.histogram.after -= unclassified;
                sampling.unclassified = unclassified;
            }
        }
        registry.insert(residue).expect("residue");
        registry.entries().to_vec()
    };

    // Never hit: the histogram's internal count is zero and the check
    // passes. Hitting the internal window is recorded, never required.
    let never_hit = entries(40, 0, 0);
    assert!(never_hit.iter().any(|entry| matches!(
        &entry.evidence,
        Evidence::RecoveryProven { sampling, .. } if sampling.histogram.internal == 0
    )));
    assert!(
        check_bijection(&inventory, &harness, &never_hit, host).is_empty(),
        "a never-hit internal class must not fail"
    );
    // Hit: also passes.
    assert!(check_bijection(&inventory, &harness, &entries(40, 9, 0), host).is_empty());
    // Unclassifiable: fails, and fails by name.
    let failures = check_bijection(&inventory, &harness, &entries(40, 9, 3), host);
    assert!(
        failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::UnclassifiableResidue { count, .. } if *count == 3
        )),
        "{failures:#?}"
    );
}

#[test]
fn a_legacy_site_carries_no_bijection_requirement_and_a_claimed_one_does() {
    let host = Host::current();
    let legacy = EffectSiteId::Event(EventSite::LegacyAppend);
    let shared = EffectSiteId::Event(EventSite::Append);
    let harness = HookHarness::new();

    // Nothing observed, nothing entered, and the Legacy site is silent.
    assert!(check_bijection(&[legacy], &harness, &[], host).is_empty());
    // The same emptiness for its Shared neighbour is a pile of failures.
    let failures = check_bijection(&[shared], &harness, &[], host);
    assert!(!failures.is_empty(), "a Shared site must carry the claim");
    assert!(
        failures
            .iter()
            .any(|f| matches!(f, BijectionFailure::Unobserved { .. }))
    );
    assert!(
        failures
            .iter()
            .any(|f| matches!(f, BijectionFailure::MissingEntry { .. }))
    );
    // The exemption is by scope, not by group: the two sites differ in
    // nothing else that the checker reads.
    assert_eq!(legacy.group(), shared.group());
    assert_eq!(legacy.row(), shared.row());
    assert_eq!(legacy.fault_row(), shared.fault_row());
    assert_ne!(legacy.scope(), shared.scope());
}

#[test]
fn a_point_is_required_on_its_own_platform_and_not_on_the_other() {
    // ST-07's evidence "executes each point on its platform", both ways: a
    // Unix suite is not asked for the Windows containment steps, and a
    // Windows suite is not asked for the Unix ones — but each is asked for
    // its own.
    let spawn = EffectSiteId::Process(ProcessSite::Spawn);
    for host in Host::ALL.iter().copied() {
        let mut harness = HookHarness::new();
        drive(&mut harness, spawn, host);
        let mut registry = FaultRegistry::new();
        for phase in [EntryPhase::Before, EntryPhase::After] {
            registry.insert(hook_entry(spawn, phase)).expect("hook");
        }
        for point in spawn.sub_effects() {
            if !point.platform().required_on(host) {
                continue;
            }
            for mode in point.modes() {
                registry
                    .insert(hook_entry(
                        spawn,
                        EntryPhase::Point {
                            point: *point,
                            mode: *mode,
                        },
                    ))
                    .expect("point");
            }
        }
        let entries = registry.entries().to_vec();
        assert!(
            check_bijection(&[spawn], &harness, &entries, host).is_empty(),
            "{host}"
        );
        // The other platform's check over the same evidence fails, which is
        // what makes the scoping a scoping rather than a hole.
        let other = host.other();
        let failures = check_bijection(&[spawn], &harness, &entries, other);
        assert!(
            failures
                .iter()
                .any(|f| matches!(f, BijectionFailure::Unobserved { .. })),
            "{host} evidence must not satisfy {other}: {failures:#?}"
        );
    }
    // Four Windows points and four Unix ones, and `Any` points are
    // required on both.
    let windows = spawn
        .sub_effects()
        .iter()
        .filter(|point| point.platform() == Platform::Windows)
        .count();
    let unix = spawn
        .sub_effects()
        .iter()
        .filter(|point| point.platform() == Platform::Unix)
        .count();
    assert_eq!((windows, unix), (4, 4));
    for host in Host::ALL.iter().copied() {
        assert!(SubEffectPoint::Written.platform().required_on(host));
    }
}

#[test]
fn there_is_no_host_on_which_a_containment_point_is_unrequired() {
    // PR3-ST07-013. `required_on` used to take a `Platform` as its host, and
    // its last arm was `(Self::Windows, _) | (Self::Unix, _) => false` — so
    // `Platform::Any`, which means "a point that exists everywhere", read as
    // "a host that is neither platform". `check_bijection(&[spawn], &empty
    // harness, &two entries, Platform::Any)` then returned success with all
    // eight containment points unobserved, unentered and unentried: the
    // strongest claim ST-07 makes about the process funnel, erased by
    // passing an enum variant that is not a machine.
    //
    // The repair is the type: `Host` has two values and no third to pass.
    // This test is the property that fix buys, stated over the whole
    // product so that it cannot be true of one host and vacuous on the
    // other, and so that a later `Host` variant has to break it.
    assert_eq!(Host::ALL.len(), 2, "a host is Windows or it is Unix");
    assert_eq!(
        Host::current(),
        if cfg!(windows) {
            Host::Windows
        } else {
            Host::Unix
        },
        "the default host is the one this build actually runs on"
    );
    assert_eq!(Host::current().other(), Host::current().other());
    assert_ne!(Host::current().other(), Host::current());
    assert_eq!(
        Host::current().platform(),
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Unix
        }
    );
    let spawn = EffectSiteId::Process(ProcessSite::Spawn);

    // (1) Over every host, every point of every site is required on at
    //     least one of them, and every containment point on exactly one.
    for point in SubEffectPoint::ALL {
        let required: Vec<Host> = Host::ALL
            .iter()
            .copied()
            .filter(|host| point.platform().required_on(*host))
            .collect();
        assert!(
            !required.is_empty(),
            "{point} is required on no host at all, so no suite has to execute it"
        );
        match point.platform() {
            Platform::Any => assert_eq!(required.len(), 2, "{point}"),
            Platform::Windows => assert_eq!(required, vec![Host::Windows], "{point}"),
            Platform::Unix => assert_eq!(required, vec![Host::Unix], "{point}"),
        }
    }

    // (2) The failing call, now for both hosts: an empty harness and a
    //     registry carrying only the two hook phases is refused on every
    //     host, and refused *for the containment points* rather than only
    //     for the hooks. Under the old wildcard the `Platform::Any` call
    //     returned an empty vector.
    for host in Host::ALL.iter().copied() {
        let mut harness = HookHarness::new();
        harness.hook(spawn, HookPhase::Before);
        harness.hook(spawn, HookPhase::After);
        let entries = vec![
            hook_entry(spawn, EntryPhase::Before),
            hook_entry(spawn, EntryPhase::After),
        ];
        let failures = check_bijection(&[spawn], &harness, &entries, host);
        let unobserved: Vec<&HookPhase> = failures
            .iter()
            .filter_map(|failure| match failure {
                BijectionFailure::Unobserved { phase, .. } => Some(phase),
                _ => None,
            })
            .collect();
        assert_eq!(
            unobserved.len(),
            match host {
                // AmbientJobJoined supports both modes; the other three
                // Windows points and all four Unix points are kill-only.
                Host::Windows => 5,
                Host::Unix => 4,
            },
            "{host}: {failures:#?}"
        );
        for point in spawn.sub_effects() {
            if !point.platform().required_on(host) {
                continue;
            }
            assert!(
                unobserved.iter().any(|phase| matches!(
                    phase,
                    HookPhase::Point { point: seen, .. } if seen == point
                )),
                "{host} accepted a check in which {point} never executed"
            );
        }
    }
}

#[test]
fn the_bijection_over_the_whole_claimed_inventory_fails_for_this_slice() {
    // Non-vacuity. The check is only as strong as the inventory it is
    // handed, so this slice states plainly what it has *not* covered: run
    // the same check over every Topology and Shared site and it fails,
    // because PR3 builds the frame and PR7-PR10 fill it.
    let host = Host::current();
    let claimed = EffectSiteId::claimed();
    assert!(claimed.len() >= 60, "{}", claimed.len());
    let failures = check_bijection(
        &claimed,
        &self_test_harness(host),
        &self_test_registry(host),
        host,
    );
    assert!(
        failures.len() > 100,
        "a framework whose full inventory nearly passes in PR3 is a framework \
             that is not checking anything: {}",
        failures.len()
    );
    // And it fails for the right reason: sites no funnel exists for yet.
    assert!(
        failures.iter().any(|failure| matches!(
            failure,
            BijectionFailure::Unobserved { site, .. }
                if *site == EffectSiteId::RunDir(RunDirSite::PublishCommitRecord)
        )),
        "{failures:#?}"
    );
}

// -----------------------------------------------------------------------
// effect_sites.json and the wire forms
// -----------------------------------------------------------------------

#[test]
fn the_generated_inventory_describes_every_site_and_invents_none() {
    let export = effect_sites();
    let sites = EffectSiteId::all();
    assert_eq!(export.len(), sites.len());
    assert_eq!(export.len(), 70, "the inventory this slice ships");
    for (entry, site) in export.iter().zip(&sites) {
        // Generated *from* the enums, so every field is the enum's answer
        // and not a second copy that could disagree with it.
        assert_eq!(entry.site, *site);
        assert_eq!(entry.group, site.group());
        assert_eq!(entry.row, site.row());
        assert_eq!(entry.domain, site.row().domain());
        assert_eq!(entry.adjacent, site.adjacent());
        assert_eq!(entry.observable_orders, site.observable_orders());
        assert_eq!(entry.fault_row, site.fault_row());
        assert_eq!(entry.scope, site.scope());
        assert_eq!(entry.module, site.module());
        assert_eq!(entry.read_only, site.is_read_only());
        assert_eq!(entry.sub_effect_points.len(), site.sub_effects().len());
        for (point, expected) in entry.sub_effect_points.iter().zip(site.sub_effects()) {
            assert_eq!(point.point, *expected);
            assert_eq!(point.platform, expected.platform());
            assert_eq!(point.modes, expected.modes());
        }
        assert_eq!(entry.residue_classes.len(), site.residue_classes().len());
        for class in &entry.residue_classes {
            assert_eq!(class.label, EvidenceLabel::RecoveryProven);
            assert_eq!(class.classified_as, ObjectResidue::Internal);
            assert_eq!(class.elements, site.residue_elements());
        }
    }

    // The document itself: a real JSON array of objects that names the
    // sites by their dotted names and round-trips.
    let json = effect_sites_json().expect("the inventory serializes");
    assert!(
        json.contains(r#""site": "RunDir.PublishCommitRecord""#),
        "{json:.400}"
    );
    assert!(json.contains(r#""row": "r21""#));
    assert!(json.contains(r#""point": "sync_prefix""#));
    assert!(json.contains(r#""class": "object_internal""#));
    assert!(json.contains(r#""label": "recovery_proven""#));
    let back: Vec<EffectSiteExport> =
        serde_json::from_str(&json).expect("the inventory round-trips");
    assert_eq!(back, export);

    // Every group, every row, both claimed scopes and the legacy one, and
    // both adjacency directions appear, so the document is a description
    // of the whole inventory rather than of one corner of it.
    let groups: BTreeSet<FunnelGroup> = export.iter().map(|entry| entry.group).collect();
    assert_eq!(groups.len(), 11);
    let rows: BTreeSet<ResourceRow> = export.iter().map(|entry| entry.row).collect();
    assert_eq!(rows.len(), 15);
    let scopes: BTreeSet<SiteScope> = export.iter().map(|entry| entry.scope).collect();
    assert_eq!(scopes.len(), 3);
    let modules: BTreeSet<&str> = export.iter().map(|entry| entry.module.as_str()).collect();
    assert_eq!(modules.len(), 7, "{modules:?}");
}

/// Every JSON pointer in `value` that addresses an object.
fn object_pointers(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            out.push(prefix.to_owned());
            for (key, child) in map {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                object_pointers(child, &format!("{prefix}/{escaped}"), out);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                object_pointers(child, &format!("{prefix}/{index}"), out);
            }
        }
        _ => {}
    }
}

#[test]
fn every_reachable_object_of_both_wire_forms_refuses_an_unknown_field() {
    // Strictness applied recursively and *proved* recursively: rather than
    // naming the types that carry `deny_unknown_fields`, this walks the
    // serialized documents and injects a key at every object node there
    // is. A type reachable only through a payload — the shape A1's review
    // found unprotected — is a node here like any other.
    let mut checked = 0;

    let inventory = serde_json::to_value(effect_sites()).expect("serialize");
    let mut pointers = Vec::new();
    object_pointers(&inventory, "", &mut pointers);
    assert!(pointers.len() > 100, "{}", pointers.len());
    for pointer in &pointers {
        let mut document = inventory.clone();
        document
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_object_mut)
            .expect("an object pointer addresses an object")
            .insert("upstroke_unknown_probe".to_owned(), serde_json::json!(1));
        assert!(
            serde_json::from_value::<Vec<EffectSiteExport>>(document).is_err(),
            "effect_sites.json accepted an unknown field at `{pointer}`"
        );
        checked += 1;
    }

    // The registry's own document, built to contain every variant that has
    // an object form: all five phases, all three evidence shapes, both
    // orders, and a residue entry with its synthetic and sampling records.
    let mut entries = self_test_registry(Host::current());
    entries.push(hook_entry(
        EffectSiteId::Object(ObjectSite::CandidateCommitTree),
        EntryPhase::Point {
            point: SubEffectPoint::IdUnread,
            mode: InjectionMode::Kill,
        },
    ));
    let shapes: BTreeSet<String> = entries
        .iter()
        .map(|entry| format!("{}", entry.phase))
        .collect();
    assert!(shapes.len() >= 5, "{shapes:?}");
    let registry = serde_json::to_value(&entries).expect("serialize");
    let mut pointers = Vec::new();
    object_pointers(&registry, "", &mut pointers);
    assert!(pointers.len() > 60, "{}", pointers.len());
    for pointer in &pointers {
        let mut document = registry.clone();
        document
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_object_mut)
            .expect("an object pointer addresses an object")
            .insert("upstroke_unknown_probe".to_owned(), serde_json::json!(1));
        assert!(
            serde_json::from_value::<Vec<RegistryEntry>>(document).is_err(),
            "registry.json accepted an unknown field at `{pointer}`"
        );
        checked += 1;
    }

    // The coverage record the harness produces is the third document a
    // gate attaches, and it is walked too.
    let mut harness = HookHarness::new();
    for site in self_test_inventory() {
        if site.skipped_on_fast_path() || !site.scope().is_claimed() {
            continue;
        }
        drive(&mut harness, site, Host::current());
    }
    let coverage = serde_json::to_value(harness.coverage()).expect("serialize");
    let mut pointers = Vec::new();
    object_pointers(&coverage, "", &mut pointers);
    assert!(pointers.len() > 20, "{}", pointers.len());
    for pointer in &pointers {
        let mut document = coverage.clone();
        document
            .pointer_mut(pointer)
            .and_then(serde_json::Value::as_object_mut)
            .expect("an object pointer addresses an object")
            .insert("upstroke_unknown_probe".to_owned(), serde_json::json!(1));
        assert!(
            serde_json::from_value::<Vec<Observation>>(document).is_err(),
            "the coverage record accepted an unknown field at `{pointer}`"
        );
        checked += 1;
    }

    assert!(checked > 200, "only {checked} object paths were probed");
}

#[test]
fn the_wire_form_refuses_an_entry_naming_a_site_the_enums_do_not_declare() {
    // `completeness_rule`: "entries for sites absent from the enums are
    // refused". In Rust the type says so; on the wire, this does.
    let entries = vec![hook_entry(
        EffectSiteId::Event(EventSite::AppendFirst),
        EntryPhase::Before,
    )];
    let json = serde_json::to_string(&entries).expect("serialize");
    assert!(json.contains(r#""site":"Event.AppendFirst""#), "{json}");
    let back: Vec<RegistryEntry> = serde_json::from_str(&json).expect("round-trips");
    assert_eq!(back, entries);

    for invented in [
        "Event.AppendSecond",
        "Ledger.Append",
        "Event.appendfirst",
        "Event.AppendFirst.Written",
    ] {
        let forged = json.replace("Event.AppendFirst", invented);
        assert_ne!(forged, json);
        let error = serde_json::from_str::<Vec<RegistryEntry>>(&forged)
            .expect_err("a site no enum declares");
        assert!(error.to_string().contains(invented), "{error}");
    }
    // The same for the generated inventory.
    let inventory = effect_sites_json().expect("serialize");
    let forged = inventory.replace("Lock.ObserveCleanupHold", "Lock.ObserveCleanupLease");
    assert!(serde_json::from_str::<Vec<EffectSiteExport>>(&forged).is_err());
}

#[test]
fn the_coverage_record_round_trips_and_names_its_phases() {
    let host = Host::current();
    let harness = self_test_harness(host);
    let json = serde_json::to_string(harness.coverage()).expect("serialize");
    let back: Vec<Observation> = serde_json::from_str(&json).expect("round-trips");
    assert_eq!(back, harness.coverage());
    assert!(json.contains(r#""phase":"before""#), "{json:.300}");
    assert!(json.contains(r#""phase":"after""#));
    assert!(
        json.contains(r#""point":{"point":"sync_prefix","mode":"error_return"}"#),
        "{json}"
    );
    // Nothing in the record can name a residue class: the type has no
    // variant for one, which is the first half of "a residue class is
    // never counted as an executed hook".
    assert!(!json.contains("residue"), "{json}");
    assert!(!json.contains("object_internal"), "{json}");
}
