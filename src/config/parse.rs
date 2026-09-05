//! Extended notes: `docs/internals/config/parse.md`

// `effects/allowlist.toml` row: a row records an allowance, and this module

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

fn config_error(repo_path: &Path, message: String) -> UpstrokeError {
    UpstrokeError::Config {
        path: repo_path.to_path_buf(),
        message,
    }
}

pub(super) fn read_runner(
    raw: Option<toml::Value>,
    repo_path: &Path,
) -> Result<RunnerSelection, UpstrokeError> {
    let Some(value) = raw else {
        return Ok(RunnerSelection::host_default());
    };
    let runner: RawRunner = value.try_into().map_err(|e| {
        config_error(
            repo_path,
            format!(
                "[runner]: {e} (expected a table with optional {RUNNER_KEYS}, where `kind` is \
                 `host` or `container`, `image` is an image reference, `credential_volumes` \
                 maps an agent id to a volume name, and `mounts` is a list of \
                 `{{ source, target, read_only }}` tables)"
            ),
        )
    })?;

    if !runner.unknown.is_empty() {
        let keys: Vec<&str> = runner.unknown.keys().map(String::as_str).collect();
        return Err(config_error(
            repo_path,
            format!(
                "unknown key `{}` in [runner] (accepted: {RUNNER_KEYS})",
                keys.join("`, `")
            ),
        ));
    }

    let kind = match runner.kind.as_deref() {
        None => RunnerKind::Host,
        Some("host") => RunnerKind::Host,
        Some("container") => RunnerKind::Container,
        Some(other) => {
            return Err(config_error(
                repo_path,
                format!(
                    "[runner] `kind = \"{other}\"` is not recognized (expected `host` or \
                     `container`)"
                ),
            ));
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
            let selected = match runner.kind {
                Some(_) => format!("`kind = \"host\"` with `{}`", stray.join("`, `")),
                None => format!(
                    "selects the host runner when `kind` is absent, and with `{}` it asks for a \
                     container's",
                    stray.join("`, `")
                ),
            };
            return Err(config_error(
                repo_path,
                format!(
                    "[runner] {selected}: the host runner has no image, no credential volumes and \
                     no mounts to give — remove the keys, or set `kind = \"container\"` to use \
                     them"
                ),
            ));
        }
    }
    let image = match runner.image {
        Some(image) if image.trim().is_empty() => {
            return Err(config_error(
                repo_path,
                "[runner] `image` is empty; give the image reference the runtime already holds \
                 (nothing is pulled), or remove the key"
                    .to_owned(),
            ));
        }
        other => other,
    };
    if kind == RunnerKind::Container && image.is_none() {
        return Err(config_error(
            repo_path,
            "[runner] `kind = \"container\"` without `image`: nothing names what would execute \
             (INV-23 records the image reference, the runtime's immutable id, and its manifest \
             digest when reported)"
                .to_owned(),
        ));
    }

    let credential_volumes = runner.credential_volumes.unwrap_or_default();
    for (agent, volume) in &credential_volumes {
        if agent.trim().is_empty() || volume.trim().is_empty() {
            return Err(config_error(
                repo_path,
                format!(
                    "[runner] credential_volumes entry `{agent}` = `{volume}`: both the agent id \
                     and the volume name must be non-empty"
                ),
            ));
        }
    }
    let mut mounts = Vec::new();

    for mount in runner.mounts.unwrap_or_default() {
        if mount.target.trim().is_empty() {
            return Err(config_error(
                repo_path,
                "[runner] a mount has an empty `target`; give the path the boundary sees it at"
                    .to_owned(),
            ));
        }
        if mount.source.as_os_str().is_empty() {
            return Err(config_error(
                repo_path,
                format!(
                    "[runner] the mount at `{}` has an empty `source`",
                    mount.target
                ),
            ));
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
        EngineLimits::SequentialResumeWithRecordedGates | EngineLimits::SequentialResume => {
            "this run was recorded by the schema-1..3 engine and keeps the boundary it started \
             with"
        }
    };
    Err(config_error(
        repo_path,
        format!(
            "[runner] `kind = \"container\"` is refused: {reading}, and that engine's pre-flight \
             probes run before any run identity or lock exists, so a container it started could \
             have no owner run, no `run.lock` and no recorded runner identity — which is exactly \
             what makes an orphaned container unreclaimable. Set `kind = \"host\"` or remove the \
             key; the container runner is selectable only by schema-4 runs."
        ),
    ))
}

pub(super) fn parse_role_effort(
    raw: Option<&str>,
    role: &str,
    repo_path: &Path,
) -> Result<Option<Effort>, UpstrokeError> {
    let Some(raw) = raw else { return Ok(None) };
    Effort::parse(raw).map(Some).ok_or_else(|| {
        config_error(
            repo_path,
            format!(
                "[routing.effort] `{role} = \"{raw}\"` is not recognized (accepted: {})",
                Effort::KNOWN
            ),
        )
    })
}

pub(super) fn parse_budgets(
    raw: Option<toml::Value>,
    repo_path: &Path,
) -> Result<Budgets, UpstrokeError> {
    let Some(value) = raw else {
        return Ok(Budgets::default());
    };
    let budgets: Budgets = value.try_into().map_err(|e| {
        config_error(
            repo_path,
            format!(
                "[budgets]: {e} (expected optional `run_usd` and `task_usd` numbers, in \
                 api-equivalent dollars)"
            ),
        )
    })?;
    for (name, limit) in [("run_usd", budgets.run_usd), ("task_usd", budgets.task_usd)] {
        let Some(limit) = limit else { continue };
        check_budget(name, limit)
            .map_err(|message| config_error(repo_path, format!("[budgets] {message}")))?;
    }
    Ok(budgets)
}

const MAX_GATE_TIMEOUT_SECS: u64 = u64::MAX / 1000;

pub(super) fn parse_gates(
    raw: Option<toml::Value>,
    repo_path: &Path,
    limits: EngineLimits,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<GateConfig>>, UpstrokeError> {
    let reading = match limits {
        EngineLimits::Fresh | EngineLimits::SequentialResume => GatesReading::Governs,
        EngineLimits::SequentialResumeWithRecordedGates => GatesReading::ComparedOnly,
    };
    let Some(value) = raw else { return Ok(None) };
    let toml::Value::Array(entries) = value else {
        let problem = format!(
            "`gates` must be an array of tables — write `[[gates]]` entries (double brackets, \
             one per gate), found a {}",
            value.type_str()
        );
        refuse_or_announce(reading, problem, repo_path, warnings)?;

        warnings.push(format!(
            "today's `gates` in {} cannot be compared with the gates this run recorded, because it \
             is not a list of entries; if a difference is reported below, it is between the \
             record and the gates derived from the repository's shape, not between the record \
             and this file",
            repo_path.display()
        ));
        return Ok(None);
    };
    let mut list: Vec<GateConfig> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let n = index + 1;
        let g: RawGate = match entry.try_into() {
            Ok(g) => g,
            Err(e) => {
                let problem = format!(
                    "[[gates]] entry {n}: {e} (each entry takes `name`, `cmd`, and an optional \
                     `timeout_secs` integer)"
                );
                refuse_or_announce(reading, problem, repo_path, warnings)?;

                continue;
            }
        };
        let unknown: Vec<&str> = g.unknown.keys().map(String::as_str).collect();

        let (name, cmd) = match (g.name, g.cmd) {
            (Some(name), Some(cmd)) => (name, cmd),
            (name, cmd) => {
                let missing: Vec<&str> = [("name", name.is_none()), ("cmd", cmd.is_none())]
                    .into_iter()
                    .filter_map(|(key, absent)| absent.then_some(key))
                    .collect();
                let mut problem = format!(
                    "[[gates]] entry {n} is missing `{}`",
                    missing.join("` and `")
                );
                if !unknown.is_empty() {
                    problem.push_str(&format!(
                        " and has unknown key `{}` — a misspelling of what is missing?",
                        unknown.join("`, `")
                    ));
                }
                problem.push_str(
                    " (each entry takes `name`, `cmd`, and an optional `timeout_secs` integer)",
                );
                refuse_or_announce(reading, problem, repo_path, warnings)?;

                continue;
            }
        };
        let blank: Vec<&str> = [
            ("name", name.trim().is_empty()),
            ("cmd", cmd.trim().is_empty()),
        ]
        .into_iter()
        .filter_map(|(key, is_blank)| is_blank.then_some(key))
        .collect();
        if !blank.is_empty() {
            refuse_or_announce(
                reading,
                format!(
                    "[[gates]] entry {n}: `{}` is blank — each entry needs a non-empty `name` \
                     and `cmd`",
                    blank.join("` and `")
                ),
                repo_path,
                warnings,
            )?;
        }
        if !unknown.is_empty() {
            refuse_or_announce(
                reading,
                format!(
                    "[[gates]] entry {n} (`{name}`) has unknown key `{}` (each entry takes \
                     `name`, `cmd`, and an optional `timeout_secs` integer)",
                    unknown.join("`, `")
                ),
                repo_path,
                warnings,
            )?;
        }
        if let Some((earlier, first)) = list
            .iter()
            .enumerate()
            .find(|(_, gate)| gate.name.eq_ignore_ascii_case(&name))
        {
            refuse_or_announce(
                reading,
                format!(
                    "[[gates]] entry {n} repeats the name `{name}` of entry {} (`{}`; names are \
                     compared without regard to ASCII case, because a case-insensitive \
                     filesystem keeps one log file for both): a gate's name is what its log \
                     file and its failure report carry, so two gates cannot share one — give \
                     each gate a name of its own",
                    earlier + 1,
                    first.name
                ),
                repo_path,
                warnings,
            )?;
        }
        if g.timeout_secs == Some(0) {
            refuse_or_announce(
                reading,
                format!(
                    "[[gates]] entry {n} (`{name}`): timeout_secs must be at least 1 — omit it \
                     for the {}s default",
                    DEFAULT_GATE_TIMEOUT.as_secs()
                ),
                repo_path,
                warnings,
            )?;
        }
        if let Some(secs) = g.timeout_secs.filter(|secs| *secs > MAX_GATE_TIMEOUT_SECS) {
            refuse_or_announce(
                reading,
                format!(
                    "[[gates]] entry {n} (`{name}`): timeout_secs = {secs} is more than the run \
                     record can hold — at most {MAX_GATE_TIMEOUT_SECS} seconds, because \
                     `run_started` records a gate's timeout in milliseconds as a 64-bit count and \
                     a larger value would be recorded saturated and resumed smaller than this file \
                     says"
                ),
                repo_path,
                warnings,
            )?;
        }
        list.push(GateConfig {
            name,
            cmd,

            timeout: g
                .timeout_secs
                .map_or(DEFAULT_GATE_TIMEOUT, Duration::from_secs),
        });
    }
    Ok(Some(list))
}

#[derive(Clone, Copy)]
enum GatesReading {
    Governs,

    ComparedOnly,
}

fn refuse_or_announce(
    reading: GatesReading,
    problem: String,
    repo_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<(), UpstrokeError> {
    match reading {
        GatesReading::Governs => Err(config_error(repo_path, problem)),
        GatesReading::ComparedOnly => {
            warnings.push(format!(
                "{problem}; in {} this resume runs the gates its log recorded, so today's \
                 [[gates]] is read only to be compared with them — a fresh run, or a resume of \
                 a log with no gate record, refuses this shape",
                repo_path.display()
            ));
            Ok(())
        }
    }
}

const ENGINE_KEYS: &str = "`shell`, `on_task_failure`, `max_parallel`, `max_merge_repairs`, \
                           `max_per_agent`, `max_per_pool`";

pub(super) fn parse_engine(
    raw: Option<toml::Value>,
    repo_path: &Path,
    limits: EngineLimits,
    warnings: &mut Vec<String>,
) -> Result<EngineSettings, UpstrokeError> {
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
        config_error(
            repo_path,
            format!(
                "[engine]: {e} (expected a table with optional `shell` and `on_task_failure` \
                 strings, and optional `max_parallel`, `max_merge_repairs`, `max_per_agent`, \
                 and `max_per_pool` whole numbers of at least 1)"
            ),
        )
    })?;
    if !engine.unknown.is_empty() {
        let keys: Vec<&str> = engine.unknown.keys().map(String::as_str).collect();
        return Err(config_error(
            repo_path,
            format!(
                "unknown key `{}` in [engine] (accepted: {ENGINE_KEYS})",
                keys.join("`, `")
            ),
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
            config_error(
                repo_path,
                format!(
                    "[engine] on_task_failure `{requested}` is not recognized (expected `halt` or \
                     `continue`)"
                ),
            )
        })?,
    };

    let limit = |key: &str, configured: Option<u32>, default: u32| -> Result<u32, UpstrokeError> {
        match configured {
            Some(0) => Err(config_error(
                repo_path,
                format!(
                    "[engine] `{key} = 0` is not a limit — omit it for the default of {default}, \
                     or give it a whole number of at least 1"
                ),
            )),
            Some(value) => Ok(value),
            None => Ok(default),
        }
    };
    let configured_parallel = limit("max_parallel", engine.max_parallel, DEFAULT_MAX_PARALLEL)?;

    let max_parallel = match (limits, configured_parallel > DEFAULT_MAX_PARALLEL) {
        (_, false) => configured_parallel,
        (EngineLimits::Fresh, true) => {
            return Err(config_error(
                repo_path,
                format!(
                    "[engine] `max_parallel = {configured_parallel}` is refused: this engine runs \
                     one attempt at a time, so the run would cost and take what one worker costs \
                     and takes while the config claims {configured_parallel} — set \
                     `max_parallel = {DEFAULT_MAX_PARALLEL}` or omit it until parallel execution \
                     ships"
                ),
            ));
        }
        (
            EngineLimits::SequentialResumeWithRecordedGates | EngineLimits::SequentialResume,
            true,
        ) => {
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
        ("max_per_agent", max_per_agent, configured_parallel),
        ("max_per_pool", max_per_pool, configured_parallel),
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
    let interaction: RawInteraction = value.try_into().map_err(|e| {
        config_error(
            repo_path,
            format!(
                "[interaction]: {e} (expected optional `mode`, `notify` list, \
                 `wait_on_block_secs`, and `ask_before` table)"
            ),
        )
    })?;

    let ask_before = match interaction.ask_before {
        None => AskBefore::default(),
        Some(value) => value.try_into().map_err(|e| {
            config_error(
                repo_path,
                format!(
                    "[interaction] ask_before: {e} (accepted: {})",
                    AskBefore::ACCEPTED.join(", ")
                ),
            )
        })?,
    };
    if let Some(threshold) = ask_before.frontier_escalation_over_usd {
        if !threshold.is_finite() || threshold < 0.0 {
            return Err(config_error(
                repo_path,
                format!(
                    "[interaction] ask_before `frontier_escalation_over_usd = {threshold}` is not a \
                     spend threshold — omit the key to never ask, or give it a number of dollars"
                ),
            ));
        }
    }
    let mode = match interaction.mode {
        None => InteractionMode::default(),
        Some(requested) => InteractionMode::parse(&requested).ok_or_else(|| {
            config_error(
                repo_path,
                format!(
                    "[interaction] mode `{requested}` is not recognized (expected `never`, \
                     `on_block`, or `on_milestone`)"
                ),
            )
        })?,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn section(body: &str) -> toml::Value {
        toml::from_str(body).expect("the fixture is valid TOML")
    }

    fn refused<T>(result: Result<T, UpstrokeError>, what: &str) -> String {
        match result {
            Err(UpstrokeError::Config { message, .. }) => message,
            Err(other) => panic!("{what}: refused as {other:?}, not as a config error"),
            Ok(_) => panic!("{what}: accepted"),
        }
    }

    fn path() -> &'static Path {
        Path::new("upstroke.toml")
    }

    #[test]
    fn an_unknown_interaction_key_is_refused_and_named() {
        for (typo, body) in [
            ("mod", "mod = \"never\"\n"),
            ("notfiy", "notfiy = [\"cli\"]\n"),
            ("wait_on_block_sec", "wait_on_block_sec = 0\n"),
            (
                "ask_befor",
                "ask_befor = { frontier_escalation_over_usd = 5.0 }\n",
            ),
        ] {
            let message = refused(parse_interaction(Some(section(body)), path()), typo);
            assert!(message.contains("[interaction]"), "`{typo}`: {message}");
            assert!(
                message.contains(&format!("`{typo}`")),
                "`{typo}`: the message must name the key: {message}"
            );
            assert!(
                message.contains("`mode`") && message.contains("`ask_before`"),
                "`{typo}`: the message must name what is accepted: {message}"
            );
        }

        let settings = parse_interaction(
            Some(section(
                "mode = \"never\"\nnotify = [\"cli\"]\nwait_on_block_secs = 0\n\
                 ask_before = { frontier_escalation_over_usd = 5.0 }\n",
            )),
            path(),
        )
        .expect("the accepted keys load");
        assert_eq!(settings.mode, InteractionMode::Never);
        assert_eq!(settings.ask_before.frontier_escalation_over_usd, Some(5.0));
    }

    #[test]
    fn an_unknown_gate_key_is_refused_on_a_fresh_run_and_announced_on_a_resume() {
        let body = "[[gates]]\nname = \"test\"\ncmd = \"cargo test\"\ntimeout_sec = 3600\n";
        let mut warnings = Vec::new();
        let message = refused(
            parse_gates(
                section(body).get("gates").cloned(),
                path(),
                EngineLimits::Fresh,
                &mut warnings,
            ),
            "timeout_sec",
        );
        assert!(message.contains("[[gates]] entry 1 (`test`)"), "{message}");
        assert!(
            message.contains("`timeout_sec`"),
            "the message must name the key: {message}"
        );
        assert!(
            message.contains("`timeout_secs`"),
            "the message must name what is accepted: {message}"
        );
        assert!(
            warnings.is_empty(),
            "a refusal is not also a warning: {warnings:?}"
        );

        let mut warnings = Vec::new();
        let gates = parse_gates(
            section(body).get("gates").cloned(),
            path(),
            EngineLimits::SequentialResumeWithRecordedGates,
            &mut warnings,
        )
        .expect("a resume is not refused over a section it only compares")
        .expect("the section was present");
        assert_eq!(
            gates.iter().map(|gate| gate.timeout).collect::<Vec<_>>(),
            vec![DEFAULT_GATE_TIMEOUT]
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings.first().is_some_and(|warning| {
                warning.contains("`timeout_sec`") && warning.contains("recorded")
            }),
            "{warnings:?}"
        );

        let mut warnings = Vec::new();
        let message = refused(
            parse_gates(
                section(body).get("gates").cloned(),
                path(),
                EngineLimits::SequentialResume,
                &mut warnings,
            ),
            "timeout_sec on a resume with no gate record",
        );
        assert!(message.contains("`timeout_sec`"), "{message}");
        assert!(warnings.is_empty(), "{warnings:?}");

        let mut warnings = Vec::new();
        let gates = parse_gates(
            section("[[gates]]\nname = \"test\"\ncmd = \"cargo test\"\ntimeout_secs = 3600\n")
                .get("gates")
                .cloned(),
            path(),
            EngineLimits::Fresh,
            &mut warnings,
        )
        .expect("the accepted keys load")
        .expect("the section was present");
        assert_eq!(
            gates.iter().map(|gate| gate.timeout).collect::<Vec<_>>(),
            vec![Duration::from_secs(3600)]
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn two_gates_with_one_name_are_refused_on_a_fresh_run_and_announced_on_a_resume() {
        let repeated = "[[gates]]\nname = \"check\"\ncmd = \"cargo check\"\n\
                        [[gates]]\nname = \"test\"\ncmd = \"cargo test\"\n\
                        [[gates]]\nname = \"check\"\ncmd = \"cargo clippy\"\n";
        let mut warnings = Vec::new();
        let message = refused(
            parse_gates(
                section(repeated).get("gates").cloned(),
                path(),
                EngineLimits::Fresh,
                &mut warnings,
            ),
            "a repeated name",
        );
        assert!(message.contains("[[gates]] entry 3"), "{message}");
        assert!(message.contains("`check`"), "names the name: {message}");
        assert!(
            message.contains("of entry 1"),
            "names the earlier entry: {message}"
        );
        assert!(warnings.is_empty(), "{warnings:?}");

        let mut warnings = Vec::new();
        let message = refused(
            parse_gates(
                section(
                    "[[gates]]\nname = \"check\"\ncmd = \"cargo check\"\n\
                     [[gates]]\nname = \"Check\"\ncmd = \"cargo clippy\"\n",
                )
                .get("gates")
                .cloned(),
                path(),
                EngineLimits::Fresh,
                &mut warnings,
            ),
            "a name repeated in another case",
        );
        assert!(
            message.contains("entry 2 repeats the name `Check` of entry 1 (`check`"),
            "{message}"
        );
        assert!(message.contains("ASCII case"), "says why: {message}");

        let mut warnings = Vec::new();
        refused(
            parse_gates(
                section(repeated).get("gates").cloned(),
                path(),
                EngineLimits::SequentialResume,
                &mut warnings,
            ),
            "a repeated name on a resume with no gate record",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let mut warnings = Vec::new();
        let gates = parse_gates(
            section(repeated).get("gates").cloned(),
            path(),
            EngineLimits::SequentialResumeWithRecordedGates,
            &mut warnings,
        )
        .expect("a resume is not refused over a section it only compares")
        .expect("the section was present");
        assert_eq!(
            gates
                .iter()
                .map(|gate| gate.name.as_str())
                .collect::<Vec<_>>(),
            vec!["check", "test", "check"]
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings.first().is_some_and(|warning| {
                warning.contains("entry 3 repeats the name `check`") && warning.contains("recorded")
            }),
            "{warnings:?}"
        );

        let mut warnings = Vec::new();
        let gates = parse_gates(
            section(
                "[[gates]]\nname = \"check\"\ncmd = \"cargo check\"\n\
                 [[gates]]\nname = \"test\"\ncmd = \"cargo test\"\n\
                 [[gates]]\nname = \"clippy\"\ncmd = \"cargo clippy\"\n",
            )
            .get("gates")
            .cloned(),
            path(),
            EngineLimits::Fresh,
            &mut warnings,
        )
        .expect("distinct names load")
        .expect("the section was present");
        assert_eq!(
            gates
                .iter()
                .map(|gate| gate.name.as_str())
                .collect::<Vec<_>>(),
            vec!["check", "test", "clippy"]
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_blank_gate_field_is_named() {
        for (body, expected) in [
            ("name = \"\"\ncmd = \"cargo test\"\n", "`name` is blank"),
            ("name = \"test\"\ncmd = \" \"\n", "`cmd` is blank"),
            ("name = \"\"\ncmd = \"\"\n", "`name` and `cmd` is blank"),
        ] {
            for limits in [EngineLimits::Fresh, EngineLimits::SequentialResume] {
                let mut warnings = Vec::new();
                let message = refused(
                    parse_gates(
                        section(&format!("[[gates]]\n{body}")).get("gates").cloned(),
                        path(),
                        limits,
                        &mut warnings,
                    ),
                    expected,
                );
                assert!(message.contains(expected), "{limits:?}: {message}");
                assert!(
                    message.contains("[[gates]] entry 1"),
                    "{limits:?}: {message}"
                );
                assert!(warnings.is_empty(), "{limits:?}: {warnings:?}");
            }
            let mut warnings = Vec::new();
            let gates = parse_gates(
                section(&format!("[[gates]]\n{body}")).get("gates").cloned(),
                path(),
                EngineLimits::SequentialResumeWithRecordedGates,
                &mut warnings,
            )
            .expect("compared only: announced, never refused")
            .expect("the section was present");
            assert_eq!(gates.len(), 1, "the entry is kept for the comparison");
            assert!(
                warnings
                    .first()
                    .is_some_and(|w| w.contains(expected) && w.contains("recorded")),
                "{warnings:?}"
            );
        }
    }

    #[test]
    fn a_compared_only_gates_section_is_never_refused_over() {
        let zero = "[[gates]]\nname = \"test\"\ncmd = \"cargo test\"\ntimeout_secs = 0\n";
        let mut warnings = Vec::new();
        let gates = parse_gates(
            section(zero).get("gates").cloned(),
            path(),
            EngineLimits::SequentialResumeWithRecordedGates,
            &mut warnings,
        )
        .expect("compared only")
        .expect("present");
        assert_eq!(
            gates.iter().map(|gate| gate.timeout).collect::<Vec<_>>(),
            vec![Duration::ZERO],
            "carried as written, for the comparison"
        );
        assert!(
            warnings.first().is_some_and(
                |w| w.contains("timeout_secs must be at least 1") && w.contains("recorded")
            ),
            "{warnings:?}"
        );

        let not_a_table = "gates = [\"cargo test\"]\n";
        let mut warnings = Vec::new();
        let gates = parse_gates(
            section(not_a_table).get("gates").cloned(),
            path(),
            EngineLimits::SequentialResumeWithRecordedGates,
            &mut warnings,
        )
        .expect("compared only")
        .expect("present");
        assert!(
            gates.is_empty(),
            "the unbuildable entry is skipped: {gates:?}"
        );
        assert!(
            warnings
                .first()
                .is_some_and(|w| w.contains("[[gates]] entry 1") && w.contains("recorded")),
            "{warnings:?}"
        );

        let not_an_array = "[gates]\ncheck = \"cargo check\"\n";
        let mut warnings = Vec::new();
        let gates = parse_gates(
            section(not_an_array).get("gates").cloned(),
            path(),
            EngineLimits::SequentialResumeWithRecordedGates,
            &mut warnings,
        )
        .expect("compared only");
        assert!(gates.is_none(), "nothing to compare: {gates:?}");
        assert!(
            warnings
                .first()
                .is_some_and(|w| w.contains("must be an array") && w.contains("recorded")),
            "{warnings:?}"
        );

        assert!(
            warnings.iter().skip(1).any(|w| {
                w.contains("cannot be compared") && w.contains("derived from the repository")
            }),
            "the comparison is disowned: {warnings:?}"
        );

        for body in [zero, not_a_table, not_an_array] {
            for limits in [EngineLimits::Fresh, EngineLimits::SequentialResume] {
                let mut warnings = Vec::new();
                refused(
                    parse_gates(
                        section(body).get("gates").cloned(),
                        path(),
                        limits,
                        &mut warnings,
                    ),
                    body,
                );
                assert!(warnings.is_empty(), "{limits:?}: {warnings:?}");
            }
        }
    }

    #[test]
    fn an_unknown_engine_key_is_refused_and_named_on_every_reading() {
        for limits in [
            EngineLimits::Fresh,
            EngineLimits::SequentialResumeWithRecordedGates,
            EngineLimits::SequentialResume,
        ] {
            let mut warnings = Vec::new();
            let message = refused(
                parse_engine(
                    Some(section("on_task_failur = \"continue\"\nmax_paralel = 4\n")),
                    path(),
                    limits,
                    &mut warnings,
                ),
                "an unknown [engine] key",
            );
            assert!(
                message.contains("`on_task_failur`"),
                "{limits:?}: {message}"
            );
            assert!(message.contains("`max_paralel`"), "{limits:?}: {message}");
            assert!(message.contains(ENGINE_KEYS), "{limits:?}: {message}");
            assert!(warnings.is_empty(), "{limits:?}: {warnings:?}");
        }

        for limits in [
            EngineLimits::Fresh,
            EngineLimits::SequentialResumeWithRecordedGates,
            EngineLimits::SequentialResume,
        ] {
            let mut warnings = Vec::new();
            let settings = parse_engine(
                Some(section("on_task_failure = \"continue\"\n")),
                path(),
                limits,
                &mut warnings,
            )
            .expect("the accepted key loads");
            assert_eq!(
                settings.on_task_failure,
                OnTaskFailure::Continue,
                "{limits:?}"
            );
            assert!(warnings.is_empty(), "{limits:?}: {warnings:?}");
        }
    }

    #[test]
    fn a_sequential_resume_announces_only_the_ceilings_the_file_wrote() {
        let mut warnings = Vec::new();
        let settings = parse_engine(
            Some(section("max_parallel = 3\n")),
            path(),
            EngineLimits::SequentialResume,
            &mut warnings,
        )
        .expect("a legacy run stays reachable");
        assert_eq!(settings.max_parallel, DEFAULT_MAX_PARALLEL);
        assert_eq!(settings.max_per_agent, 3);
        assert_eq!(settings.max_per_pool, 3);
        assert_eq!(
            warnings.len(),
            1,
            "one key was written, so one key is announced: {warnings:?}"
        );
        assert!(
            warnings
                .first()
                .is_some_and(|warning| warning.contains("`max_parallel = 3`")),
            "{warnings:?}"
        );

        let mut warnings = Vec::new();
        parse_engine(
            Some(section(
                "max_parallel = 3\nmax_per_agent = 2\nmax_per_pool = 3\n",
            )),
            path(),
            EngineLimits::SequentialResume,
            &mut warnings,
        )
        .expect("a legacy run stays reachable");
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("`max_per_agent = 2`")),
            "{warnings:?}"
        );
        assert!(
            !warnings
                .iter()
                .any(|warning| warning.contains("max_per_pool")),
            "written at its default, `max_per_pool` changes nothing: {warnings:?}"
        );
    }

    #[test]
    fn a_host_refusal_quotes_the_kind_only_when_the_file_wrote_one() {
        let written = refused(
            read_runner(
                Some(section("kind = \"host\"\nimage = \"upstroke/ci:3.2\"\n")),
                path(),
            ),
            "host with an image",
        );
        assert!(
            written.contains("[runner] `kind = \"host\"` with `image`"),
            "{written}"
        );

        let absent = refused(
            read_runner(Some(section("image = \"upstroke/ci:3.2\"\n")), path()),
            "an image with no kind",
        );
        assert!(
            !absent.contains("kind = \"host\""),
            "the file wrote no `kind`, so the refusal must not quote one: {absent}"
        );
        assert!(
            absent.contains("`kind` is absent") && absent.contains("with `image`"),
            "{absent}"
        );
    }

    #[test]
    fn every_unknown_runner_key_is_named() {
        let message = refused(
            read_runner(
                Some(section(
                    "knid = \"container\"\nimag = \"upstroke/ci:3.2\"\n",
                )),
                path(),
            ),
            "two unknown keys",
        );
        assert!(message.contains("`knid`"), "{message}");
        assert!(
            message.contains("`imag`"),
            "the second unknown key must be named too: {message}"
        );
        assert!(message.contains(RUNNER_KEYS), "{message}");
    }

    #[test]
    fn the_runner_kind_words_are_the_wire_spelling() {
        for (word, expected) in [
            ("host", RunnerKind::Host),
            ("container", RunnerKind::Container),
        ] {
            let body = match expected {
                RunnerKind::Host => format!("kind = \"{word}\"\n"),
                RunnerKind::Container => format!("kind = \"{word}\"\nimage = \"i\"\n"),
            };
            let selection = read_runner(Some(section(&body)), path()).expect("the word parses");
            assert_eq!(selection.kind, expected, "`{word}`");
            let wire: RunnerKind = toml::Value::String(word.to_owned())
                .try_into()
                .expect("the type spells the same word");
            assert_eq!(wire, expected, "`{word}` on the wire");
        }

        for word in ["Host", "CONTAINER", "container "] {
            let message = refused(
                read_runner(
                    Some(section(&format!("kind = \"{word}\"\nimage = \"i\"\n"))),
                    path(),
                ),
                word,
            );
            assert!(
                message.contains(&format!("`kind = \"{word}\"`")),
                "{message}"
            );
            let wire: Result<RunnerKind, _> = toml::Value::String(word.to_owned()).try_into();
            assert!(
                wire.is_err(),
                "`{word}`: the wire accepts what this reader refuses"
            );
        }
    }

    #[test]
    fn an_ask_before_threshold_that_is_not_a_spend_is_refused() {
        for value in ["-1.0", "-inf", "inf", "nan"] {
            let message = refused(
                parse_interaction(
                    Some(section(&format!(
                        "ask_before = {{ frontier_escalation_over_usd = {value} }}\n"
                    ))),
                    path(),
                ),
                value,
            );
            assert!(
                message.contains("not a spend threshold"),
                "`{value}`: {message}"
            );
            assert!(
                message.contains("frontier_escalation_over_usd"),
                "`{value}`: {message}"
            );
        }

        for (value, expected) in [("0.0", 0.0), ("5.0", 5.0), ("12", 12.0)] {
            let settings = parse_interaction(
                Some(section(&format!(
                    "ask_before = {{ frontier_escalation_over_usd = {value} }}\n"
                ))),
                path(),
            )
            .expect("a finite non-negative threshold loads");
            assert_eq!(
                settings.ask_before.frontier_escalation_over_usd,
                Some(expected),
                "`{value}`"
            );
        }
    }

    #[test]
    fn a_budget_that_is_not_finite_is_refused() {
        for body in [
            "run_usd = nan",
            "run_usd = inf",
            "task_usd = -inf",
            "task_usd = nan",
        ] {
            let message = refused(
                parse_budgets(Some(section(&format!("{body}\n"))), path()),
                body,
            );
            assert!(message.contains("[budgets]"), "`{body}`: {message}");
            assert!(
                message.contains("not a spendable ceiling"),
                "`{body}`: {message}"
            );
        }
        let budgets = parse_budgets(Some(section("run_usd = 15\ntask_usd = 4.5\n")), path())
            .expect("finite positive ceilings load");
        assert_eq!(budgets.run_usd, Some(15.0));
        assert_eq!(budgets.task_usd, Some(4.5));
    }

    #[test]
    fn a_missing_gate_key_is_named_beside_the_unknown_key_that_replaced_it() {
        let typo = "[[gates]]\nnmae = \"check\"\ncmd = \"cargo check\"\n";
        for limits in [EngineLimits::Fresh, EngineLimits::SequentialResume] {
            let mut warnings = Vec::new();
            let message = refused(
                parse_gates(
                    section(typo).get("gates").cloned(),
                    path(),
                    limits,
                    &mut warnings,
                ),
                "a misspelled required key",
            );
            assert!(
                message.contains("[[gates]] entry 1 is missing `name`"),
                "{limits:?}: {message}"
            );
            assert!(
                message.contains("has unknown key `nmae`"),
                "{limits:?}: the misspelling is named: {message}"
            );
            assert!(warnings.is_empty(), "{limits:?}: {warnings:?}");
        }
        let mut warnings = Vec::new();
        let gates = parse_gates(
            section(typo).get("gates").cloned(),
            path(),
            EngineLimits::SequentialResumeWithRecordedGates,
            &mut warnings,
        )
        .expect("compared only")
        .expect("present");
        assert!(
            gates.is_empty(),
            "an entry without a name cannot be built: {gates:?}"
        );
        assert!(
            warnings.first().is_some_and(|w| {
                w.contains("missing `name`") && w.contains("`nmae`") && w.contains("recorded")
            }),
            "{warnings:?}"
        );

        let mut warnings = Vec::new();
        let message = refused(
            parse_gates(
                section("[[gates]]\nname = \"check\"\n")
                    .get("gates")
                    .cloned(),
                path(),
                EngineLimits::Fresh,
                &mut warnings,
            ),
            "a missing cmd",
        );
        assert!(message.contains("is missing `cmd`"), "{message}");
        assert!(!message.contains("unknown key"), "{message}");
    }

    #[test]
    fn a_gate_timeout_the_record_cannot_hold_is_refused() {
        assert!(
            u64::try_from(Duration::from_secs(MAX_GATE_TIMEOUT_SECS).as_millis()).is_ok(),
            "the bound itself is representable"
        );
        assert!(
            u64::try_from(Duration::from_secs(MAX_GATE_TIMEOUT_SECS + 1).as_millis()).is_err(),
            "one second more is not"
        );

        let over = format!(
            "[[gates]]\nname = \"slow\"\ncmd = \"cargo test\"\ntimeout_secs = {}\n",
            MAX_GATE_TIMEOUT_SECS + 1
        );
        for limits in [EngineLimits::Fresh, EngineLimits::SequentialResume] {
            let mut warnings = Vec::new();
            let message = refused(
                parse_gates(
                    section(&over).get("gates").cloned(),
                    path(),
                    limits,
                    &mut warnings,
                ),
                "a timeout the record cannot hold",
            );
            assert!(
                message.contains("more than the run record can hold"),
                "{limits:?}: {message}"
            );
            assert!(
                message.contains(&MAX_GATE_TIMEOUT_SECS.to_string()),
                "{limits:?}: names the bound: {message}"
            );
            assert!(warnings.is_empty(), "{limits:?}: {warnings:?}");
        }

        let mut warnings = Vec::new();
        let gates = parse_gates(
            section(&over).get("gates").cloned(),
            path(),
            EngineLimits::SequentialResumeWithRecordedGates,
            &mut warnings,
        )
        .expect("compared only")
        .expect("present");
        assert_eq!(
            gates.iter().map(|gate| gate.timeout).collect::<Vec<_>>(),
            vec![Duration::from_secs(MAX_GATE_TIMEOUT_SECS + 1)]
        );
        assert!(
            warnings
                .first()
                .is_some_and(|w| w.contains("record can hold") && w.contains("recorded")),
            "{warnings:?}"
        );

        let at = format!(
            "[[gates]]\nname = \"slow\"\ncmd = \"cargo test\"\ntimeout_secs = {MAX_GATE_TIMEOUT_SECS}\n"
        );
        for limits in [
            EngineLimits::Fresh,
            EngineLimits::SequentialResume,
            EngineLimits::SequentialResumeWithRecordedGates,
        ] {
            let mut warnings = Vec::new();
            let gates = parse_gates(
                section(&at).get("gates").cloned(),
                path(),
                limits,
                &mut warnings,
            )
            .expect("the bound loads")
            .expect("present");
            assert_eq!(
                gates.iter().map(|gate| gate.timeout).collect::<Vec<_>>(),
                vec![Duration::from_secs(MAX_GATE_TIMEOUT_SECS)],
                "{limits:?}"
            );
            assert!(warnings.is_empty(), "{limits:?}: {warnings:?}");
        }
    }
}
