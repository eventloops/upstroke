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
//! (`RawGate` and `RawInteraction` now refuse what they do not read). **And an
//! unknown key is never a warning**, in any section: a key typo cannot be told
//! from a deleted key, and every section here holds at least one control —
//! `[engine]` warned until 2026-09-05, and `on_task_failur = "continue"` was
//! a halted run the operator asked to continue, with a footnote.
//!
//! **A resume reads `[[gates]]` by what its log records** ([`EngineLimits`]):
//! with a gate record, today's section is compared with the record and never
//! refused over (`design/15`); without one, it settles the run's gates and is
//! read as a fresh run reads it. The `[engine]` ceilings warn on either resume.
//! Nothing else reads differently on a resume.
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
    // An error. A typo here — `knid = "container"`, `iamge = "..."` — leaves
    // the run executing on the **host** while the operator believes gate code
    // is confined, which is the one thing this section exists to decide. Same
    // rule as every other section, for the same reason: silently ignoring a
    // key is silently deleting a control. Every unknown key is named, so one
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
/// ## Every reading refuses, and that is the whole of today's answer
///
/// [`EngineLimits`] distinguishes a run being created from a sequential run's
/// resume — in two flavours since 2026-09-05, by whether the log records its
/// gates, which changes how `[[gates]]` is read and nothing here — and
/// `expected_failures_refusals[0]` names a fresh run **and** a resume. No
/// reading accepts a container selection in this build: `EngineLimits::Fresh`
/// means "a run being created now", and every run this binary creates is
/// schema-3. `PR12 config acceptance for fresh schema-4 runs only` (INV-23's
/// `enforced_by`) is where a fresh schema-4 run learns to accept it, and that
/// is a new reading rather than a relaxation of this one.
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

/// The largest `timeout_secs` the run record can carry back exactly.
///
/// `run_started` records each gate's timeout as `timeout_ms`, a `u64` written
/// by `crate::util::duration_millis`, which **saturates** at `u64::MAX`
/// milliseconds rather than failing. A larger value would load, be recorded
/// smaller, and be resumed at the recorded value with a drift warning against
/// a file nobody edited — the opposite of `design/15`'s record-and-resume-
/// exactly. So the reader refuses what the record cannot hold; the tests pin
/// this bound to the serialiser's own arithmetic.
const MAX_GATE_TIMEOUT_SECS: u64 = u64::MAX / 1000;

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
/// **Which reading, and why the parser cannot choose it alone.** Under
/// [`EngineLimits::Fresh`] today's section governs the run, and every shape
/// above refuses. Under [`EngineLimits::SequentialResumeWithRecordedGates`] the run's log
/// records its gates and `design/15` is explicit that they are taken from the
/// record, not re-derived, and **not refused over**: today's section is read
/// only to be compared with them, so nothing here refuses — every shape,
/// including a zero timeout, a blank field, an entry that is not a table and a
/// section that is not an array, is a warning naming the recorded gates as
/// what runs, and the list carries what could be read so the comparison can
/// still say what moved (an unreadable entry is skipped; an unreadable section
/// is `Ok(None)`). Under [`EngineLimits::SequentialResume`] the
/// log has no gate record, this resume settles the run's gates from today's
/// file and records them, so the section governs and is read exactly as a
/// fresh run reads it. Which of the two resumes applies is a fact about the
/// log, not about the config, which is why `EngineLimits::for_resume` takes
/// it from `events::recorded_gates` and this function only asks the reading.
/// An earlier version keyed the downgrade off "a resume" alone and was wrong
/// both ways: it promised the record would run when a legacy log had none,
/// and it kept refusing a run that had one. A later draft put the compare-only
/// reading on `SequentialResume` itself, which silently changed what a caller
/// passing that public variant directly had always got; the reading lives on
/// the variant that names it, and `SequentialResume` governs as it always did.
///
/// # Errors
///
/// Under `Fresh` and `SequentialResume`, [`UpstrokeError::Config`]
/// naming the entry by position: `gates` is not an array; an entry is not a
/// table; `name` or `cmd` is missing, named beside any unknown key the entry
/// carries; a blank `name` or `cmd`, naming which; a key outside those three
/// on an entry that has both; `timeout_secs = 0`, or more than
/// [`MAX_GATE_TIMEOUT_SECS`]; a `name` an earlier entry has, compared without
/// regard to ASCII case. Under
/// `SequentialResumeWithRecordedGates`, never: each of those is a warning in `warnings`. On
/// every reading `Ok(None)` is an absent section and `Ok(Some(vec![]))` an
/// explicitly empty one — the parent's `Config::gates` says what each means —
/// with one exception the warning names: under `SequentialResumeWithRecordedGates` a section
/// that is not a list is also `Ok(None)`, there being no other shape for "no
/// list", so the engine derives defaults to compare with the record and the
/// warning says any reported difference is against those, not the file
/// (`SWEEP-CONFIG-PARSE-026`).
pub(super) fn parse_gates(
    raw: Option<toml::Value>,
    repo_path: &Path,
    limits: EngineLimits,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<GateConfig>>, UpstrokeError> {
    // The one exhaustive decision: which of the three readings this is.
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
        // Compared only, and nothing in the section can be compared. `None`
        // is the only shape `Config::gates` has for "no list", and downstream
        // it means "derive from the repository", so the comparison the engine
        // then reports is against derived defaults rather than the file — the
        // warning says so, and `SWEEP-CONFIG-PARSE-026` records the typed
        // state that would let the comparison be skipped instead.
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
                // Compared only, and this entry cannot be built: skipped, and
                // the comparison says the record has a gate today's file lacks.
                continue;
            }
        };
        let unknown: Vec<&str> = g.unknown.keys().map(String::as_str).collect();
        // A required key that is absent is named beside any key the entry has
        // that nothing reads: `nmae = "check"` is a missing `name` and an
        // unknown `nmae`, and the operator is told both, since the second is
        // almost always the first misspelt. Serde's own "missing field" would
        // have named only the first.
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
                // Compared only, and this entry cannot be built: skipped.
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
            // An absent key, not a failed reading: a `timeout_secs` that is
            // not a whole number was refused (or announced and skipped) above.
            // Compared only, a zero written today is carried as zero so the
            // comparison can name it; it never runs, the record does.
            timeout: g
                .timeout_secs
                .map_or(DEFAULT_GATE_TIMEOUT, Duration::from_secs),
        });
    }
    Ok(Some(list))
}

/// What today's `[[gates]]` section is *for* on this load — the one question
/// [`parse_gates`] asks of [`EngineLimits`]. See its doc for which reading
/// maps to which.
#[derive(Clone, Copy)]
enum GatesReading {
    /// The gates this run executes come from this section: refuse a shape
    /// the engine cannot act on.
    Governs,
    /// The gates this run executes come from its record; this section is
    /// read only to say what moved. Nothing refuses.
    ComparedOnly,
}

/// A `[[gates]]` shape the engine cannot act on: refused where the section
/// governs the run, announced where it is only compared with the recorded
/// gates (`design/15`: taken from the record and not refused over). See
/// [`parse_gates`].
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

/// Every accepted `[engine]` key, written out, for the same reason as
/// [`RUNNER_KEYS`]: the refusal names this list, so a key that stops being
/// read is a key that stops being offered.
const ENGINE_KEYS: &str = "`shell`, `on_task_failure`, `max_parallel`, `max_merge_repairs`, \
                           `max_per_agent`, `max_per_pool`";

/// `[engine]` (§17).
///
/// Every key here is now consumed or refused. Nothing is
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
/// **Which side each key sits on.** An unknown key **errors**, every one
/// named with the accepted set, on every reading. Until 2026-09-05 it warned,
/// on the reasoning that the keys here are ceilings and timeouts a typo merely
/// leaves at their defaults — but `on_task_failure` and `shell` sit in the
/// same table, and `on_task_failur = "continue"` under that reasoning was a
/// run that halted on its first failed task with a warning beside it: a
/// deleted control with a footnote. A key typo cannot be told from a deleted
/// key, so the table refuses, as every other section does; a misspelled key
/// governs no run, recorded or not, so the resume readings change nothing
/// here. An unknown `shell` **value** **warns** and takes the platform default
/// — the gate commands still run, under the shell the platform would have used
/// — and it is the one value in this module that degrades rather than refuses
/// (`SWEEP-CONFIG-PARSE-012`). `on_task_failure` **errors** on an unknown
/// value: a misspelling there decides whether a failed task stops the run. A
/// zero ceiling **errors** on every reading of `limits`: "no ceiling" and
/// "nothing may run" are two meanings, and a resume must not become a way
/// around the check.
///
/// # Errors
///
/// [`UpstrokeError::Config`]: the section is not a table of the six optional
/// keys; a key outside [`ENGINE_KEYS`]; `on_task_failure` is not `halt` or
/// `continue`; any ceiling is zero or not a whole number; `max_parallel`
/// above [`DEFAULT_MAX_PARALLEL`] on a fresh run.
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

        // A run whose log records its gates is resumed through the same
        // file: the section is compared only, so the key is named, the entry
        // is kept at the default the typo left it at, and the recorded gates
        // are named as what runs.
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

        // A run whose log has no gate record settles them from this file, so
        // the file governs and the typo is refused exactly as for a fresh run:
        // the reviewer's slow gate is not killed at 600 s with a footnote.
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

        // A run whose log records its gates is resumed through the same
        // file: both entries are kept, so the record can be compared with
        // them, and the repeat is announced with the recorded gates named as
        // what runs. A run with no gate record refuses it as a fresh run does.
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
        // A blank field refuses wherever the section governs the run, and the
        // refusal says which field, where it used to say only that one of the
        // two was blank. Where the section is only compared with a record it
        // is announced, with the same words, and the entry kept as written.
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
        // The shapes a fresh run refuses that the tests above do not already
        // drive under the compare-only reading: a zero timeout (carried as
        // zero so the comparison can name it), an entry that is not a table
        // (skipped, since nothing can be built from it), and a section that
        // is not an array (nothing to compare: `None`). Each is announced
        // with the recorded gates named as what runs, and none refuses —
        // `design/15`'s "not refused over", shape by shape. The same three
        // refuse under the two readings where the section governs.
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
        // And the second warning disowns the comparison the engine will make
        // against derived defaults, so a "difference" reported after it is
        // not read as an edit to this file.
        assert!(
            warnings.iter().skip(1).any(|w| {
                w.contains("cannot be compared") && w.contains("derived from the repository")
            }),
            "the comparison is disowned: {warnings:?}"
        );

        // The control: where the section governs, each of the three refuses.
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
        // `on_task_failur = "continue"` used to warn and leave `halt` in
        // place: a deleted control with a footnote. Every unknown key is
        // named, the accepted set is listed, and no reading softens it — a
        // misspelled key governs no run, recorded or not.
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
        // The control: the key spelt right is consumed, on every reading.
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

    #[test]
    fn a_missing_gate_key_is_named_beside_the_unknown_key_that_replaced_it() {
        // `nmae = "check"` used to fail inside serde as "missing field `name`",
        // which names the field the operator did not write and not the one
        // they did. Both are named now, on the readings where the section
        // governs; where it is only compared, the entry cannot be built and
        // is skipped with the same words.
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

        // Missing with nothing misspelt is named alone, so the sentence about
        // a misspelling is not printed where there is none to point at.
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
        // The bound is the serialiser's own arithmetic, asserted here rather
        // than restated: `duration_millis::serialize` writes
        // `u64::try_from(d.as_millis()).unwrap_or(u64::MAX)`, so the largest
        // whole second it carries exactly is the one whose millisecond count
        // still converts.
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
        // Compared only: announced and carried as written, since it never runs.
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

        // The bound itself loads, on every reading, and reaches the gate.
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
