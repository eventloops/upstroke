//! Extended notes: `docs/internals/capacity.md`

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use crate::events::{AttemptRecord, Event, EventBody, ReviewPassOutcome};
use crate::ladder::FailureKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolKind {
    SubscriptionWindow,
    Credits,
    RequestPool,
    ApiKey,
    Unmetered,
}

impl PoolKind {
    pub const ACCEPTED: [&'static str; 5] = [
        "subscription-window",
        "credits",
        "request-pool",
        "api-key",
        "unmetered",
    ];

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "subscription-window" => Some(Self::SubscriptionWindow),
            "credits" => Some(Self::Credits),
            "request-pool" => Some(Self::RequestPool),
            "api-key" => Some(Self::ApiKey),
            "unmetered" => Some(Self::Unmetered),
            _ => None,
        }
    }
}

impl fmt::Display for PoolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SubscriptionWindow => "subscription-window",
            Self::Credits => "credits",
            Self::RequestPool => "request-pool",
            Self::ApiKey => "api-key",
            Self::Unmetered => "unmetered",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    ProviderEndpoint,
    LocalLogs,
    SelfMetered,
    Signals,
}

impl Source {
    pub const ACCEPTED: [&'static str; 4] = ["signals", "self", "local-logs", "provider-endpoint"];

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "signals" => Some(Self::Signals),
            "self" => Some(Self::SelfMetered),
            "local-logs" => Some(Self::LocalLogs),
            "provider-endpoint" => Some(Self::ProviderEndpoint),
            _ => None,
        }
    }

    pub fn read_in_v0_1(self) -> bool {
        matches!(self, Self::Signals | Self::SelfMetered)
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Signals => "signals",
            Self::SelfMetered => "self",
            Self::LocalLogs => "local-logs",
            Self::ProviderEndpoint => "provider-endpoint",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Allowance {
    Auto,
    Units(f64),
}

impl fmt::Display for Allowance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Units(units) => write!(f, "{units}"),
        }
    }
}

pub const DEFAULT_SAFETY_MARGIN: f64 = 0.15;
pub const DEFAULT_RESERVE: f64 = 0.20;

#[derive(Debug, Clone, PartialEq)]
pub struct Pool {
    pub name: String,
    pub kind: PoolKind,
    pub agent: String,
    pub window: Option<Duration>,
    pub weekly: bool,
    pub sources: Vec<Source>,
    pub safety_margin: f64,
    pub reserve: f64,
    pub monthly_allowance: Allowance,
    pub endpoint: Option<String>,
    pub profile: Option<String>,
    pub usable: bool,
}

impl Pool {
    pub fn discovered(name: &str, kind: PoolKind, agent: &str, sources: Vec<Source>) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            agent: agent.to_owned(),
            window: (kind == PoolKind::SubscriptionWindow).then(|| Duration::from_secs(5 * 3600)),
            weekly: kind == PoolKind::SubscriptionWindow,
            sources,
            safety_margin: DEFAULT_SAFETY_MARGIN,
            reserve: DEFAULT_RESERVE,
            monthly_allowance: Allowance::Auto,
            endpoint: None,
            profile: None,
            usable: true,
        }
    }

    pub fn describe(&self) -> String {
        let mut line = format!("{} [{}] agent={}", self.name, self.kind, self.agent);
        if let Some(window) = self.window {
            line.push_str(&format!(" window={}", render_duration(window)));
        }
        if self.weekly {
            line.push_str(" +weekly");
        }
        if let Some(profile) = &self.profile {
            line.push_str(&format!(" profile={profile}"));
        }
        if let Some(endpoint) = &self.endpoint {
            line.push_str(&format!(" endpoint={endpoint}"));
        }
        line.push_str(&format!(
            " margin={:.2} reserve={:.2}",
            self.safety_margin, self.reserve
        ));
        if !self.usable {
            line.push_str(" (no adapter in this build)");
        }
        line
    }
}

pub fn pool_for<'a>(agent: &str, pools: &'a [Pool]) -> Option<&'a Pool> {
    pools.iter().find(|pool| pool.agent == agent)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spend {
    pub usd: Option<f64>,
    pub attempts: u32,
    pub unpriced: u32,
}

impl Spend {
    fn add(&mut self, cost: Option<f64>) {
        self.attempts = self.attempts.saturating_add(1);
        match cost {
            Some(cost) => self.usd = Some(self.usd.unwrap_or(0.0) + cost),
            None => self.unpriced = self.unpriced.saturating_add(1),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observations {
    pub exhausted: BTreeMap<String, Option<String>>,
    pub self_spend: BTreeMap<String, Spend>,
}

impl Observations {
    pub fn is_empty(&self) -> bool {
        self.exhausted.is_empty() && self.self_spend.is_empty()
    }
}

pub fn observe(events: &[Event]) -> Observations {
    let mut obs = Observations::default();
    for event in events {
        match &event.body {
            EventBody::PoolExhausted { data, .. } => {
                obs.exhausted
                    .insert(data.pool.clone(), data.reset_at.clone());
            }
            EventBody::AttemptFinished { data, .. }
            | EventBody::AttemptInterrupted { data, .. } => {
                accumulate(&mut obs.self_spend, data);
                retire_signals(&mut obs.exhausted, data);
            }
            _ => {}
        }
    }
    obs
}

fn retire_signals(exhausted: &mut BTreeMap<String, Option<String>>, record: &AttemptRecord) {
    let worker_served = record.failure.as_ref().is_none_or(|failure| {
        failure.kind != FailureKind::Interrupted
            && !(failure.kind == FailureKind::RateLimited
                && failure.origin == crate::ladder::FailureOrigin::Worker)
    });
    if worker_served {
        if let Some(pool) = &record.pool {
            exhausted.remove(pool);
        }
    }
    for review in &record.reviews {
        if review.outcome != ReviewPassOutcome::Unavailable {
            if let Some(pool) = &review.pool {
                exhausted.remove(pool);
            }
        }
    }
    if let Some(failure) = &record.failure {
        if failure.kind == FailureKind::RateLimited {
            let pool = match failure.origin {
                crate::ladder::FailureOrigin::Worker => record.pool.as_ref(),
                crate::ladder::FailureOrigin::Reviewer => record
                    .reviews
                    .last()
                    .and_then(|review| review.pool.as_ref()),
            };
            if let Some(pool) = pool {
                exhausted.insert(pool.clone(), None);
            }
        }
    }
}

pub fn drain_of<'a>(
    records: impl IntoIterator<Item = &'a AttemptRecord>,
) -> BTreeMap<String, Spend> {
    let mut drain = BTreeMap::new();
    for record in records {
        accumulate(&mut drain, record);
    }
    drain
}

fn accumulate(drain: &mut BTreeMap<String, Spend>, record: &AttemptRecord) {
    if let Some(pool) = &record.pool {
        drain.entry(pool.clone()).or_default().add(record.cost_usd);
    }
    for review in &record.reviews {
        if let Some(pool) = &review.pool {
            drain.entry(pool.clone()).or_default().add(review.cost_usd);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Remaining {
    Unknown,
    Exhausted,
    AtMost(f64),
    Unmetered,
}

impl fmt::Display for Remaining {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => f.write_str("unknown"),
            Self::Exhausted => f.write_str("exhausted"),
            Self::AtMost(bound) => write!(f, "≤{:.0}%", bound * 100.0),
            Self::Unmetered => f.write_str("unmetered"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    Unknown,
    Assumed,
    SelfMetered,
    Signal,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unknown => "unknown",
            Self::Assumed => "assumed",
            Self::SelfMetered => "self-metered",
            Self::Signal => "signal",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoolEstimate {
    pub pool: String,
    pub agent: String,
    pub kind: PoolKind,
    pub profile: Option<String>,
    pub remaining: Remaining,
    pub confidence: Confidence,
    pub reset_at: Option<String>,
    pub self_spend: Option<Spend>,
    pub notes: Vec<String>,
}

impl PoolEstimate {
    pub fn describe(&self) -> String {
        let mut line = format!("{}: {} [{}]", self.pool, self.remaining, self.confidence);
        if let Some(reset) = &self.reset_at {
            line.push_str(&format!(" resets {reset}"));
        }
        if let Some(spend) = &self.self_spend {
            let unpriced = if spend.unpriced > 0 { "?" } else { "" };
            match spend.usd {
                Some(usd) => line.push_str(&format!(
                    " — this run drew ${usd:.4}{unpriced} over {} attempt(s)",
                    spend.attempts
                )),
                None => line.push_str(&format!(
                    " — this run drew {} attempt(s), none of which reported spend",
                    spend.attempts
                )),
            }
        }
        line
    }
}

pub fn estimate(pools: &[Pool], obs: &Observations) -> Vec<PoolEstimate> {
    pools.iter().map(|pool| estimate_one(pool, obs)).collect()
}

fn estimate_one(pool: &Pool, obs: &Observations) -> PoolEstimate {
    let mut notes = Vec::new();
    let self_spend = obs.self_spend.get(&pool.name).cloned();

    let mut remaining = Remaining::Unknown;
    let mut confidence = Confidence::Unknown;
    let mut reset_at = None;
    let mut take = |candidate: Remaining, rank: Confidence| {
        if rank > confidence {
            remaining = candidate;
            confidence = rank;
            true
        } else {
            false
        }
    };

    if let Some(reset) = obs.exhausted.get(&pool.name) {
        take(Remaining::Exhausted, Confidence::Signal);
        reset_at = reset.clone();
        if reset.is_none() {
            notes.push(
                "the rate-limit signal carried no reset time, so when it comes back is unknown"
                    .to_owned(),
            );
        }
    }

    if pool.kind == PoolKind::Unmetered {
        take(Remaining::Unmetered, Confidence::Assumed);
    }

    if let (Some(spend), Allowance::Units(allowance)) = (&self_spend, pool.monthly_allowance) {
        if allowance > 0.0 {
            if let Some(usd) = spend.usd {
                let raw = 1.0 - (usd / allowance);
                if take(
                    Remaining::AtMost(effective_remaining(raw, pool)),
                    Confidence::SelfMetered,
                ) {
                    notes.push(
                        "a ceiling, not a measurement: this counts only what upstroke spawned in this \
                         repository, so earlier runs, other repositories, and your own interactive \
                         sessions have all drawn against the same allowance unseen"
                            .to_owned(),
                    );
                    if spend.unpriced > 0 {
                        notes.push(format!(
                            "{} attempt(s) on this pool reported no spend, so even the draw behind that \
                             ceiling is a floor (§13)",
                            spend.unpriced
                        ));
                    }
                }
            }
        }
    }

    if confidence == Confidence::Unknown {
        notes.push(
            "nothing has measured this pool, so what is left is unknown — not full (§13)"
                .to_owned(),
        );
    }

    let unread: Vec<String> = pool
        .sources
        .iter()
        .filter(|source| !source.read_in_v0_1())
        .map(ToString::to_string)
        .collect();
    if !unread.is_empty() {
        notes.push(format!(
            "source(s) {} are parsed but not read in v0.1, so usage they would see — including \
             your own interactive sessions — is not in this figure",
            unread.join(", ")
        ));
    }
    if !pool.usable {
        notes.push(format!(
            "no adapter for agent `{}` in this build, so this engine can never draw from it",
            pool.agent
        ));
    }

    PoolEstimate {
        pool: pool.name.clone(),
        agent: pool.agent.clone(),
        kind: pool.kind,
        profile: pool.profile.clone(),
        remaining,
        confidence,
        reset_at,
        self_spend,
        notes,
    }
}

pub fn effective_remaining(raw: f64, pool: &Pool) -> f64 {
    if !raw.is_finite() {
        return 0.0;
    }
    let discounted = raw.clamp(0.0, 1.0) * (1.0 - pool.safety_margin) - pool.reserve;
    discounted.clamp(0.0, 1.0)
}

pub fn parse_duration(raw: &str) -> Option<Duration> {
    let text = raw.trim();
    let (digits, unit) = text.split_at(text.len().checked_sub(1)?);
    let value: u64 = digits.trim().parse().ok()?;
    let seconds = match unit {
        "s" | "S" => 1,
        "m" | "M" => 60,
        "h" | "H" => 3600,
        "d" | "D" => 86_400,
        _ => return None,
    };
    Some(Duration::from_secs(value.checked_mul(seconds)?))
}

pub fn render_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    for (unit, size) in [("d", 86_400u64), ("h", 3600), ("m", 60)] {
        if seconds >= size && seconds % size == 0 {
            return format!("{}{unit}", seconds / size);
        }
    }
    format!("{seconds}s")
}

pub fn strategy_preview(mode: &str, estimates: &[PoolEstimate]) -> Vec<String> {
    let exhausted: Vec<&str> = estimates
        .iter()
        .filter(|e| e.remaining == Remaining::Exhausted)
        .map(|e| e.pool.as_str())
        .collect();
    let measured = estimates
        .iter()
        .any(|e| e.confidence >= Confidence::SelfMetered);

    let mut lines = Vec::new();
    lines.push(match mode {
        "value-max" => "value-max: prepaid capacity that expires unused has zero marginal cost, \
                        so surplus near a reset would bias default tiers UP (spend-down), bounded \
                        by each task's min/max and the pool reserve"
            .to_owned(),
        "deadline" => "deadline: wall-clock first — throughput within capacity, spilling to API \
                       dollars where a $/hour ceiling justified it"
            .to_owned(),
        _ => "conserve: route down aggressively, escalate only on failure, and defer \
              frontier-hungry tasks toward a window reset when a pool is projected to run dry"
            .to_owned(),
    });
    if !exhausted.is_empty() {
        lines.push(format!(
            "exhausted now: {} — under a capacity-driven binder these would demote or defer \
             (§13); today a rate limit still only defers the task that hit it (§19)",
            exhausted.join(", ")
        ));
    }
    if !measured {
        lines.push(
            "no pool has a measured estimate yet, so every strategy above would be working from \
             the same absence of evidence"
                .to_owned(),
        );
    }
    lines.push(
        "v0.1 ships the capacity engine read-only (§13): none of the above changes what binds — \
         the binder still picks from the catalog and your pins."
            .to_owned(),
    );
    lines
}

#[derive(Debug, Clone, Default)]
pub struct CapacityOptions {
    pub config_path: Option<std::path::PathBuf>,
    pub pools_path: Option<std::path::PathBuf>,
    pub repo_root: std::path::PathBuf,
}

#[derive(Debug)]
pub struct CapacityReport {
    pub pools: Vec<Pool>,
    pub estimates: Vec<PoolEstimate>,
    pub agents: Vec<AgentStatus>,
    pub strategy: String,
    pub run_id: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct AgentStatus {
    pub agent: String,
    pub auth: String,
    pub version: Option<String>,
    pub notes: Vec<String>,
}

pub fn report(
    opts: &CapacityOptions,
    adapters: &dyn crate::agent::AdapterSource,
) -> Result<CapacityReport, crate::error::UpstrokeError> {
    let mut warnings = Vec::new();
    let config = crate::config::load(
        opts.config_path.as_deref(),
        &opts.repo_root,
        opts.pools_path.as_deref(),
        &mut warnings,
    )?;

    let (observations, run_id) = match crate::rundir::latest_run(&opts.repo_root) {
        Some(run_id) => {
            let path = crate::rundir::public_dir(&opts.repo_root, &run_id).join("events.jsonl");
            match crate::events::read_all(&path, &mut warnings) {
                Ok(events) => (observe(&events), Some(run_id)),
                Err(error) => {
                    warnings.push(format!(
                        "could not fold run {run_id} for self-metered spend ({error}); showing \
                         signals only"
                    ));
                    (Observations::default(), None)
                }
            }
        }
        None => (Observations::default(), None),
    };

    let mut agents: Vec<AgentStatus> = Vec::new();
    for pool in &config.pools {
        if agents.iter().any(|a| a.agent == pool.agent) {
            continue;
        }
        let Some(adapter) = adapters.get(&pool.agent) else {
            agents.push(AgentStatus {
                agent: pool.agent.clone(),
                auth: "no adapter in this build".to_owned(),
                version: None,
                notes: Vec::new(),
            });
            continue;
        };
        let runner = crate::runner::host::HostRunner::new();
        match adapter.probe(&runner).and_then(|caps| {
            adapter
                .discover(&runner, &caps)
                .map(|discovery| (caps.version.clone(), discovery))
        }) {
            Ok((version, discovery)) => {
                let missing = crate::catalog::missing_from(&pool.agent, &discovery.models);
                if !missing.is_empty() {
                    warnings.push(format!(
                        "{} does not advertise catalogued model(s): {}. Pins and cross-family \
                         review may bind to a `--model` value this CLI rejects — upgrade upstroke \
                         or pin a model it lists.",
                        pool.agent,
                        missing.join(", ")
                    ));
                }
                agents.push(AgentStatus {
                    agent: pool.agent.clone(),
                    auth: discovery.auth.to_string(),
                    version: Some(version),
                    notes: discovery.notes,
                });
            }
            Err(error) => agents.push(AgentStatus {
                agent: pool.agent.clone(),
                auth: format!("could not probe: {error}"),
                version: None,
                notes: Vec::new(),
            }),
        }
    }

    let estimates = estimate(&config.pools, &observations);
    Ok(CapacityReport {
        pools: config.pools,
        estimates,
        agents,
        strategy: config.strategy.mode.clone(),
        run_id,
        warnings,
    })
}

impl CapacityReport {
    pub fn render(&self) -> String {
        use std::fmt::Write as _;

        let mut out = String::new();
        for warning in &self.warnings {
            let _ = writeln!(out, "warning: {warning}");
        }
        if self.pools.is_empty() {
            out.push_str(
                "no pools connected. Run `upstroke connect` to discover the agent CLIs on this \
                 machine and write ~/.upstroke/pools.toml.\n",
            );
            return out;
        }
        for status in &self.agents {
            let _ = writeln!(
                out,
                "{} {}: {}",
                status.agent,
                status.version.as_deref().unwrap_or("(version unknown)"),
                status.auth
            );
            for note in &status.notes {
                let _ = writeln!(out, "  {note}");
            }
        }
        out.push('\n');
        for (pool, estimate) in self.pools.iter().zip(&self.estimates) {
            let _ = writeln!(out, "{}", pool.describe());
            let _ = writeln!(out, "  {}", estimate.describe());
            for note in &estimate.notes {
                let _ = writeln!(out, "  - {note}");
            }
        }
        out.push('\n');
        match &self.run_id {
            Some(run_id) => {
                let _ = writeln!(
                    out,
                    "self-metered draw folded from run {run_id}, the latest in this repository"
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "no run in this repository yet, so estimates rest on rate-limit signals \
                     alone — estimates need a repo with runs"
                );
            }
        }
        let _ = writeln!(out, "strategy: {}", self.strategy);
        for line in strategy_preview(&self.strategy, &self.estimates) {
            let _ = writeln!(out, "  {line}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(name: &str) -> Pool {
        Pool::discovered(
            name,
            PoolKind::SubscriptionWindow,
            "claude-code",
            vec![Source::Signals, Source::SelfMetered],
        )
    }

    #[test]
    fn an_unmeasured_pool_is_unknown_never_full() {
        let estimates = estimate(&[pool("claude-max")], &Observations::default());
        assert_eq!(estimates[0].remaining, Remaining::Unknown);
        assert_eq!(estimates[0].confidence, Confidence::Unknown);
        assert!(
            estimates[0].notes.iter().any(|n| n.contains("not full")),
            "notes: {:?}",
            estimates[0].notes
        );
    }

    #[test]
    fn margins_apply_multiplicatively_then_subtract_the_reserve() {
        let pool = pool("claude-max");
        assert!((effective_remaining(0.5, &pool) - 0.225).abs() < 1e-9);
        assert_eq!(effective_remaining(0.1, &pool), 0.0);
        assert!((effective_remaining(2.0, &pool) - 0.65).abs() < 1e-9);
        assert_eq!(effective_remaining(f64::NAN, &pool), 0.0);

        let mut generous = pool.clone();
        generous.safety_margin = 0.0;
        generous.reserve = 0.0;
        assert!((effective_remaining(0.5, &generous) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_self_metered_estimate_is_conservative_end_to_end() {
        let mut p = pool("api");
        p.kind = PoolKind::ApiKey;
        p.monthly_allowance = Allowance::Units(100.0);
        let mut obs = Observations::default();
        obs.self_spend.insert(
            "api".to_owned(),
            Spend {
                usd: Some(50.0),
                attempts: 4,
                unpriced: 0,
            },
        );
        let estimates = estimate(&[p], &obs);
        assert_eq!(estimates[0].confidence, Confidence::SelfMetered);
        let Remaining::AtMost(left) = estimates[0].remaining else {
            panic!("expected an upper bound: {:?}", estimates[0].remaining);
        };
        assert!((left - 0.225).abs() < 1e-9, "left: {left}");
        assert!(
            estimates[0].describe().contains("≤22%"),
            "{}",
            estimates[0].describe()
        );
        assert!(
            estimates[0]
                .notes
                .iter()
                .any(|n| n.contains("a ceiling, not a measurement")),
            "notes: {:?}",
            estimates[0].notes
        );
    }

    #[test]
    fn a_pool_that_serves_again_stops_reading_as_exhausted() {
        use crate::events::{AttemptRecord, Event, EventBody, PoolExhausted};
        use crate::ladder::{FailureKind, FailureOrigin};

        let record = |failure: Option<FailureKind>| {
            Event::now(EventBody::AttemptFinished {
                task: "t1".to_owned(),
                attempt: 1,
                rung: 0,
                profile: "p".to_owned(),
                parking: None,
                transition: None,
                prepared_commit: None,
                data: Box::new(AttemptRecord {
                    attempt: 1,
                    tier: "small".to_owned(),
                    model: "m".to_owned(),
                    pool: Some("claude-max".to_owned()),
                    resumed: false,
                    duration: Duration::ZERO,
                    cost_usd: None,
                    reviews: Vec::new(),
                    session_id: None,
                    usage: None,
                    failure: failure.map(|kind| crate::events::FailureRecord {
                        kind,
                        origin: FailureOrigin::Worker,
                        reason: "r".to_owned(),
                        detail: None,
                    }),
                }),
            })
        };
        let signal = Event::now(EventBody::PoolExhausted {
            task: "t1".to_owned(),
            data: PoolExhausted {
                pool: "claude-max".to_owned(),
                agent: "claude-code".to_owned(),
                reset_at: None,
                detail: "5-hour limit reached".to_owned(),
            },
        });

        let obs = observe(&[signal.clone(), record(None)]);
        assert!(obs.exhausted.is_empty(), "{:?}", obs.exhausted);

        let obs = observe(&[signal.clone(), record(Some(FailureKind::GateFailed))]);
        assert!(obs.exhausted.is_empty(), "{:?}", obs.exhausted);

        for still_down in [FailureKind::RateLimited, FailureKind::Interrupted] {
            let obs = observe(&[signal.clone(), record(Some(still_down))]);
            assert!(
                obs.exhausted.contains_key("claude-max"),
                "{still_down:?} must not retire the signal"
            );
        }

        let obs = observe(&[signal.clone(), record(None), signal]);
        assert!(obs.exhausted.contains_key("claude-max"));

        let reviewer_limited = Event::now(EventBody::AttemptFinished {
            task: "t1".to_owned(),
            attempt: 2,
            rung: 0,
            profile: "p".to_owned(),
            parking: None,
            transition: None,
            prepared_commit: None,
            data: Box::new(AttemptRecord {
                attempt: 2,
                tier: "small".to_owned(),
                model: "worker".to_owned(),
                pool: Some("worker-pool".to_owned()),
                resumed: false,
                duration: Duration::ZERO,
                cost_usd: None,
                reviews: vec![
                    crate::events::ReviewRecord {
                        pass: "review".to_owned(),
                        agent: "codex".to_owned(),
                        model: "sol".to_owned(),
                        adapter: None,
                        preflight_cli_version: None,
                        effort: None,
                        pool: Some("recovered-reviewer".to_owned()),
                        cost_usd: None,
                        outcome: crate::events::ReviewPassOutcome::Passed,
                    },
                    crate::events::ReviewRecord {
                        pass: "second-opinion".to_owned(),
                        agent: "claude-code".to_owned(),
                        model: "opus".to_owned(),
                        adapter: None,
                        preflight_cli_version: None,
                        effort: None,
                        pool: Some("limited-reviewer".to_owned()),
                        cost_usd: None,
                        outcome: crate::events::ReviewPassOutcome::Unavailable,
                    },
                ],
                session_id: None,
                usage: None,
                failure: Some(crate::events::FailureRecord {
                    kind: FailureKind::RateLimited,
                    origin: FailureOrigin::Reviewer,
                    reason: "review pool limited".to_owned(),
                    detail: None,
                }),
            }),
        });
        let mut prior = Observations::default();
        for pool in ["worker-pool", "recovered-reviewer", "limited-reviewer"] {
            prior.exhausted.insert(pool.to_owned(), None);
        }
        let mut events = Vec::new();
        for pool in prior.exhausted.keys() {
            events.push(Event::now(EventBody::PoolExhausted {
                task: "t1".to_owned(),
                data: PoolExhausted {
                    pool: pool.clone(),
                    agent: "agent".to_owned(),
                    reset_at: None,
                    detail: "old signal".to_owned(),
                },
            }));
        }
        events.push(reviewer_limited);
        let obs = observe(&events);
        assert!(!obs.exhausted.contains_key("worker-pool"), "{obs:?}");
        assert!(!obs.exhausted.contains_key("recovered-reviewer"), "{obs:?}");
        assert!(obs.exhausted.contains_key("limited-reviewer"), "{obs:?}");
    }

    #[test]
    fn self_metering_cannot_talk_an_exhausted_pool_back_up() {
        let mut p = pool("claude-max");
        p.monthly_allowance = Allowance::Units(100.0);
        let mut obs = Observations::default();
        obs.exhausted.insert(
            "claude-max".to_owned(),
            Some("2026-08-09T18:00:00Z".to_owned()),
        );
        obs.self_spend.insert(
            "claude-max".to_owned(),
            Spend {
                usd: Some(1.0),
                attempts: 1,
                unpriced: 0,
            },
        );
        let estimates = estimate(&[p], &obs);
        assert_eq!(estimates[0].remaining, Remaining::Exhausted);
        assert_eq!(estimates[0].confidence, Confidence::Signal);
        assert_eq!(
            estimates[0].reset_at.as_deref(),
            Some("2026-08-09T18:00:00Z")
        );
        assert_eq!(
            estimates[0].self_spend.as_ref().map(|s| s.attempts),
            Some(1)
        );
    }

    #[test]
    fn an_unknown_allowance_reports_the_draw_without_inventing_a_ceiling() {
        let mut obs = Observations::default();
        obs.self_spend.insert(
            "claude-max".to_owned(),
            Spend {
                usd: Some(3.0),
                attempts: 2,
                unpriced: 1,
            },
        );
        let estimates = estimate(&[pool("claude-max")], &obs);
        assert_eq!(estimates[0].remaining, Remaining::Unknown);
        assert!(
            estimates[0].describe().contains("$3.0000?"),
            "{}",
            estimates[0].describe()
        );
    }

    #[test]
    fn unread_sources_get_a_note_rather_than_a_pretend_estimate() {
        let mut p = pool("claude-max");
        p.sources = vec![Source::Signals, Source::SelfMetered, Source::LocalLogs];
        let estimates = estimate(&[p], &Observations::default());
        assert!(
            estimates[0]
                .notes
                .iter()
                .any(|n| n.contains("local-logs") && n.contains("not read")),
            "notes: {:?}",
            estimates[0].notes
        );
    }

    #[test]
    fn a_local_pool_is_unmetered_by_shape_not_by_measurement() {
        let mut p = Pool::discovered("local", PoolKind::Unmetered, "aider", vec![Source::Signals]);
        p.usable = false;
        let estimates = estimate(&[p], &Observations::default());
        assert_eq!(estimates[0].remaining, Remaining::Unmetered);
        assert_eq!(estimates[0].confidence, Confidence::Assumed);
        assert!(
            estimates[0].notes.iter().any(|n| n.contains("no adapter")),
            "notes: {:?}",
            estimates[0].notes
        );
    }

    #[test]
    fn pool_selection_is_first_match_in_file_order() {
        let pools = vec![pool("claude-max-a"), pool("claude-max-b")];
        assert_eq!(
            pool_for("claude-code", &pools).map(|p| p.name.as_str()),
            Some("claude-max-a")
        );
        assert!(pool_for("copilot", &pools).is_none());
    }

    #[test]
    fn durations_round_trip_through_their_own_spellings() {
        for (text, secs) in [
            ("5h", 18_000u64),
            ("30m", 1800),
            ("7d", 604_800),
            ("90s", 90),
        ] {
            let parsed = parse_duration(text).expect(text);
            assert_eq!(parsed.as_secs(), secs);
            assert_eq!(render_duration(parsed), text);
        }
        for bad in [
            "",
            "h",
            "5",
            "5 hours",
            "-1h",
            "5x",
            "999999999999999999999h",
        ] {
            assert!(parse_duration(bad).is_none(), "should reject: {bad:?}");
        }
    }

    #[test]
    fn every_strategy_preview_says_it_changes_nothing() {
        for mode in ["conserve", "value-max", "deadline", "something-else"] {
            let lines = strategy_preview(mode, &[]);
            assert!(
                lines.iter().any(|l| l.contains("read-only")),
                "{mode}: {lines:?}"
            );
        }
    }
}
