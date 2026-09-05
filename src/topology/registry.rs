//! Extended notes: `docs/internals/topology/registry.md`

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::events::{ChainSummary, RunStarted};
use crate::ir::{ArtifactId, Plan, ResolvedEffortPolicy, Task, TaskId, TaskKind, Tier};
use crate::review::PassBinding;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct TaskKey(pub u32);

impl TaskKey {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for TaskKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Original,
    MergeRepair,
}

impl Origin {
    fn tag(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::MergeRepair => "merge-repair",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lineage {
    pub root: TaskKey,
    pub parent: TaskKey,
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenRung {
    pub tier: Tier,
    pub agent: String,
    pub model: String,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Admission {
    Runnable,
    HumanBinding { options: Vec<String> },
}

impl Admission {
    fn tag(&self) -> &'static str {
        match self {
            Self::Runnable => "runnable",
            Self::HumanBinding { .. } => "human-binding",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenLadder {
    pub tiers: Vec<Tier>,
    pub attempts_per: u32,
    pub rungs: Vec<FrozenRung>,
    #[serde(deserialize_with = "crate::topology::events::strict::required")]
    pub floor: Option<Tier>,
    #[serde(deserialize_with = "crate::topology::events::strict::required")]
    pub ceiling: Option<Tier>,
    #[serde(deserialize_with = "crate::topology::events::strict::field")]
    pub effort: ResolvedEffortPolicy,
    pub admission: Admission,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenTaskSpec {
    pub kind: TaskKind,
    pub title: String,
    pub body: String,
    pub acceptance: Vec<String>,
    pub path_hints: Vec<String>,
    #[serde(deserialize_with = "crate::topology::events::strict::required")]
    pub suggested_tier: Option<Tier>,
    #[serde(deserialize_with = "crate::topology::events::strict::required")]
    pub min_tier: Option<Tier>,
    pub artifacts_in: Vec<ArtifactId>,
    pub artifacts_out: Vec<ArtifactId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenReviews {
    pub enabled: bool,
    pub alternative_available: bool,
    pub pass_timeout_secs: u64,
    #[serde(deserialize_with = "crate::topology::events::strict::optional")]
    pub primary: Option<PassBinding>,
    #[serde(deserialize_with = "crate::topology::events::strict::optional")]
    pub alternative: Option<PassBinding>,
    #[serde(deserialize_with = "crate::topology::events::strict::optional")]
    pub second_opinion: Option<PassBinding>,
}

impl FrozenReviews {
    #[must_use]
    pub fn bindings(&self) -> Option<crate::review::ReviewBindings<'_>> {
        self.enabled.then_some(crate::review::ReviewBindings {
            primary: self.primary.as_ref(),
            alternative: self.alternative.as_ref(),
            second_opinion: self.second_opinion.as_ref(),
        })
    }

    #[must_use]
    pub fn obliged_lenses(&self) -> Vec<crate::review::Lens> {
        self.bindings()
            .map(crate::review::obliged_lenses)
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEntry {
    pub key: TaskKey,
    pub display_id: TaskId,
    pub origin: Origin,
    pub spec: FrozenTaskSpec,
    pub deps: Vec<TaskKey>,
    pub display_deps: Vec<TaskId>,
    pub ladder: FrozenLadder,
    pub reviews: FrozenReviews,
    pub allowed_agents: Vec<String>,
    #[serde(deserialize_with = "crate::topology::events::strict::required")]
    pub lineage: Option<Lineage>,
}

impl TaskEntry {
    pub fn legacy_task(&self) -> Task {
        Task {
            id: self.display_id.clone(),
            kind: self.spec.kind,
            title: self.spec.title.clone(),
            body: self.spec.body.clone(),
            depends_on: self.display_deps.clone(),
            acceptance: self.spec.acceptance.clone(),
            path_hints: self.spec.path_hints.clone(),
            suggested_tier: self.spec.suggested_tier,
            min_tier: self.spec.min_tier,
            artifacts_in: self.spec.artifacts_in.clone(),
            artifacts_out: self.spec.artifacts_out.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRegistry {
    entries: Vec<TaskEntry>,
    by_display: BTreeMap<String, TaskKey>,
    originals: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error("duplicate task id `{id}` in the frozen plan; a display id names exactly one task")]
    DuplicateDisplayId { id: String },

    #[error(
        "task `{id}` sits in the id namespace reserved for merge repairs \
         (`merge-fix-<index>-<task>`); a plan may not name a task the merge queue could generate"
    )]
    ReservedDisplayId { id: String },

    #[error("task `{task}` depends on unknown id `{dep}`")]
    UnknownDependency { task: String, dep: String },

    #[error("run-start chain task `{task}` is absent from the frozen plan")]
    ChainWithoutTask { task: String },

    #[error("duplicate run-start chain for task `{task}`")]
    DuplicateChain { task: String },

    #[error("frozen-plan task `{task}` has no run-start chain")]
    TaskWithoutChain { task: String },

    #[error(
        "the recorded chain for task `{task}` has no rungs; an original's ladder is the one its \
         run resolved, and a run that resolved nothing recorded no way to admit the task either"
    )]
    EmptyLadder { task: String },

    #[error("the recorded chain for task `{task}` allows 0 attempts per rung")]
    ZeroAttempts { task: String },

    #[error(
        "the recorded chain for task `{task}` has {bindings} binding(s) for {tiers} rung(s); the \
         event log cannot say which model belongs to which rung"
    )]
    BindingCount {
        task: String,
        bindings: usize,
        tiers: usize,
    },

    #[error("the recorded chain for task `{task}` assigns tier `{binding}` to a `{tier}` rung")]
    BindingTier {
        task: String,
        tier: Tier,
        binding: Tier,
    },

    #[error(
        "this run's record has no {field}; a registry is derived from what the run itself froze, \
         and a record that never froze it cannot authenticate one"
    )]
    IncompleteRunRecord { field: &'static str },

    #[error(
        "this run records {recorded} second-opinion slot(s) for {tasks} task(s); a misaligned \
         review identity would give some task another task's reviewer"
    )]
    ReviewAlignment { recorded: usize, tasks: usize },

    #[error("a plan with {tasks} tasks exceeds what a dense TaskKey can address")]
    TooManyTasks { tasks: usize },

    #[error("registry digest `{actual}` does not match the recorded digest `{expected}`")]
    DigestMismatch { expected: String, actual: String },
}

const REPAIR_PREFIX: &str = "merge-fix-";

const REPAIR_INDEX_WIDTH: usize = 4;

pub fn repair_display_id(lineage_index: u32, root: &TaskId) -> String {
    format!(
        "{REPAIR_PREFIX}{lineage_index:0width$}-{root}",
        width = REPAIR_INDEX_WIDTH
    )
}

pub fn is_reserved_display_id(id: &str) -> bool {
    let Some((head, rest)) = id.split_at_checked(REPAIR_PREFIX.len()) else {
        return false;
    };
    if !head.eq_ignore_ascii_case(REPAIR_PREFIX) {
        return false;
    }
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits >= REPAIR_INDEX_WIDTH && rest.as_bytes().get(digits) == Some(&b'-')
}

impl TaskRegistry {
    pub fn originals(plan: &Plan, started: &RunStarted) -> Result<Self, RegistryError> {
        Self::originals_with_agents(plan, started, &[])
    }

    pub fn originals_with_agents(
        plan: &Plan,
        started: &RunStarted,
        probed_agents: &[String],
    ) -> Result<Self, RegistryError> {
        let effort = started
            .effort_policy
            .ok_or(RegistryError::IncompleteRunRecord {
                field: "effort policy",
            })?;
        let reviews = started
            .reviews
            .as_ref()
            .ok_or(RegistryError::IncompleteRunRecord {
                field: "review plan",
            })?;
        let enabled = reviews.enabled.ok_or(RegistryError::IncompleteRunRecord {
            field: "reviews.enabled marker",
        })?;
        let alternative_available =
            reviews
                .alternative_available
                .ok_or(RegistryError::IncompleteRunRecord {
                    field: "reviews.alternative_available marker",
                })?;
        let pass_timeout_secs =
            reviews
                .pass_timeout_secs
                .ok_or(RegistryError::IncompleteRunRecord {
                    field: "per-pass review timeout",
                })?;
        if enabled && reviews.second_opinion.len() != plan.tasks.len() {
            return Err(RegistryError::ReviewAlignment {
                recorded: reviews.second_opinion.len(),
                tasks: plan.tasks.len(),
            });
        }

        let by_display = keys_by_display_id(plan)?;
        let chains = chains_by_task(&started.chains, &by_display)?;

        let mut entries = Vec::with_capacity(plan.tasks.len());
        for (index, task) in plan.tasks.iter().enumerate() {
            let key = TaskKey(index_key(index, plan.tasks.len())?);
            let chain =
                *chains
                    .get(task.id.as_str())
                    .ok_or_else(|| RegistryError::TaskWithoutChain {
                        task: task.id.to_string(),
                    })?;
            let mut deps = Vec::with_capacity(task.depends_on.len());
            for dep in &task.depends_on {
                deps.push(*by_display.get(dep.as_str()).ok_or_else(|| {
                    RegistryError::UnknownDependency {
                        task: task.id.to_string(),
                        dep: dep.to_string(),
                    }
                })?);
            }
            entries.push(TaskEntry {
                key,
                display_id: task.id.clone(),
                origin: Origin::Original,
                spec: FrozenTaskSpec {
                    kind: task.kind,
                    title: task.title.clone(),
                    body: task.body.clone(),
                    acceptance: task.acceptance.clone(),
                    path_hints: task.path_hints.clone(),
                    suggested_tier: task.suggested_tier,
                    min_tier: task.min_tier,
                    artifacts_in: task.artifacts_in.clone(),
                    artifacts_out: task.artifacts_out.clone(),
                },
                deps,
                display_deps: task.depends_on.clone(),
                ladder: frozen_ladder(task, chain, effort)?,
                reviews: FrozenReviews {
                    enabled,
                    alternative_available,
                    pass_timeout_secs,
                    primary: reviews.primary.clone(),
                    alternative: reviews.alternative.clone(),
                    second_opinion: reviews.second_opinion.get(index).cloned().flatten(),
                },
                allowed_agents: probed_agents.to_vec(),
                lineage: None,
            });
        }

        Ok(Self {
            originals: entries.len(),
            entries,
            by_display,
        })
    }

    pub fn register(&mut self, entry: TaskEntry) {
        self.by_display
            .insert(entry.display_id.to_string(), entry.key);
        self.entries.push(entry);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[TaskEntry] {
        &self.entries
    }

    pub fn get(&self, key: TaskKey) -> Option<&TaskEntry> {
        self.entries.get(key.index())
    }

    pub fn key_of(&self, display_id: &str) -> Option<TaskKey> {
        self.by_display.get(display_id).copied()
    }

    pub fn legacy_tasks(&self) -> Vec<Task> {
        self.entries.iter().map(TaskEntry::legacy_task).collect()
    }

    pub fn originals_len(&self) -> usize {
        self.originals
    }

    pub fn digest(&self) -> String {
        format!(
            "sha256:{:x}",
            Sha256::digest(self.encode(self.originals.min(self.entries.len())))
        )
    }

    pub fn verify_digest(&self, recorded: &str) -> Result<(), RegistryError> {
        let actual = self.digest();
        if actual == recorded {
            return Ok(());
        }
        Err(RegistryError::DigestMismatch {
            expected: recorded.to_owned(),
            actual,
        })
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        self.encode(self.entries.len())
    }

    fn encode(&self, entries: usize) -> Vec<u8> {
        let entries = &self.entries[..entries.min(self.entries.len())];
        let mut out = Vec::new();
        field(&mut out, "upstroke.registry.v1");
        count(&mut out, entries.len());
        for entry in entries {
            encode_entry(&mut out, entry);
        }
        out
    }
}

fn keys_by_display_id(plan: &Plan) -> Result<BTreeMap<String, TaskKey>, RegistryError> {
    let mut out = BTreeMap::new();
    for (index, task) in plan.tasks.iter().enumerate() {
        if is_reserved_display_id(task.id.as_str()) {
            return Err(RegistryError::ReservedDisplayId {
                id: task.id.to_string(),
            });
        }
        let key = TaskKey(index_key(index, plan.tasks.len())?);
        if out.insert(task.id.to_string(), key).is_some() {
            return Err(RegistryError::DuplicateDisplayId {
                id: task.id.to_string(),
            });
        }
    }
    Ok(out)
}

fn index_key(index: usize, tasks: usize) -> Result<u32, RegistryError> {
    u32::try_from(index).map_err(|_| RegistryError::TooManyTasks { tasks })
}

fn chains_by_task<'a>(
    chains: &'a [ChainSummary],
    by_display: &BTreeMap<String, TaskKey>,
) -> Result<BTreeMap<&'a str, &'a ChainSummary>, RegistryError> {
    let mut out: BTreeMap<&str, &ChainSummary> = BTreeMap::new();
    for chain in chains {
        if !by_display.contains_key(chain.task.as_str()) {
            return Err(RegistryError::ChainWithoutTask {
                task: chain.task.clone(),
            });
        }
        if out.insert(chain.task.as_str(), chain).is_some() {
            return Err(RegistryError::DuplicateChain {
                task: chain.task.clone(),
            });
        }
    }
    for task in by_display.keys() {
        if !out.contains_key(task.as_str()) {
            return Err(RegistryError::TaskWithoutChain { task: task.clone() });
        }
    }
    Ok(out)
}

fn frozen_ladder(
    task: &Task,
    chain: &ChainSummary,
    effort: ResolvedEffortPolicy,
) -> Result<FrozenLadder, RegistryError> {
    if chain.tiers.is_empty() {
        return Err(RegistryError::EmptyLadder {
            task: task.id.to_string(),
        });
    }
    if chain.attempts_per == 0 {
        return Err(RegistryError::ZeroAttempts {
            task: task.id.to_string(),
        });
    }
    let bindings = chain
        .bindings
        .as_ref()
        .ok_or(RegistryError::IncompleteRunRecord {
            field: "resolved rung bindings",
        })?;
    if bindings.len() != chain.tiers.len() {
        return Err(RegistryError::BindingCount {
            task: task.id.to_string(),
            bindings: bindings.len(),
            tiers: chain.tiers.len(),
        });
    }
    let mut rungs = Vec::with_capacity(bindings.len());
    for (tier, binding) in chain.tiers.iter().copied().zip(bindings) {
        if binding.tier != tier {
            return Err(RegistryError::BindingTier {
                task: task.id.to_string(),
                tier,
                binding: binding.tier,
            });
        }
        rungs.push(FrozenRung {
            tier,
            agent: binding.agent.clone(),
            model: binding.model.clone(),
            pinned: binding.pinned,
        });
    }
    Ok(FrozenLadder {
        tiers: chain.tiers.clone(),
        attempts_per: chain.attempts_per,
        rungs,
        floor: task.min_tier,
        ceiling: chain.tiers.iter().copied().max(),
        effort,
        admission: Admission::Runnable,
    })
}

fn field(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(value.len().to_string().as_bytes());
    out.push(b':');
    out.extend_from_slice(value.as_bytes());
    out.push(b';');
}

fn count(out: &mut Vec<u8>, value: usize) {
    field(out, &value.to_string());
}

fn flag(out: &mut Vec<u8>, value: bool) {
    field(out, if value { "1" } else { "0" });
}

fn key(out: &mut Vec<u8>, value: TaskKey) {
    field(out, &value.0.to_string());
}

fn strings(out: &mut Vec<u8>, values: impl ExactSizeIterator<Item = impl AsRef<str>>) {
    count(out, values.len());
    for value in values {
        field(out, value.as_ref());
    }
}

fn optional_tier(out: &mut Vec<u8>, value: Option<Tier>) {
    match value {
        Some(tier) => {
            flag(out, true);
            field(out, &tier.to_string());
        }
        None => flag(out, false),
    }
}

fn optional_binding(out: &mut Vec<u8>, value: Option<&PassBinding>) {
    match value {
        Some(binding) => {
            flag(out, true);
            field(out, &binding.agent);
            field(out, &binding.model);
        }
        None => flag(out, false),
    }
}

fn encode_entry(out: &mut Vec<u8>, entry: &TaskEntry) {
    key(out, entry.key);
    field(out, entry.display_id.as_str());
    field(out, entry.origin.tag());
    match &entry.lineage {
        Some(lineage) => {
            flag(out, true);
            key(out, lineage.root);
            key(out, lineage.parent);
            field(out, &lineage.index.to_string());
        }
        None => flag(out, false),
    }

    let spec = &entry.spec;
    field(out, &spec.kind.to_string());
    field(out, &spec.title);
    field(out, &spec.body);
    strings(out, spec.acceptance.iter());
    strings(out, spec.path_hints.iter());
    optional_tier(out, spec.suggested_tier);
    optional_tier(out, spec.min_tier);
    strings(out, spec.artifacts_in.iter().map(ArtifactId::as_str));
    strings(out, spec.artifacts_out.iter().map(ArtifactId::as_str));

    count(out, entry.deps.len());
    for dep in &entry.deps {
        key(out, *dep);
    }
    strings(out, entry.display_deps.iter().map(TaskId::as_str));

    let ladder = &entry.ladder;
    strings(out, ladder.tiers.iter().map(Tier::to_string));
    field(out, &ladder.attempts_per.to_string());
    count(out, ladder.rungs.len());
    for rung in &ladder.rungs {
        field(out, &rung.tier.to_string());
        field(out, &rung.agent);
        field(out, &rung.model);
        flag(out, rung.pinned);
    }
    optional_tier(out, ladder.floor);
    optional_tier(out, ladder.ceiling);
    field(out, &ladder.effort.small.to_string());
    field(out, &ladder.effort.mid.to_string());
    field(out, &ladder.effort.frontier.to_string());
    field(out, &ladder.effort.review.to_string());
    field(out, ladder.admission.tag());
    match &ladder.admission {
        Admission::Runnable => {}
        Admission::HumanBinding { options } => strings(out, options.iter()),
    }

    let reviews = &entry.reviews;
    flag(out, reviews.enabled);
    flag(out, reviews.alternative_available);
    field(out, &reviews.pass_timeout_secs.to_string());
    optional_binding(out, reviews.primary.as_ref());
    optional_binding(out, reviews.alternative.as_ref());
    optional_binding(out, reviews.second_opinion.as_ref());

    strings(out, entry.allowed_agents.iter());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{
        AttemptRecord, AttemptStarted, AttemptTransition, BindingSummary, Event, EventBody,
        FailureRecord, LadderEscalated, RunFinished, RunOutcome, TaskCommitted,
    };
    use crate::ir::{Artifact, Effort, PlanSource};
    use crate::ladder::{FailureKind, FailureOrigin};
    use crate::review::ReviewPlan;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    const RUN_ID: &str = "01REGISTRY0000000000000000";

    type BreakRecord = fn(&mut RunStarted);

    type MoveInput = fn(&mut Plan, &mut RunStarted, &mut Vec<String>);

    type MoveField = fn(&mut TaskEntry);

    type PermuteTask = fn(&mut Task);

    fn task(id: &str, deps: &[&str]) -> Task {
        Task {
            id: TaskId::from(id),
            kind: TaskKind::Fix,
            title: format!("{id} title"),
            body: format!("{id} body"),
            depends_on: deps.iter().copied().map(TaskId::from).collect(),
            acceptance: vec![format!("{id} passes"), "and keeps passing".to_owned()],
            path_hints: vec![format!("src/{id}.rs"), "src/shared.rs".to_owned()],
            suggested_tier: Some(Tier::Mid),
            min_tier: Some(Tier::Small),
            artifacts_in: vec![ArtifactId::from("contract")],
            artifacts_out: vec![ArtifactId::from(format!("{id}-out").as_str())],
        }
    }

    fn plan_of(tasks: Vec<Task>) -> Plan {
        Plan {
            source: PlanSource {
                adapter: "markdown".to_owned(),
                hash: "frozen-hash".to_owned(),
            },
            tasks,
            artifacts: vec![Artifact {
                id: ArtifactId::from("contract"),
                produced_by: Some(TaskId::from("alpha")),
            }],
        }
    }

    fn sample_plan() -> Plan {
        plan_of(vec![
            task("zeta", &["alpha"]),
            task("alpha", &[]),
            task("mid", &["alpha", "zeta"]),
        ])
    }

    fn chain(task: &str) -> ChainSummary {
        ChainSummary {
            task: task.to_owned(),
            tiers: vec![Tier::Small, Tier::Mid],
            attempts_per: 2,
            bindings: Some(vec![
                BindingSummary {
                    tier: Tier::Small,
                    agent: "claude-code".to_owned(),
                    model: "claude-haiku-4-5".to_owned(),
                    pinned: false,
                },
                BindingSummary {
                    tier: Tier::Mid,
                    agent: "codex".to_owned(),
                    model: "gpt-5.6-sol".to_owned(),
                    pinned: true,
                },
            ]),
        }
    }

    fn sample_effort() -> ResolvedEffortPolicy {
        ResolvedEffortPolicy {
            small: Effort::Low,
            mid: Effort::Medium,
            frontier: Effort::High,
            review: Effort::High,
        }
    }

    fn review_plan(tasks: usize) -> ReviewPlan {
        ReviewPlan {
            enabled: Some(true),
            alternative_available: Some(true),
            pass_timeout_secs: Some(900),
            primary: Some(PassBinding::new("claude-code", "claude-opus-5")),
            alternative: Some(PassBinding::new("copilot", "gpt-5.6")),
            second_opinion: (0..tasks)
                .map(|index| (index % 2 == 1).then(|| PassBinding::new("copilot", "gpt-5.6")))
                .collect(),
        }
    }

    fn started_for(plan: &Plan) -> RunStarted {
        RunStarted {
            schema: 2,
            upstroke_version: "0.1.0".to_owned(),
            run_id: RUN_ID.to_owned(),
            branch: format!("upstroke/run-{RUN_ID}"),
            base_sha: "a".repeat(40),
            plan_path: "plan.md".to_owned(),
            config_path: Some("upstroke.toml".to_owned()),
            plan_hash: plan.source.hash.clone(),
            normalized_plan_digest: None,
            private_dir: "/private/runs".to_owned(),
            gates: vec!["check".to_owned()],
            gates_from_config: true,
            interaction_mode: "never".to_owned(),
            chains: plan.tasks.iter().map(|t| chain(t.id.as_str())).collect(),
            effort_policy: Some(sample_effort()),
            gate_cmds: None,
            reviews: Some(review_plan(plan.tasks.len())),
        }
    }

    fn varied_chain(task: &str) -> ChainSummary {
        let tiers = match task {
            "zeta" => vec![Tier::Small, Tier::Mid, Tier::Frontier],
            "alpha" => vec![Tier::Mid],
            _ => vec![Tier::Small, Tier::Frontier],
        };
        let attempts_per = match task {
            "zeta" => 1,
            "alpha" => 3,
            _ => 5,
        };
        ChainSummary {
            task: task.to_owned(),
            attempts_per,
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

    fn unordered_chain(task: &str) -> ChainSummary {
        let tiers = vec![Tier::Mid, Tier::Frontier, Tier::Small];
        ChainSummary {
            task: task.to_owned(),
            attempts_per: 2,
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

    fn varied_plan() -> Plan {
        let mut plan = sample_plan();
        plan.tasks[0].min_tier = Some(Tier::Small);
        plan.tasks[1].min_tier = None;
        plan.tasks[2].min_tier = Some(Tier::Mid);
        plan
    }

    fn varied_started_for(plan: &Plan) -> RunStarted {
        let mut started = started_for(plan);
        started.chains = ["alpha", "mid", "zeta"]
            .into_iter()
            .map(varied_chain)
            .collect();
        started.reviews = Some(ReviewPlan {
            second_opinion: vec![
                Some(PassBinding::new("zeta-second-agent", "second-for-zeta")),
                None,
                Some(PassBinding::new("mid-second-agent", "second-for-mid")),
            ],
            ..review_plan(plan.tasks.len())
        });
        started
    }

    fn expected_ladder(plan: &Plan, task: &str) -> FrozenLadder {
        let chain = varied_chain(task);
        FrozenLadder {
            rungs: chain
                .bindings
                .expect("the varied fixture records bindings")
                .into_iter()
                .map(|binding| FrozenRung {
                    tier: binding.tier,
                    agent: binding.agent,
                    model: binding.model,
                    pinned: binding.pinned,
                })
                .collect(),
            floor: plan
                .tasks
                .iter()
                .find(|candidate| candidate.id.as_str() == task)
                .expect("the varied fixture's task")
                .min_tier,
            ceiling: chain.tiers.iter().copied().max(),
            attempts_per: chain.attempts_per,
            tiers: chain.tiers,
            effort: sample_effort(),
            admission: Admission::Runnable,
        }
    }

    fn projection_plan() -> Plan {
        let mut plan = sample_plan();
        plan.tasks.push(task("beta", &[]));
        plan.tasks.push(task("gamma", &["beta"]));
        plan
    }

    fn dependency_order_plan() -> Plan {
        plan_of(vec![
            task("zeta", &[]),
            task("echo", &[]),
            task("alpha", &[]),
            task("beta", &[]),
            task("omega", &["zeta", "alpha", "echo", "beta"]),
        ])
    }

    fn artifact_order_plan() -> Plan {
        let mut plan = dependency_order_plan();
        let omega = plan.tasks.last_mut().expect("omega is the last task");
        omega.artifacts_in = ["contract", "api", "schema"]
            .into_iter()
            .map(ArtifactId::from)
            .collect();
        omega.artifacts_out = ["omega-out", "audit", "notes"]
            .into_iter()
            .map(ArtifactId::from)
            .collect();
        plan
    }

    fn sample_agents() -> Vec<String> {
        vec![
            "ÜBER-agent-Ωmega".to_owned(),
            "claude-code".to_owned(),
            "  Codex-CLI  ".to_owned(),
            "a".repeat(300),
            "copilot".to_owned(),
        ]
    }

    fn originals_of(plan: &Plan, started: &RunStarted) -> Result<TaskRegistry, RegistryError> {
        TaskRegistry::originals_with_agents(plan, started, &sample_agents())
    }

    fn registry_of(plan: &Plan) -> TaskRegistry {
        originals_of(plan, &started_for(plan)).expect("the sample record is complete")
    }

    #[test]
    fn keys_are_dense_and_assigned_in_plan_order() {
        let plan = sample_plan();
        let registry = registry_of(&plan);

        let keyed: Vec<(u32, &str)> = registry
            .entries()
            .iter()
            .map(|entry| (entry.key.0, entry.display_id.as_str()))
            .collect();
        assert_eq!(
            keyed,
            vec![(0, "zeta"), (1, "alpha"), (2, "mid")],
            "keys are dense from 0 in plan order — not in display-id order, and not in \
             topological order"
        );

        assert_eq!(registry.len(), 3);
        assert!(!registry.is_empty());
        for (index, entry) in registry.entries().iter().enumerate() {
            assert_eq!(entry.key.index(), index);
            assert_eq!(registry.key_of(entry.display_id.as_str()), Some(entry.key));
            assert_eq!(
                registry.get(entry.key).map(|found| &found.display_id),
                Some(&entry.display_id)
            );
            assert_eq!(entry.origin, Origin::Original);
            assert_eq!(entry.lineage, None);
        }
        assert_eq!(registry.key_of("no-such-task"), None);
        assert_eq!(registry.get(TaskKey(3)), None);
    }

    #[test]
    fn dependencies_are_stored_as_keys_and_projected_as_written() {
        let plan = sample_plan();
        let registry = registry_of(&plan);

        let deps: Vec<(&str, Vec<u32>, Vec<&str>)> = registry
            .entries()
            .iter()
            .map(|entry| {
                (
                    entry.display_id.as_str(),
                    entry.deps.iter().map(|key| key.0).collect(),
                    entry.display_deps.iter().map(TaskId::as_str).collect(),
                )
            })
            .collect();

        assert_eq!(
            deps,
            vec![
                ("zeta", vec![1], vec!["alpha"]),
                ("alpha", vec![], vec![]),
                ("mid", vec![1, 0], vec!["alpha", "zeta"]),
            ]
        );

        let plan = dependency_order_plan();
        let registry = originals_of(&plan, &started_for(&plan))
            .expect("the dependency-order record is complete");
        let omega = registry
            .get(registry.key_of("omega").expect("omega is registered"))
            .expect("omega's entry");

        let written: Vec<&str> = omega.display_deps.iter().map(TaskId::as_str).collect();
        assert_eq!(
            written,
            vec!["zeta", "alpha", "echo", "beta"],
            "display dependencies are the plan's own order, not a sorted one"
        );
        assert_eq!(
            omega.deps,
            vec![TaskKey(0), TaskKey(2), TaskKey(1), TaskKey(3)],
            "and the keys are those ids resolved in place, not a sorted or a positional list"
        );

        for (what, ordered) in [
            ("sorted", {
                let mut sorted = written.clone();
                sorted.sort_unstable();
                sorted
            }),
            ("reverse-sorted", {
                let mut reversed = written.clone();
                reversed.sort_unstable_by(|left, right| right.cmp(left));
                reversed
            }),
        ] {
            assert_ne!(
                written, ordered,
                "the fixture's dependency list must not already be in {what} order"
            );
        }
        let keys: Vec<u32> = omega.deps.iter().map(|key| key.0).collect();
        assert_ne!(keys, vec![0, 1, 2, 3], "nor may the keys already be sorted");
        assert_ne!(keys, vec![3, 2, 1, 0], "nor reverse-sorted");

        for (position, (key, display)) in omega.deps.iter().zip(&omega.display_deps).enumerate() {
            assert_eq!(
                registry.get(*key).map(|entry| &entry.display_id),
                Some(display),
                "the key and the display id at position {position} name different tasks"
            );
        }

        assert_eq!(
            omega
                .legacy_task()
                .depends_on
                .iter()
                .map(TaskId::as_str)
                .collect::<Vec<_>>(),
            written
        );
    }

    #[test]
    fn artifact_lists_keep_the_order_the_plan_wrote_them_in() {
        let plan = artifact_order_plan();
        let registry = originals_of(&plan, &started_for(&plan))
            .expect("the artifact-order record is complete");
        let omega = registry
            .get(registry.key_of("omega").expect("omega is registered"))
            .expect("omega's entry");

        assert_eq!(
            omega
                .spec
                .artifacts_in
                .iter()
                .map(ArtifactId::as_str)
                .collect::<Vec<_>>(),
            vec!["contract", "api", "schema"]
        );
        assert_eq!(
            omega
                .spec
                .artifacts_out
                .iter()
                .map(ArtifactId::as_str)
                .collect::<Vec<_>>(),
            vec!["omega-out", "audit", "notes"]
        );

        for (what, list) in [
            ("artifacts in", &omega.spec.artifacts_in),
            ("artifacts out", &omega.spec.artifacts_out),
        ] {
            let written: Vec<&str> = list.iter().map(ArtifactId::as_str).collect();
            let mut sorted = written.clone();
            sorted.sort_unstable();
            assert_ne!(
                written, sorted,
                "the fixture's {what} list must not already be sorted"
            );
            sorted.reverse();
            assert_ne!(written, sorted, "nor reverse-sorted, for the same reason");
        }

        assert_eq!(
            normalized_bytes(&round_tripped(&plan)),
            normalized_bytes(&plan),
            "an artifact list came back in an order the frozen plan did not write"
        );

        let baseline = registry.canonical_bytes();
        let permutations: [(&str, PermuteTask); 2] = [
            ("artifacts in", |task| task.artifacts_in.swap(0, 1)),
            ("artifacts out", |task| task.artifacts_out.swap(0, 1)),
        ];
        for (what, permute) in permutations {
            let mut moved = artifact_order_plan();
            permute(moved.tasks.last_mut().expect("omega is the last task"));
            let rebuilt = originals_of(&moved, &started_for(&moved))
                .expect("a permuted artifact list still builds");
            assert_ne!(
                rebuilt.canonical_bytes(),
                baseline,
                "permuting {what} left the canonical bytes where they were"
            );
        }
    }

    #[test]
    fn the_frozen_ladder_is_the_chain_the_run_recorded() {
        let plan = sample_plan();
        let registry = registry_of(&plan);

        for entry in registry.entries() {
            let id = &entry.display_id;
            assert_eq!(entry.ladder.tiers, vec![Tier::Small, Tier::Mid], "{id}");
            assert_eq!(entry.ladder.attempts_per, 2, "{id}");
            assert_eq!(
                entry.ladder.rungs,
                vec![
                    FrozenRung {
                        tier: Tier::Small,
                        agent: "claude-code".to_owned(),
                        model: "claude-haiku-4-5".to_owned(),
                        pinned: false,
                    },
                    FrozenRung {
                        tier: Tier::Mid,
                        agent: "codex".to_owned(),
                        model: "gpt-5.6-sol".to_owned(),
                        pinned: true,
                    },
                ],
                "{id}"
            );
            assert_eq!(
                entry.ladder.floor,
                Some(Tier::Small),
                "{id}: the task's min="
            );
            assert_eq!(
                entry.ladder.ceiling,
                Some(Tier::Mid),
                "{id}: the top of the frozen chain, which a repair may not exceed"
            );
            assert_eq!(entry.ladder.admission, Admission::Runnable, "{id}");
            assert_eq!(
                entry.ladder.effort,
                sample_effort(),
                "{id}: the whole resolved standard, not one member of it"
            );
        }
    }

    #[test]
    fn each_entry_takes_the_chain_recorded_for_its_own_display_id() {
        let plan = varied_plan();
        let started = varied_started_for(&plan);
        let registry = originals_of(&plan, &started).expect("the varied record is complete");

        let ladders: Vec<&FrozenLadder> = registry
            .entries()
            .iter()
            .map(|entry| &entry.ladder)
            .collect();
        assert_eq!(ladders.len(), 3);
        for (index, left) in ladders.iter().enumerate() {
            for right in &ladders[index + 1..] {
                assert_ne!(
                    left, right,
                    "the varied fixture must give no two tasks the same ladder"
                );
            }
        }

        for entry in registry.entries() {
            assert_eq!(registry.get(entry.key), Some(entry));
            assert_eq!(registry.key_of(entry.display_id.as_str()), Some(entry.key));
            assert_eq!(
                entry.ladder,
                expected_ladder(&plan, entry.display_id.as_str()),
                "`{}` (key {}) was given a ladder that is not its own",
                entry.display_id,
                entry.key
            );
        }

        let first_chain = started.chains[0].task.as_str();
        for (index, entry) in registry.entries().iter().enumerate() {
            let positional = started.chains[index].task.as_str();
            assert_ne!(
                positional,
                entry.display_id.as_str(),
                "the record's chain order must stay a derangement of plan order"
            );
            assert_ne!(
                entry.ladder,
                expected_ladder(&plan, positional),
                "`{}` must not be satisfiable by the chain at its own index",
                entry.display_id
            );
            if entry.display_id.as_str() != first_chain {
                assert_ne!(
                    entry.ladder,
                    expected_ladder(&plan, first_chain),
                    "`{}` must not be satisfiable by the first recorded chain",
                    entry.display_id
                );
            }
        }
    }

    #[test]
    fn the_ladder_ceiling_is_the_highest_tier_recorded_not_an_end_of_the_list() {
        let plan = plan_of(vec![task("alpha", &[])]);
        let mut started = started_for(&plan);
        started.chains = vec![unordered_chain("alpha")];
        let registry = originals_of(&plan, &started).expect("the unordered record is complete");
        let entry = &registry.entries()[0];

        assert_eq!(
            entry.ladder.tiers,
            vec![Tier::Mid, Tier::Frontier, Tier::Small],
            "the fixture records the top rung in the middle"
        );
        assert_ne!(
            entry.ladder.tiers.first().copied(),
            Some(Tier::Frontier),
            "the first recorded tier must not be the highest, or a ceiling read off the front of \
             the list is indistinguishable from one taken over all of it"
        );
        assert_ne!(
            entry.ladder.tiers.last().copied(),
            Some(Tier::Frontier),
            "nor the last, for the same reason"
        );

        assert_eq!(
            entry.ladder.ceiling,
            Some(Tier::Frontier),
            "the ceiling is the highest tier the ladder reaches — the policy ceiling a repair \
             descended from this entry may not exceed — not whichever tier the record happened \
             to write first or last"
        );
        assert_eq!(
            entry.ladder.floor,
            Some(Tier::Small),
            "the task's min=, not a position in the recorded chain"
        );
    }

    #[test]
    fn frozen_reviews_take_each_task_s_own_second_opinion_slot() {
        let plan = sample_plan();
        let registry = registry_of(&plan);
        let slots: Vec<Option<&str>> = registry
            .entries()
            .iter()
            .map(|entry| {
                entry
                    .reviews
                    .second_opinion
                    .as_ref()
                    .map(|binding| binding.model.as_str())
            })
            .collect();
        assert_eq!(
            slots,
            vec![None, Some("gpt-5.6"), None],
            "slots are read at the task's own index, so a shifted read is visible"
        );
        for entry in registry.entries() {
            assert!(entry.reviews.enabled);
            assert!(entry.reviews.alternative_available);
            assert_eq!(entry.reviews.pass_timeout_secs, 900);
            assert_eq!(
                entry.reviews.primary,
                Some(PassBinding::new("claude-code", "claude-opus-5"))
            );
            assert_eq!(
                entry.reviews.alternative,
                Some(PassBinding::new("copilot", "gpt-5.6"))
            );
        }

        let plan = varied_plan();
        let registry =
            originals_of(&plan, &varied_started_for(&plan)).expect("the varied record is complete");
        let named: Vec<(&str, Option<(&str, &str)>)> = registry
            .entries()
            .iter()
            .map(|entry| {
                (
                    entry.display_id.as_str(),
                    entry
                        .reviews
                        .second_opinion
                        .as_ref()
                        .map(|binding| (binding.agent.as_str(), binding.model.as_str())),
                )
            })
            .collect();
        assert_eq!(
            named,
            vec![
                ("zeta", Some(("zeta-second-agent", "second-for-zeta"))),
                ("alpha", None),
                ("mid", Some(("mid-second-agent", "second-for-mid"))),
            ],
            "each entry holds both components of the slot recorded at its own plan index"
        );

        for entry in registry.entries() {
            assert_eq!(
                entry.reviews.primary,
                Some(PassBinding::new("claude-code", "claude-opus-5"))
            );
            assert_eq!(
                entry.reviews.alternative,
                Some(PassBinding::new("copilot", "gpt-5.6"))
            );
        }
    }

    struct SourceCase {
        label: &'static str,
        mutate: fn(&mut RunStarted),
        restore: fn(&mut TaskEntry, &TaskEntry),
    }

    #[test]
    fn moving_one_recorded_value_moves_exactly_the_entry_field_it_feeds() {
        let cases: [SourceCase; 13] = [
            SourceCase {
                label: "small-tier effort standard",
                mutate: |started| {
                    started.effort_policy.as_mut().expect("effort policy").small = Effort::Max;
                },
                restore: |entry, base| entry.ladder.effort.small = base.ladder.effort.small,
            },
            SourceCase {
                label: "mid-tier effort standard",
                mutate: |started| {
                    started.effort_policy.as_mut().expect("effort policy").mid = Effort::Max;
                },
                restore: |entry, base| entry.ladder.effort.mid = base.ladder.effort.mid,
            },
            SourceCase {
                label: "frontier-tier effort standard",
                mutate: |started| {
                    started
                        .effort_policy
                        .as_mut()
                        .expect("effort policy")
                        .frontier = Effort::Low;
                },
                restore: |entry, base| entry.ladder.effort.frontier = base.ladder.effort.frontier,
            },
            SourceCase {
                label: "review effort standard",
                mutate: |started| {
                    started
                        .effort_policy
                        .as_mut()
                        .expect("effort policy")
                        .review = Effort::Low;
                },
                restore: |entry, base| entry.ladder.effort.review = base.ladder.effort.review,
            },
            SourceCase {
                label: "reviews.enabled marker",
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").enabled = Some(false);
                },
                restore: |entry, base| entry.reviews.enabled = base.reviews.enabled,
            },
            SourceCase {
                label: "reviews.alternative_available marker",
                mutate: |started| {
                    started
                        .reviews
                        .as_mut()
                        .expect("reviews")
                        .alternative_available = Some(false);
                },
                restore: |entry, base| {
                    entry.reviews.alternative_available = base.reviews.alternative_available;
                },
            },
            SourceCase {
                label: "per-pass review timeout",
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").pass_timeout_secs = Some(60);
                },
                restore: |entry, base| {
                    entry.reviews.pass_timeout_secs = base.reviews.pass_timeout_secs;
                },
            },
            SourceCase {
                label: "primary reviewer agent",
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").primary =
                        Some(PassBinding::new("copilot", "claude-opus-5"));
                },
                restore: |entry, base| {
                    restore_agent(&mut entry.reviews.primary, &base.reviews.primary)
                },
            },
            SourceCase {
                label: "primary reviewer model",
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").primary =
                        Some(PassBinding::new("claude-code", "gpt-5.6"));
                },
                restore: |entry, base| {
                    restore_model(&mut entry.reviews.primary, &base.reviews.primary)
                },
            },
            SourceCase {
                label: "primary reviewer absence",
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").primary = None;
                },
                restore: |entry, base| entry.reviews.primary = base.reviews.primary.clone(),
            },
            SourceCase {
                label: "alternative reviewer agent",
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").alternative =
                        Some(PassBinding::new("claude-code", "gpt-5.6"));
                },
                restore: |entry, base| {
                    restore_agent(&mut entry.reviews.alternative, &base.reviews.alternative);
                },
            },
            SourceCase {
                label: "alternative reviewer model",
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").alternative =
                        Some(PassBinding::new("copilot", "gpt-5.6-sol"));
                },
                restore: |entry, base| {
                    restore_model(&mut entry.reviews.alternative, &base.reviews.alternative);
                },
            },
            SourceCase {
                label: "alternative reviewer absence",
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").alternative = None;
                },
                restore: |entry, base| entry.reviews.alternative = base.reviews.alternative.clone(),
            },
        ];

        let plan = sample_plan();
        let baseline = registry_of(&plan);
        for SourceCase {
            label,
            mutate,
            restore,
        } in cases
        {
            let mut started = started_for(&plan);
            mutate(&mut started);
            let moved = originals_of(&plan, &started)
                .unwrap_or_else(|error| panic!("the {label} case must still build: {error}"));
            assert_eq!(moved.len(), baseline.len(), "{label}");

            for (index, (entry, base)) in moved.entries().iter().zip(baseline.entries()).enumerate()
            {
                assert_ne!(
                    entry, base,
                    "moving the {label} left entry {index} exactly as it was, so that field is \
                     not read from the run record"
                );
                let mut restored = entry.clone();
                restore(&mut restored, base);
                assert_eq!(
                    &restored, base,
                    "moving the {label} moved something else in entry {index} too, so the \
                     recorded value reaches a field it does not belong to"
                );
            }
        }
    }

    fn restore_agent(entry: &mut Option<PassBinding>, base: &Option<PassBinding>) {
        if let (Some(moved), Some(original)) = (entry.as_mut(), base.as_ref()) {
            moved.agent.clone_from(&original.agent);
        }
    }

    fn restore_model(entry: &mut Option<PassBinding>, base: &Option<PassBinding>) {
        if let (Some(moved), Some(original)) = (entry.as_mut(), base.as_ref()) {
            moved.model.clone_from(&original.model);
        }
    }

    struct SlotCase {
        label: &'static str,
        slot: usize,
        mutate: fn(&mut RunStarted),
        restore: fn(&mut TaskEntry, &TaskEntry),
    }

    #[test]
    fn moving_one_second_opinion_slot_moves_exactly_that_entry_s_component() {
        let cases: [SlotCase; 7] = [
            SlotCase {
                label: "zeta's second-opinion agent",
                slot: 0,
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").second_opinion[0] =
                        Some(PassBinding::new("moved-agent-for-zeta", "second-for-zeta"));
                },
                restore: |entry, base| {
                    restore_agent(
                        &mut entry.reviews.second_opinion,
                        &base.reviews.second_opinion,
                    );
                },
            },
            SlotCase {
                label: "zeta's second-opinion model",
                slot: 0,
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").second_opinion[0] = Some(
                        PassBinding::new("zeta-second-agent", "moved-model-for-zeta"),
                    );
                },
                restore: |entry, base| {
                    restore_model(
                        &mut entry.reviews.second_opinion,
                        &base.reviews.second_opinion,
                    );
                },
            },
            SlotCase {
                label: "zeta's second-opinion absence",
                slot: 0,
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").second_opinion[0] = None;
                },
                restore: |entry, base| {
                    entry.reviews.second_opinion = base.reviews.second_opinion.clone();
                },
            },
            SlotCase {
                label: "mid's second-opinion agent",
                slot: 2,
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").second_opinion[2] =
                        Some(PassBinding::new("moved-agent-for-mid", "second-for-mid"));
                },
                restore: |entry, base| {
                    restore_agent(
                        &mut entry.reviews.second_opinion,
                        &base.reviews.second_opinion,
                    );
                },
            },
            SlotCase {
                label: "mid's second-opinion model",
                slot: 2,
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").second_opinion[2] =
                        Some(PassBinding::new("mid-second-agent", "moved-model-for-mid"));
                },
                restore: |entry, base| {
                    restore_model(
                        &mut entry.reviews.second_opinion,
                        &base.reviews.second_opinion,
                    );
                },
            },
            SlotCase {
                label: "mid's second-opinion absence",
                slot: 2,
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").second_opinion[2] = None;
                },
                restore: |entry, base| {
                    entry.reviews.second_opinion = base.reviews.second_opinion.clone();
                },
            },
            SlotCase {
                label: "alpha's second-opinion presence",
                slot: 1,
                mutate: |started| {
                    started.reviews.as_mut().expect("reviews").second_opinion[1] =
                        Some(PassBinding::new("alpha-second-agent", "second-for-alpha"));
                },
                restore: |entry, base| {
                    entry.reviews.second_opinion = base.reviews.second_opinion.clone();
                },
            },
        ];

        let plan = varied_plan();
        let baseline =
            originals_of(&plan, &varied_started_for(&plan)).expect("the varied record is complete");
        for SlotCase {
            label,
            slot,
            mutate,
            restore,
        } in cases
        {
            let mut started = varied_started_for(&plan);
            mutate(&mut started);
            let moved = originals_of(&plan, &started)
                .unwrap_or_else(|error| panic!("the {label} case must still build: {error}"));
            assert_eq!(moved.len(), baseline.len(), "{label}");

            let entry = &moved.entries()[slot];
            let base = &baseline.entries()[slot];
            assert_ne!(
                entry, base,
                "moving {label} left entry {slot} exactly as it was, so that component is not \
                 read from the run record"
            );
            let mut restored = entry.clone();
            restore(&mut restored, base);
            assert_eq!(
                &restored, base,
                "moving {label} moved something else in entry {slot} too, so the recorded value \
                 reaches a component it does not belong to"
            );

            for (index, (other, base_other)) in
                moved.entries().iter().zip(baseline.entries()).enumerate()
            {
                if index == slot {
                    continue;
                }
                assert_eq!(
                    other, base_other,
                    "moving {label} moved entry {index} as well"
                );
            }
        }
    }

    #[test]
    fn the_fixtures_give_every_binding_component_its_own_literal() {
        let plan = varied_plan();
        let started = varied_started_for(&plan);
        let reviews = started
            .reviews
            .as_ref()
            .expect("the varied record's review plan");

        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        let mut record = |label: String, value: &str| {
            if let Some(first) = seen.insert(value.to_owned(), label.clone()) {
                panic!(
                    "the varied fixture gives `{value}` to both {first} and {label}; a component \
                     read from either where it meant the other would still look right"
                );
            }
        };

        for chain in &started.chains {
            let task = &chain.task;
            for binding in chain
                .bindings
                .as_ref()
                .expect("the varied fixture records bindings")
            {
                let tier = binding.tier;
                record(
                    format!("chain `{task}`'s {tier} rung agent"),
                    &binding.agent,
                );
                record(
                    format!("chain `{task}`'s {tier} rung model"),
                    &binding.model,
                );
            }
        }
        for (what, binding) in [
            ("the primary reviewer", reviews.primary.as_ref()),
            ("the alternative reviewer", reviews.alternative.as_ref()),
        ] {
            let binding = binding.expect("the varied fixture records the run-level reviewers");
            record(format!("{what}'s agent"), &binding.agent);
            record(format!("{what}'s model"), &binding.model);
        }
        for (index, slot) in reviews.second_opinion.iter().enumerate() {
            let Some(binding) = slot.as_ref() else {
                continue;
            };
            let task = plan.tasks[index].id.as_str();
            record(format!("`{task}`'s second-opinion agent"), &binding.agent);
            record(format!("`{task}`'s second-opinion model"), &binding.model);
        }

        assert_eq!(
            seen.len(),
            20,
            "six recorded rungs, two run-level reviewers and two occupied second-opinion slots, \
             each contributing an agent and a model"
        );
    }

    #[test]
    fn the_reserved_namespace_covers_every_id_the_repair_generator_can_emit() {
        for index in [0u32, 1, 9, 99, 999, 1000, 9999, 10_000, u32::MAX] {
            for root in ["a", "alpha", "0001", "x-1", "merge-fix-0001-alpha"] {
                let id = repair_display_id(index, &TaskId::from(root));
                assert!(
                    is_reserved_display_id(&id),
                    "the generator emitted `{id}`, which the refusal does not reserve"
                );
            }
        }
        assert_eq!(
            repair_display_id(1, &TaskId::from("zeta")),
            "merge-fix-0001-zeta",
            "the shape the merge-queue decision records"
        );
        assert_eq!(
            repair_display_id(12_345, &TaskId::from("omega")),
            "merge-fix-12345-omega",
            "an index past the pad width widens rather than truncating"
        );

        for outside in [
            "alpha",
            "merge",
            "merge-fix",
            "merge-fix-",
            "merge-fix-001-a",
            "merge-fix-abcd-a",
            "merge-fix-0001a",
            "merge-fixed-0001-a",
            "x-merge-fix-0001-a",
            "mérge-fix-0001-a",
        ] {
            assert!(
                !is_reserved_display_id(outside),
                "`{outside}` is not an id the merge queue can generate"
            );
        }
        assert!(
            is_reserved_display_id("MERGE-FIX-0001-a"),
            "a reserved namespace is not escaped by shouting"
        );
    }

    #[test]
    fn a_repair_id_is_its_prefix_its_index_and_the_root_display_id_itself() {
        const PREFIX: &str = "merge-fix-";
        const SEPARATOR: &str = "-";
        const PAD_WIDTH: usize = 4;

        for root in [
            "kestrel",
            "quartz",
            "0042",
            "-",
            "merge-fix-0042-kestrel",
            "Kestrel",
            "  quartz  ",
            "a-root-identifier-that-is-longer-than-thirty-two-bytes",
            "café-kestrel",
        ] {
            for index in [0u32, 1, 4_242, 9_999, 10_000, u32::MAX] {
                let id = repair_display_id(index, &TaskId::from(root));

                let framed = id
                    .strip_prefix(PREFIX)
                    .unwrap_or_else(|| panic!("`{id}` does not open with `{PREFIX}`"));
                let digits = framed.bytes().take_while(u8::is_ascii_digit).count();
                let (rendered, tail) = framed.split_at(digits);
                assert_eq!(
                    rendered.parse::<u32>().ok(),
                    Some(index),
                    "`{id}` does not carry lineage index {index}"
                );
                assert_eq!(
                    rendered.len(),
                    index.to_string().len().max(PAD_WIDTH),
                    "`{id}` renders its index at neither {PAD_WIDTH} padded digits nor its own width"
                );
                let suffix = tail.strip_prefix(SEPARATOR).unwrap_or_else(|| {
                    panic!("`{id}` does not part its index from its root with `{SEPARATOR}`")
                });

                assert_eq!(
                    suffix, root,
                    "`{id}` was generated for root `{root}` and does not end in it"
                );
            }
        }
    }

    #[test]
    fn an_original_may_not_take_a_reserved_repair_id() {
        const RESERVED: &str = "merge-fix-0001-alpha";

        let plan = plan_of(vec![task("alpha", &[]), task(RESERVED, &[])]);
        let refusal = originals_of(&plan, &started_for(&plan))
            .expect_err("the reserved namespace belongs to the merge queue");
        assert_eq!(
            refusal,
            RegistryError::ReservedDisplayId {
                id: RESERVED.to_owned()
            }
        );
        assert!(refusal.to_string().contains(RESERVED));
    }

    #[test]
    fn a_duplicate_display_id_is_refused() {
        let plan = plan_of(vec![task("alpha", &[]), task("alpha", &[])]);
        assert_eq!(
            originals_of(&plan, &started_for(&plan)),
            Err(RegistryError::DuplicateDisplayId {
                id: "alpha".to_owned()
            })
        );
    }

    #[test]
    fn an_unknown_dependency_is_refused() {
        let plan = plan_of(vec![task("alpha", &["ghost"])]);
        assert_eq!(
            originals_of(&plan, &started_for(&plan)),
            Err(RegistryError::UnknownDependency {
                task: "alpha".to_owned(),
                dep: "ghost".to_owned(),
            })
        );
    }

    #[test]
    fn an_incomplete_run_record_cannot_authenticate_a_registry() {
        let cases: [(&str, BreakRecord); 6] = [
            ("effort policy", |started| started.effort_policy = None),
            ("review plan", |started| started.reviews = None),
            ("reviews.enabled marker", |started| {
                started.reviews.as_mut().expect("reviews").enabled = None;
            }),
            ("reviews.alternative_available marker", |started| {
                started
                    .reviews
                    .as_mut()
                    .expect("reviews")
                    .alternative_available = None;
            }),
            ("per-pass review timeout", |started| {
                started.reviews.as_mut().expect("reviews").pass_timeout_secs = None;
            }),
            ("resolved rung bindings", |started| {
                started.chains[0].bindings = None;
            }),
        ];
        for (field, break_it) in cases {
            let plan = sample_plan();
            let mut started = started_for(&plan);
            break_it(&mut started);
            assert_eq!(
                originals_of(&plan, &started),
                Err(RegistryError::IncompleteRunRecord { field }),
                "a record missing its {field} must refuse rather than default"
            );
        }
    }

    #[test]
    fn a_record_that_does_not_describe_the_frozen_plan_is_refused() {
        let cases: [(BreakRecord, RegistryError); 6] = [
            (
                |started| started.chains[0].task = "ghost".to_owned(),
                RegistryError::ChainWithoutTask {
                    task: "ghost".to_owned(),
                },
            ),
            (
                |started| started.chains[1].task = "zeta".to_owned(),
                RegistryError::DuplicateChain {
                    task: "zeta".to_owned(),
                },
            ),
            (
                |started| {
                    started.chains.pop();
                },
                RegistryError::TaskWithoutChain {
                    task: "mid".to_owned(),
                },
            ),
            (
                |started| {
                    started.chains[0].tiers.clear();
                    started.chains[0].bindings = Some(Vec::new());
                },
                RegistryError::EmptyLadder {
                    task: "zeta".to_owned(),
                },
            ),
            (
                |started| started.chains[0].attempts_per = 0,
                RegistryError::ZeroAttempts {
                    task: "zeta".to_owned(),
                },
            ),
            (
                |started| {
                    started.chains[0].bindings.as_mut().expect("bindings").pop();
                },
                RegistryError::BindingCount {
                    task: "zeta".to_owned(),
                    bindings: 1,
                    tiers: 2,
                },
            ),
        ];
        for (break_it, expected) in cases {
            let plan = sample_plan();
            let mut started = started_for(&plan);
            break_it(&mut started);
            assert_eq!(originals_of(&plan, &started), Err(expected));
        }

        let plan = sample_plan();
        let mut started = started_for(&plan);
        started.chains[0].bindings.as_mut().expect("bindings")[0].tier = Tier::Frontier;
        assert_eq!(
            originals_of(&plan, &started),
            Err(RegistryError::BindingTier {
                task: "zeta".to_owned(),
                tier: Tier::Small,
                binding: Tier::Frontier,
            })
        );

        let plan = sample_plan();
        let mut started = started_for(&plan);
        started
            .reviews
            .as_mut()
            .expect("reviews")
            .second_opinion
            .pop();
        assert_eq!(
            originals_of(&plan, &started),
            Err(RegistryError::ReviewAlignment {
                recorded: 2,
                tasks: 3
            })
        );
    }

    const SAMPLE_DIGEST: &str =
        "sha256:4a08825ac234223cb53bd79736f240f8869369d4c07c3eeced3760632d594eb6";

    const SAMPLE_CANONICAL_BYTES: usize = 2522;

    #[test]
    fn the_registry_digest_is_its_frozen_vector() {
        let plan = sample_plan();
        let registry = registry_of(&plan);
        assert_eq!(
            registry.digest(),
            SAMPLE_DIGEST,
            "the canonical serialization is frozen; a recorded digest outlives this binary"
        );
        assert_eq!(
            registry.canonical_bytes().len(),
            SAMPLE_CANONICAL_BYTES,
            "the digest is taken over a different number of bytes than the frozen encoding"
        );
        assert_eq!(registry_of(&sample_plan()).digest(), SAMPLE_DIGEST);
        assert_eq!(registry.digest().len(), "sha256:".len() + 64);
    }

    #[test]
    fn a_record_that_names_no_probed_agents_derives_an_empty_allow_list() {
        let plan = sample_plan();
        let started = started_for(&plan);
        let legacy =
            TaskRegistry::originals(&plan, &started).expect("the sample record is complete");

        for entry in legacy.entries() {
            assert!(
                entry.allowed_agents.is_empty(),
                "`{}` took an allow-list from a record that has no place to record one",
                entry.display_id
            );
        }
        assert_eq!(
            legacy,
            TaskRegistry::originals_with_agents(&plan, &started, &[])
                .expect("the sample record is complete"),
            "the two-argument derivation must be the no-agents case of the three-argument one, \
             not a second derivation that could drift from it"
        );
        assert_ne!(legacy.digest(), SAMPLE_DIGEST);
    }

    #[test]
    fn a_digest_mismatch_is_refused_and_a_match_is_not() {
        let registry = registry_of(&sample_plan());
        let recorded = registry.digest();
        assert_eq!(registry.verify_digest(&recorded), Ok(()));

        let mut moved = sample_plan();
        moved.tasks[0].body.push('!');
        let rebuilt = registry_of(&moved);
        assert_eq!(
            rebuilt.verify_digest(&recorded),
            Err(RegistryError::DigestMismatch {
                expected: recorded,
                actual: rebuilt.digest(),
            })
        );
    }

    #[test]
    fn the_digest_covers_every_field_it_authenticates() {
        let cases: [(&str, MoveInput); 30] = [
            ("display id", |plan, started, _| {
                plan.tasks[0].id = TaskId::from("zeta-renamed");
                plan.tasks[2].depends_on[1] = TaskId::from("zeta-renamed");
                started.chains[0].task = "zeta-renamed".to_owned();
            }),
            ("kind", |plan, _, _| plan.tasks[0].kind = TaskKind::Docs),
            ("title", |plan, _, _| plan.tasks[0].title.push('!')),
            ("body", |plan, _, _| plan.tasks[0].body.push('!')),
            ("acceptance", |plan, _, _| {
                plan.tasks[0].acceptance.push("more".to_owned());
            }),
            ("path hints", |plan, _, _| {
                plan.tasks[0].path_hints.push("src/extra.rs".to_owned());
            }),
            ("suggested tier", |plan, _, _| {
                plan.tasks[0].suggested_tier = Some(Tier::Frontier);
            }),
            ("suggested tier absent", |plan, _, _| {
                plan.tasks[0].suggested_tier = None;
            }),
            ("min tier", |plan, _, _| {
                plan.tasks[0].min_tier = Some(Tier::Mid);
            }),
            ("artifacts in", |plan, _, _| {
                plan.tasks[0].artifacts_in.push(ArtifactId::from("extra"));
            }),
            ("artifacts out", |plan, _, _| {
                plan.tasks[0].artifacts_out.push(ArtifactId::from("extra"));
            }),
            ("dependencies", |plan, _, _| {
                plan.tasks[0].depends_on.clear();
            }),
            ("dependency order", |plan, _, _| {
                plan.tasks[2].depends_on.swap(0, 1);
            }),
            ("plan order", |plan, started, _| {
                plan.tasks.swap(0, 1);
                started.chains.swap(0, 1);
            }),
            ("chain tiers", |_, started, _| {
                started.chains[0].tiers[1] = Tier::Frontier;
                started.chains[0].bindings.as_mut().expect("bindings")[1].tier = Tier::Frontier;
            }),
            ("attempts per rung", |_, started, _| {
                started.chains[0].attempts_per = 3;
            }),
            ("rung agent", |_, started, _| {
                started.chains[0].bindings.as_mut().expect("bindings")[0].agent =
                    "copilot".to_owned();
            }),
            ("rung model", |_, started, _| {
                started.chains[0].bindings.as_mut().expect("bindings")[0].model =
                    "claude-sonnet-5".to_owned();
            }),
            ("rung pin", |_, started, _| {
                started.chains[0].bindings.as_mut().expect("bindings")[0].pinned = true;
            }),
            ("effort policy", |_, started, _| {
                started
                    .effort_policy
                    .as_mut()
                    .expect("effort policy")
                    .frontier = Effort::Max;
            }),
            ("review pass timeout", |_, started, _| {
                started.reviews.as_mut().expect("reviews").pass_timeout_secs = Some(60);
            }),
            ("primary reviewer", |_, started, _| {
                started.reviews.as_mut().expect("reviews").primary =
                    Some(PassBinding::new("copilot", "gpt-5.6"));
            }),
            ("alternative reviewer", |_, started, _| {
                started.reviews.as_mut().expect("reviews").alternative = None;
            }),
            ("alternative available marker", |_, started, _| {
                started
                    .reviews
                    .as_mut()
                    .expect("reviews")
                    .alternative_available = Some(false);
            }),
            ("reviews enabled marker", |_, started, _| {
                started.reviews.as_mut().expect("reviews").enabled = Some(false);
            }),
            ("second opinion slot", |_, started, _| {
                started.reviews.as_mut().expect("reviews").second_opinion[1] = None;
            }),
            ("probed agent value", |_, _, agents| agents[1].push('!')),
            ("probed agent count", |_, _, agents| {
                agents.push("gemini".to_owned());
            }),
            ("probed agent order", |_, _, agents| agents.swap(0, 1)),
            ("probed agents absent", |_, _, agents| agents.clear()),
        ];

        let baseline = registry_of(&sample_plan()).digest();
        let mut digests: BTreeSet<String> = BTreeSet::new();
        digests.insert(baseline.clone());
        for (label, mutate) in cases {
            let mut plan = sample_plan();
            let mut started = started_for(&plan);
            let mut agents = sample_agents();
            mutate(&mut plan, &mut started, &mut agents);
            let digest = TaskRegistry::originals_with_agents(&plan, &started, &agents)
                .unwrap_or_else(|error| panic!("the {label} case must still build: {error}"))
                .digest();
            assert_ne!(
                digest, baseline,
                "changing the {label} left the digest where it was, so the digest does not \
                 authenticate it"
            );
            assert!(
                digests.insert(digest),
                "the {label} case collided with another mutation's digest"
            );
        }
        assert_eq!(digests.len(), cases.len() + 1);
    }

    #[test]
    fn changing_one_entry_field_alone_changes_the_canonical_bytes() {
        const MOVED: usize = 2;
        let cases: [(&str, MoveField); 64] = [
            ("key", |entry| entry.key = TaskKey(7)),
            ("display id", |entry| {
                entry.display_id = TaskId::from("zeta-renamed");
            }),
            ("origin", |entry| entry.origin = Origin::MergeRepair),
            ("lineage present", |entry| {
                entry.lineage = Some(Lineage {
                    root: TaskKey(1),
                    parent: TaskKey(2),
                    index: 4,
                });
            }),
            ("lineage root", |entry| {
                entry.lineage = Some(Lineage {
                    root: TaskKey(2),
                    parent: TaskKey(2),
                    index: 4,
                });
            }),
            ("lineage parent", |entry| {
                entry.lineage = Some(Lineage {
                    root: TaskKey(1),
                    parent: TaskKey(0),
                    index: 4,
                });
            }),
            ("lineage index", |entry| {
                entry.lineage = Some(Lineage {
                    root: TaskKey(1),
                    parent: TaskKey(2),
                    index: 5,
                });
            }),
            ("kind", |entry| entry.spec.kind = TaskKind::Docs),
            ("title", |entry| entry.spec.title.push('!')),
            ("body", |entry| entry.spec.body.push('!')),
            ("acceptance value", |entry| {
                entry.spec.acceptance[0].push('!')
            }),
            ("acceptance count", |entry| {
                entry.spec.acceptance.push("more".to_owned());
            }),
            ("acceptance order", |entry| entry.spec.acceptance.swap(0, 1)),
            ("path hint value", |entry| {
                entry.spec.path_hints[0].push('!')
            }),
            ("path hint count", |entry| {
                entry.spec.path_hints.push("src/extra.rs".to_owned());
            }),
            ("path hint order", |entry| entry.spec.path_hints.swap(0, 1)),
            ("suggested tier", |entry| {
                entry.spec.suggested_tier = Some(Tier::Frontier);
            }),
            ("suggested tier absent", |entry| {
                entry.spec.suggested_tier = None;
            }),
            ("min tier", |entry| entry.spec.min_tier = Some(Tier::Mid)),
            ("min tier absent", |entry| entry.spec.min_tier = None),
            ("artifacts in", |entry| {
                entry.spec.artifacts_in.push(ArtifactId::from("extra"));
            }),
            ("artifacts out", |entry| {
                entry.spec.artifacts_out.push(ArtifactId::from("extra"));
            }),
            ("dependency key", |entry| entry.deps[0] = TaskKey(2)),
            ("dependency count", |entry| entry.deps.push(TaskKey(2))),
            ("dependency key order", |entry| entry.deps.swap(0, 1)),
            ("display dependency", |entry| {
                entry.display_deps[0] = TaskId::from("mid");
            }),
            ("display dependency count", |entry| {
                entry.display_deps.push(TaskId::from("mid"));
            }),
            ("display dependency order", |entry| {
                entry.display_deps.swap(0, 1);
            }),
            ("ladder tier", |entry| {
                entry.ladder.tiers[1] = Tier::Frontier;
            }),
            ("ladder tier count", |entry| {
                entry.ladder.tiers.push(Tier::Frontier);
            }),
            ("ladder tier order", |entry| entry.ladder.tiers.swap(0, 1)),
            ("attempts per rung", |entry| entry.ladder.attempts_per = 3),
            ("rung tier", |entry| {
                entry.ladder.rungs[0].tier = Tier::Frontier;
            }),
            ("rung agent", |entry| {
                entry.ladder.rungs[0].agent = "copilot".to_owned();
            }),
            ("rung model", |entry| {
                entry.ladder.rungs[0].model = "claude-sonnet-5".to_owned();
            }),
            ("rung pin", |entry| entry.ladder.rungs[0].pinned = true),
            ("rung count", |entry| {
                entry.ladder.rungs.push(FrozenRung {
                    tier: Tier::Frontier,
                    agent: "codex".to_owned(),
                    model: "gpt-5.6-sol".to_owned(),
                    pinned: true,
                });
            }),
            ("rung order", |entry| entry.ladder.rungs.swap(0, 1)),
            ("ladder floor", |entry| entry.ladder.floor = Some(Tier::Mid)),
            ("ladder floor absent", |entry| entry.ladder.floor = None),
            ("ladder ceiling", |entry| {
                entry.ladder.ceiling = Some(Tier::Frontier);
            }),
            ("ladder ceiling absent", |entry| entry.ladder.ceiling = None),
            ("effort small", |entry| {
                entry.ladder.effort.small = Effort::High;
            }),
            ("effort mid", |entry| entry.ladder.effort.mid = Effort::Max),
            ("effort frontier", |entry| {
                entry.ladder.effort.frontier = Effort::Low;
            }),
            ("effort review", |entry| {
                entry.ladder.effort.review = Effort::Max;
            }),
            ("admission", |entry| {
                entry.ladder.admission = Admission::HumanBinding {
                    options: Vec::new(),
                };
            }),
            ("admission options", |entry| {
                entry.ladder.admission = Admission::HumanBinding {
                    options: vec!["small/claude-haiku-4-5".to_owned()],
                };
            }),
            ("reviews enabled", |entry| entry.reviews.enabled = false),
            ("reviews alternative available", |entry| {
                entry.reviews.alternative_available = false;
            }),
            ("review pass timeout", |entry| {
                entry.reviews.pass_timeout_secs = 60;
            }),
            ("primary reviewer agent", |entry| {
                entry.reviews.primary = Some(PassBinding::new("copilot", "claude-opus-5"));
            }),
            ("primary reviewer model", |entry| {
                entry.reviews.primary = Some(PassBinding::new("claude-code", "gpt-5.6"));
            }),
            ("primary reviewer absent", |entry| {
                entry.reviews.primary = None;
            }),
            ("alternative reviewer agent", |entry| {
                entry.reviews.alternative = Some(PassBinding::new("claude-code", "gpt-5.6"));
            }),
            ("alternative reviewer model", |entry| {
                entry.reviews.alternative = Some(PassBinding::new("copilot", "gpt-5.6-sol"));
            }),
            ("alternative reviewer absent", |entry| {
                entry.reviews.alternative = None;
            }),
            ("second opinion present", |entry| {
                entry.reviews.second_opinion = Some(PassBinding::new("copilot", "gpt-5.6"));
            }),
            ("second opinion agent", |entry| {
                entry.reviews.second_opinion = Some(PassBinding::new("claude-code", "gpt-5.6"));
            }),
            ("second opinion model", |entry| {
                entry.reviews.second_opinion = Some(PassBinding::new("copilot", "claude-opus-5"));
            }),
            ("allowed agent value", |entry| {
                entry.allowed_agents[1].push('!');
            }),
            ("allowed agent count", |entry| {
                entry.allowed_agents.push("gemini".to_owned());
            }),
            ("allowed agent order", |entry| {
                entry.allowed_agents.swap(0, 1);
            }),
            ("allowed agents absent", |entry| {
                entry.allowed_agents.clear();
            }),
        ];

        let baseline = registry_of(&sample_plan());
        let baseline_bytes = baseline.canonical_bytes();
        let mut encodings: BTreeSet<Vec<u8>> = BTreeSet::new();
        encodings.insert(baseline_bytes.clone());
        for (label, mutate) in cases {
            let mut registry = registry_of(&sample_plan());
            mutate(&mut registry.entries[MOVED]);

            assert_ne!(
                registry.entries[MOVED], baseline.entries[MOVED],
                "the {label} case left the entry as it found it, so it tests nothing"
            );
            assert_eq!(
                registry.entries[..MOVED],
                baseline.entries[..MOVED],
                "{label}"
            );
            assert_eq!(
                registry.entries[MOVED + 1..],
                baseline.entries[MOVED + 1..],
                "{label}"
            );

            let bytes = registry.canonical_bytes();
            assert_ne!(
                bytes, baseline_bytes,
                "changing the {label} alone left the canonical bytes where they were, so the \
                 digest does not authenticate it"
            );
            assert_ne!(registry.digest(), baseline.digest(), "{label}");
            assert!(
                encodings.insert(bytes),
                "the {label} case encodes to bytes another case already reached"
            );
        }
        assert_eq!(encodings.len(), cases.len() + 1);
    }

    #[test]
    fn the_canonical_encoding_cannot_shift_text_between_adjacent_fields() {
        fn adjacent(title: &str, body: &str) -> Plan {
            let mut plan = sample_plan();
            plan.tasks[0].title = title.to_owned();
            plan.tasks[0].body = body.to_owned();
            plan
        }

        for [left_title, left_body, right_title, right_body] in [
            ["ab", "c", "a", "bc"],
            ["a;", "b", "a", ";b"],
            ["é;", "b", "é", ";b"],
            ["a:", "b", "a", ":b"],
            ["x2:;", "y", "x", "2:;y"],
        ] {
            assert_eq!(
                format!("{left_title}{left_body}"),
                format!("{right_title}{right_body}"),
                "the pair must be one run of text split two ways, or it proves nothing"
            );
            let left = registry_of(&adjacent(left_title, left_body));
            let right = registry_of(&adjacent(right_title, right_body));
            assert_ne!(
                left.canonical_bytes(),
                right.canonical_bytes(),
                "`{left_title}`/`{left_body}` and `{right_title}`/`{right_body}` encode alike, so \
                 text can be shifted between adjacent fields"
            );
            assert_ne!(left.digest(), right.digest());
        }

        let bytes = registry_of(&adjacent("é", "b")).canonical_bytes();
        assert!(
            bytes.windows(5).any(|window| window == "2:é;".as_bytes()),
            "the length prefix counts bytes"
        );
    }

    fn round_tripped(plan: &Plan) -> Plan {
        Plan {
            source: plan.source.clone(),
            tasks: registry_of(plan).legacy_tasks(),
            artifacts: plan.artifacts.clone(),
        }
    }

    fn normalized_bytes(plan: &Plan) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(plan).expect("serialize plan");
        bytes.push(b'\n');
        bytes
    }

    #[test]
    fn the_registry_round_trips_the_frozen_plan_byte_for_byte() {
        for (fixture, raw) in [
            ("sample-plan.md", crate::plan::corpus::SAMPLE_PLAN),
            ("bare-plan.md", crate::plan::corpus::BARE_PLAN),
            ("steps-plan.md", crate::plan::corpus::STEPS_PLAN),
        ] {
            let plan = crate::plan::detect(raw)
                .expect("a markdown plan")
                .parse(raw)
                .expect("parses");
            assert!(!plan.tasks.is_empty(), "{fixture} has tasks");
            assert_eq!(
                normalized_bytes(&round_tripped(&plan)),
                normalized_bytes(&plan),
                "{fixture} did not survive the registry byte-for-byte"
            );
        }

        let bare = plan_of(vec![Task {
            id: TaskId::from("solo"),
            kind: TaskKind::Chore,
            title: String::new(),
            body: String::new(),
            depends_on: Vec::new(),
            acceptance: Vec::new(),
            path_hints: Vec::new(),
            suggested_tier: None,
            min_tier: None,
            artifacts_in: Vec::new(),
            artifacts_out: Vec::new(),
        }]);
        for plan in [sample_plan(), bare, dependency_order_plan()] {
            assert_eq!(
                normalized_bytes(&round_tripped(&plan)),
                normalized_bytes(&plan)
            );
        }
    }

    fn event_log(started: &RunStarted) -> Vec<Event> {
        fn at(seconds: u32) -> String {
            format!("2026-08-01T00:00:{seconds:02}.000Z")
        }
        fn model_for(tier: &str) -> String {
            if tier == "small" {
                "claude-haiku-4-5".to_owned()
            } else {
                "gpt-5.6-sol".to_owned()
            }
        }
        fn record(attempt: u32, tier: &str, failure: Option<FailureRecord>) -> Box<AttemptRecord> {
            Box::new(AttemptRecord {
                attempt,
                tier: tier.to_owned(),
                model: model_for(tier),
                pool: Some("claude-max".to_owned()),
                resumed: false,
                duration: Duration::from_secs(7),
                cost_usd: Some(0.25),
                reviews: Vec::new(),
                session_id: None,
                usage: None,
                failure,
            })
        }
        fn start(tier: &str) -> AttemptStarted {
            AttemptStarted {
                tier: tier.to_owned(),
                agent: "claude-code".to_owned(),
                model: model_for(tier),
                adapter: Some("claude-code".to_owned()),
                preflight_cli_version: Some("1.2.3".to_owned()),
                effort: Some(Effort::Low),
                selection_origin: None,
                pool: Some("claude-max".to_owned()),
                resume_session: None,
            }
        }
        fn event(ts: String, body: EventBody) -> Event {
            Event { ts, body }
        }

        vec![
            event(
                at(0),
                EventBody::RunStarted {
                    data: Box::new(started.clone()),
                },
            ),
            event(
                at(1),
                EventBody::AttemptStarted {
                    task: "alpha".to_owned(),
                    attempt: 1,
                    rung: 0,
                    profile: "small-worker".to_owned(),
                    data: start("small"),
                },
            ),
            event(
                at(2),
                EventBody::AttemptFinished {
                    task: "alpha".to_owned(),
                    attempt: 1,
                    rung: 0,
                    profile: "small-worker".to_owned(),
                    data: record(1, "small", None),
                    parking: None,
                    transition: None,
                    prepared_commit: None,
                },
            ),
            event(
                at(3),
                EventBody::TaskCommitted {
                    task: "alpha".to_owned(),
                    data: TaskCommitted {
                        sha: "b".repeat(40),
                        message: "[upstroke] alpha: alpha title".to_owned(),
                    },
                },
            ),
            event(
                at(4),
                EventBody::AttemptStarted {
                    task: "zeta".to_owned(),
                    attempt: 1,
                    rung: 0,
                    profile: "small-worker".to_owned(),
                    data: start("small"),
                },
            ),
            event(
                at(5),
                EventBody::AttemptFinished {
                    task: "zeta".to_owned(),
                    attempt: 1,
                    rung: 0,
                    profile: "small-worker".to_owned(),
                    data: record(
                        1,
                        "small",
                        Some(FailureRecord {
                            kind: FailureKind::GateFailed,
                            origin: FailureOrigin::Worker,
                            reason: "gate `check` failed".to_owned(),
                            detail: None,
                        }),
                    ),
                    parking: None,
                    transition: Some(Box::new(AttemptTransition::Escalate(LadderEscalated {
                        to_rung: 1,
                        tier: "small".to_owned(),
                        summary: "escalate".to_owned(),
                        detail: None,
                    }))),
                    prepared_commit: None,
                },
            ),
            event(
                at(6),
                EventBody::AttemptStarted {
                    task: "zeta".to_owned(),
                    attempt: 2,
                    rung: 1,
                    profile: "mid-worker".to_owned(),
                    data: start("mid"),
                },
            ),
            event(
                at(7),
                EventBody::AttemptInterrupted {
                    task: "zeta".to_owned(),
                    attempt: 2,
                    rung: 1,
                    profile: "mid-worker".to_owned(),
                    data: record(
                        2,
                        "mid",
                        Some(FailureRecord {
                            kind: FailureKind::Interrupted,
                            origin: FailureOrigin::Worker,
                            reason: "the engine died mid-attempt".to_owned(),
                            detail: None,
                        }),
                    ),
                },
            ),
            event(
                at(8),
                EventBody::RunFinished {
                    data: RunFinished {
                        outcome: RunOutcome::Parked,
                        halted_at: None,
                        committed: 1,
                        parked: 0,
                    },
                },
            ),
        ]
    }

    fn replayed(plan: &Plan, log: &[Event]) -> crate::events::RunState {
        crate::events::replay(
            log.to_vec(),
            plan.tasks.iter().map(|t| t.id.to_string()).collect(),
            Path::new("events.jsonl"),
        )
        .expect("the fixture log replays")
        .state
    }

    #[test]
    fn the_report_and_status_projections_are_byte_identical_through_the_registry() {
        let plan = projection_plan();
        let rebuilt = round_tripped(&plan);
        let started = started_for(&plan);
        let log = event_log(&started);

        let build = |plan: &Plan| {
            crate::engine::RunReport::from_state(
                &started,
                plan,
                &replayed(plan, &log),
                vec!["a warning".to_owned()],
                false,
                true,
            )
        };
        let from_plan = build(&plan);
        let from_registry = build(&rebuilt);

        assert_eq!(
            serde_json::to_vec_pretty(&from_registry).expect("serialize report"),
            serde_json::to_vec_pretty(&from_plan).expect("serialize report"),
            "report.json is written from exactly these bytes"
        );
        let rendered = |report: &crate::engine::RunReport| {
            let mut out = report.render();
            out.push_str(&report.render_ledger());
            out
        };
        assert_eq!(rendered(&from_registry), rendered(&from_plan));
        assert_eq!(
            from_plan
                .tasks
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta", "mid", "beta", "gamma"]
        );
        assert!(from_plan.total_cost_usd > 0.0);
        assert!(
            rendered(&from_plan).contains("alpha title"),
            "a committed task's title reaches the rendered view"
        );
        let json =
            String::from_utf8(serde_json::to_vec_pretty(&from_plan).expect("serialize report"))
                .expect("utf-8 report");
        for title in [
            "zeta title",
            "alpha title",
            "mid title",
            "beta title",
            "gamma title",
        ] {
            assert!(
                json.contains(title),
                "`{title}` is part of what the byte comparison covers"
            );
        }
    }

    struct RunFixture {
        root: PathBuf,
        public: PathBuf,
    }

    impl RunFixture {
        fn new(tag: &str, plan: &Plan, log: &[Event]) -> Self {
            let root = std::env::temp_dir()
                .join(format!("upstroke-registry-{tag}-{}", std::process::id()));
            let public = crate::rundir::public_dir(&root, RUN_ID);
            let hooks = &mut crate::rundir::NoHooks;
            crate::rundir::create_public_dir(&public, hooks).expect("run directory");
            crate::rundir::write_plan(&public, &normalized_bytes(plan), hooks)
                .expect("frozen plan");
            let mut warnings = Vec::new();
            let mut writer = crate::events::EventLog::open(
                crate::topology::effects::EventSite::LegacyOpenLog,
                &public.join("events.jsonl"),
                &mut warnings,
            )
            .expect("event log");
            assert!(warnings.is_empty(), "{warnings:?}");
            for event in log {
                writer
                    .append(
                        crate::topology::effects::EventSite::LegacyAppend,
                        event.body.clone(),
                    )
                    .expect("append");
            }
            Self { root, public }
        }

        fn reproject(&self, plan: &Plan) {
            crate::rundir::write_plan(
                &self.public,
                &normalized_bytes(plan),
                &mut crate::rundir::NoHooks,
            )
            .expect("frozen plan");
        }

        fn exported(&self, format: crate::export::Format) -> Vec<u8> {
            let loaded = crate::export::load(&self.root, RUN_ID).expect("export loads");
            let mut out = Vec::new();
            crate::export::write(&loaded.rows, format, &mut out).expect("export writes");
            out
        }
    }

    impl Drop for RunFixture {
        fn drop(&mut self) {
            let _ = crate::rundir::remove_public_husk(&self.public, &mut crate::rundir::NoHooks);
        }
    }

    #[test]
    fn the_export_projection_is_byte_identical_through_the_registry() {
        let plan = projection_plan();
        let rebuilt = round_tripped(&plan);
        let log = event_log(&started_for(&plan));

        let run = RunFixture::new("projection", &plan, &log);

        for format in [crate::export::Format::Jsonl, crate::export::Format::Csv] {
            run.reproject(&plan);
            let expected = run.exported(format);
            run.reproject(&rebuilt);
            assert_eq!(
                run.exported(format),
                expected,
                "the export projection moved"
            );
            let text = String::from_utf8(expected).expect("utf-8 export");
            assert!(text.contains("zeta") && text.contains("alpha"), "{text}");
            assert!(text.len() > 512, "{text}");
        }
    }
}
