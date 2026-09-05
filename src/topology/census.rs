//! Extended notes: `docs/internals/topology/census.md`

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::topology::events::{DerivedOutcome, TopologyEvent};
use crate::topology::fold::TopologyFold;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CensusBounds {
    pub originals: u32,
    pub repairs: u32,
    pub generations_per_task: u32,
    pub attempts_per_generation: u32,
    pub sequences: u32,
    pub defers: u32,
    pub questions: u32,
    pub resumes: u32,
    pub max_trace: usize,
    pub max_states: usize,
}

impl CensusBounds {
    pub const fn dimensions(&self) -> [(&'static str, u32); 8] {
        [
            ("originals", self.originals),
            ("repairs", self.repairs),
            ("generations_per_task", self.generations_per_task),
            ("attempts_per_generation", self.attempts_per_generation),
            ("sequences", self.sequences),
            ("defers", self.defers),
            ("questions", self.questions),
            ("resumes", self.resumes),
        ]
    }
}

impl Default for CensusBounds {
    fn default() -> Self {
        Self {
            originals: 3,
            repairs: 2,
            generations_per_task: 2,
            attempts_per_generation: 2,
            sequences: 4,
            defers: 2,
            questions: 2,
            resumes: 2,
            max_trace: 12,
            max_states: 20_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub label: String,
    pub event: TopologyEvent,
}

impl Candidate {
    pub fn new(label: impl Into<String>, event: TopologyEvent) -> Self {
        Self {
            label: label.into(),
            event,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionOutcome {
    Accepted { to: usize },
    Refused { reason: String },
    Truncated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CensusTransition {
    pub from: usize,
    pub label: String,
    pub outcome: TransitionOutcome,
}

#[derive(Debug, Clone)]
pub struct CensusState {
    pub id: usize,
    pub trace: Vec<TopologyEvent>,
    pub fold: TopologyFold,
    pub outcome: DerivedOutcome,
}

#[derive(Debug, Clone)]
pub struct Census {
    bounds: CensusBounds,
    states: Vec<CensusState>,
    transitions: Vec<CensusTransition>,
    truncated: bool,
}

impl Census {
    pub fn explore<F>(
        start: TopologyFold,
        seed: Vec<TopologyEvent>,
        bounds: CensusBounds,
        classes: F,
    ) -> Self
    where
        F: Fn(&TopologyFold) -> Vec<Candidate>,
    {
        let mut states: Vec<CensusState> = Vec::new();
        let mut transitions: Vec<CensusTransition> = Vec::new();
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        let mut frontier: VecDeque<usize> = VecDeque::new();
        let mut truncated = false;

        seen.insert(fingerprint(&start), 0);
        states.push(CensusState {
            id: 0,
            outcome: start.derived_outcome(),
            trace: seed,
            fold: start,
        });
        frontier.push_back(0);

        while let Some(id) = frontier.pop_front() {
            if states[id].trace.len() >= bounds.max_trace {
                continue;
            }
            for candidate in classes(&states[id].fold) {
                let outcome = match states[id].fold.plan_transition(&candidate.event) {
                    Err(error) => TransitionOutcome::Refused {
                        reason: error.to_string(),
                    },
                    Ok(delta) => {
                        let mut next = states[id].fold.clone();
                        next.apply_delta(delta);
                        let key = fingerprint(&next);
                        match seen.get(&key) {
                            Some(existing) => TransitionOutcome::Accepted { to: *existing },
                            None => {
                                if states.len() >= bounds.max_states {
                                    truncated = true;
                                    transitions.push(CensusTransition {
                                        from: id,
                                        label: candidate.label,
                                        outcome: TransitionOutcome::Truncated,
                                    });
                                    continue;
                                }
                                let to = states.len();
                                let mut trace = states[id].trace.clone();
                                trace.push(candidate.event.clone());
                                seen.insert(key, to);
                                states.push(CensusState {
                                    id: to,
                                    trace,
                                    outcome: next.derived_outcome(),
                                    fold: next,
                                });
                                frontier.push_back(to);
                                TransitionOutcome::Accepted { to }
                            }
                        }
                    }
                };
                transitions.push(CensusTransition {
                    from: id,
                    label: candidate.label,
                    outcome,
                });
            }
        }

        Self {
            bounds,
            states,
            transitions,
            truncated,
        }
    }

    pub fn bounds(&self) -> CensusBounds {
        self.bounds
    }

    pub fn states(&self) -> &[CensusState] {
        &self.states
    }

    pub fn transitions(&self) -> &[CensusTransition] {
        &self.transitions
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn outgoing(&self, id: usize) -> impl Iterator<Item = &CensusTransition> {
        self.transitions
            .iter()
            .filter(move |transition| transition.from == id)
    }

    pub fn has_legal_transition(&self, id: usize) -> bool {
        self.outgoing(id)
            .any(|transition| matches!(transition.outcome, TransitionOutcome::Accepted { .. }))
    }

    pub fn accepted_labels(&self) -> BTreeSet<&str> {
        self.labels(true)
    }

    pub fn refused_labels(&self) -> BTreeSet<&str> {
        self.labels(false)
    }

    fn labels(&self, accepted: bool) -> BTreeSet<&str> {
        self.transitions
            .iter()
            .filter(|transition| {
                matches!(transition.outcome, TransitionOutcome::Accepted { .. }) == accepted
            })
            .map(|transition| transition.label.as_str())
            .collect()
    }

    pub fn states_with(&self, outcome: &DerivedOutcome) -> Vec<&CensusState> {
        self.states
            .iter()
            .filter(|state| &state.outcome == outcome)
            .collect()
    }

    pub fn totality_audit(&self) -> TotalityAudit {
        TotalityAudit::over(&self.states)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotalityAudit {
    pub evaluated: Vec<usize>,
    pub fold_errors: Vec<usize>,
    pub disagreements: Vec<usize>,
    pub not_ending: usize,
    pub ending: usize,
}

impl TotalityAudit {
    pub fn over(states: &[CensusState]) -> Self {
        let mut audit = Self {
            evaluated: Vec::with_capacity(states.len()),
            fold_errors: Vec::new(),
            disagreements: Vec::new(),
            not_ending: 0,
            ending: 0,
        };
        for state in states {
            audit.evaluated.push(state.id);
            let raw = state.fold.derived_outcome();
            if raw == DerivedOutcome::FoldError || state.outcome == DerivedOutcome::FoldError {
                audit.fold_errors.push(state.id);
            }
            if raw != state.outcome {
                audit.disagreements.push(state.id);
            }
            match raw {
                DerivedOutcome::NotEnding => audit.not_ending += 1,
                DerivedOutcome::Ending(_) => audit.ending += 1,
                DerivedOutcome::FoldError => {}
            }
        }
        audit
    }
}

fn fingerprint(fold: &TopologyFold) -> String {
    format!("{:?}|{:?}", fold.state(), fold.is_poisoned())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::OnceLock;
    use std::time::Duration;

    use super::*;
    use crate::events::{
        AttemptRecord, BindingSummary, BudgetKind, ChainSummary, GateSummary, ReviewPassOutcome,
        ReviewRecord, RunOutcome,
    };
    use crate::gates::ShellKind;
    use crate::ir::{
        Artifact, ArtifactId, Effort, Plan, PlanSource, QuestionId, QuestionKind,
        ResolvedEffortPolicy, Task, TaskId, TaskKind, Tier,
    };
    use crate::review::{PassBinding, ReviewPlan};
    use crate::topology::events::{
        AttemptFinished4, AttemptNumber, AttemptSettlement, AttemptStarted4, BudgetExceeded4,
        CandidateLeaseEffect, CandidatePrepared, CandidateRef, CommitSha, DeferWaitElapsed4,
        GenerationCloseReason, GenerationClosed, GenerationId, GitRef, ImageIdentity,
        IncarnationId, InfrastructureKind, LeaseDisposition, LeaseGrant, MergeLeaseRelease,
        MergePrepared, MergeVerificationStarted, MergeVerificationUnavailable, PreparedDisposition,
        RunStarted4, RungBinding, RunnerContract, RunnerKind, RunnerPolicy, SequenceId,
        SettlementTransition, TaskCandidateCreated, TaskDispatched, TaskMerged, TopologyEvent,
        TopologyEventBody, TopologyLimits, UnavailableCause, UnavailableOutcome, VerificationBasis,
        VerificationRecord, VerificationSource, VerificationVerdict,
    };
    use crate::topology::fold::{
        FrozenInputs, GenerationClass, PreparedCandidate, TaskState, TopologyFold, TransactionClass,
    };
    use crate::topology::paths::{GitPath, PathGrammar, PathPolicy, PathPolicyVersion, PathSet};
    use crate::topology::registry::{TaskKey, TaskRegistry};
    use crate::topology::schema::TOPOLOGY_SCHEMA;

    const RUN_ID: &str = "01CENSUS000000000000000009";
    const ALEPH: TaskKey = TaskKey(0);
    const BET: TaskKey = TaskKey(1);

    fn sha(label: &str) -> CommitSha {
        let mut value = format!("{label:-<40}");
        value.truncate(40);
        CommitSha(value)
    }

    fn git_ref(name: &str) -> GitRef {
        GitRef(format!("refs/upstroke/census/{RUN_ID}/{name}"))
    }

    fn task_of(id: &str, deps: &[&str], hint: &str) -> Task {
        Task {
            id: TaskId::from(id),
            kind: if id == "aleph" {
                TaskKind::Refactor
            } else {
                TaskKind::Test
            },
            title: format!("  {id} — Ünicode title  "),
            body: format!("{id} body"),
            depends_on: deps.iter().copied().map(TaskId::from).collect(),
            acceptance: vec![format!("{id} holds")],
            path_hints: vec![hint.to_owned()],
            suggested_tier: if id == "aleph" {
                Some(Tier::Mid)
            } else {
                Some(Tier::Small)
            },
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: vec![ArtifactId::from(format!("{id}-out").as_str())],
        }
    }

    fn plan() -> Plan {
        Plan {
            source: PlanSource {
                adapter: "markdown".to_owned(),
                hash: "census-frozen-hash".to_owned(),
            },
            tasks: vec![
                task_of("aleph", &[], "src/aleph/"),
                task_of("bet", &[], "src/bet/"),
            ],
            artifacts: vec![Artifact {
                id: ArtifactId::from("aleph-out"),
                produced_by: Some(TaskId::from("aleph")),
            }],
        }
    }

    fn chain(task: &str) -> ChainSummary {
        let tiers = if task == "aleph" {
            vec![Tier::Mid, Tier::Frontier]
        } else {
            vec![Tier::Small]
        };
        ChainSummary {
            task: task.to_owned(),
            attempts_per: if task == "aleph" { 2 } else { 1 },
            bindings: Some(
                tiers
                    .iter()
                    .map(|tier| BindingSummary {
                        tier: *tier,
                        agent: format!("{task}-{tier}-agent"),
                        model: format!("{task}-{tier}-model"),
                        pinned: *tier == Tier::Frontier,
                    })
                    .collect(),
            ),
            tiers,
        }
    }

    const NORMALIZED_DIGEST: &str =
        "sha256:5555555555555555555555555555555555555555555555555555555555555555";

    fn path_policy() -> PathPolicy {
        PathPolicy {
            version: PathPolicyVersion::V1,
            case_fold: true,
            grammar: PathGrammar::Globset,
        }
    }

    fn inputs() -> FrozenInputs {
        FrozenInputs {
            plan: plan(),
            normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
        }
    }

    fn probed_agents() -> Vec<String> {
        vec![
            "  Codex-CLI  ".to_owned(),
            "aleph-Mid-agent".to_owned(),
            "bet-Small-agent".to_owned(),
            "aleph-Frontier-agent".to_owned(),
        ]
    }

    fn run_started_unauthenticated() -> RunStarted4 {
        RunStarted4 {
            schema: TOPOLOGY_SCHEMA,
            upstroke_version: "0.2.0-census".to_owned(),
            run_id: RUN_ID.to_owned(),
            incarnation: IncarnationId("01J8ZQKB2M7NC5PQR0TVWXYZ77".to_owned()),
            runner: RunnerPolicy {
                kind: RunnerKind::Container,
                policy: RunnerContract::ContainerV1,
                image: Some(ImageIdentity {
                    reference: "ghcr.io/example/census-runner:3.4".to_owned(),
                    id: "sha256:3333333333333333333333333333333333333333333333333333333333333333"
                        .to_owned(),
                    digest: Some(
                        "sha256:4444444444444444444444444444444444444444444444444444444444444444"
                            .to_owned(),
                    ),
                }),
                credential_volumes: Some(
                    [
                        (
                            "aleph-Mid-agent".to_owned(),
                            "upstroke-creds-Ünicode".to_owned(),
                        ),
                        (
                            "  Codex-CLI  ".to_owned(),
                            "upstroke-creds-codex".to_owned(),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            },
            probed_agents: probed_agents(),
            branch: format!("upstroke/run-{RUN_ID}"),
            integration_ref: git_ref("integration"),
            base_sha: sha("base"),
            execution_root: "/var/lib/Upstroke/census execution roots".to_owned(),
            private_dir: "/var/lib/Upstroke/census private".to_owned(),
            plan_path: "docs/Census Plan.md".to_owned(),
            config_path: None,
            plan_hash: "census-frozen-hash".to_owned(),
            normalized_plan_digest: NORMALIZED_DIGEST.to_owned(),
            registry_digest: String::new(),
            path_policy: path_policy(),
            limits: TopologyLimits {
                max_parallel: 3,
                max_defers: 2,
                max_merge_repairs: 1,
            },
            gates: vec!["fmt".to_owned()],
            gates_from_config: false,
            gate_cmds: vec![GateSummary {
                name: "fmt".to_owned(),
                cmd: "cargo fmt --check".to_owned(),
                timeout: Duration::from_secs(451),
                shell: ShellKind::Bash,
            }],
            interaction_mode: "never".to_owned(),
            chains: vec![chain("aleph"), chain("bet")],
            effort_policy: ResolvedEffortPolicy {
                small: Effort::Low,
                mid: Effort::High,
                frontier: Effort::Max,
                review: Effort::Medium,
            },
            reviews: ReviewPlan {
                enabled: Some(true),
                alternative_available: Some(false),
                pass_timeout_secs: Some(97),
                primary: Some(PassBinding::new("aleph-Mid-agent", "aleph-Mid-model")),
                alternative: None,
                second_opinion: vec![None, None],
            },
        }
    }

    fn run_started() -> RunStarted4 {
        let started = run_started_unauthenticated();
        let digest = TaskRegistry::originals_with_agents(
            &plan(),
            &started.registry_record(),
            &started.probed_agents,
        )
        .expect("the fixture derives a registry")
        .digest();
        RunStarted4 {
            registry_digest: digest,
            ..started
        }
    }

    fn ev(body: TopologyEventBody) -> TopologyEvent {
        TopologyEvent {
            ts: "2026-08-17T19:04:11Z".to_owned(),
            body,
        }
    }

    fn started() -> TopologyFold {
        let mut fold = TopologyFold::new(inputs());
        let event = ev(TopologyEventBody::RunStarted {
            data: Box::new(run_started()),
        });
        let delta = fold
            .plan_transition(&event)
            .expect("the fixture's run_started applies");
        fold.apply_delta(delta);
        fold
    }

    fn region(key: TaskKey) -> PathSet {
        PathSet::Prefixes {
            paths: vec![GitPath::from(if key == ALEPH {
                "src/aleph"
            } else {
                "src/bet"
            })],
        }
    }

    fn overlap_region() -> PathSet {
        PathSet::Prefixes {
            paths: vec![GitPath::from("src/aleph"), GitPath::from("src/bet")],
        }
    }

    fn label(key: TaskKey) -> &'static str {
        if key == ALEPH { "aleph" } else { "bet" }
    }

    fn binding(fold: &TopologyFold, key: TaskKey, rung: usize) -> RungBinding {
        let registry = fold.registry().expect("started");
        let entry = registry.get(key).expect("a registered task");
        let frozen = &entry.ladder.rungs[rung];
        RungBinding::from_frozen(frozen, entry.ladder.effort.implementation_for(frozen.tier))
    }

    fn attempt_record(attempt: u32) -> AttemptRecord {
        AttemptRecord {
            attempt,
            tier: "mid".to_owned(),
            model: "aleph-Mid-model".to_owned(),
            pool: None,
            resumed: false,
            duration: Duration::from_millis(4_321),
            cost_usd: Some(0.75),
            reviews: vec![ReviewRecord {
                pass: "review".to_owned(),
                agent: "claude-code".to_owned(),
                model: "claude-opus-5".to_owned(),
                adapter: Some("claude-code".to_owned()),
                preflight_cli_version: None,
                effort: None,
                pool: None,
                cost_usd: None,
                outcome: ReviewPassOutcome::Passed,
            }],
            session_id: None,
            usage: None,
            failure: None,
        }
    }

    fn dispatch(key: TaskKey, generation: u32) -> TopologyEvent {
        dispatch_over(key, generation, region(key))
    }

    fn dispatch_over(key: TaskKey, generation: u32, paths: PathSet) -> TopologyEvent {
        dispatch_at(key, generation, paths, sha("base"))
    }

    fn dispatch_at(
        key: TaskKey,
        generation: u32,
        paths: PathSet,
        base: CommitSha,
    ) -> TopologyEvent {
        ev(TopologyEventBody::TaskDispatched {
            data: TaskDispatched {
                key,
                generation: GenerationId(generation),
                base_sha: base,
                worktree_path: format!("/tmp/census/{}", label(key)),
                lease: LeaseGrant::Predicted { paths },
                source_candidate: None,
            },
        })
    }

    fn attempt_started(
        fold: &TopologyFold,
        key: TaskKey,
        generation: u32,
        attempt: u32,
    ) -> TopologyEvent {
        ev(TopologyEventBody::AttemptStarted {
            data: AttemptStarted4 {
                key,
                generation: GenerationId(generation),
                attempt: AttemptNumber(attempt),
                rung: 0,
                binding: binding(fold, key, 0),
                pool: None,
                resume_session: None,
                materialization_observed: None,
            },
        })
    }

    fn settle(
        key: TaskKey,
        generation: u32,
        attempt: u32,
        transition: SettlementTransition,
        lease: LeaseDisposition,
    ) -> TopologyEvent {
        ev(TopologyEventBody::AttemptFinished {
            data: Box::new(AttemptFinished4 {
                key,
                generation: GenerationId(generation),
                attempt: AttemptNumber(attempt),
                record: Box::new({
                    let mut record = attempt_record(attempt);
                    record.failure = Some(crate::events::FailureRecord {
                        kind: crate::ladder::FailureKind::GateFailed,
                        origin: crate::ladder::FailureOrigin::Worker,
                        reason: "the fixture's judged failure".to_owned(),
                        detail: None,
                    });
                    record
                }),
                settlement: AttemptSettlement::Closed { transition, lease },
            }),
        })
    }

    fn candidate_of(key: TaskKey, generation: u32) -> CandidateRef {
        CandidateRef {
            key,
            generation: GenerationId(generation),
            commit_sha: sha(&format!("commit-{}-{generation}", label(key))),
            candidate_ref: git_ref(&format!("candidates/{}/{generation}", label(key))),
        }
    }

    fn candidate_prepared(key: TaskKey, generation: u32, attempt: u32) -> TopologyEvent {
        candidate_prepared_over(key, generation, attempt, region(key))
    }

    fn candidate_prepared_over(
        key: TaskKey,
        generation: u32,
        attempt: u32,
        paths: PathSet,
    ) -> TopologyEvent {
        candidate_prepared_at(
            key,
            generation,
            attempt,
            paths,
            sha("base"),
            candidate_of(key, generation).commit_sha,
        )
    }

    fn candidate_prepared_at(
        key: TaskKey,
        generation: u32,
        attempt: u32,
        paths: PathSet,
        base: CommitSha,
        commit: CommitSha,
    ) -> TopologyEvent {
        ev(TopologyEventBody::CandidatePrepared {
            data: Box::new(CandidatePrepared {
                key,
                generation: GenerationId(generation),
                attempt: Box::new(attempt_record(attempt)),
                base_sha: base.clone(),
                parent_sha: base,
                tree_sha: sha(&format!("tree-{}", label(key))),
                commit_sha: commit,
                message: format!("{}: census candidate", label(key)),
                prepared_ref: git_ref(&format!("prepared-candidate/{}", label(key))),
                candidate_ref: candidate_of(key, generation).candidate_ref,
                actual_paths: paths.clone(),
                lease_effect: CandidateLeaseEffect::ReplacesPredicted { paths },
            }),
        })
    }

    fn candidate_at(key: TaskKey, generation: u32, commit: CommitSha) -> CandidateRef {
        CandidateRef {
            commit_sha: commit,
            ..candidate_of(key, generation)
        }
    }

    fn candidate_created(key: TaskKey, generation: u32) -> TopologyEvent {
        candidate_created_of(candidate_of(key, generation))
    }

    fn candidate_created_of(candidate: CandidateRef) -> TopologyEvent {
        ev(TopologyEventBody::TaskCandidateCreated {
            data: TaskCandidateCreated { candidate },
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn merge_prepared(
        sequence: u32,
        key: TaskKey,
        generation: u32,
        disposition: PreparedDisposition,
        expected_head: CommitSha,
        proposed_sha: CommitSha,
        prepared_ref: Option<GitRef>,
        source: VerificationSource,
    ) -> TopologyEvent {
        merge_prepared_for(
            sequence,
            candidate_of(key, generation),
            disposition,
            expected_head,
            proposed_sha,
            prepared_ref,
            source,
        )
    }

    fn merge_prepared_for(
        sequence: u32,
        candidate: CandidateRef,
        disposition: PreparedDisposition,
        expected_head: CommitSha,
        proposed_sha: CommitSha,
        prepared_ref: Option<GitRef>,
        source: VerificationSource,
    ) -> TopologyEvent {
        let CandidateRef {
            key,
            generation,
            commit_sha,
            candidate_ref,
        } = candidate;
        ev(TopologyEventBody::MergePrepared {
            data: Box::new(MergePrepared {
                sequence: SequenceId(sequence),
                disposition,
                expected_head,
                proposed_sha,
                key,
                generation,
                candidate_sha: commit_sha,
                candidate_ref,
                prepared_ref,
                verification_source: source.clone(),
                verification: match &source {
                    VerificationSource::CandidatePrepared { .. } => None,
                    VerificationSource::Verification { .. } => Some(VerificationRecord {
                        verdict: VerificationVerdict::Passed,
                        gates_passed: true,
                        reviews: Vec::new(),
                        detail: "census verification".to_owned(),
                    }),
                },
                satisfies: vec![key],
            }),
        })
    }

    fn task_merged(
        fold: &TopologyFold,
        sequence: u32,
        key: TaskKey,
        generation: u32,
    ) -> TopologyEvent {
        let (merged_sha, satisfies) = match fold.transaction().map(|open| &open.class) {
            Some(TransactionClass::Prepared {
                proposed_sha,
                satisfies,
            }) => (proposed_sha.clone(), satisfies.clone()),
            _ => (candidate_of(key, generation).commit_sha, vec![key]),
        };
        ev(TopologyEventBody::TaskMerged {
            data: TaskMerged {
                sequence: SequenceId(sequence),
                merged_sha,
                satisfies,
                lease_release: MergeLeaseRelease::Candidate {
                    key,
                    generation: GenerationId(generation),
                },
            },
        })
    }

    fn run_finished(fold: &TopologyFold, outcome: RunOutcome) -> TopologyEvent {
        ev(TopologyEventBody::RunFinished {
            data: crate::topology::events::RunFinished4 {
                outcome,
                halted_at: fold.halted_at(),
                merged: 0,
                parked: 0,
            },
        })
    }

    fn classes(fold: &TopologyFold) -> Vec<Candidate> {
        let mut out = Vec::new();
        let sequence = fold.transaction().map_or(0, |t| t.sequence.0);

        for key in [ALEPH, BET] {
            let name = label(key);
            for generation in 0..2 {
                out.push(Candidate::new(
                    format!("task_dispatched/{name}/g{generation}"),
                    dispatch(key, generation),
                ));
                for attempt in 1..=2 {
                    out.push(Candidate::new(
                        format!("attempt_started/{name}/g{generation}/a{attempt}"),
                        attempt_started(fold, key, generation, attempt),
                    ));
                    for (tag, transition, lease) in [
                        (
                            "succeeded",
                            SettlementTransition::Succeeded,
                            LeaseDisposition::PredictedRetained,
                        ),
                        (
                            "retry",
                            SettlementTransition::Retry,
                            LeaseDisposition::PredictedReleased,
                        ),
                        (
                            "failed",
                            SettlementTransition::Failed {
                                halts_run: false,
                                reason: "census failure".to_owned(),
                            },
                            LeaseDisposition::PredictedReleased,
                        ),
                        (
                            "halting",
                            SettlementTransition::Failed {
                                halts_run: true,
                                reason: "census halting failure".to_owned(),
                            },
                            LeaseDisposition::PredictedReleased,
                        ),
                        (
                            "deferred",
                            SettlementTransition::Deferred {
                                defers: 1,
                                reason: "census outage".to_owned(),
                            },
                            LeaseDisposition::PredictedReleased,
                        ),
                        (
                            "parked",
                            SettlementTransition::Parked {
                                question: crate::topology::events::FrozenQuestion {
                                    id: QuestionId::from(format!("q-{name}-{generation}").as_str()),
                                    key,
                                    kind: QuestionKind::Unblock,
                                    context: "  a question only a person settles  ".to_owned(),
                                    options: vec!["yes".to_owned(), "no".to_owned()],
                                },
                            },
                            LeaseDisposition::PredictedReleased,
                        ),
                    ] {
                        out.push(Candidate::new(
                            format!("attempt_finished/{tag}/{name}/g{generation}/a{attempt}"),
                            settle(key, generation, attempt, transition, lease),
                        ));
                    }
                    out.push(Candidate::new(
                        format!("candidate_prepared/{name}/g{generation}/a{attempt}"),
                        candidate_prepared(key, generation, attempt),
                    ));
                }
                out.push(Candidate::new(
                    format!("task_candidate_created/{name}/g{generation}"),
                    candidate_created(key, generation),
                ));
                out.push(Candidate::new(
                    format!("generation_closed/{name}/g{generation}"),
                    ev(TopologyEventBody::GenerationClosed {
                        data: GenerationClosed {
                            key,
                            generation: GenerationId(generation),
                            reason: GenerationCloseReason::RunEnding {
                                outcome: RunOutcome::Complete,
                            },
                            lease: LeaseDisposition::PredictedReleased,
                        },
                    }),
                ));

                let candidate = candidate_of(key, generation);
                let source = VerificationSource::CandidatePrepared {
                    key,
                    generation: GenerationId(generation),
                };
                out.push(Candidate::new(
                    format!("merge_prepared/fast/match/{name}/g{generation}"),
                    merge_prepared(
                        sequence,
                        key,
                        generation,
                        PreparedDisposition::Fast,
                        sha("base"),
                        candidate.commit_sha.clone(),
                        None,
                        source.clone(),
                    ),
                ));
                out.push(Candidate::new(
                    format!("merge_prepared/fast/moved-head/{name}/g{generation}"),
                    merge_prepared(
                        sequence,
                        key,
                        generation,
                        PreparedDisposition::Fast,
                        sha("moved-head"),
                        candidate.commit_sha.clone(),
                        None,
                        source.clone(),
                    ),
                ));
                out.push(Candidate::new(
                    format!("merge_prepared/fast/other-proposed/{name}/g{generation}"),
                    merge_prepared(
                        sequence,
                        key,
                        generation,
                        PreparedDisposition::Fast,
                        sha("base"),
                        sha("not-the-candidate"),
                        None,
                        source.clone(),
                    ),
                ));
                out.push(Candidate::new(
                    format!("merge_prepared/fast/with-pin/{name}/g{generation}"),
                    merge_prepared(
                        sequence,
                        key,
                        generation,
                        PreparedDisposition::Fast,
                        sha("base"),
                        candidate.commit_sha.clone(),
                        Some(git_ref(&format!("prepared/{sequence}"))),
                        source.clone(),
                    ),
                ));
                out.push(Candidate::new(
                    format!("merge_verification_started/stale/{name}/g{generation}"),
                    ev(TopologyEventBody::MergeVerificationStarted {
                        data: MergeVerificationStarted {
                            sequence: SequenceId(sequence),
                            candidate: candidate.clone(),
                            basis: VerificationBasis::StaleClean {
                                prepared_ref: git_ref(&format!("prepared/{sequence}")),
                            },
                            expected_head: sha("moved-head"),
                            proposed_sha: sha(&format!("proposal-{name}")),
                        },
                    }),
                ));
                out.push(Candidate::new(
                    format!("merge_verification_started/present/{name}/g{generation}"),
                    ev(TopologyEventBody::MergeVerificationStarted {
                        data: MergeVerificationStarted {
                            sequence: SequenceId(sequence),
                            candidate: candidate.clone(),
                            basis: VerificationBasis::AlreadyPresent,
                            expected_head: candidate.commit_sha.clone(),
                            proposed_sha: candidate.commit_sha.clone(),
                        },
                    }),
                ));
                let verified = VerificationSource::Verification {
                    sequence: SequenceId(sequence),
                };
                out.push(Candidate::new(
                    format!("merge_prepared/stale_clean/match/{name}/g{generation}"),
                    merge_prepared(
                        sequence,
                        key,
                        generation,
                        PreparedDisposition::StaleClean,
                        sha("moved-head"),
                        sha(&format!("proposal-{name}")),
                        Some(git_ref(&format!("prepared/{sequence}"))),
                        verified.clone(),
                    ),
                ));
                out.push(Candidate::new(
                    format!("merge_prepared/stale_clean/mismatch/{name}/g{generation}"),
                    merge_prepared(
                        sequence,
                        key,
                        generation,
                        PreparedDisposition::StaleClean,
                        sha("moved-head"),
                        sha("not-the-pinned-proposal"),
                        Some(git_ref(&format!("prepared/{sequence}"))),
                        verified.clone(),
                    ),
                ));
                out.push(Candidate::new(
                    format!("merge_prepared/already_present/match/{name}/g{generation}"),
                    merge_prepared(
                        sequence,
                        key,
                        generation,
                        PreparedDisposition::AlreadyPresent,
                        candidate.commit_sha.clone(),
                        candidate.commit_sha.clone(),
                        None,
                        verified.clone(),
                    ),
                ));
                out.push(Candidate::new(
                    format!("merge_prepared/already_present/mismatch/{name}/g{generation}"),
                    merge_prepared(
                        sequence,
                        key,
                        generation,
                        PreparedDisposition::AlreadyPresent,
                        candidate.commit_sha.clone(),
                        sha("not-the-head"),
                        None,
                        verified.clone(),
                    ),
                ));
                out.push(Candidate::new(
                    format!("task_merged/{name}/g{generation}"),
                    task_merged(fold, sequence, key, generation),
                ));
            }
        }

        out.push(Candidate::new(
            "defer_wait_elapsed",
            ev(TopologyEventBody::DeferWaitElapsed {
                data: DeferWaitElapsed4 {
                    waited_ms: 30_000,
                    round: 1,
                },
            }),
        ));
        out.push(Candidate::new(
            "budget_exceeded",
            ev(TopologyEventBody::BudgetExceeded {
                data: BudgetExceeded4 {
                    epoch: fold.epoch().unwrap_or(crate::topology::events::Epoch(0)),
                    budget: BudgetKind::Run,
                    limit_usd: 12.5,
                    spent_usd: 12.75,
                    key: Some(ALEPH),
                },
            }),
        ));
        for outcome in [
            RunOutcome::Complete,
            RunOutcome::Parked,
            RunOutcome::Halted,
            RunOutcome::BudgetExceeded,
        ] {
            out.push(Candidate::new(
                format!("run_finished/{outcome:?}"),
                run_finished(fold, outcome),
            ));
        }
        out
    }

    fn run_started_event() -> TopologyEvent {
        ev(TopologyEventBody::RunStarted {
            data: Box::new(run_started()),
        })
    }

    fn census() -> &'static Census {
        static CENSUS: OnceLock<Census> = OnceLock::new();
        CENSUS.get_or_init(|| {
            Census::explore(
                started(),
                vec![run_started_event()],
                CensusBounds::default(),
                classes,
            )
        })
    }

    fn common(fold: &TopologyFold) -> bool {
        let no_open_generation = [ALEPH, BET].iter().all(|key| {
            fold.task(*key).is_none_or(|task| {
                task.generations
                    .iter()
                    .all(|generation| generation.class == GenerationClass::Closed)
            })
        });
        no_open_generation && fold.transaction().is_none()
    }

    fn backoff_pending(fold: &TopologyFold) -> bool {
        let deferred_task = [ALEPH, BET]
            .iter()
            .any(|key| fold.task_state(*key) == Some(TaskState::Deferred));
        let deferred_candidate = fold.queue().is_some_and(|queue| {
            queue
                .entries()
                .iter()
                .any(|entry| entry.verification_deferred)
        });
        deferred_task || deferred_candidate
    }

    fn questions_open(fold: &TopologyFold) -> bool {
        fold.open_questions()
            .is_some_and(|questions| !questions.is_empty())
    }

    fn complete_shape(fold: &TopologyFold) -> bool {
        let every_task_terminal = [ALEPH, BET].iter().all(|key| {
            matches!(
                fold.task_state(*key),
                Some(TaskState::Merged | TaskState::Failed)
            )
        });
        let queue_empty = fold.queue().is_none_or(|queue| queue.is_empty());
        let no_lease = fold
            .leases()
            .is_none_or(|leases| !leases.any_candidate_or_lineage());
        every_task_terminal && queue_empty && no_lease && !questions_open(fold)
    }

    #[test]
    fn the_derived_outcome_is_total_over_every_explored_state() {
        let census = census();
        assert!(!census.states().is_empty());
        let audit = census.totality_audit();

        let reached: BTreeSet<usize> =
            std::iter::once(0)
                .chain(census.transitions().iter().filter_map(
                    |transition| match transition.outcome {
                        TransitionOutcome::Accepted { to } => Some(to),
                        TransitionOutcome::Refused { .. } | TransitionOutcome::Truncated => None,
                    },
                ))
                .collect();
        assert_eq!(
            audit.evaluated,
            (0..census.states().len()).collect::<Vec<_>>(),
            "one evaluation per explored state, in order, and no more"
        );
        assert_eq!(
            audit.evaluated.iter().copied().collect::<BTreeSet<_>>(),
            reached,
            "the states that were evaluated and the states the transitions reach are not the \
             same set"
        );

        assert!(
            audit.fold_errors.is_empty(),
            "the arm the design argues is unreachable was reached at states {:?}, the first after \
             {:?}",
            audit.fold_errors,
            audit.fold_errors.first().map(|id| census.states()[*id]
                .trace
                .iter()
                .map(|event| event.body.kind())
                .collect::<Vec<_>>())
        );
        assert!(
            audit.disagreements.is_empty(),
            "the recorded outcome and a fresh evaluation of the same fold disagree at {:?}",
            audit.disagreements
        );
        assert_eq!(
            audit.not_ending + audit.ending,
            census.states().len(),
            "every explored state answered exactly one of the two"
        );
        let (not_ending, ending) = (audit.not_ending, audit.ending);
        assert!(not_ending > 0 && ending > 0, "{not_ending}/{ending}");

        for state in census.states() {
            let fold = &state.fold;
            let common = common(fold);
            let halting = fold.halted_at().is_some();
            let budget = fold
                .budget_stop()
                .is_some_and(|stop| Some(stop.epoch) == fold.epoch());
            if !common {
                assert_eq!(
                    state.outcome,
                    DerivedOutcome::NotEnding,
                    "state {}: a run with open work is not ending",
                    state.id
                );
            } else if halting {
                assert_eq!(
                    state.outcome,
                    DerivedOutcome::Ending(RunOutcome::Halted),
                    "state {}: halt outranks everything",
                    state.id
                );
            } else if budget {
                assert_eq!(
                    state.outcome,
                    DerivedOutcome::Ending(RunOutcome::BudgetExceeded),
                    "state {}: budget outranks parked and complete",
                    state.id
                );
            } else if backoff_pending(fold) {
                assert_eq!(
                    state.outcome,
                    DerivedOutcome::NotEnding,
                    "state {}: pending backoff blocks Parked and Complete",
                    state.id
                );
            } else if complete_shape(fold) {
                assert_eq!(
                    state.outcome,
                    DerivedOutcome::Ending(RunOutcome::Complete),
                    "state {}: nothing is open and nothing is asked",
                    state.id
                );
            }
            match &state.outcome {
                DerivedOutcome::Ending(RunOutcome::Parked) => {
                    assert!(questions_open(fold), "state {}", state.id);
                    assert!(!backoff_pending(fold), "state {}", state.id);
                    assert!(common, "state {}", state.id);
                }
                DerivedOutcome::Ending(RunOutcome::Complete) => {
                    assert!(!questions_open(fold), "state {}", state.id);
                    assert!(complete_shape(fold), "state {}", state.id);
                }
                DerivedOutcome::Ending(RunOutcome::Halted) => {
                    assert!(halting, "state {}", state.id);
                }
                DerivedOutcome::Ending(RunOutcome::BudgetExceeded) => {
                    assert!(budget && !halting, "state {}", state.id);
                }
                DerivedOutcome::NotEnding | DerivedOutcome::FoldError => {}
            }
        }
    }

    #[test]
    fn a_state_with_admissible_work_and_no_budget_exceeded_classifies_not_ending() {
        let census = census();
        let mut before = 0;
        let mut after = 0;
        for state in census.states() {
            let fold = &state.fold;
            let has_record = fold.budget_stop().is_some();
            if !has_record && fold.halted_at().is_none() {
                assert_ne!(
                    state.outcome,
                    DerivedOutcome::Ending(RunOutcome::BudgetExceeded),
                    "state {}: a run that recorded no budget_exceeded cannot end for budget",
                    state.id
                );
            }
            let admissible_work = [ALEPH, BET].iter().any(|key| {
                fold.task(*key).is_some_and(|task| {
                    task.generations
                        .iter()
                        .any(|generation| generation.class != GenerationClass::Closed)
                })
            });
            if admissible_work && !has_record {
                assert_eq!(
                    state.outcome,
                    DerivedOutcome::NotEnding,
                    "state {}",
                    state.id
                );
                before += 1;
            }
            if has_record && fold.halted_at().is_none() && common(fold) {
                assert_eq!(
                    state.outcome,
                    DerivedOutcome::Ending(RunOutcome::BudgetExceeded),
                    "state {}: once common holds, the record decides",
                    state.id
                );
                after += 1;
            }
        }
        assert!(before > 0, "no pre-budget_exceeded prefix was explored");
        assert!(after > 0, "no post-budget_exceeded state was explored");
    }

    #[test]
    fn every_deferred_state_has_a_legal_next_transition() {
        let census = census();
        let mut deferred_states = 0;
        let mut at_ceiling = 0;
        let mut below_ceiling = 0;
        let mut ceiling_wakes = 0;
        let mut ceiling_closes = 0;
        for state in census.states() {
            if !backoff_pending(&state.fold) || state.fold.finished().is_some() {
                continue;
            }
            deferred_states += 1;
            let at_the_ceiling = state.trace.len() >= census.bounds().max_trace;
            let accepted: BTreeSet<String> = classes(&state.fold)
                .into_iter()
                .filter(|candidate| state.fold.plan_transition(&candidate.event).is_ok())
                .map(|candidate| candidate.label)
                .collect();
            if at_the_ceiling {
                at_ceiling += 1;
                assert_eq!(
                    census.outgoing(state.id).count(),
                    0,
                    "state {} sits at the trace ceiling and was extended anyway",
                    state.id
                );
            } else {
                below_ceiling += 1;
                let recorded: BTreeSet<String> = census
                    .outgoing(state.id)
                    .filter(|transition| {
                        matches!(transition.outcome, TransitionOutcome::Accepted { .. })
                    })
                    .map(|transition| transition.label.clone())
                    .collect();
                assert_eq!(
                    recorded, accepted,
                    "state {}: the recorded offers and the fold disagree about what is accepted \
                     here",
                    state.id
                );
                assert_eq!(
                    census.has_legal_transition(state.id),
                    !accepted.is_empty(),
                    "state {}: the accessor and the fold disagree about whether anything is \
                     accepted here",
                    state.id
                );
            }
            assert!(
                !accepted.is_empty(),
                "state {} has a deferred item and no way out: {:?}",
                state.id,
                state
                    .trace
                    .iter()
                    .map(|event| event.body.kind())
                    .collect::<Vec<_>>()
            );
            let halting = state.fold.halted_at().is_some();
            let stopped = state.fold.budget_stop().is_some();
            if !halting && !stopped {
                assert!(
                    accepted.contains("defer_wait_elapsed"),
                    "state {}: an unhalted, unstopped backoff wakes: {accepted:?}",
                    state.id
                );
                ceiling_wakes += usize::from(at_the_ceiling);
            } else {
                assert!(
                    !accepted.contains("defer_wait_elapsed"),
                    "state {}: halt and budget outrank backoff: {accepted:?}",
                    state.id
                );
                assert!(
                    accepted.iter().any(|label| {
                        [
                            "attempt_finished/",
                            "attempt_interrupted",
                            "candidate_prepared/",
                            "generation_closed/",
                            "task_candidate_created/",
                            "merge_prepared/",
                            "task_merged/",
                            "run_finished/",
                        ]
                        .iter()
                        .any(|closure| label.starts_with(closure))
                    }),
                    "state {}: a halted or stopped backoff closes: {accepted:?}\n  \
                     outcome={:?} halted={:?} stop={:?}\n  trace={:?}",
                    state.id,
                    state.outcome,
                    state.fold.halted_at(),
                    state.fold.budget_stop(),
                    state
                        .trace
                        .iter()
                        .map(|e| e.body.kind())
                        .collect::<Vec<_>>(),
                );
                ceiling_closes += usize::from(at_the_ceiling);
            }
        }
        assert!(deferred_states > 0, "no deferred state was explored");
        assert!(
            below_ceiling > 0,
            "every deferred state sat at the trace ceiling, so the recorded table was never \
             cross-checked against the fold"
        );
        assert!(
            at_ceiling > 0,
            "no deferred state sat at the trace ceiling, so the unextended states this assertion \
             now covers are hypothetical"
        );
        assert!(
            ceiling_wakes > 0 && ceiling_closes > 0,
            "the ceiling holds {ceiling_wakes} waking and {ceiling_closes} closing deferred \
             states; both arms of the condition are owed one"
        );

        assert!(
            census
                .states()
                .iter()
                .all(|state| state.fold.queue().is_none_or(|queue| queue
                    .entries()
                    .iter()
                    .all(|entry| { !entry.verification_deferred }))),
            "this fixture reached a verification-deferred candidate after all, and the assertion \
             above no longer needs its companion"
        );
    }

    fn deferral_classes(fold: &TopologyFold) -> Vec<Candidate> {
        let mut out = overlap_classes(fold);
        out.retain(|candidate| !candidate.label.starts_with("candidate_prepared/region-ab"));
        out.push(Candidate::new(
            "merge_verification_unavailable/deferred",
            verification_deferred_by_outage(0, 1),
        ));
        out.push(Candidate::new(
            "defer_wait_elapsed",
            ev(TopologyEventBody::DeferWaitElapsed {
                data: DeferWaitElapsed4 {
                    waited_ms: 30_000,
                    round: 1,
                },
            }),
        ));
        out
    }

    #[test]
    fn a_verification_deferred_candidate_is_a_deferred_state_with_a_way_out() {
        let census = Census::explore(
            started(),
            vec![run_started_event()],
            CensusBounds::default(),
            deferral_classes,
        );
        assert!(!census.truncated());
        let deferred: Vec<&CensusState> = census
            .states()
            .iter()
            .filter(|state| {
                state.fold.queue().is_some_and(|queue| {
                    queue
                        .entries()
                        .iter()
                        .any(|entry| entry.verification_deferred)
                })
            })
            .collect();
        assert!(
            !deferred.is_empty(),
            "no candidate was verification-deferred"
        );
        for state in &deferred {
            assert!(state.trace.len() < census.bounds().max_trace);
            assert!(state.fold.halted_at().is_none());
            assert!(state.fold.budget_stop().is_none());
            assert!(
                census.has_legal_transition(state.id),
                "state {} defers a verification and has no way out",
                state.id
            );
            let accepted: BTreeSet<&str> = census
                .outgoing(state.id)
                .filter(|transition| {
                    matches!(transition.outcome, TransitionOutcome::Accepted { .. })
                })
                .map(|transition| transition.label.as_str())
                .collect();
            assert!(
                accepted.contains("defer_wait_elapsed"),
                "state {}: {accepted:?}",
                state.id
            );
            assert!(
                !accepted.contains("merge_verification_started/aleph/g0"),
                "state {}: a deferred candidate was re-offered for verification",
                state.id
            );
            assert_eq!(
                state.outcome,
                DerivedOutcome::NotEnding,
                "state {}",
                state.id
            );
        }
    }

    #[test]
    fn the_publication_relations_are_exercised_in_both_directions() {
        let census = census();
        let accepted = census.accepted_labels();
        let refused = census.refused_labels();

        for matching in [
            "merge_prepared/fast/match/aleph/g0",
            "merge_prepared/stale_clean/match/aleph/g0",
            "merge_prepared/already_present/match/aleph/g0",
        ] {
            assert!(
                accepted.contains(matching),
                "`{matching}` was never accepted: {:?}",
                accepted
                    .iter()
                    .filter(|label| label.starts_with("merge_prepared/"))
                    .collect::<Vec<_>>()
            );
        }
        for mismatching in [
            "merge_prepared/fast/moved-head/aleph/g0",
            "merge_prepared/fast/other-proposed/aleph/g0",
            "merge_prepared/fast/with-pin/aleph/g0",
            "merge_prepared/stale_clean/mismatch/aleph/g0",
            "merge_prepared/already_present/mismatch/aleph/g0",
        ] {
            assert!(
                refused.contains(mismatching),
                "`{mismatching}` was never refused"
            );
            assert!(
                !accepted.contains(mismatching),
                "`{mismatching}` was accepted somewhere, and it names a relation the fold must refuse"
            );
        }
        assert!(census.transitions().iter().any(|transition| {
            transition.label == "merge_prepared/fast/with-pin/aleph/g0"
                && matches!(transition.outcome, TransitionOutcome::Refused { .. })
        }));
    }

    #[test]
    fn no_offer_is_unmapped_and_every_class_is_offered_everywhere() {
        let census = census();
        let per_state = classes(&started()).len();
        assert!(per_state > 60, "{per_state} classes is a thin census");
        let extendable = census
            .states()
            .iter()
            .filter(|state| state.trace.len() < census.bounds().max_trace)
            .count();
        assert_eq!(
            census.transitions().len(),
            extendable * per_state,
            "an offer produced neither an acceptance nor a refusal"
        );
        for transition in census.transitions() {
            match &transition.outcome {
                TransitionOutcome::Accepted { to } => assert!(*to < census.states().len()),
                TransitionOutcome::Refused { reason } => {
                    assert!(!reason.is_empty(), "{}", transition.label);
                }
                TransitionOutcome::Truncated => {
                    panic!("the census truncated at {}", transition.label)
                }
            }
        }
        assert!(!census.accepted_labels().is_empty());
        assert!(!census.refused_labels().is_empty());
        assert!(
            !census.truncated(),
            "the census hit its state ceiling; every assertion over it is about a subset"
        );
    }

    #[test]
    fn replaying_every_explored_trace_reaches_the_state_it_was_explored_at() {
        let census = census();
        for state in census.states() {
            let replayed = TopologyFold::replay(inputs(), &state.trace)
                .unwrap_or_else(|error| panic!("state {} does not replay: {error}", state.id));
            assert!(
                replayed.state() == state.fold.state(),
                "state {} replays to a different state",
                state.id
            );
            assert_eq!(
                replayed.derived_outcome(),
                state.outcome,
                "state {} classifies differently live and on replay",
                state.id
            );
            let again = TopologyFold::replay(inputs(), &state.trace).expect("replays again");
            assert!(again.state() == replayed.state(), "state {}", state.id);
        }
    }

    #[test]
    fn the_census_reaches_every_outcome_and_says_what_it_did_not_reach() {
        let census = census();
        let reached: BTreeSet<String> = census
            .states()
            .iter()
            .filter_map(|state| match &state.outcome {
                DerivedOutcome::Ending(outcome) => Some(format!("{outcome:?}")),
                _ => None,
            })
            .collect();
        for outcome in ["Complete", "Halted", "BudgetExceeded", "Parked"] {
            assert!(
                reached.contains(outcome),
                "{outcome} unreached: {reached:?}"
            );
        }
        let mut compared = 0;
        for state in census.states() {
            if state.fold.finished().is_some() {
                for outcome in [
                    RunOutcome::Complete,
                    RunOutcome::Parked,
                    RunOutcome::Halted,
                    RunOutcome::BudgetExceeded,
                ] {
                    let event = run_finished(&state.fold, outcome.clone());
                    assert!(
                        state.fold.plan_transition(&event).is_err(),
                        "state {}: a run ends once",
                        state.id
                    );
                }
                continue;
            }
            for outcome in [
                RunOutcome::Complete,
                RunOutcome::Parked,
                RunOutcome::Halted,
                RunOutcome::BudgetExceeded,
            ] {
                let event = run_finished(&state.fold, outcome.clone());
                let accepted = state.fold.plan_transition(&event).is_ok();
                assert_eq!(
                    accepted,
                    state.outcome == DerivedOutcome::Ending(outcome.clone()),
                    "state {}: run_finished({outcome:?}) against {:?}",
                    state.id,
                    state.outcome
                );
                compared += 1;
            }
        }
        assert!(compared > 100, "only {compared} guards were compared");
    }

    #[test]
    fn the_skeleton_states_the_bounds_it_ran_under_and_the_ones_it_did_not() {
        let bounds = CensusBounds::default();
        assert_eq!(bounds.originals, 3);
        assert_eq!(bounds.repairs, 2);
        assert_eq!(bounds.generations_per_task, 2);
        assert_eq!(bounds.attempts_per_generation, 2);
        assert_eq!(bounds.sequences, 4);
        assert_eq!(bounds.defers, 2);

        let census = census();
        let registry = started().registry().expect("started").len();
        assert_eq!(registry, 2, "the fixture plan is two originals");
        assert!(
            u32::try_from(registry).unwrap_or(u32::MAX) < bounds.originals,
            "the skeleton runs below the design's bound and says so"
        );
        assert!(
            !census
                .transitions()
                .iter()
                .any(|transition| transition.label.starts_with("task_spawned/")),
            "the skeleton offers no repair spawn"
        );
        for state in census.states() {
            assert!(
                state
                    .fold
                    .leases()
                    .is_none_or(|leases| leases.lineages().is_empty()),
                "state {} holds a lineage lease the skeleton cannot have made",
                state.id
            );
        }
    }

    #[test]
    fn the_fixture_varies_every_field_a_relation_reads() {
        let started = run_started();
        let limits = BTreeSet::from([
            started.limits.max_parallel,
            started.limits.max_defers,
            started.limits.max_merge_repairs,
        ]);
        assert_eq!(limits.len(), 3, "a fold reading one limit for another");
        let efforts = BTreeSet::from([
            format!("{:?}", started.effort_policy.small),
            format!("{:?}", started.effort_policy.mid),
            format!("{:?}", started.effort_policy.frontier),
            format!("{:?}", started.effort_policy.review),
        ]);
        assert_eq!(efforts.len(), 4);
        assert_ne!(started.chains[0].tiers.len(), started.chains[1].tiers.len());
        assert_ne!(
            started.chains[0].attempts_per,
            started.chains[1].attempts_per
        );
        assert_ne!(region(ALEPH), region(BET));
        let shas = BTreeSet::from([
            sha("base"),
            candidate_of(ALEPH, 0).commit_sha,
            candidate_of(BET, 0).commit_sha,
            sha("moved-head"),
            sha("proposal-aleph"),
            sha("proposal-bet"),
            sha("not-the-candidate"),
            sha("not-the-pinned-proposal"),
            sha("not-the-head"),
            sha("tree-aleph"),
        ]);
        assert_eq!(shas.len(), 10, "two roles share a literal");
        assert_ne!(started.registry_digest, String::new());
        assert_ne!(started.registry_digest, started.normalized_plan_digest);
    }

    #[test]
    fn a_census_that_hits_its_ceiling_says_so() {
        let tight = CensusBounds {
            max_states: 3,
            ..CensusBounds::default()
        };
        let stopped = Census::explore(started(), vec![run_started_event()], tight, classes);
        assert!(stopped.truncated());
        assert!(stopped.states().len() <= 3);
        assert!(!census().truncated());

        let shallow = CensusBounds {
            max_trace: 2,
            ..CensusBounds::default()
        };
        let shallow = Census::explore(started(), vec![run_started_event()], shallow, classes);
        assert!(!shallow.truncated());
        assert!(shallow.states().len() > 1);
        assert_eq!(
            shallow.transitions().len(),
            classes(&started()).len(),
            "only the root was extended"
        );
    }

    #[test]
    fn a_transaction_class_is_reachable_and_blocks_the_run_from_ending() {
        let census = census();
        let with_transaction: Vec<&CensusState> = census
            .states()
            .iter()
            .filter(|state| state.fold.transaction().is_some())
            .collect();
        assert!(
            !with_transaction.is_empty(),
            "no state held an unresolved transaction"
        );
        for state in &with_transaction {
            assert_eq!(
                state.outcome,
                DerivedOutcome::NotEnding,
                "state {}",
                state.id
            );
        }
        let classes_seen: BTreeSet<&'static str> = with_transaction
            .iter()
            .map(|state| {
                match state
                    .fold
                    .transaction()
                    .map(|transaction| &transaction.class)
                {
                    Some(TransactionClass::VerificationStarted { .. }) => "verification",
                    Some(TransactionClass::Prepared { .. }) => "prepared",
                    None => "none",
                }
            })
            .collect();
        assert_eq!(
            classes_seen,
            BTreeSet::from(["verification", "prepared"]),
            "{classes_seen:?}"
        );
    }

    fn state_at(id: usize, fold: TopologyFold, outcome: DerivedOutcome) -> CensusState {
        CensusState {
            id,
            trace: Vec::new(),
            fold,
            outcome,
        }
    }

    fn fold_with(outcome: &DerivedOutcome) -> TopologyFold {
        census()
            .states_with(outcome)
            .first()
            .unwrap_or_else(|| panic!("no census state is {outcome:?}"))
            .fold
            .clone()
    }

    #[test]
    fn the_totality_audit_reports_a_fold_error_a_normalisation_and_a_short_domain() {
        let ending = fold_with(&DerivedOutcome::Ending(RunOutcome::Complete));
        let not_ending = started();

        let sentinel = vec![
            state_at(0, not_ending.clone(), DerivedOutcome::NotEnding),
            state_at(1, ending.clone(), DerivedOutcome::FoldError),
        ];
        let audit = TotalityAudit::over(&sentinel);
        assert_eq!(audit.fold_errors, vec![1]);
        assert_eq!(audit.evaluated, vec![0, 1]);

        let normalised = vec![state_at(0, ending.clone(), DerivedOutcome::NotEnding)];
        let audit = TotalityAudit::over(&normalised);
        assert_eq!(audit.disagreements, vec![0]);
        assert!(audit.fold_errors.is_empty());
        assert_eq!((audit.not_ending, audit.ending), (0, 1));

        let short = vec![
            state_at(0, not_ending.clone(), DerivedOutcome::NotEnding),
            state_at(2, not_ending.clone(), DerivedOutcome::NotEnding),
        ];
        let audit = TotalityAudit::over(&short);
        assert_eq!(audit.evaluated, vec![0, 2]);
        assert_ne!(audit.evaluated, vec![0, 1]);

        let clean = vec![
            state_at(0, not_ending, DerivedOutcome::NotEnding),
            state_at(1, ending, DerivedOutcome::Ending(RunOutcome::Complete)),
        ];
        let audit = TotalityAudit::over(&clean);
        assert!(audit.fold_errors.is_empty() && audit.disagreements.is_empty());
        assert_eq!((audit.not_ending, audit.ending), (1, 1));
    }

    #[test]
    fn the_census_transition_table_is_reproducible_from_the_folds_alone() {
        let census = census();
        let mut rows = 0usize;
        for state in census.states() {
            let recorded: Vec<&CensusTransition> = census.outgoing(state.id).collect();
            if state.trace.len() >= census.bounds().max_trace {
                assert!(
                    recorded.is_empty(),
                    "state {} sits at the trace ceiling and was extended anyway",
                    state.id
                );
                assert!(
                    !census.has_legal_transition(state.id),
                    "state {} was never extended and reports a transition",
                    state.id
                );
                continue;
            }
            let offers = classes(&state.fold);
            assert_eq!(
                recorded.len(),
                offers.len(),
                "state {} recorded {} answers for {} offers",
                state.id,
                recorded.len(),
                offers.len()
            );
            let mut any_accepted = false;
            for (offer, row) in offers.iter().zip(&recorded) {
                assert_eq!(row.from, state.id);
                assert_eq!(row.label, offer.label, "state {}", state.id);
                match (state.fold.plan_transition(&offer.event), &row.outcome) {
                    (Err(error), TransitionOutcome::Refused { reason }) => {
                        assert_eq!(*reason, error.to_string(), "state {}", state.id);
                    }
                    (Ok(delta), TransitionOutcome::Accepted { to }) => {
                        any_accepted = true;
                        let mut next = state.fold.clone();
                        next.apply_delta(delta);
                        let landed = &census.states()[*to];
                        assert_eq!(
                            fingerprint(&next),
                            fingerprint(&landed.fold),
                            "state {} --{}--> {to} is not the state applying it reaches",
                            state.id,
                            offer.label
                        );
                        assert_eq!(
                            landed.outcome,
                            next.derived_outcome(),
                            "state {to} was recorded with an outcome its own fold does not give"
                        );
                    }
                    (Ok(_), answer) => panic!(
                        "state {}: the fold accepts `{}` and the census recorded {answer:?}",
                        state.id, offer.label
                    ),
                    (Err(error), answer) => panic!(
                        "state {}: the fold refuses `{}` with `{error}` and the census recorded \
                         {answer:?}",
                        state.id, offer.label
                    ),
                }
                rows += 1;
            }
            assert_eq!(
                census.has_legal_transition(state.id),
                any_accepted,
                "state {}",
                state.id
            );
        }
        assert_eq!(
            rows,
            census.transitions().len(),
            "the census holds a row no offer produced"
        );
    }

    #[test]
    fn the_seed_state_is_evaluated_rather_than_assumed_not_ending() {
        let ended = census()
            .states()
            .iter()
            .find(|state| {
                state.fold.finished().is_some()
                    && state.outcome == DerivedOutcome::Ending(RunOutcome::Complete)
            })
            .expect("the census reaches a completed run");
        let bounds = CensusBounds {
            max_trace: 0,
            ..CensusBounds::default()
        };
        let seeded = Census::explore(ended.fold.clone(), ended.trace.clone(), bounds, classes);
        assert_eq!(seeded.states().len(), 1, "nothing was extended");
        assert!(seeded.transitions().is_empty());
        assert!(!seeded.truncated());
        assert_eq!(
            seeded.states()[0].outcome,
            DerivedOutcome::Ending(RunOutcome::Complete),
            "the seed was assumed rather than evaluated"
        );
        let audit = seeded.totality_audit();
        assert_eq!(audit.evaluated, vec![0]);
        assert!(audit.disagreements.is_empty() && audit.fold_errors.is_empty());
        assert_eq!((audit.not_ending, audit.ending), (0, 1));
    }

    fn unresolvable_merge() -> TopologyEvent {
        ev(TopologyEventBody::TaskMerged {
            data: TaskMerged {
                sequence: SequenceId(0),
                merged_sha: sha("base"),
                satisfies: vec![ALEPH],
                lease_release: MergeLeaseRelease::Candidate {
                    key: ALEPH,
                    generation: GenerationId(0),
                },
            },
        })
    }

    fn only_refused(_: &TopologyFold) -> Vec<Candidate> {
        vec![Candidate::new(
            "task_merged/no-transaction",
            unresolvable_merge(),
        )]
    }

    fn dispatch_once_then_dead(fold: &TopologyFold) -> Vec<Candidate> {
        let mut out = only_refused(fold);
        if fold
            .task(ALEPH)
            .is_none_or(|task| task.generations.is_empty())
        {
            out.push(Candidate::new(
                "task_dispatched/aleph/g0",
                dispatch(ALEPH, 0),
            ));
        }
        out
    }

    #[test]
    fn has_legal_transition_is_local_to_the_state_and_excludes_refusals() {
        let refusals = Census::explore(
            started(),
            vec![run_started_event()],
            CensusBounds::default(),
            only_refused,
        );
        assert_eq!(refusals.states().len(), 1);
        assert_eq!(refusals.transitions().len(), 1);
        assert!(matches!(
            refusals.transitions()[0].outcome,
            TransitionOutcome::Refused { .. }
        ));
        assert!(
            !refusals.has_legal_transition(0),
            "every offer at this state was refused"
        );

        let mixed = Census::explore(
            started(),
            vec![run_started_event()],
            CensusBounds::default(),
            dispatch_once_then_dead,
        );
        assert_eq!(mixed.states().len(), 2, "one live state and one dead one");
        assert!(mixed.has_legal_transition(0), "the root dispatches");
        assert!(
            !mixed.has_legal_transition(1),
            "the dispatched state has no accepted offer of its own"
        );
        assert_eq!(mixed.outgoing(1).count(), 1);
        assert!(!mixed.has_legal_transition(2));
        assert!(!mixed.has_legal_transition(usize::MAX));
    }

    fn verification_started(
        sequence: u32,
        key: TaskKey,
        generation: u32,
        pin: &str,
        expected_head: CommitSha,
        proposed_sha: CommitSha,
    ) -> TopologyEvent {
        ev(TopologyEventBody::MergeVerificationStarted {
            data: MergeVerificationStarted {
                sequence: SequenceId(sequence),
                candidate: candidate_of(key, generation),
                basis: VerificationBasis::StaleClean {
                    prepared_ref: git_ref(pin),
                },
                expected_head,
                proposed_sha,
            },
        })
    }

    fn verification_parked(sequence: u32, key: TaskKey, id: &str) -> TopologyEvent {
        ev(TopologyEventBody::MergeVerificationUnavailable {
            data: MergeVerificationUnavailable {
                sequence: SequenceId(sequence),
                cause: UnavailableCause::HumanRequired {
                    verdict: "  a reviewer found something only a person decides  ".to_owned(),
                },
                outcome: UnavailableOutcome::Parked {
                    question: crate::topology::events::FrozenQuestion {
                        id: QuestionId::from(id),
                        key,
                        kind: QuestionKind::Unblock,
                        context: "  the verification could not run  ".to_owned(),
                        options: vec!["retry".to_owned(), "abandon".to_owned()],
                    },
                },
            },
        })
    }

    fn verification_deferred_by_outage(sequence: u32, defers: u32) -> TopologyEvent {
        ev(TopologyEventBody::MergeVerificationUnavailable {
            data: MergeVerificationUnavailable {
                sequence: SequenceId(sequence),
                cause: UnavailableCause::Infrastructure {
                    kind: InfrastructureKind::RateLimited,
                },
                outcome: UnavailableOutcome::Deferred { defers },
            },
        })
    }

    fn queued_candidate_trace(paths: PathSet) -> Vec<TopologyEvent> {
        let mut fold = started();
        let mut trace = vec![run_started_event()];
        for event in [
            dispatch(ALEPH, 0),
            attempt_started(&fold, ALEPH, 0, 1),
            candidate_prepared_over(ALEPH, 0, 1, paths.clone()),
            candidate_created(ALEPH, 0),
        ] {
            let delta = fold
                .plan_transition(&event)
                .unwrap_or_else(|error| panic!("the shared prefix applies: {error}"));
            fold.apply_delta(delta);
            trace.push(event);
        }
        trace
    }

    fn queued_candidate_at(base: CommitSha, commit: CommitSha) -> Vec<TopologyEvent> {
        let mut fold = started();
        let mut trace = vec![run_started_event()];
        for event in [
            dispatch_at(ALEPH, 0, region(ALEPH), base.clone()),
            attempt_started(&fold, ALEPH, 0, 1),
            candidate_prepared_at(ALEPH, 0, 1, region(ALEPH), base, commit.clone()),
            candidate_created_of(candidate_at(ALEPH, 0, commit)),
        ] {
            let delta = fold
                .plan_transition(&event)
                .unwrap_or_else(|error| panic!("a candidate-side prefix applies: {error}"));
            fold.apply_delta(delta);
            trace.push(event);
        }
        trace
    }

    fn fast_publication(base: CommitSha, commit: CommitSha) -> TopologyEvent {
        merge_prepared_for(
            0,
            candidate_at(ALEPH, 0, commit.clone()),
            PreparedDisposition::Fast,
            base,
            commit,
            None,
            VerificationSource::CandidatePrepared {
                key: ALEPH,
                generation: GenerationId(0),
            },
        )
    }

    fn prepared_record(fold: &TopologyFold) -> PreparedCandidate {
        fold.task(ALEPH)
            .and_then(|task| task.generations.first())
            .and_then(|generation| generation.candidate.clone())
            .expect("a candidate-side witness leg prepares a candidate")
    }

    fn replayed(trace: &[TopologyEvent]) -> TopologyFold {
        TopologyFold::replay(inputs(), trace)
            .unwrap_or_else(|error| panic!("a witness trace does not replay: {error}"))
    }

    enum WitnessShape {
        OneField { from: String, to: String },
        OneLabel { from: String, to: String },
        OneRegion,
        OneAppend,
        Reordered,
    }

    enum RecordedOperand {
        Base,
        Commit,
    }

    impl RecordedOperand {
        fn copied(
            &self,
            from: &PreparedCandidate,
            mut into: PreparedCandidate,
        ) -> PreparedCandidate {
            match self {
                Self::Base => into.base_sha = from.base_sha.clone(),
                Self::Commit => into.candidate.commit_sha = from.candidate.commit_sha.clone(),
            }
            into
        }
    }

    struct RelationWitness {
        relation: &'static str,
        left: Vec<TopologyEvent>,
        right: Vec<TopologyEvent>,
        shape: WitnessShape,
        opposed: Option<(TopologyEvent, TopologyEvent)>,
        recorded: Option<RecordedOperand>,
    }

    fn abstraction_witnesses() -> Vec<RelationWitness> {
        let base = queued_candidate_trace(region(ALEPH));
        let verification = |pin: &str, head: CommitSha, proposed: CommitSha| {
            let mut trace = base.clone();
            trace.push(verification_started(0, ALEPH, 0, pin, head, proposed));
            trace
        };
        let deferred = {
            let mut trace = verification("prepared/0", sha("moved-head"), sha("proposal-aleph"));
            trace.push(verification_deferred_by_outage(0, 1));
            trace
        };
        let mut woken = deferred.clone();
        woken.push(ev(TopologyEventBody::DeferWaitElapsed {
            data: DeferWaitElapsed4 {
                waited_ms: 30_000,
                round: 1,
            },
        }));

        let mut aleph_first = vec![run_started_event()];
        let mut bet_first = vec![run_started_event()];
        for key in [ALEPH, BET] {
            let mut fold = started();
            let mut leg = Vec::new();
            for event in [
                dispatch(key, 0),
                attempt_started(&fold, key, 0, 1),
                candidate_prepared(key, 0, 1),
                candidate_created(key, 0),
            ] {
                if let Ok(delta) = fold.plan_transition(&event) {
                    fold.apply_delta(delta);
                }
                leg.push(event);
            }
            if key == ALEPH {
                aleph_first.splice(1..1, leg.clone());
                bet_first.extend(leg);
            } else {
                aleph_first.extend(leg.clone());
                bet_first.splice(1..1, leg);
            }
        }

        let shared_commit = candidate_of(ALEPH, 0).commit_sha;
        let base_a = sha("candidate-base-a");
        let base_b = sha("candidate-base-b");
        let commit_one = sha("candidate-commit-one");
        let commit_two = sha("candidate-commit-two");

        vec![
            RelationWitness {
                relation: "the region a candidate's lease holds (A versus AB)",
                left: base.clone(),
                right: queued_candidate_trace(overlap_region()),
                shape: WitnessShape::OneRegion,
                opposed: None,
                recorded: None,
            },
            RelationWitness {
                relation: "merge_prepared: expected_head",
                left: verification("prepared/0", sha("moved-head"), sha("proposal-aleph")),
                right: verification("prepared/0", sha("other-head"), sha("proposal-aleph")),
                shape: WitnessShape::OneField {
                    from: sha("moved-head").0,
                    to: sha("other-head").0,
                },
                opposed: None,
                recorded: None,
            },
            RelationWitness {
                relation: "merge_prepared: proposed_sha",
                left: verification("prepared/0", sha("moved-head"), sha("proposal-aleph")),
                right: verification("prepared/0", sha("moved-head"), sha("other-proposal")),
                shape: WitnessShape::OneField {
                    from: sha("proposal-aleph").0,
                    to: sha("other-proposal").0,
                },
                opposed: None,
                recorded: None,
            },
            RelationWitness {
                relation: "merge_prepared: the pinned proposal ref",
                left: verification("prepared/0", sha("moved-head"), sha("proposal-aleph")),
                right: verification("prepared/9", sha("moved-head"), sha("proposal-aleph")),
                shape: WitnessShape::OneField {
                    from: git_ref("prepared/0").0,
                    to: git_ref("prepared/9").0,
                },
                opposed: None,
                recorded: None,
            },
            RelationWitness {
                relation: "verification_deferred on a queued candidate",
                left: deferred,
                right: woken,
                shape: WitnessShape::OneAppend,
                opposed: None,
                recorded: None,
            },
            RelationWitness {
                relation: "the queue's order",
                left: aleph_first,
                right: bet_first,
                shape: WitnessShape::Reordered,
                opposed: None,
                recorded: None,
            },
            RelationWitness {
                relation: "merge_prepared: the candidate's own base label",
                left: queued_candidate_at(base_a.clone(), shared_commit.clone()),
                right: queued_candidate_at(base_b.clone(), shared_commit.clone()),
                shape: WitnessShape::OneLabel {
                    from: base_a.0.clone(),
                    to: base_b.0.clone(),
                },
                opposed: Some((
                    fast_publication(base_a, shared_commit.clone()),
                    fast_publication(base_b, shared_commit),
                )),
                recorded: Some(RecordedOperand::Base),
            },
            RelationWitness {
                relation: "merge_prepared: the candidate's own commit label",
                left: queued_candidate_at(sha("base"), commit_one.clone()),
                right: queued_candidate_at(sha("base"), commit_two.clone()),
                shape: WitnessShape::OneLabel {
                    from: commit_one.0.clone(),
                    to: commit_two.0.clone(),
                },
                opposed: Some((
                    fast_publication(sha("base"), commit_one),
                    fast_publication(sha("base"), commit_two),
                )),
                recorded: Some(RecordedOperand::Commit),
            },
        ]
    }

    #[test]
    fn the_abstraction_key_separates_states_that_differ_in_one_retained_relation() {
        let witnesses = abstraction_witnesses();
        assert!(witnesses.len() >= 8);
        for witness in &witnesses {
            let left = replayed(&witness.left);
            let right = replayed(&witness.right);
            let name = witness.relation;

            match &witness.shape {
                WitnessShape::OneField { from, to } => {
                    assert_eq!(witness.left.len(), witness.right.len(), "{name}");
                    let differing: Vec<usize> = (0..witness.left.len())
                        .filter(|index| witness.left[*index] != witness.right[*index])
                        .collect();
                    assert_eq!(differing.len(), 1, "{name}: not one event");
                    let index = differing[0];
                    let before = format!("{:?}", witness.left[index].body);
                    let after = format!("{:?}", witness.right[index].body);
                    assert_ne!(before, after, "{name}");
                    assert_eq!(
                        before.replace(from.as_str(), to.as_str()),
                        after,
                        "{name}: more than one field moved"
                    );
                }
                WitnessShape::OneLabel { from, to } => {
                    assert_eq!(witness.left.len(), witness.right.len(), "{name}");
                    let differing = (0..witness.left.len())
                        .filter(|index| witness.left[*index] != witness.right[*index])
                        .count();
                    assert!(
                        differing > 1,
                        "{name}: one event moved, so `OneField` is the honest shape and the \
                         stricter check"
                    );
                    let rendered = |trace: &[TopologyEvent]| {
                        trace
                            .iter()
                            .map(|event| format!("{:?}", event.body))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    assert_eq!(
                        rendered(&witness.left).replace(from.as_str(), to.as_str()),
                        rendered(&witness.right),
                        "{name}: more than one label moved"
                    );
                }
                WitnessShape::OneRegion => {
                    assert_eq!(witness.left.len(), witness.right.len(), "{name}");
                    let differing = (0..witness.left.len())
                        .filter(|index| witness.left[*index] != witness.right[*index])
                        .count();
                    assert_eq!(differing, 1, "{name}: not one event");
                }
                WitnessShape::OneAppend => {
                    assert_eq!(witness.right.len(), witness.left.len() + 1, "{name}");
                    assert_eq!(
                        witness.right[..witness.left.len()],
                        witness.left[..],
                        "{name}"
                    );
                }
                WitnessShape::Reordered => {
                    assert_ne!(witness.left, witness.right, "{name}");
                    let sorted = |trace: &[TopologyEvent]| {
                        let mut rendered: Vec<String> = trace
                            .iter()
                            .map(|event| format!("{:?}", event.body))
                            .collect();
                        rendered.sort();
                        rendered
                    };
                    assert_eq!(sorted(&witness.left), sorted(&witness.right), "{name}");
                }
            }

            if let Some(operand) = &witness.recorded {
                let (kept_left, kept_right) = (prepared_record(&left), prepared_record(&right));
                assert_ne!(
                    kept_left, kept_right,
                    "{name}: the two legs record the same candidate"
                );
                assert_eq!(
                    operand.copied(&kept_left, kept_right),
                    kept_left,
                    "{name}: more than one recorded field moved"
                );
            }

            assert!(
                left.state() != right.state(),
                "{name}: the witness pair is one state, so it witnesses nothing"
            );
            assert_ne!(
                fingerprint(&left),
                fingerprint(&right),
                "the key does not read {name}"
            );

            if let Some((for_left, for_right)) = &witness.opposed {
                assert!(
                    left.plan_transition(for_left).is_ok(),
                    "{name}: the publication built from the left leg's own labels is refused \
                     there: {:?}",
                    left.plan_transition(for_left).err()
                );
                assert!(
                    right.plan_transition(for_left).is_err(),
                    "{name}: the left leg's publication is accepted at the right leg too, so the \
                     two states answer it alike"
                );
                assert!(
                    right.plan_transition(for_right).is_ok(),
                    "{name}: the publication built from the right leg's own labels is refused \
                     there: {:?}",
                    right.plan_transition(for_right).err()
                );
                assert!(
                    left.plan_transition(for_right).is_err(),
                    "{name}: the right leg's publication is accepted at the left leg too, so the \
                     two states answer it alike"
                );
            }
        }
        let named: BTreeSet<&str> = witnesses.iter().map(|witness| witness.relation).collect();
        assert_eq!(named.len(), witnesses.len());
        assert_eq!(
            witnesses
                .iter()
                .filter(|witness| witness.opposed.is_some() && witness.recorded.is_some())
                .count(),
            2,
            "the candidate's base and the candidate's commit each owe an opposed publication"
        );
    }

    fn overlap_classes(fold: &TopologyFold) -> Vec<Candidate> {
        vec![
            Candidate::new("task_dispatched/aleph/g0", dispatch(ALEPH, 0)),
            Candidate::new(
                "attempt_started/aleph/g0/a1",
                attempt_started(fold, ALEPH, 0, 1),
            ),
            Candidate::new(
                "attempt_finished/succeeded/aleph/g0/a1",
                settle(
                    ALEPH,
                    0,
                    1,
                    SettlementTransition::Succeeded,
                    LeaseDisposition::PredictedRetained,
                ),
            ),
            Candidate::new(
                "candidate_prepared/region-a/aleph/g0/a1",
                candidate_prepared_over(ALEPH, 0, 1, region(ALEPH)),
            ),
            Candidate::new(
                "candidate_prepared/region-ab/aleph/g0/a1",
                candidate_prepared_over(ALEPH, 0, 1, overlap_region()),
            ),
            Candidate::new(
                "task_candidate_created/aleph/g0",
                candidate_created(ALEPH, 0),
            ),
            Candidate::new(
                "merge_verification_started/aleph/g0",
                verification_started(
                    0,
                    ALEPH,
                    0,
                    "prepared/0",
                    sha("moved-head"),
                    sha("proposal-aleph"),
                ),
            ),
            Candidate::new(
                "merge_verification_unavailable/parked",
                verification_parked(0, ALEPH, "q-overlap-park"),
            ),
            Candidate::new(
                "run_finished/Parked",
                run_finished(fold, RunOutcome::Parked),
            ),
        ]
    }

    #[test]
    fn an_overlapping_region_is_explored_and_changes_a_transition_answer() {
        let census = Census::explore(
            started(),
            vec![run_started_event()],
            CensusBounds::default(),
            overlap_classes,
        );
        assert!(!census.truncated());

        let parked: Vec<&CensusState> = census
            .states()
            .iter()
            .filter(|state| {
                state
                    .fold
                    .open_questions()
                    .is_some_and(|open| !open.is_empty())
                    && state.fold.transaction().is_none()
                    && state.fold.finished().is_none()
            })
            .collect();
        assert_eq!(
            parked.len(),
            2,
            "A and AB reached {} parked state(s), not two",
            parked.len()
        );

        let holds_bet = |state: &CensusState| {
            state.trace.iter().any(|event| match &event.body {
                TopologyEventBody::CandidatePrepared { data } => {
                    data.actual_paths == overlap_region()
                }
                _ => false,
            })
        };
        let wide = parked
            .iter()
            .find(|state| holds_bet(state))
            .expect("one parked state took region AB");
        let narrow = parked
            .iter()
            .find(|state| !holds_bet(state))
            .expect("one parked state took region A");

        assert_eq!(wide.trace.len(), narrow.trace.len());
        let differing: Vec<usize> = (0..wide.trace.len())
            .filter(|index| wide.trace[*index] != narrow.trace[*index])
            .collect();
        assert_eq!(differing, vec![3], "more than the region moved");

        assert_ne!(wide.id, narrow.id);
        assert_eq!(narrow.outcome, DerivedOutcome::NotEnding);
        assert_eq!(wide.outcome, DerivedOutcome::Ending(RunOutcome::Parked));
        let answer = |state: &CensusState| {
            census
                .outgoing(state.id)
                .find(|transition| transition.label == "run_finished/Parked")
                .map(|transition| matches!(transition.outcome, TransitionOutcome::Accepted { .. }))
                .unwrap_or_else(|| panic!("state {} never offered run_finished", state.id))
        };
        assert!(!answer(narrow), "region A leaves bet dispatchable");
        assert!(answer(wide), "region AB blocks bet and the run parks");
        assert_ne!(region(ALEPH), overlap_region());
        assert_ne!(region(BET), overlap_region());
    }

    fn generated_by_the_classes() -> (BTreeSet<u32>, BTreeSet<u32>, BTreeSet<String>) {
        let mut generations = BTreeSet::new();
        let mut attempts = BTreeSet::new();
        let mut questions = BTreeSet::new();
        for candidate in classes(&started()) {
            match &candidate.event.body {
                TopologyEventBody::TaskDispatched { data } => {
                    generations.insert(data.generation.0);
                }
                TopologyEventBody::AttemptStarted { data } => {
                    generations.insert(data.generation.0);
                    attempts.insert(data.attempt.0);
                }
                TopologyEventBody::AttemptFinished { data } => {
                    generations.insert(data.generation.0);
                    attempts.insert(data.attempt.0);
                    if let AttemptSettlement::Closed {
                        transition: SettlementTransition::Parked { question },
                        ..
                    } = &data.settlement
                    {
                        questions.insert(question.id.to_string());
                    }
                }
                TopologyEventBody::CandidatePrepared { data } => {
                    generations.insert(data.generation.0);
                    attempts.insert(data.attempt.attempt);
                }
                TopologyEventBody::GenerationClosed { data } => {
                    generations.insert(data.generation.0);
                }
                _ => {}
            }
        }
        (generations, attempts, questions)
    }

    #[test]
    fn every_declared_dimension_reports_what_the_fixture_generated() {
        let bounds = CensusBounds::default();
        let census = census();
        let (generations, attempts, question_ids) = generated_by_the_classes();

        let open_questions = census
            .states()
            .iter()
            .filter_map(|state| state.fold.open_questions().map(BTreeMap::len))
            .max()
            .unwrap_or(0);
        let mut sequences = BTreeSet::new();
        let mut defers = 0;
        for state in census.states() {
            if let Some(transaction) = state.fold.transaction() {
                sequences.insert(transaction.sequence.0);
            }
            if let Some(queue) = state.fold.queue() {
                for entry in queue.entries() {
                    defers = defers.max(entry.defers);
                }
            }
        }
        let originals = u32::try_from(started().registry().expect("started").len()).unwrap_or(0);
        let repairs = u32::try_from(
            census
                .transitions()
                .iter()
                .filter(|transition| transition.label.starts_with("task_spawned/"))
                .count(),
        )
        .unwrap_or(0);
        let resumes = u32::try_from(
            census
                .transitions()
                .iter()
                .filter(|transition| transition.label.starts_with("run_resumed"))
                .count(),
        )
        .unwrap_or(0);

        let generated: BTreeMap<&str, u32> = [
            ("originals", originals),
            ("repairs", repairs),
            (
                "generations_per_task",
                u32::try_from(generations.len()).unwrap_or(0),
            ),
            (
                "attempts_per_generation",
                attempts.iter().copied().max().unwrap_or(0),
            ),
            ("sequences", u32::try_from(sequences.len()).unwrap_or(0)),
            ("defers", defers),
            ("questions", u32::try_from(open_questions).unwrap_or(0)),
            ("resumes", resumes),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            bounds
                .dimensions()
                .iter()
                .map(|(name, _)| *name)
                .collect::<BTreeSet<_>>(),
            generated.keys().copied().collect::<BTreeSet<_>>()
        );
        let rendered = format!("{bounds:#?}");
        let fields: BTreeSet<&str> = rendered
            .lines()
            .filter_map(|line| line.trim().split_once(':'))
            .map(|(name, _)| name)
            .filter(|name| *name != "max_trace" && *name != "max_states")
            .collect();
        assert_eq!(
            fields,
            bounds
                .dimensions()
                .iter()
                .map(|(name, _)| *name)
                .collect::<BTreeSet<_>>(),
            "a bound the struct declares and `dimensions()` does not"
        );

        let at_maximum = [
            "attempts_per_generation",
            "generations_per_task",
            "questions",
        ];
        let below_maximum = ["originals", "repairs", "sequences", "defers", "resumes"];
        assert_eq!(
            at_maximum.len() + below_maximum.len(),
            bounds.dimensions().len(),
            "a declared dimension is in neither list"
        );
        for (name, declared) in bounds.dimensions() {
            let made = generated[name];
            assert!(
                made <= declared,
                "{name}: the fixture generated {made} and the census declares {declared}"
            );
            if at_maximum.contains(&name) {
                assert_eq!(
                    made, declared,
                    "{name}: declared {declared} and generated {made}; a boundary this skeleton \
                     did not generate is not evidence it explored"
                );
            } else {
                assert!(
                    below_maximum.contains(&name),
                    "{name} is classified twice or not at all"
                );
                assert!(
                    made < declared,
                    "{name}: generated {made} of {declared}, so it belongs in the other list"
                );
            }
        }

        assert_eq!(attempts, BTreeSet::from([1, 2]));
        assert!(!attempts.contains(&(bounds.attempts_per_generation + 1)));
        assert_eq!(generations, BTreeSet::from([0, 1]));
        assert!(!generations.contains(&bounds.generations_per_task));
        assert!(
            open_questions <= usize::try_from(bounds.questions).unwrap_or(usize::MAX),
            "{open_questions} questions were open at once"
        );
        assert_eq!(question_ids.len(), 4);
    }

    fn merge_prepared_of(label: &str) -> MergePrepared {
        let candidate = classes(&started())
            .into_iter()
            .find(|candidate| candidate.label == label)
            .unwrap_or_else(|| panic!("the classes offer no `{label}`"));
        match candidate.event.body {
            TopologyEventBody::MergePrepared { data } => *data,
            other => panic!("`{label}` is a {:?}", other.kind()),
        }
    }

    fn merge_prepared_diff(left: &MergePrepared, right: &MergePrepared) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (name, differs) in [
            ("sequence", left.sequence != right.sequence),
            ("disposition", left.disposition != right.disposition),
            ("expected_head", left.expected_head != right.expected_head),
            ("proposed_sha", left.proposed_sha != right.proposed_sha),
            ("key", left.key != right.key),
            ("generation", left.generation != right.generation),
            ("candidate_sha", left.candidate_sha != right.candidate_sha),
            ("candidate_ref", left.candidate_ref != right.candidate_ref),
            ("prepared_ref", left.prepared_ref != right.prepared_ref),
            (
                "verification_source",
                left.verification_source != right.verification_source,
            ),
            ("verification", left.verification != right.verification),
            ("satisfies", left.satisfies != right.satisfies),
        ] {
            if differs {
                out.push(name);
            }
        }
        out
    }

    #[test]
    fn every_publication_negative_differs_from_its_positive_in_exactly_one_field() {
        let fast = merge_prepared_of("merge_prepared/fast/match/aleph/g0");
        let stale = merge_prepared_of("merge_prepared/stale_clean/match/aleph/g0");
        let present = merge_prepared_of("merge_prepared/already_present/match/aleph/g0");

        let rendered = format!("{fast:#?}");
        let fields: BTreeSet<&str> = rendered
            .lines()
            .filter_map(|line| line.trim().split_once(':'))
            .map(|(name, _)| name)
            .filter(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            })
            .collect();
        let mut every = fast.clone();
        every.sequence = SequenceId(97);
        every.disposition = PreparedDisposition::AlreadyPresent;
        every.expected_head = sha("nothing-alike");
        every.proposed_sha = sha("nothing-alike-either");
        every.key = BET;
        every.generation = GenerationId(9);
        every.candidate_sha = sha("nor-this");
        every.candidate_ref = git_ref("nor/this");
        every.prepared_ref = Some(git_ref("nor/that"));
        every.verification_source = VerificationSource::Verification {
            sequence: SequenceId(97),
        };
        every.verification = Some(VerificationRecord {
            verdict: VerificationVerdict::Passed,
            gates_passed: false,
            reviews: Vec::new(),
            detail: "different".to_owned(),
        });
        every.satisfies = vec![BET, ALEPH];
        assert_eq!(
            merge_prepared_diff(&fast, &every)
                .into_iter()
                .collect::<BTreeSet<_>>(),
            fields,
            "the field-by-field diff and the record's own fields are not the same list"
        );

        let census = census();
        let accepted = census.accepted_labels();
        let refused = census.refused_labels();
        for (positive, label, field) in [
            (
                &fast,
                "merge_prepared/fast/moved-head/aleph/g0",
                "expected_head",
            ),
            (
                &fast,
                "merge_prepared/fast/other-proposed/aleph/g0",
                "proposed_sha",
            ),
            (
                &fast,
                "merge_prepared/fast/with-pin/aleph/g0",
                "prepared_ref",
            ),
            (
                &stale,
                "merge_prepared/stale_clean/mismatch/aleph/g0",
                "proposed_sha",
            ),
            (
                &present,
                "merge_prepared/already_present/mismatch/aleph/g0",
                "proposed_sha",
            ),
        ] {
            let negative = merge_prepared_of(label);
            assert_eq!(
                merge_prepared_diff(positive, &negative),
                vec![field],
                "`{label}` is not its positive with one field moved"
            );
            assert!(refused.contains(label), "`{label}` was never refused");
            assert!(
                !accepted.contains(label),
                "`{label}` was accepted somewhere"
            );
        }
        for positive in [
            "merge_prepared/fast/match/aleph/g0",
            "merge_prepared/stale_clean/match/aleph/g0",
            "merge_prepared/already_present/match/aleph/g0",
        ] {
            assert!(
                accepted.contains(positive),
                "`{positive}` was never accepted"
            );
        }
    }
}
