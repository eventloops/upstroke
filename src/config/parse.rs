//! The `upstroke.toml` section readers (DESIGN.md §17).
//!
//! One function per section — `[runner]`, `[routing.effort]`, `[budgets]`,
//! `[[gates]]`, `[engine]`, `[interaction]` — each taking the section's raw
//! `toml::Value` and returning the typed shape the parent's `Config` is built
//! from. The parent owns the ladder that calls them and the types they return;
//! this module owns only the readings.
//!
//! **The error-versus-warning split is a rule, not a per-section taste.** A key
//! whose typo would silently delete a control the operator asked for is a hard
//! error; a key whose typo only degrades what the run can say about itself is a
//! warning that names the key. Each function's own documentation records which
//! side of that line its keys sit on and why, because the reason is what a
//! later reader needs in order to place a new key correctly.
//!
//! Nothing here reads a path, starts a process, or writes anything: every input
//! arrives as an already-parsed `toml::Value`, and the reading of the bytes it
//! came from belongs to `super::read`.

// **This child states its own lint level and inherits nothing.** A Rust lint
// level is scoped by the module tree rather than by the file, so an out-of-line
// child of `src/config.rs` inherits that file's inner
// `#![allow(clippy::disallowed_methods)]` unless it says otherwise --
// `PR6-LANEF-004`, and the mistake two W1 pull requests then made
// independently (#100 and #102). Nothing here reaches a governed primitive, so
// all three governed lints are DENIED and this module takes no
// `effects/allowlist.toml` row: a row records an allowance, and this module
// takes none.
//
// The three are not equally load-bearing, and which is which is worth stating.
// `src/config.rs` allows `clippy::disallowed_methods` and that lint alone, so
// the first line below is the one that restores a level the parent removed
// outright: without it, a denied method here raises no diagnostic at all. The
// other two raise this module from clippy's default `warn` to `deny`, so a
// denied type or macro fails here on its own rather than only under CI's
// `-D warnings`. All three are written out because what decides the first one
// is a property of the parent's attribute rather than of this file, and a
// parent's attribute can widen without this file changing.
#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::disallowed_macros
)]

use super::{
    AskBefore, Budgets, DEFAULT_GATE_TIMEOUT, DEFAULT_MAX_MERGE_REPAIRS, DEFAULT_MAX_PARALLEL,
    DEFAULT_WAIT_ON_BLOCK, Duration, Effort, EngineLimits, EngineSettings, GateConfig,
    InteractionMode, InteractionSettings, OnTaskFailure, Path, RUNNER_KEYS, RawEngine, RawGate,
    RawInteraction, RawRunner, RunnerKind, RunnerMount, RunnerSelection, ShellKind, UpstrokeError,
    check_budget,
};

/// `[runner]` as written, with no policy applied.
pub(super) fn read_runner(
    raw: Option<toml::Value>,
    repo_path: &Path,
) -> Result<RunnerSelection, UpstrokeError> {
    let config_error = |message: String| UpstrokeError::Config {
        path: repo_path.to_path_buf(),
        message,
    };
    let Some(value) = raw else {
        return Ok(RunnerSelection::host_default());
    };
    let runner: RawRunner = value.try_into().map_err(|e| {
        config_error(format!(
            "[runner]: {e} (expected a table with optional {RUNNER_KEYS}, where `kind` is \
             `host` or `container`, `image` is an image reference, `credential_volumes` maps \
             an agent id to a volume name, and `mounts` is a list of \
             `{{ source, target, read_only }}` tables)"
        ))
    })?;
    // An error, where `[engine]` warns. `[engine]`'s unknown keys are ceilings
    // and timeouts: a typo leaves a default in place and the run does slightly
    // less than asked. A typo here — `knid = "container"`, `iamge = "..."` —
    // leaves the run executing on the **host** while the operator believes gate
    // code is confined, which is the one thing this section exists to decide.
    // Same rule as `[interaction] ask_before` and `[budgets]`, for the same
    // reason: silently ignoring a key is silently deleting a control.
    if let Some(key) = runner.unknown.keys().next() {
        return Err(config_error(format!(
            "unknown key `{key}` in [runner] (accepted: {RUNNER_KEYS})"
        )));
    }
    let kind = match runner.kind.as_deref() {
        None => RunnerKind::Host,
        Some("host") => RunnerKind::Host,
        Some("container") => RunnerKind::Container,
        Some(other) => {
            return Err(config_error(format!(
                "[runner] `kind = \"{other}\"` is not recognized (expected `host` or `container`)"
            )));
        }
    };
    if kind == RunnerKind::Host {
        // The config-side twin of `RunnerRecordDefect::HostWithContainerFields`,
        // which PR3 already refuses on the recorded side. An operator who set
        // `kind = "host"` under an image line has described two boundaries and
        // gets one; accepting it silently is how a run executes unconfined
        // while its config reads as if it did not.
        let stray: Vec<&str> = [
            ("image", runner.image.is_some()),
            ("credential_volumes", runner.credential_volumes.is_some()),
            ("mounts", runner.mounts.is_some()),
        ]
        .into_iter()
        .filter_map(|(key, present)| present.then_some(key))
        .collect();
        if !stray.is_empty() {
            return Err(config_error(format!(
                "[runner] `kind = \"host\"` with `{}`: the host runner has no image, no \
                 credential volumes and no mounts to give — remove the keys, or set \
                 `kind = \"container\"` to use them",
                stray.join("`, `")
            )));
        }
    }
    let image = match runner.image {
        Some(image) if image.trim().is_empty() => {
            return Err(config_error(
                "[runner] `image` is empty; give the image reference the runtime already holds \
                 (nothing is pulled), or remove the key"
                    .to_owned(),
            ));
        }
        other => other,
    };
    if kind == RunnerKind::Container && image.is_none() {
        return Err(config_error(
            "[runner] `kind = \"container\"` without `image`: nothing names what would execute \
             (INV-23 records the image reference, the runtime's immutable id, and its manifest \
             digest when reported)"
                .to_owned(),
        ));
    }
    let credential_volumes = runner.credential_volumes.unwrap_or_default();
    for (agent, volume) in &credential_volumes {
        if agent.trim().is_empty() || volume.trim().is_empty() {
            return Err(config_error(format!(
                "[runner] credential_volumes entry `{agent}` = `{volume}`: both the agent id \
                 and the volume name must be non-empty"
            )));
        }
    }
    let mut mounts = Vec::new();
    for mount in runner.mounts.unwrap_or_default() {
        if mount.target.trim().is_empty() {
            return Err(config_error(
                "[runner] a mount has an empty `target`; give the path the boundary sees it at"
                    .to_owned(),
            ));
        }
        if mount.source.as_os_str().is_empty() {
            return Err(config_error(format!(
                "[runner] the mount at `{}` has an empty `source`",
                mount.target
            )));
        }
        mounts.push(RunnerMount {
            source: mount.source,
            target: mount.target,
            // Writable is a thing you say. See `RunnerMount`.
            read_only: mount.read_only.unwrap_or(true),
        });
    }
    Ok(RunnerSelection {
        kind,
        image,
        credential_volumes,
        mounts,
        from_config: true,
    })
}

/// `expected_failures_refusals[0]`: "`[runner] kind = container` under a
/// schema-1..3 fresh run **or** resume -> config error before any effect".
///
/// ## Why this is structural and not stylistic
///
/// `production_effect`: "the legacy engine's preflight probes precede any run
/// identity or lock and can own no container intent". R26's own rule is that
/// "no container ever lacks a race-free owner or a durable boundary identity",
/// and the schema-1..3 engine has nothing to give one: it probes before it has a
/// run id, a `run.lock` or a recorded runner. So refusing **late** — after a
/// probe, after a lock, after any effect — is not a weaker version of this
/// refusal, it is a different and broken one: the container it would refuse
/// already exists and belongs to nobody.
///
/// ## Where "before any effect" is bought
///
/// Here, by position. Both write commands run
/// `preflight::validate_inputs` — which is `config::load_captured` — as their
/// first statement, before `Workspace::open`, before `WorktreeLock::acquire_in`
/// and before `RunPaths::create`: `coordinator.rs`'s comment on that line is
/// "every read-only refusal precedes every lock", and `resume.rs` marks the
/// line after it "the first effect of the command".
/// `runner::container::resolve::tests::legacy_container_selection_refused_before_effects`
/// drives both commands and asserts the tree afterwards.
///
/// ## Both readings refuse, and that is the whole of today's answer
///
/// [`EngineLimits`] distinguishes a run being created from a sequential run's
/// resume, and `expected_failures_refusals[0]` names **both**. There is no
/// third reading in this build: `EngineLimits::Fresh` means "a run being
/// created now", and every run this binary creates is schema-3.
/// `PR12 config acceptance for fresh schema-4 runs only` (INV-23's
/// `enforced_by`) is where a fresh schema-4 run learns to accept it, and that is
/// a new reading rather than a relaxation of this one.
///
/// # Errors
///
/// [`UpstrokeError::Config`] when `selection` is a container selection.
pub(super) fn refuse_legacy_container_selection(
    selection: &RunnerSelection,
    repo_path: &Path,
    limits: EngineLimits,
) -> Result<(), UpstrokeError> {
    if selection.kind != RunnerKind::Container {
        return Ok(());
    }
    let reading = match limits {
        EngineLimits::Fresh => "this run is being created by the schema-1..3 engine",
        EngineLimits::SequentialResume => {
            "this run was recorded by the schema-1..3 engine and keeps the boundary it started \
             with"
        }
    };
    Err(UpstrokeError::Config {
        path: repo_path.to_path_buf(),
        message: format!(
            "[runner] `kind = \"container\"` is refused: {reading}, and that engine's pre-flight \
             probes run before any run identity or lock exists, so a container it started could \
             have no owner run, no `run.lock` and no recorded runner identity — which is exactly \
             what makes an orphaned container unreclaimable. Set `kind = \"host\"` or remove the \
             key; the container runner is selectable only by schema-4 runs."
        ),
    })
}

/// Parse one role's explicit effort at config load. All three providers reject
/// an unknown value after process launch, so accepting a typo here would burn an
/// attempt for a routing policy the operator never actually selected.
pub(super) fn parse_role_effort(
    raw: Option<&str>,
    role: &str,
    repo_path: &Path,
) -> Result<Option<Effort>, UpstrokeError> {
    let Some(raw) = raw else { return Ok(None) };
    Effort::parse(raw)
        .map(Some)
        .ok_or_else(|| UpstrokeError::Config {
            path: repo_path.to_path_buf(),
            message: format!(
                "[routing.effort] `{role} = \"{raw}\"` is not recognized (accepted: {})",
                Effort::KNOWN
            ),
        })
}

/// `[budgets]` (§17). A ceiling that is zero, negative, or not a number is a
/// hard error: every one of those readings would either stop the run before it
/// began or be ignored, and which of the two happened must not be a surprise.
pub(super) fn parse_budgets(
    raw: Option<toml::Value>,
    repo_path: &Path,
) -> Result<Budgets, UpstrokeError> {
    let Some(value) = raw else {
        return Ok(Budgets::default());
    };
    let budgets: Budgets = value.try_into().map_err(|e| UpstrokeError::Config {
        path: repo_path.to_path_buf(),
        message: format!(
            "[budgets]: {e} (expected optional `run_usd` and `task_usd` numbers, in \
             api-equivalent dollars)"
        ),
    })?;
    for (name, limit) in [("run_usd", budgets.run_usd), ("task_usd", budgets.task_usd)] {
        let Some(limit) = limit else { continue };
        check_budget(name, limit).map_err(|message| UpstrokeError::Config {
            path: repo_path.to_path_buf(),
            message: format!("[budgets] {message}"),
        })?;
    }
    Ok(budgets)
}

/// `[[gates]]` parsing with actionable shape errors: a `[gates]` table, a
/// wrong-typed field, or `timeout_secs = 0` all name what was expected.
pub(super) fn parse_gates(
    raw: Option<toml::Value>,
    repo_path: &Path,
) -> Result<Option<Vec<GateConfig>>, UpstrokeError> {
    let config_error = |message: String| UpstrokeError::Config {
        path: repo_path.to_path_buf(),
        message,
    };
    let Some(value) = raw else { return Ok(None) };
    let toml::Value::Array(entries) = value else {
        return Err(config_error(format!(
            "`gates` must be an array of tables — write `[[gates]]` entries (double brackets, \
             one per gate), found a {}",
            value.type_str()
        )));
    };
    let mut list = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let n = index + 1;
        let g: RawGate = entry.try_into().map_err(|e| {
            config_error(format!(
                "[[gates]] entry {n}: {e} (each entry takes `name`, `cmd`, and an optional \
                 `timeout_secs` integer)"
            ))
        })?;
        if g.name.trim().is_empty() || g.cmd.trim().is_empty() {
            return Err(config_error(format!(
                "[[gates]] entry {n} needs a non-empty `name` and `cmd`"
            )));
        }
        if g.timeout_secs == Some(0) {
            return Err(config_error(format!(
                "[[gates]] entry {n} (`{}`): timeout_secs must be at least 1 — omit it for the \
                 {}s default",
                g.name,
                DEFAULT_GATE_TIMEOUT.as_secs()
            )));
        }
        list.push(GateConfig {
            name: g.name,
            cmd: g.cmd,
            timeout: g
                .timeout_secs
                .map_or(DEFAULT_GATE_TIMEOUT, Duration::from_secs),
        });
    }
    Ok(Some(list))
}

/// `[engine]` (§17).
///
/// Every key here is now consumed, refused, or named in a warning. Nothing is
/// read past: accepting `max_parallel = 4` and then running one attempt at a
/// time is the failure a config file exists to prevent — the operator believes
/// they bought four workers, the run costs and takes what one worker costs and
/// takes, and nothing anywhere says otherwise. That is the same silent-ignore
/// harm `second_opinion` and `[budgets] pool_fraction` each earned a refusal
/// for, and it is this section's own long-standing defect.
///
/// The three ceilings split from `max_parallel` on which reading is wrong.
/// `max_parallel` above 1 describes a run **this engine cannot perform**, so on
/// a fresh run it is a hard error — raised here, which is before a lock, a
/// workspace, or a run directory exists. `max_merge_repairs`, `max_per_agent`,
/// and `max_per_pool` bound a topology that arrives with the parallel engine; a
/// nondefault value is a true statement about a later run and a silent no-op in
/// this one, so it parses, is kept, and warns.
///
/// `limits` is what keeps that refusal from reaching a run it cannot help. See
/// [`EngineLimits`]: on a sequential run's resume every one of these keys is
/// about a future run, `max_parallel` included, so all four warn and the resume
/// continues on the ceiling it recorded.
pub(super) fn parse_engine(
    raw: Option<toml::Value>,
    repo_path: &Path,
    limits: EngineLimits,
    warnings: &mut Vec<String>,
) -> Result<EngineSettings, UpstrokeError> {
    let config_error = |message: String| UpstrokeError::Config {
        path: repo_path.to_path_buf(),
        message,
    };
    let Some(value) = raw else {
        return Ok(EngineSettings {
            shell: ShellKind::native(),
            on_task_failure: OnTaskFailure::Halt,
            max_parallel: DEFAULT_MAX_PARALLEL,
            max_merge_repairs: DEFAULT_MAX_MERGE_REPAIRS,
            max_per_agent: DEFAULT_MAX_PARALLEL,
            max_per_pool: DEFAULT_MAX_PARALLEL,
        });
    };
    let engine: RawEngine = value.try_into().map_err(|e| {
        config_error(format!(
            "[engine]: {e} (expected a table with optional `shell` and `on_task_failure` \
             strings, and optional `max_parallel`, `max_merge_repairs`, `max_per_agent`, and \
             `max_per_pool` whole numbers of at least 1)"
        ))
    })?;
    for key in engine.unknown.keys() {
        warnings.push(format!(
            "unknown key `{key}` in [engine] in {} (ignored)",
            repo_path.display()
        ));
    }
    let shell = match engine.shell {
        None => ShellKind::native(),
        Some(requested) => ShellKind::parse(&requested).unwrap_or_else(|| {
            warnings.push(format!(
                "unknown [engine] shell `{requested}` in {} (using the platform default; known: \
                 cmd, sh, bash, powershell, pwsh)",
                repo_path.display()
            ));
            ShellKind::native()
        }),
    };
    // A misspelling here decides whether a failed task stops the run, so it
    // errors rather than warning: silently halting a run the user asked to
    // continue (or the reverse) is not a recoverable surprise.
    let on_task_failure = match engine.on_task_failure {
        None => OnTaskFailure::Halt,
        Some(requested) => OnTaskFailure::parse(&requested).ok_or_else(|| {
            config_error(format!(
                "[engine] on_task_failure `{requested}` is not recognized (expected `halt` or \
                     `continue`)"
            ))
        })?,
    };
    // Zero has two readings — "no ceiling" and "nothing may run" — and which one
    // happened must never be a surprise. The rule `attempts_per` and every
    // `timeout_secs` already follow.
    let limit = |key: &str, configured: Option<u32>, default: u32| -> Result<u32, UpstrokeError> {
        match configured {
            Some(0) => Err(config_error(format!(
                "[engine] `{key} = 0` is not a limit — omit it for the default of {default}, or \
                 give it a whole number of at least 1"
            ))),
            Some(value) => Ok(value),
            None => Ok(default),
        }
    };
    let configured_parallel = limit("max_parallel", engine.max_parallel, DEFAULT_MAX_PARALLEL)?;
    // What this load's run will actually be allowed to do. It parts company
    // with what the file says in exactly one case — a sequential run's resume,
    // whose ceiling is a fact about the run and not about today's config — and
    // that case says so out loud below rather than carrying the file's number
    // into a Config field nothing may act on.
    let max_parallel = match (limits, configured_parallel > DEFAULT_MAX_PARALLEL) {
        (_, false) => configured_parallel,
        (EngineLimits::Fresh, true) => {
            return Err(config_error(format!(
                "[engine] `max_parallel = {configured_parallel}` is refused: this engine runs one \
                 attempt at a time, so the run would cost and take what one worker costs and \
                 takes while the config claims {configured_parallel} — set `max_parallel = \
                 {DEFAULT_MAX_PARALLEL}` or omit it until parallel execution ships"
            )));
        }
        (EngineLimits::SequentialResume, true) => {
            warnings.push(format!(
                "[engine] `max_parallel = {configured_parallel}` in {} is parsed but not acted on \
                 by this resume: this run was recorded by an engine that runs one attempt at a \
                 time, and a run keeps the execution shape it started with, so it continues at \
                 `max_parallel = {DEFAULT_MAX_PARALLEL}`. A fresh run refuses this value outright \
                 until parallel execution ships.",
                repo_path.display()
            ));
            DEFAULT_MAX_PARALLEL
        }
    };
    let max_merge_repairs = limit(
        "max_merge_repairs",
        engine.max_merge_repairs,
        DEFAULT_MAX_MERGE_REPAIRS,
    )?;
    // Defaulted off what the file asked for rather than off the effective
    // ceiling: `max_parallel = 3` with neither companion written is one
    // statement about a future run, and splitting it into a refused 3 and two
    // inherited 1s would announce two edits the operator never made.
    let max_per_agent = limit("max_per_agent", engine.max_per_agent, configured_parallel)?;
    let max_per_pool = limit("max_per_pool", engine.max_per_pool, configured_parallel)?;
    // Kept, and announced as inert. A warning rather than an error because the
    // value is not wrong — it is simply about a run this build cannot perform
    // yet, and erroring would refuse a config an operator wrote for the engine
    // they are waiting for.
    for (key, configured, default) in [
        (
            "max_merge_repairs",
            max_merge_repairs,
            DEFAULT_MAX_MERGE_REPAIRS,
        ),
        ("max_per_agent", max_per_agent, max_parallel),
        ("max_per_pool", max_per_pool, max_parallel),
    ] {
        if configured != default {
            warnings.push(format!(
                "[engine] `{key} = {configured}` in {} is parsed but not acted on by this engine, \
                 which runs one attempt and merges one candidate at a time (default {default})",
                repo_path.display()
            ));
        }
    }
    Ok(EngineSettings {
        shell,
        on_task_failure,
        max_parallel,
        max_merge_repairs,
        max_per_agent,
        max_per_pool,
    })
}

/// `[interaction]` (§12).
///
/// Everything here is a hard error or nothing: `mode` and `ask_before` both
/// decide whether a human is ever asked, so a typo in either must not degrade
/// quietly. Notifier ids are the one soft setting, and they are validated by
/// `notifiers_for` at run time rather than here — which is why this function
/// takes no warning sink.
pub(super) fn parse_interaction(
    raw: Option<toml::Value>,
    repo_path: &Path,
) -> Result<InteractionSettings, UpstrokeError> {
    let default_notify = || vec!["cli".to_owned()];
    let Some(value) = raw else {
        return Ok(InteractionSettings {
            mode: InteractionMode::default(),
            notify: default_notify(),
            wait_on_block: DEFAULT_WAIT_ON_BLOCK,
            ask_before: AskBefore::default(),
        });
    };
    let interaction: RawInteraction = value.try_into().map_err(|e| UpstrokeError::Config {
        path: repo_path.to_path_buf(),
        message: format!(
            "[interaction]: {e} (expected optional `mode`, `notify` list, \
             `wait_on_block_secs`, and `ask_before` table)"
        ),
    })?;
    // An unknown key inside `ask_before` errors rather than warning: the whole
    // point of the table is to stop the run and ask, so a misspelling that
    // silently drops the threshold spends the money the operator asked to be
    // consulted about. Same reasoning as `second_opinion`.
    let ask_before = match interaction.ask_before {
        None => AskBefore::default(),
        Some(value) => value.try_into().map_err(|e| UpstrokeError::Config {
            path: repo_path.to_path_buf(),
            message: format!(
                "[interaction] ask_before: {e} (accepted: {})",
                AskBefore::ACCEPTED.join(", ")
            ),
        })?,
    };
    if let Some(threshold) = ask_before.frontier_escalation_over_usd {
        if !threshold.is_finite() || threshold < 0.0 {
            return Err(UpstrokeError::Config {
                path: repo_path.to_path_buf(),
                message: format!(
                    "[interaction] ask_before `frontier_escalation_over_usd = {threshold}` is not a \
                     spend threshold — omit the key to never ask, or give it a number of dollars"
                ),
            });
        }
    }
    let mode = match interaction.mode {
        None => InteractionMode::default(),
        Some(requested) => {
            InteractionMode::parse(&requested).ok_or_else(|| UpstrokeError::Config {
                path: repo_path.to_path_buf(),
                message: format!(
                    "[interaction] mode `{requested}` is not recognized (expected `never`, \
                     `on_block`, or `on_milestone`)"
                ),
            })?
        }
    };
    Ok(InteractionSettings {
        mode,
        notify: interaction.notify.unwrap_or_else(default_notify),
        wait_on_block: interaction
            .wait_on_block_secs
            .map_or(DEFAULT_WAIT_ON_BLOCK, Duration::from_secs),
        ask_before,
    })
}
