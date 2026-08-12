//! Local, read-only projection of a run's recorded routing decisions.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::error::TactusError;
use crate::events::{self, AttemptRecord, Event, EventBody, ReviewPassOutcome, SelectionOrigin};
use crate::ir::{Effort, Plan, Task, Usage};
use crate::ladder::{FailureKind, FailureOrigin};
use crate::rundir;

const EXPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Format {
    Jsonl,
    Csv,
}

#[derive(Serialize)]
pub struct Row {
    schema_version: u32,
    run_id: String,
    tactus_version: String,
    run_started_at: String,
    attempt_started_at: String,
    attempt_finished_at: Option<String>,
    task_id: String,
    task_title: String,
    attempt: u32,
    rung: u32,
    task_features: TaskFeatures,
    chain: Chain,
    selected_tier: String,
    selection_origin: &'static str,
    adapter_id: String,
    adapter_cli_version: Option<String>,
    model: String,
    effort: Option<Effort>,
    pool: Option<String>,
    session_resumed: bool,
    duration_ms: Option<u64>,
    cost_usd: Option<f64>,
    usage: Option<Usage>,
    outcome: &'static str,
    failure_kind: Option<FailureKind>,
    failure_origin: Option<FailureOrigin>,
    failure_category: Option<&'static str>,
    work_evidence: Option<&'static str>,
    failure_reason: Option<String>,
    reviews: Vec<Review>,
}

#[derive(Serialize)]
struct TaskFeatures {
    kind: String,
    suggested_tier: Option<String>,
    minimum_tier: Option<String>,
    dependency_count: usize,
    acceptance_count: usize,
    path_hints: Vec<String>,
    artifact_input_count: usize,
    artifact_output_count: usize,
}

#[derive(Serialize)]
struct Chain {
    tiers: Vec<String>,
    attempts_per: u32,
}

#[derive(Serialize)]
struct Review {
    pass: String,
    adapter_id: String,
    adapter_cli_version: Option<String>,
    model: String,
    effort: Option<Effort>,
    pool: Option<String>,
    cost_usd: Option<f64>,
    outcome: ReviewPassOutcome,
}

type AttemptKey = (String, u32, u32);

struct Settlement<'a> {
    ts: &'a str,
    profile: &'a str,
    record: &'a AttemptRecord,
}

/// Load and validate one stable run snapshot. No config, source plan, adapter,
/// or report is consulted.
pub fn load(repo_root: &Path, wanted: &str) -> Result<Vec<Row>, TactusError> {
    let run_id = rundir::resolve_run_id(repo_root, wanted)?;
    let public = rundir::public_dir(repo_root, &run_id);
    if rundir::is_running(&public) {
        return Err(TactusError::Refused {
            message: format!(
                "run `{run_id}` is live and its decision dataset is still moving; wait for it to finish or stop it before exporting"
            ),
        });
    }
    let events_path = public.join("events.jsonl");
    let mut warnings = Vec::new();
    let log = events::read_all(&events_path, &mut warnings)?;
    if !warnings.is_empty() {
        return invalid(&events_path, warnings.join("; "));
    }
    let mut run_starts = log
        .iter()
        .filter(|event| matches!(event.body, EventBody::RunStarted { .. }));
    let started_event = run_starts.next().ok_or_else(|| TactusError::EventLog {
        path: events_path.clone(),
        message: "no run_started event".to_owned(),
    })?;
    if run_starts.next().is_some() {
        return invalid(&events_path, "duplicate run_started event".to_owned());
    }
    let EventBody::RunStarted { data: started } = &started_event.body else {
        unreachable!()
    };
    if started.run_id != run_id {
        return invalid(
            &events_path,
            format!(
                "run_started id `{}` does not match directory `{run_id}`",
                started.run_id
            ),
        );
    }

    let plan_path = public.join("plan.normalized.json");
    let plan_text = std::fs::read_to_string(&plan_path).map_err(|source| TactusError::Io {
        path: plan_path.clone(),
        source,
    })?;
    let plan: Plan = serde_json::from_str(&plan_text).map_err(|error| TactusError::Parse {
        message: format!("{}: {error}", plan_path.display()),
    })?;
    if plan.source.hash != started.plan_hash {
        return invalid(
            &plan_path,
            format!(
                "frozen plan hash `{}` does not match run-start hash `{}`",
                plan.source.hash, started.plan_hash
            ),
        );
    }
    let tasks = unique_tasks(&plan, &events_path)?;
    let chains = unique_chains(&started.chains, &events_path)?;
    let settlements = settlements(&log, &events_path)?;
    let mut seen_starts = BTreeSet::new();
    let mut rows = Vec::new();

    for event in &log {
        let EventBody::AttemptStarted {
            task,
            attempt,
            rung,
            profile,
            data,
        } = &event.body
        else {
            continue;
        };
        let key = (task.clone(), *attempt, *rung);
        if *attempt == 0 {
            return invalid(
                &events_path,
                format!("attempt number must be positive for {}", key_text(&key)),
            );
        }
        if !seen_starts.insert(key.clone()) {
            return invalid(
                &events_path,
                format!("duplicate attempt start {}", key_text(&key)),
            );
        }
        let task_plan = tasks
            .get(task)
            .ok_or_else(|| bad_join(&events_path, task, "frozen plan"))?;
        let chain = chains
            .get(task)
            .ok_or_else(|| bad_join(&events_path, task, "run-start chains"))?;
        if chain.tiers.is_empty() || chain.attempts_per == 0 {
            return invalid(
                &events_path,
                format!("invalid recorded chain for attempted task `{task}`"),
            );
        }
        let expected_tier = usize::try_from(*rung)
            .ok()
            .and_then(|index| chain.tiers.get(index))
            .ok_or_else(|| TactusError::EventLog {
                path: events_path.clone(),
                message: format!("rung is outside the recorded chain for {}", key_text(&key)),
            })?;
        if expected_tier.to_string() != data.tier {
            return invalid(
                &events_path,
                format!(
                    "start tier `{}` does not match recorded rung tier `{expected_tier}` for {}",
                    data.tier,
                    key_text(&key)
                ),
            );
        }
        let settlement = settlements.get(&key);
        if let Some(done) = settlement
            && (done.profile != profile
                || done.record.attempt != *attempt
                || done.record.tier != data.tier)
        {
            return invalid(
                &events_path,
                format!("mismatched settlement for {}", key_text(&key)),
            );
        }
        rows.push(build_row(
            &run_id,
            &started.tactus_version,
            &started_event.ts,
            event,
            task_plan,
            chain,
            settlement,
        )?);
    }
    for key in settlements.keys() {
        if !seen_starts.contains(key) {
            return invalid(
                &events_path,
                format!("settlement without a start for {}", key_text(key)),
            );
        }
    }
    Ok(rows)
}

fn unique_tasks<'a>(
    plan: &'a Plan,
    path: &Path,
) -> Result<BTreeMap<String, &'a Task>, TactusError> {
    let mut out = BTreeMap::new();
    for task in &plan.tasks {
        if out.insert(task.id.to_string(), task).is_some() {
            return invalid(path, format!("duplicate task `{}` in frozen plan", task.id));
        }
    }
    Ok(out)
}

fn unique_chains<'a>(
    chains: &'a [events::ChainSummary],
    path: &Path,
) -> Result<BTreeMap<String, &'a events::ChainSummary>, TactusError> {
    let mut out = BTreeMap::new();
    for chain in chains {
        if out.insert(chain.task.clone(), chain).is_some() {
            return invalid(
                path,
                format!("duplicate recorded chain for task `{}`", chain.task),
            );
        }
    }
    Ok(out)
}

fn settlements<'a>(
    log: &'a [Event],
    path: &Path,
) -> Result<BTreeMap<AttemptKey, Settlement<'a>>, TactusError> {
    let mut out = BTreeMap::new();
    for event in log {
        let (task, attempt, rung, profile, record) = match &event.body {
            EventBody::AttemptFinished {
                task,
                attempt,
                rung,
                profile,
                data,
            }
            | EventBody::AttemptInterrupted {
                task,
                attempt,
                rung,
                profile,
                data,
            } => (task, attempt, rung, profile, &**data),
            _ => continue,
        };
        let key = (task.clone(), *attempt, *rung);
        if out
            .insert(
                key.clone(),
                Settlement {
                    ts: &event.ts,
                    profile,
                    record,
                },
            )
            .is_some()
        {
            return invalid(path, format!("duplicate settlement for {}", key_text(&key)));
        }
    }
    Ok(out)
}

fn build_row(
    run_id: &str,
    version: &str,
    run_started_at: &str,
    start_event: &Event,
    task: &Task,
    chain: &events::ChainSummary,
    settlement: Option<&Settlement<'_>>,
) -> Result<Row, TactusError> {
    let EventBody::AttemptStarted {
        attempt,
        rung,
        data,
        ..
    } = &start_event.body
    else {
        unreachable!()
    };
    let failure = settlement.and_then(|done| done.record.failure.as_ref());
    let kind = failure
        .map(|f| f.kind)
        .or_else(|| settlement.is_none().then_some(FailureKind::Interrupted));
    let origin = failure
        .map(|f| f.origin)
        .or_else(|| settlement.is_none().then_some(FailureOrigin::Worker));
    let (category, evidence) = kind.map(failure_projection).unzip();
    let interrupted = kind == Some(FailureKind::Interrupted);
    let record = settlement.map(|done| done.record);
    let duration_ms = record.map(|r| duration_ms(r.duration)).transpose()?;
    validate_cost("attempt", record.and_then(|r| r.cost_usd))?;
    if let Some(record) = record {
        for (index, review) in record.reviews.iter().enumerate() {
            validate_cost(&format!("review pass {index}"), review.cost_usd)?;
        }
    }
    Ok(Row {
        schema_version: EXPORT_SCHEMA_VERSION,
        run_id: run_id.to_owned(),
        tactus_version: version.to_owned(),
        run_started_at: run_started_at.to_owned(),
        attempt_started_at: start_event.ts.clone(),
        attempt_finished_at: settlement.map(|done| done.ts.to_owned()),
        task_id: task.id.to_string(),
        task_title: task.title.clone(),
        attempt: *attempt,
        rung: *rung,
        task_features: TaskFeatures {
            kind: task.kind.to_string(),
            suggested_tier: task.suggested_tier.map(|v| v.to_string()),
            minimum_tier: task.min_tier.map(|v| v.to_string()),
            dependency_count: task.depends_on.len(),
            acceptance_count: task.acceptance.len(),
            path_hints: task.path_hints.clone(),
            artifact_input_count: task.artifacts_in.len(),
            artifact_output_count: task.artifacts_out.len(),
        },
        chain: Chain {
            tiers: chain.tiers.iter().map(ToString::to_string).collect(),
            attempts_per: chain.attempts_per,
        },
        selected_tier: data.tier.clone(),
        selection_origin: selection_origin(data.selection_origin),
        adapter_id: data.adapter.clone().unwrap_or_else(|| data.agent.clone()),
        adapter_cli_version: data.preflight_cli_version.clone(),
        model: data.model.clone(),
        effort: data.effort,
        pool: data.pool.clone(),
        session_resumed: data.resume_session.is_some(),
        duration_ms,
        cost_usd: record.and_then(|r| r.cost_usd),
        usage: record.and_then(|r| r.usage.clone()),
        outcome: if interrupted {
            "interrupted"
        } else if kind.is_some() {
            "failed"
        } else {
            "passed"
        },
        failure_kind: kind,
        failure_origin: origin,
        failure_category: category,
        work_evidence: evidence,
        failure_reason: failure.map(|f| f.reason.clone()),
        reviews: record
            .map(|r| r.reviews.iter().map(review).collect())
            .unwrap_or_default(),
    })
}

fn review(value: &events::ReviewRecord) -> Review {
    Review {
        pass: value.pass.clone(),
        adapter_id: value.adapter.clone().unwrap_or_else(|| value.agent.clone()),
        adapter_cli_version: value.preflight_cli_version.clone(),
        model: value.model.clone(),
        effort: value.effort,
        pool: value.pool.clone(),
        cost_usd: value.cost_usd,
        outcome: value.outcome,
    }
}

fn selection_origin(value: SelectionOrigin) -> &'static str {
    match value {
        SelectionOrigin::Unknown => "unknown",
        SelectionOrigin::Auto => "auto",
        SelectionOrigin::Pin => "pin",
        SelectionOrigin::UserOverride => "user_override",
        SelectionOrigin::Exploration => "exploration",
    }
}

/// Deliberately exhaustive and wildcard-free: a new FailureKind is a compile error here.
fn failure_projection(kind: FailureKind) -> (&'static str, &'static str) {
    match kind {
        FailureKind::GateFailed => ("capability", "gate"),
        FailureKind::ReviewFailed => ("capability", "review"),
        FailureKind::AgentError | FailureKind::RateLimited | FailureKind::ReviewUnavailable => {
            ("provider", "none")
        }
        FailureKind::Timeout | FailureKind::Interrupted => ("infrastructure", "none"),
        FailureKind::NoChain | FailureKind::NeedsHuman | FailureKind::Declined => {
            ("policy", "none")
        }
        FailureKind::EmptyDiff | FailureKind::TestProvenance => ("policy", "engine"),
    }
}

fn duration_ms(value: std::time::Duration) -> Result<u64, TactusError> {
    u64::try_from(value.as_millis()).map_err(|_| TactusError::Refused {
        message: "attempt duration exceeds export schema range".to_owned(),
    })
}

fn validate_cost(label: &str, cost: Option<f64>) -> Result<(), TactusError> {
    if let Some(cost) = cost
        && (!cost.is_finite() || cost < 0.0)
    {
        return Err(TactusError::Refused {
            message: format!("{label} cost must be finite and non-negative, got {cost}"),
        });
    }
    Ok(())
}

pub fn write(rows: &[Row], format: Format, out: &mut impl Write) -> anyhow::Result<()> {
    match format {
        Format::Jsonl => {
            for row in rows {
                serde_json::to_writer(&mut *out, row)?;
                out.write_all(b"\n")?;
            }
        }
        Format::Csv => write_csv(rows, out)?,
    }
    Ok(())
}

const CSV_HEADER: &str = "schema_version,run_id,tactus_version,run_started_at,attempt_started_at,attempt_finished_at,task_id,task_title,attempt,rung,task_kind,suggested_tier,minimum_tier,dependency_count,acceptance_count,path_hints_json,artifact_input_count,artifact_output_count,chain_tiers_json,attempts_per,selected_tier,selection_origin,adapter_id,adapter_cli_version,model,effort,pool,session_resumed,duration_ms,cost_usd,usage_input_tokens,usage_output_tokens,usage_cache_creation_input_tokens,usage_cache_read_input_tokens,usage_num_turns,usage_reasoning_output_tokens,outcome,failure_kind,failure_origin,failure_category,work_evidence,failure_reason,reviews_json\r\n";

fn write_csv(rows: &[Row], out: &mut impl Write) -> anyhow::Result<()> {
    out.write_all(CSV_HEADER.as_bytes())?;
    for row in rows {
        let usage = row.usage.as_ref();
        let fields = vec![
            row.schema_version.to_string(),
            row.run_id.clone(),
            row.tactus_version.clone(),
            row.run_started_at.clone(),
            row.attempt_started_at.clone(),
            opt(&row.attempt_finished_at),
            row.task_id.clone(),
            row.task_title.clone(),
            row.attempt.to_string(),
            row.rung.to_string(),
            row.task_features.kind.clone(),
            opt(&row.task_features.suggested_tier),
            opt(&row.task_features.minimum_tier),
            row.task_features.dependency_count.to_string(),
            row.task_features.acceptance_count.to_string(),
            serde_json::to_string(&row.task_features.path_hints)?,
            row.task_features.artifact_input_count.to_string(),
            row.task_features.artifact_output_count.to_string(),
            serde_json::to_string(&row.chain.tiers)?,
            row.chain.attempts_per.to_string(),
            row.selected_tier.clone(),
            row.selection_origin.to_owned(),
            row.adapter_id.clone(),
            opt(&row.adapter_cli_version),
            row.model.clone(),
            row.effort.map(|v| v.to_string()).unwrap_or_default(),
            opt(&row.pool),
            row.session_resumed.to_string(),
            scalar(row.duration_ms),
            scalar(row.cost_usd),
            scalar(usage.and_then(|u| u.input_tokens)),
            scalar(usage.and_then(|u| u.output_tokens)),
            scalar(usage.and_then(|u| u.cache_creation_input_tokens)),
            scalar(usage.and_then(|u| u.cache_read_input_tokens)),
            scalar(usage.and_then(|u| u.num_turns)),
            scalar(usage.and_then(|u| u.reasoning_output_tokens)),
            row.outcome.to_owned(),
            json_enum(row.failure_kind)?,
            json_enum(row.failure_origin)?,
            row.failure_category.unwrap_or_default().to_owned(),
            row.work_evidence.unwrap_or_default().to_owned(),
            opt(&row.failure_reason),
            serde_json::to_string(&row.reviews)?,
        ];
        for (index, field) in fields.iter().enumerate() {
            if index != 0 {
                out.write_all(b",")?;
            }
            write_csv_field(field, out)?;
        }
        out.write_all(b"\r\n")?;
    }
    Ok(())
}

fn write_csv_field(value: &str, out: &mut impl Write) -> std::io::Result<()> {
    if value.contains([',', '"', '\r', '\n']) {
        out.write_all(b"\"")?;
        out.write_all(value.replace('"', "\"\"").as_bytes())?;
        out.write_all(b"\"")
    } else {
        out.write_all(value.as_bytes())
    }
}

fn opt(value: &Option<String>) -> String {
    value.clone().unwrap_or_default()
}
fn scalar<T: ToString>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}
fn json_enum<T: Serialize>(value: Option<T>) -> anyhow::Result<String> {
    Ok(value
        .map(|v| serde_json::to_string(&v).map(|s| s.trim_matches('"').to_owned()))
        .transpose()?
        .unwrap_or_default())
}
fn key_text(key: &AttemptKey) -> String {
    format!("task `{}`, attempt {}, rung {}", key.0, key.1, key.2)
}
fn bad_join(path: &Path, task: &str, source: &str) -> TactusError {
    TactusError::EventLog {
        path: path.to_owned(),
        message: format!("attempt task `{task}` is absent from {source}"),
    }
}
fn invalid<T>(path: &Path, message: String) -> Result<T, TactusError> {
    Err(TactusError::EventLog {
        path: path.to_owned(),
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const RUN_ID: &str = "01EXPORTTEST00000000000000";

    struct Fixture {
        root: PathBuf,
        public: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str, events: Vec<Value>, tasks: Vec<Value>) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "tactus-export-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let public = rundir::public_dir(&root, RUN_ID);
            fs::create_dir_all(&public).expect("run directory");
            fs::write(
                public.join("plan.normalized.json"),
                serde_json::to_vec(&json!({
                    "source": { "adapter": "frozen", "hash": "frozen-hash" },
                    "tasks": tasks,
                    "artifacts": []
                }))
                .expect("plan json"),
            )
            .expect("write frozen plan");
            let log = events
                .iter()
                .map(|event| serde_json::to_string(event).expect("event json"))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            fs::write(public.join("events.jsonl"), log).expect("write event log");
            Self { root, public }
        }

        fn rows(&self) -> Vec<Row> {
            load(&self.root, "01EXPORT").expect("prefix resolves and export loads")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn task(id: &str, title: &str) -> Value {
        json!({
            "id": id,
            "kind": "fix",
            "title": title,
            "body": "frozen body",
            "depends_on": ["dep-a", "dep-b"],
            "acceptance": ["one", "two", "three"],
            "path_hints": ["src/exact,a.rs", "tests/\"quoted\".rs"],
            "suggested_tier": "mid",
            "min_tier": "small",
            "artifacts_in": ["input"],
            "artifacts_out": ["output-a", "output-b"]
        })
    }

    fn run_started(tasks: &[&str]) -> Value {
        let chains = tasks
            .iter()
            .map(|task| json!({ "task": task, "tiers": ["small", "mid"], "attempts_per": 2 }))
            .collect::<Vec<_>>();
        json!({
            "ts": "2026-08-01T00:00:00.000Z",
            "event": "run_started",
            "data": {
                "schema": 1,
                "tactus_version": "0.0-old",
                "run_id": RUN_ID,
                "branch": "tactus/run-test",
                "base_sha": "abc",
                "plan_path": "today.md",
                "config_path": "tactus.toml",
                "plan_hash": "frozen-hash",
                "private_dir": "/not/read",
                "gates": [],
                "gates_from_config": false,
                "interaction_mode": "never",
                "chains": chains
            }
        })
    }

    fn attempt_started(task: &str, attempt: u32, ts: &str, legacy: bool) -> Value {
        let mut data = json!({
            "tier": "small",
            "agent": "recorded-agent",
            "model": "recorded/model",
            "pool": "recorded-pool",
            "resume_session": null
        });
        if !legacy {
            data["adapter"] = json!("recorded-adapter");
            data["preflight_cli_version"] = json!("1.2.3");
            data["effort"] = json!("low");
            data["selection_origin"] = json!("auto");
        }
        json!({
            "ts": ts,
            "event": "attempt_started",
            "task": task,
            "attempt": attempt,
            "rung": 0,
            "profile": "small-worker",
            "data": data
        })
    }

    fn attempt_finished(
        task: &str,
        attempt: u32,
        ts: &str,
        failure: Option<FailureKind>,
        with_review: bool,
    ) -> Value {
        let reviews = if with_review {
            vec![json!({
                "pass": "review",
                "agent": "review-agent",
                "adapter": "review-adapter",
                "preflight_cli_version": "9.0",
                "model": "review/model",
                "effort": "high",
                "pool": "review-pool",
                "cost_usd": 0.25,
                "outcome": "passed"
            })]
        } else {
            Vec::new()
        };
        let failure = failure.map(|kind| {
            json!({
                "kind": kind,
                "origin": "worker",
                "reason": format!("recorded {kind:?}")
            })
        });
        json!({
            "ts": ts,
            "event": "attempt_finished",
            "task": task,
            "attempt": attempt,
            "rung": 0,
            "profile": "small-worker",
            "data": {
                "attempt": attempt,
                "tier": "small",
                "model": "recorded/model",
                "pool": "recorded-pool",
                "resumed": false,
                "duration_ms": 1234,
                "cost_usd": 1.5,
                "reviews": reviews,
                "session_id": null,
                "usage": { "input_tokens": 10, "output_tokens": 20 },
                "failure": failure
            }
        })
    }

    fn snapshot(path: &Path) -> BTreeMap<PathBuf, (u64, std::time::SystemTime)> {
        fn visit(
            root: &Path,
            path: &Path,
            out: &mut BTreeMap<PathBuf, (u64, std::time::SystemTime)>,
        ) {
            for entry in fs::read_dir(path).expect("read snapshot directory") {
                let entry = entry.expect("directory entry");
                let metadata = entry.metadata().expect("metadata");
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .expect("relative path")
                    .to_owned();
                out.insert(
                    relative,
                    (metadata.len(), metadata.modified().expect("modified time")),
                );
                if metadata.is_dir() {
                    visit(root, &entry.path(), out);
                }
            }
        }
        let mut out = BTreeMap::new();
        visit(path, path, &mut out);
        out
    }

    fn load_error(fixture: &Fixture) -> String {
        match load(&fixture.root, RUN_ID) {
            Ok(_) => panic!("invalid fixture exported successfully"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn csv_quotes_rfc_4180_special_characters() {
        for (input, expected) in [
            ("plain", "plain"),
            ("a,b", "\"a,b\""),
            ("a\"b", "\"a\"\"b\""),
            ("a\nb", "\"a\nb\""),
            ("a\rb", "\"a\rb\""),
            ("a\r\nb", "\"a\r\nb\""),
        ] {
            let mut output = Vec::new();
            write_csv_field(input, &mut output).expect("write");
            assert_eq!(String::from_utf8(output).expect("utf8"), expected);
        }
    }

    #[test]
    fn every_failure_kind_has_the_decided_projection() {
        let cases = [
            (FailureKind::GateFailed, "capability", "gate"),
            (FailureKind::ReviewFailed, "capability", "review"),
            (FailureKind::AgentError, "provider", "none"),
            (FailureKind::RateLimited, "provider", "none"),
            (FailureKind::ReviewUnavailable, "provider", "none"),
            (FailureKind::Timeout, "infrastructure", "none"),
            (FailureKind::Interrupted, "infrastructure", "none"),
            (FailureKind::NoChain, "policy", "none"),
            (FailureKind::EmptyDiff, "policy", "engine"),
            (FailureKind::TestProvenance, "policy", "engine"),
            (FailureKind::NeedsHuman, "policy", "none"),
            (FailureKind::Declined, "policy", "none"),
        ];
        for (kind, category, evidence) in cases {
            assert_eq!(failure_projection(kind), (category, evidence));
        }
    }

    #[test]
    fn both_formats_preserve_start_order_reviews_and_frozen_features() {
        let fixture = Fixture::new(
            "formats",
            vec![
                run_started(&["first", "second"]),
                attempt_started("first", 1, "2026-08-01T00:00:01.000Z", false),
                attempt_started("second", 1, "2026-08-01T00:00:02.000Z", false),
                attempt_finished("second", 1, "2026-08-01T00:00:03.000Z", None, false),
                attempt_finished("first", 1, "2026-08-01T00:00:04.000Z", None, true),
            ],
            vec![task("first", "first, \"quoted\""), task("second", "second")],
        );
        // These current inputs are traps: the exporter must never consult them.
        fs::write(
            fixture.root.join("today.md"),
            "# today\n<!-- tactus: kind=docs tier=frontier paths=WRONG -->",
        )
        .expect("source-plan trap");
        fs::write(
            fixture.root.join("tactus.toml"),
            "[routing]\nfix = { chain = [\"frontier\"], attempts_per = 99 }\n",
        )
        .expect("config trap");
        let before = snapshot(&fixture.public);
        let rows = fixture.rows();
        assert_eq!(snapshot(&fixture.public), before, "export changed the run");

        let mut jsonl = Vec::new();
        write(&rows, Format::Jsonl, &mut jsonl).expect("jsonl");
        let values = String::from_utf8(jsonl)
            .expect("utf8")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("valid JSONL row"))
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0]["task_id"], "first");
        assert_eq!(values[1]["task_id"], "second");
        assert_eq!(values[0]["reviews"][0]["adapter_id"], "review-adapter");
        assert_eq!(values[1]["reviews"], json!([]));
        assert_eq!(
            values[0]["task_features"],
            json!({
                "kind": "fix", "suggested_tier": "mid", "minimum_tier": "small",
                "dependency_count": 2, "acceptance_count": 3,
                "path_hints": ["src/exact,a.rs", "tests/\"quoted\".rs"],
                "artifact_input_count": 1, "artifact_output_count": 2
            })
        );
        assert!(values[0].get("diff_size").is_none());
        assert!(values[0]["task_features"].get("diff_size").is_none());
        assert!(!values[0].to_string().contains("WRONG"));

        let mut csv = Vec::new();
        write(&rows, Format::Csv, &mut csv).expect("csv");
        let csv = String::from_utf8(csv).expect("utf8 csv");
        assert!(csv.starts_with(CSV_HEADER));
        assert_eq!(csv.matches("\r\n").count(), 3, "header plus two rows");
        assert!(csv.contains("\"first, \"\"quoted\"\"\""));
        assert!(csv.contains("src/exact,a.rs"));
        assert!(csv.contains("quoted"));
        assert!(csv.contains("review-adapter"));
    }

    #[test]
    fn dangling_and_legacy_attempts_stay_unknown_and_interrupted() {
        let fixture = Fixture::new(
            "dangling",
            vec![
                run_started(&["old"]),
                attempt_started("old", 1, "2026-08-01T00:00:01.000Z", true),
            ],
            vec![task("old", "old task")],
        );
        fs::write(
            fixture.root.join("tactus.toml"),
            "[pins.small]\nagent = \"today-agent\"\nmodel = \"today-model\"\n",
        )
        .expect("config trap");
        let rows = fixture.rows();
        let value = serde_json::to_value(&rows[0]).expect("row value");
        assert_eq!(value["attempt_finished_at"], Value::Null);
        assert_eq!(value["duration_ms"], Value::Null);
        assert_eq!(value["cost_usd"], Value::Null);
        assert_eq!(value["usage"], Value::Null);
        assert_eq!(value["outcome"], "interrupted");
        assert_eq!(value["failure_kind"], "interrupted");
        assert_eq!(value["failure_origin"], "worker");
        assert_eq!(value["failure_category"], "infrastructure");
        assert_eq!(value["work_evidence"], "none");
        assert_eq!(value["failure_reason"], Value::Null);
        assert_eq!(value["selection_origin"], "unknown");
        assert_eq!(value["adapter_id"], "recorded-agent");
        assert_eq!(value["adapter_cli_version"], Value::Null);
        assert_ne!(value["adapter_id"], "today-agent");
    }

    #[test]
    fn xhigh_worker_and_max_review_effort_are_preserved_in_schema_one() {
        let mut start = attempt_started("task", 1, "2026-08-01T00:00:01.000Z", false);
        start["data"]["effort"] = json!("xhigh");
        let mut finish = attempt_finished("task", 1, "2026-08-01T00:00:02.000Z", None, true);
        finish["data"]["reviews"][0]["effort"] = json!("max");
        let fixture = Fixture::new(
            "role-effort",
            vec![run_started(&["task"]), start, finish],
            vec![task("task", "role effort")],
        );

        let rows = fixture.rows();
        let value = serde_json::to_value(&rows[0]).expect("row value");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["effort"], "xhigh");
        assert_eq!(value["reviews"][0]["effort"], "max");
        let mut csv = Vec::new();
        write(&rows, Format::Csv, &mut csv).expect("csv");
        let csv = String::from_utf8(csv).expect("utf8 csv");
        assert!(csv.contains("xhigh"), "worker effort is represented: {csv}");
        assert!(csv.contains("max"), "review effort is represented: {csv}");
    }

    #[test]
    fn a_live_run_is_refused_actionably() {
        let fixture = Fixture::new(
            "live",
            vec![run_started(&["task"])],
            vec![task("task", "task")],
        );
        let _lock = rundir::RunLock::acquire(&fixture.public).expect("hold live lock");
        let error = match load(&fixture.root, RUN_ID) {
            Ok(_) => panic!("live export was not refused"),
            Err(error) => error,
        };
        let message = error.to_string();
        assert!(message.contains("is live"));
        assert!(message.contains("wait for it to finish or stop it"));
    }

    #[test]
    fn invalid_recorded_invariants_are_refused() {
        let hash = Fixture::new(
            "bad-hash",
            vec![run_started(&["task"])],
            vec![task("task", "task")],
        );
        let plan_path = hash.public.join("plan.normalized.json");
        let mut plan: Value =
            serde_json::from_slice(&fs::read(&plan_path).expect("read plan")).expect("plan json");
        plan["source"]["hash"] = json!("tampered");
        fs::write(&plan_path, serde_json::to_vec(&plan).expect("plan json"))
            .expect("write tampered plan");
        assert!(load_error(&hash).contains("frozen plan hash"));

        let zero = Fixture::new(
            "zero-attempt",
            vec![
                run_started(&["task"]),
                attempt_started("task", 0, "2026-08-01T00:00:01.000Z", false),
            ],
            vec![task("task", "task")],
        );
        assert!(load_error(&zero).contains("must be positive"));

        let mut wrong_tier = attempt_started("task", 1, "2026-08-01T00:00:01.000Z", false);
        wrong_tier["data"]["tier"] = json!("mid");
        let tier = Fixture::new(
            "wrong-tier",
            vec![run_started(&["task"]), wrong_tier],
            vec![task("task", "task")],
        );
        assert!(load_error(&tier).contains("does not match recorded rung tier"));

        let mut out_of_range = attempt_started("task", 1, "2026-08-01T00:00:01.000Z", false);
        out_of_range["rung"] = json!(9);
        let rung = Fixture::new(
            "bad-rung",
            vec![run_started(&["task"]), out_of_range],
            vec![task("task", "task")],
        );
        assert!(load_error(&rung).contains("outside the recorded chain"));

        let mut bad_review = attempt_finished("task", 1, "2026-08-01T00:00:02.000Z", None, true);
        bad_review["data"]["reviews"][0]["cost_usd"] = json!(-0.25);
        let cost = Fixture::new(
            "bad-review-cost",
            vec![
                run_started(&["task"]),
                attempt_started("task", 1, "2026-08-01T00:00:01.000Z", false),
                bad_review,
            ],
            vec![task("task", "task")],
        );
        assert!(load_error(&cost).contains("review pass 0 cost"));
    }

    #[test]
    fn every_failure_kind_reaches_an_emitted_row() {
        let cases = [
            (FailureKind::GateFailed, "capability", "gate"),
            (FailureKind::ReviewFailed, "capability", "review"),
            (FailureKind::AgentError, "provider", "none"),
            (FailureKind::RateLimited, "provider", "none"),
            (FailureKind::ReviewUnavailable, "provider", "none"),
            (FailureKind::Timeout, "infrastructure", "none"),
            (FailureKind::Interrupted, "infrastructure", "none"),
            (FailureKind::NoChain, "policy", "none"),
            (FailureKind::EmptyDiff, "policy", "engine"),
            (FailureKind::TestProvenance, "policy", "engine"),
            (FailureKind::NeedsHuman, "policy", "none"),
            (FailureKind::Declined, "policy", "none"),
        ];
        let ids = (0..cases.len()).map(|index| format!("f{index}"));
        let mut events = vec![run_started(
            &ids.clone()
                .map(|id| Box::leak(id.into_boxed_str()) as &str)
                .collect::<Vec<_>>(),
        )];
        let ids = (0..cases.len())
            .map(|index| format!("f{index}"))
            .collect::<Vec<_>>();
        for (index, (id, (kind, _, _))) in ids.iter().zip(cases.iter()).enumerate() {
            events.push(attempt_started(
                id,
                1,
                &format!("2026-08-01T00:01:{index:02}.000Z"),
                false,
            ));
            events.push(attempt_finished(
                id,
                1,
                &format!("2026-08-01T00:02:{index:02}.000Z"),
                Some(*kind),
                false,
            ));
        }
        let fixture = Fixture::new(
            "failures",
            events,
            ids.iter().map(|id| task(id, id)).collect(),
        );
        let rows = fixture.rows();
        let mut jsonl = Vec::new();
        write(&rows, Format::Jsonl, &mut jsonl).expect("jsonl");
        let values = String::from_utf8(jsonl)
            .expect("utf8")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("row"))
            .collect::<Vec<_>>();
        assert_eq!(values.len(), cases.len());
        for (value, (kind, category, evidence)) in values.iter().zip(cases) {
            assert_eq!(
                value["failure_kind"],
                serde_json::to_value(kind).expect("kind")
            );
            assert_eq!(value["failure_category"], category);
            assert_eq!(value["work_evidence"], evidence);
            assert_eq!(
                value["outcome"],
                if kind == FailureKind::Interrupted {
                    "interrupted"
                } else {
                    "failed"
                }
            );
        }
    }
}
