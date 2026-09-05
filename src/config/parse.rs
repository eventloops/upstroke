//! Extended notes: `docs/internals/config/parse.md`

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
    let on_task_failure = match engine.on_task_failure {
        None => OnTaskFailure::Halt,
        Some(requested) => OnTaskFailure::parse(&requested).ok_or_else(|| {
            config_error(format!(
                "[engine] on_task_failure `{requested}` is not recognized (expected `halt` or \
                     `continue`)"
            ))
        })?,
    };
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
    let max_per_agent = limit("max_per_agent", engine.max_per_agent, configured_parallel)?;
    let max_per_pool = limit("max_per_pool", engine.max_per_pool, configured_parallel)?;
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
