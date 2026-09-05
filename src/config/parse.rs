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
//! later reader needs in order to place a new key correctly. **No section may
//! sit on neither side.** A key that is neither consumed, refused nor named is
//! a key that vanished, and a vanished key is indistinguishable from one that
//! was never written: `[[gates]]` and `[interaction]` sat there until 2026-09-04
//! (`RawGate` and `RawInteraction` now refuse what they do not read).
//!
//! **A parse failure never becomes a default, with one stated exception.**
//! Every `unwrap_or`, `map_or` and `unwrap_or_else` in this module but one
//! folds an *absent* key into the default the design gives it; a key that was
//! written and could not be read is refused by the section's `try_into` before
//! any fold is reached. The one exception is `[engine] shell`, whose
//! `unwrap_or_else` folds a written value the parser did not recognise into the
//! platform default, and warns by name — see [`parse_engine`].
//!
//! Nothing here reads a path, starts a process, or writes anything: every input
//! arrives as an already-parsed `toml::Value`, and the reading of the bytes it
//! came from belongs to `super::read`.
//!
//! **What a refusal carries.** Every error is [`UpstrokeError::Config`] with
//! the file's path and a message that names the section and the key, and the
//! value written when the value is what is wrong (`[engine] \`max_parallel = 4\``,
//! `[runner] \`kind = "vm"\``, `[[gates]] entry 2 (\`test\`)`); an unknown key is
//! named without its value, since the key is the mistake. That is enough to
//! find the line by eye; a byte offset is not carried, because `toml::Value`
//! has already dropped the span by the time a section reaches here.

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

/// The one error shape every reader here returns: [`UpstrokeError::Config`]
/// for the file at `repo_path`.
///
/// A function rather than a closure per reader, so a refusal is built the same
/// way at every site. The path copy is the refusal's own: an error value is
/// returned past the borrow it was built under, so it owns what it names (§6).
fn config_error(repo_path: &Path, message: String) -> UpstrokeError {
    UpstrokeError::Config {
        path: repo_path.to_path_buf(),
        message,
    }
}

/// `[runner]` as written, with no policy applied.
///
/// Every key is an **error** when wrong, and an unknown key is an error too —
/// the section exists to decide whether gate code executes confined, and a
/// typo that leaves it on the host is the one mistake it must not absorb. The
/// only defaults here are for keys that are *absent*: no `kind` is the host
/// runner, no `credential_volumes` or `mounts` is none of either, and a mount
/// with no `read_only` is read-only. A key that is present and unreadable is
/// refused by the `try_into` before any of them is reached.
///
/// # Errors
///
/// [`UpstrokeError::Config`], naming the key: the section is not a table of
/// the accepted keys; a key outside [`RUNNER_KEYS`]; a `kind` other than
/// `host` or `container`; `image`, `credential_volumes` or `mounts` under the
/// host runner; a container with no `image`, or a blank one; a blank agent id
/// or volume name; a mount with a blank `target` or an empty `source`.
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
    // An error, where `[engine]` warns. `[engine]`'s unknown keys are ceilings
    // and timeouts: a typo leaves a default in place and the run does slightly
    // less than asked. A typo here — `knid = "container"`, `iamge = "..."` —
    // leaves the run executing on the **host** while the operator believes gate
    // code is confined, which is the one thing this section exists to decide.
    // Same rule as `[interaction]`, `[[gates]]` and `[budgets]`, for the same
    // reason: silently ignoring a key is silently deleting a control. Every
    // unknown key is named, as `[engine]` names every one of its own, so one
    // load reports the whole section.
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
    // Spelled here and pinned to `RunnerKind`'s own wire spelling by
    // `tests::the_runner_kind_words_are_the_wire_spelling`, so the config and
    // the record cannot drift apart without a test saying so.
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
            // The message quotes what the file says. `kind = "host"` was
            // written in one case and not in the other, and a refusal that
            // quotes a line the operator never wrote sends them looking for
            // it.
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
    // Absent is none, not a failure: an unreadable map was refused above.
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
    // Absent is none, not a failure, as for the volumes above.
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
            // Writable is a thing you say. See `RunnerMount`. An absent key,
            // not a failed reading: a `read_only` that is not a boolean was
            // refused by the `try_into` above.
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

/// Parse one role's explicit effort at config load. All three providers reject
/// an unknown value after process launch, so accepting a typo here would burn an
/// attempt for a routing policy the operator never actually selected.
///
/// # Errors
///
/// [`UpstrokeError::Config`] naming the role and the value when `raw` is not
/// one of [`Effort::KNOWN`]. An absent value is `Ok(None)`: no policy.
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

/// `[budgets]` (§17). A ceiling that is zero, negative, or not a number is a
/// hard error: every one of those readings would either stop the run before it
/// began or be ignored, and which of the two happened must not be a surprise.
/// An unknown key is a hard error too, by `Budgets`' own derive.
///
/// # Errors
///
/// [`UpstrokeError::Config`]: the section is not a table of the two optional
/// numbers, or a ceiling fails [`check_budget`] — non-positive, or not finite,
/// which TOML can write as `inf` and `nan`.
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

/// `[[gates]]` parsing with actionable shape errors: a `[gates]` table, a
/// wrong-typed field, or `timeout_secs = 0` all name what was expected.
///
/// **Every key is an error when wrong, and an unknown key is an error on a
/// fresh run.** An entry has three keys and the only optional one,
/// `timeout_secs`, decides when a running gate is killed and reported as
/// failed: `timeout_sec = 3600` on a gate that needs an hour is a gate that
/// fails at the 600 s default, and the ladder then spends attempts repairing
/// code that passes. That is a control deleted by a typo, which is the module's
/// rule for an error, not a warning.
///
/// **Two entries may not share a `name`, on a fresh run.** The name is what a
/// gate's log file is written under (`<task>-<attempt>-<name>.log`, in
/// `gates::run_all`) and what its failure report carries, so a second gate with
/// the first's name replaces the first's log in the same attempt and reports a
/// failure the operator cannot attribute. Names are compared **without regard
/// to ASCII case**: two of the three CI platforms keep their logs on a
/// case-insensitive filesystem (NTFS, APFS as shipped), where `check` and
/// `Check` are one file, and a config must not behave differently per platform.
/// ASCII case and no more, because `util::filename_component` maps every
/// non-ASCII character to `-` before the name reaches the filesystem, so ASCII
/// case is the only folding the filesystem can apply to what is written; the
/// collisions that mapping itself creates (`lint fast` and `lint-fast`) are the
/// log writer's, `SWEEP-CONFIG-PARSE-011`.
///
/// **On a sequential run's resume those two refusals are warnings.** Both
/// shapes were legal to record before 2026-09-05, and `design/15` is explicit
/// that gates are taken from the record and not refused over: today's section
/// is read only to be compared with the recorded gates, so refusing over it
/// would strand a run for a section that does not govern it. This is the
/// reading `[engine] max_parallel` already takes through [`EngineLimits`], and
/// it is applied here to the two shapes and to nothing else — a blank field or
/// a zero timeout never recorded a run, so those refuse on both readings.
///
/// # Errors
///
/// [`UpstrokeError::Config`] naming the entry by position: `gates` is not an
/// array; an entry is not a table of `name`, `cmd` and optional `timeout_secs`;
/// a blank `name` or `cmd`, naming which; `timeout_secs = 0`; and, under
/// [`EngineLimits::Fresh`] only, a key outside those three or a `name` an
/// earlier entry has (compared without regard to ASCII case) — under
/// [`EngineLimits::SequentialResume`] each of those is a warning in `warnings`
/// naming the recorded gates as what runs, and the entry is kept so the record
/// can be compared with it. `Ok(None)` is an absent section and
/// `Ok(Some(vec![]))` an explicitly empty one — the parent's `Config::gates`
/// says what each means.
pub(super) fn parse_gates(
    raw: Option<toml::Value>,
    repo_path: &Path,
    limits: EngineLimits,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<GateConfig>>, UpstrokeError> {
    let Some(value) = raw else { return Ok(None) };
    let toml::Value::Array(entries) = value else {
        return Err(config_error(
            repo_path,
            format!(
                "`gates` must be an array of tables — write `[[gates]]` entries (double brackets, \
                 one per gate), found a {}",
                value.type_str()
            ),
        ));
    };
    let mut list: Vec<GateConfig> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let n = index + 1;
        let g: RawGate = entry.try_into().map_err(|e| {
            config_error(
                repo_path,
                format!(
                    "[[gates]] entry {n}: {e} (each entry takes `name`, `cmd`, and an optional \
                     `timeout_secs` integer)"
                ),
            )
        })?;
        let blank: Vec<&str> = [
            ("name", g.name.trim().is_empty()),
            ("cmd", g.cmd.trim().is_empty()),
        ]
        .into_iter()
        .filter_map(|(key, is_blank)| is_blank.then_some(key))
        .collect();
        if !blank.is_empty() {
            return Err(config_error(
                repo_path,
                format!(
                    "[[gates]] entry {n}: `{}` is blank — each entry needs a non-empty `name` \
                     and `cmd`",
                    blank.join("` and `")
                ),
            ));
        }
        if !g.unknown.is_empty() {
            let keys: Vec<&str> = g.unknown.keys().map(String::as_str).collect();
            refuse_or_announce(
                limits,
                format!(
                    "[[gates]] entry {n} (`{}`) has unknown key `{}` (each entry takes `name`, \
                     `cmd`, and an optional `timeout_secs` integer)",
                    g.name,
                    keys.join("`, `")
                ),
                repo_path,
                warnings,
            )?;
        }
        if let Some((earlier, first)) = list
            .iter()
            .enumerate()
            .find(|(_, gate)| gate.name.eq_ignore_ascii_case(&g.name))
        {
            refuse_or_announce(
                limits,
                format!(
                    "[[gates]] entry {n} repeats the name `{}` of entry {} (`{}`; names are \
                     compared without regard to ASCII case, because a case-insensitive \
                     filesystem keeps one log file for both): a gate's name is what its log \
                     file and its failure report carry, so two gates cannot share one — give \
                     each gate a name of its own",
                    g.name,
                    earlier + 1,
                    first.name
                ),
                repo_path,
                warnings,
            )?;
        }
        if g.timeout_secs == Some(0) {
            return Err(config_error(
                repo_path,
                format!(
                    "[[gates]] entry {n} (`{}`): timeout_secs must be at least 1 — omit it for \
                     the {}s default",
                    g.name,
                    DEFAULT_GATE_TIMEOUT.as_secs()
                ),
            ));
        }
        list.push(GateConfig {
            name: g.name,
            cmd: g.cmd,
            // An absent key, not a failed reading: a `timeout_secs` that is
            // not a whole number was refused by the `try_into` above.
            timeout: g
                .timeout_secs
                .map_or(DEFAULT_GATE_TIMEOUT, Duration::from_secs),
        });
    }
    Ok(Some(list))
}

/// A `[[gates]]` shape a run could have been recorded under before it became a
/// refusal: refused on a run being created now, announced on a sequential run's
/// resume, where the recorded gates are what executes (`design/15`) and today's
/// section is read only to be compared with them. See [`parse_gates`].
fn refuse_or_announce(
    limits: EngineLimits,
    problem: String,
    repo_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<(), UpstrokeError> {
    match limits {
        EngineLimits::Fresh => Err(config_error(repo_path, problem)),
        EngineLimits::SequentialResume => {
            warnings.push(format!(
                "{problem}; in {} this resume runs the gates the run recorded, so today's \
                 [[gates]] is read only to be compared with them, and a fresh run refuses this \
                 shape",
                repo_path.display()
            ));
            Ok(())
        }
    }
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
///
/// **Which side each key sits on.** An unknown key **warns** by name: every
/// key here is a ceiling, a timeout or an interpreter choice, and a typo leaves
/// the default in place, so the run does slightly less than asked and the
/// warning says which key bought nothing. An unknown `shell` value **warns**
/// and takes the platform default, on the same reading — the gate commands
/// still run, under the shell the platform would have used — and it is the one
/// value in this module that degrades rather than refuses. `on_task_failure`
/// **errors**: a misspelling there decides whether a failed task stops the
/// run. A zero ceiling **errors** on both readings of `limits`: "no ceiling"
/// and "nothing may run" are two meanings, and a resume must not become a way
/// around the check.
///
/// # Errors
///
/// [`UpstrokeError::Config`]: the section is not a table of the six optional
/// keys; `on_task_failure` is not `halt` or `continue`; any ceiling is zero or
/// not a whole number; `max_parallel` above [`DEFAULT_MAX_PARALLEL`] on a
/// fresh run.
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
    for key in engine.unknown.keys() {
        warnings.push(format!(
            "unknown key `{key}` in [engine] in {} (ignored)",
            repo_path.display()
        ));
    }
    // The one degrade-and-warn in the module; the reason is in the doc above.
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
            config_error(
                repo_path,
                format!(
                    "[engine] on_task_failure `{requested}` is not recognized (expected `halt` or \
                     `continue`)"
                ),
            )
        })?,
    };
    // Zero has two readings — "no ceiling" and "nothing may run" — and which one
    // happened must never be a surprise. The rule `attempts_per` and every
    // `timeout_secs` already follow. `None` is an absent key and takes the
    // default; a written value that is not a whole number never reaches here.
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
    // What this load's run will actually be allowed to do. It parts company
    // with what the file says in exactly one case — a sequential run's resume,
    // whose ceiling is a fact about the run and not about today's config — and
    // that case says so out loud below rather than carrying the file's number
    // into a Config field nothing may act on.
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
    //
    // Each companion is compared against the ceiling it was defaulted from —
    // the one the file asked for, not the effective one — so a companion the
    // operator never wrote is never announced: on a sequential resume,
    // `max_parallel = 3` alone is one warning naming `max_parallel`, not three
    // naming two keys the file does not contain. On a fresh run the two
    // ceilings are equal and the choice makes no difference.
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

/// `[interaction]` (§12).
///
/// Everything here is a hard error or nothing: `mode` and `ask_before` both
/// decide whether a human is ever asked, so a typo in either must not degrade
/// quietly. Notifier ids are the one soft setting, and they are validated by
/// `notifiers_for` at run time rather than here — which is why this function
/// takes no warning sink.
///
/// **An unknown key is a hard error** (`RawInteraction` refuses it), for the
/// reason the two hard keys give: a key typo cannot be told from a deleted
/// key, so `ask_befor = { frontier_escalation_over_usd = 5.0 }` is a spend
/// approval that no longer exists and `mod = "never"` is a CI run that will
/// stop to ask a person. Until 2026-09-04 both loaded without a word.
///
/// # Errors
///
/// [`UpstrokeError::Config`]: the section is not a table of the four optional
/// keys; a key outside them; an `ask_before` key outside
/// [`AskBefore::ACCEPTED`]; a `frontier_escalation_over_usd` that is negative
/// or not finite (`0.0` is a threshold: ask before any frontier escalation); a
/// `mode` other than `never`, `on_block` or `on_milestone`.
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
    // An unknown key inside `ask_before` errors rather than warning: the whole
    // point of the table is to stop the run and ask, so a misspelling that
    // silently drops the threshold spends the money the operator asked to be
    // consulted about. Same reasoning as `second_opinion`.
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
        // §5: a floating-point input rejects non-finite values before it is
        // budgeted. `nan` compares false with everything, so a NaN threshold
        // is one that never fires; `-inf` is one that always does.
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
        // Absent keys take §12's defaults; a written key that could not be
        // read was refused by the `try_into` above.
        notify: interaction.notify.unwrap_or_else(default_notify),
        wait_on_block: interaction
            .wait_on_block_secs
            .map_or(DEFAULT_WAIT_ON_BLOCK, Duration::from_secs),
        ask_before,
    })
}

/// The readers driven directly, each on a `toml::Value` built in the test, so
/// every assertion is about one section and no file is written. The parent's
/// suite drives the same readers through `load` and a scratch file; these pin
/// the refusals and diagnostics that suite did not, each named for the
/// sentence it proves. The mutation each was witnessed against is recorded in
/// the Validation section of the pull request that added it.
#[cfg(test)]
mod tests {
    use super::*;

    /// A section body as the `toml::Value` the readers take.
    fn section(body: &str) -> toml::Value {
        toml::from_str(body).expect("the fixture is valid TOML")
    }

    /// The message of a config refusal, or a panic naming what was expected.
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
        // Each is the realistic typo of one accepted key, and each used to load
        // with that key's default in place: `mod = "never"` was a CI run that
        // would stop to ask a person, `ask_befor` a spend approval that no
        // longer existed.
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
        // The control: the four accepted keys together are the shape every
        // refusal above is one letter away from.
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
        // `timeout_sec = 3600` on a gate that needs an hour used to be a gate
        // killed at the 600 s default, with nothing said.
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

        // A run recorded under the old rules is resumed through the same file:
        // the key is named, the entry is kept at the default the typo left it
        // at, and the recorded gates are named as what runs.
        let mut warnings = Vec::new();
        let gates = parse_gates(
            section(body).get("gates").cloned(),
            path(),
            EngineLimits::SequentialResume,
            &mut warnings,
        )
        .expect("a resume is not refused over a section it does not run")
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

        // The control: the same entry with the key spelt right, and the value
        // reaches the gate under either reading.
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

        // Two names that differ only in ASCII case are one log file on NTFS
        // and on APFS as shipped, so they are one name here on every platform.
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

        // A run recorded under the old rules is resumed through the same file:
        // both entries are kept, so the record can be compared with them, and
        // the repeat is announced with the recorded gates named as what runs.
        let mut warnings = Vec::new();
        let gates = parse_gates(
            section(repeated).get("gates").cloned(),
            path(),
            EngineLimits::SequentialResume,
            &mut warnings,
        )
        .expect("a resume is not refused over a section it does not run")
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

        // The control: the same three commands under three names, kept in file
        // order, with nothing announced.
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
        // A blank field refuses on both readings — no run was ever recorded
        // with one — and the refusal says which field, where it used to say
        // only that one of the two was blank.
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
        }
    }

    #[test]
    fn a_sequential_resume_announces_only_the_ceilings_the_file_wrote() {
        // `max_parallel = 3` alone is one statement about a future run. The
        // two companions default to it, and a companion the operator never
        // wrote must not be announced as if the file contained it.
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

        // A companion the file did write at a value other than its default is
        // announced beside it, so the rule tracks what was written and not the
        // key's presence alone.
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
        // `RunnerKind` is PR3's wire kind, and the config and the record have
        // to be comparable. The words this reader accepts are pinned to the
        // words the type's own `Deserialize` accepts, so neither can move
        // without the other.
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
        // And a spelling the wire refuses is refused here, so this reader is
        // not looser than the record it must compare against.
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
        // A NaN threshold compares false with every spend and never fires; a
        // negative or `-inf` one fires before a dollar is spent. Neither is a
        // threshold the operator can have meant.
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
        // Zero is a threshold — ask before any frontier escalation — and so is
        // any finite non-negative number.
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
        // TOML writes `inf` and `nan` as numbers, and neither is a ceiling: a
        // NaN ceiling is never reached and an infinite one never fires, which
        // is "unlimited" spelt as a limit. The parent's suite pins zero and a
        // negative; these are the other two arms of `check_budget`.
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
}
