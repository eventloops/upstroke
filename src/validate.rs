//! `upstroke validate`: parse → config → graph checks → routing preview →
//! rendered report. No execution of anything.
// LEGACY-EFFECT: this module is in the **frozen legacy section** of
// `effects/allowlist.toml`, which carries its justification and the condition
// under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods)]

mod graph;
mod render;

use std::fs;
use std::path::{Path, PathBuf};

use crate::agent;
use crate::capacity;
use crate::config::{self, Config};
use crate::error::UpstrokeError;
use crate::gates::{self, ShellGate};
use crate::ir::Plan;
use crate::plan::{self, Parsed};
use crate::review;
use crate::route::{self, ResolvedChain};

#[derive(Debug, Clone)]
pub struct ValidateOptions {
    pub plan_path: PathBuf,
    /// Explicit `--config` path; `None` looks for `upstroke.toml` in
    /// `config_root`.
    pub config_path: Option<PathBuf>,
    /// Root of the repo the plan targets: config discovery and gate
    /// derivation both resolve here, never against the process CWD.
    pub config_root: PathBuf,
    /// Pools file override for tests; `None` discovers `~/.upstroke/pools.toml`.
    pub pools_path: Option<PathBuf>,
    /// Which reading of `[engine]`'s ceilings applies (see
    /// [`config::EngineLimits`]). `Fresh` for `upstroke validate` and for a run
    /// about to be created; a resume passes the reading its own recorded schema
    /// selects.
    ///
    /// Carried here rather than decided inside `analyze` because only the
    /// caller knows which it is, and the difference is a refusal.
    pub engine_limits: config::EngineLimits,
}

#[derive(Debug)]
pub struct Row {
    pub id: String,
    pub kind: String,
    pub deps: String,
    pub chain: String,
}

#[derive(Debug)]
pub struct Report {
    pub plan: Plan,
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
    pub strategy: String,
    pub capacity: String,
    /// Who reviews, and where a second opinion applies (§11.2–§11.3).
    pub review: String,
    /// Effective reasoning policy before any process is spawned.
    pub effort: String,
    pub gates: Vec<String>,
    pub gates_from_config: bool,
}

/// The shared front half of `validate` and the engine's pre-flight (§14:
/// "plan parses cycle-free"): parse, load config, check the graph, resolve
/// every routing chain. Executes nothing.
#[derive(Debug)]
pub struct Analysis {
    pub plan: Plan,
    pub config: Config,
    /// One resolved chain per task, aligned with `plan.tasks`.
    pub chains: Vec<ResolvedChain>,
    /// Effective gates: `[[gates]]` verbatim, else derived from the repo's
    /// shape (§17) — the single derivation point for validate and the engine.
    pub gates: Vec<ShellGate>,
    pub gates_from_config: bool,
    pub warnings: Vec<String>,
}

/// Every file an [`Analysis`] is derived from, captured at one instant.
///
/// The set has to be *complete* to be worth anything. A capture that covers the
/// config but not the plan, or the plan but not the files the gate derivation
/// reads, licenses exactly the confusion it was introduced to rule out: a caller
/// compares equal captures, concludes nothing moved, and adopts an analysis that
/// depended on something outside the comparison. So this names all of them, and
/// [`analyze_captured`] parses out of it rather than beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedInputs {
    plan: config::FileSnapshot,
    config: config::CapturedConfig,
    /// The worktree files the gate derivation looks at when `[[gates]]` does not
    /// spell the gates out: `Cargo.toml`, `go.mod`, and `package.json` beside
    /// the repo root, which are what [`crate::gates::derive`] consults and the
    /// whole of what it consults. Captured here so that a change to one of them
    /// is a change to this analysis's inputs and not an unobserved edit —
    /// keep this list in step with `gates::derive` itself.
    gate_inputs: Vec<config::FileSnapshot>,
}

/// The gate derivation's inputs, relative to the repo root — see
/// [`CapturedInputs::gate_inputs`].
const GATE_DERIVATION_INPUTS: &[&str] = &["Cargo.toml", "go.mod", "package.json"];

impl CapturedInputs {
    /// Capture what an [`analyze`] with these options reads.
    #[must_use]
    pub fn capture(opts: &ValidateOptions) -> Self {
        Self {
            plan: config::snapshot_file(&opts.plan_path, true),
            config: config::CapturedConfig::capture(
                opts.config_path.as_deref(),
                &opts.config_root,
                opts.pools_path.as_deref(),
            ),
            gate_inputs: GATE_DERIVATION_INPUTS
                .iter()
                .map(|name| config::snapshot_file(&opts.config_root.join(name), false))
                .collect(),
        }
    }

    /// Every captured file, in a stable order, for a caller that has to name
    /// them in a message.
    pub fn paths(&self) -> Vec<PathBuf> {
        std::iter::once(&self.plan)
            .chain(self.config.files())
            .chain(&self.gate_inputs)
            .map(|file| file.path().to_path_buf())
            .collect()
    }
}

pub fn analyze(opts: &ValidateOptions) -> Result<Analysis, UpstrokeError> {
    analyze_captured(&CapturedInputs::capture(opts), opts)
}

/// [`analyze`], out of bytes that were captured earlier.
///
/// The plan, the repo config and the pools file are parsed from `captured` and
/// from nowhere else, so the analysis this returns is bound to those exact
/// bytes: a caller holding the same `CapturedInputs` can prove what was
/// validated by comparing it against the filesystem, and a file that changed and
/// changed back cannot slip between the check and the answer, because there is
/// only one read.
///
/// The one input still read from the filesystem here is the gate derivation's:
/// [`crate::gates::derive`] takes a directory, and the three files it looks at
/// are captured but not consumed. A caller that needs the derivation pinned runs
/// this where the worktree cannot move — see the engine's pre-flight, which
/// takes its answer under the worktree lease.
pub fn analyze_captured(
    captured: &CapturedInputs,
    opts: &ValidateOptions,
) -> Result<Analysis, UpstrokeError> {
    // Named off the capture rather than off `opts`, so an error cannot report a
    // path other than the one that was actually read.
    let raw = captured.plan.text()?.ok_or_else(|| UpstrokeError::Io {
        path: captured.plan.path().to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "plan not found"),
    })?;
    let Parsed {
        plan,
        warnings: mut all_warnings,
    } = plan::detect(&raw)?.parse_with_warnings(&raw)?;
    let config = config::load_captured(&captured.config, opts.engine_limits, &mut all_warnings)?;
    graph::check_graph(&plan, &mut all_warnings)?;
    let config_path = || {
        opts.config_path
            .clone()
            .unwrap_or_else(|| opts.config_root.join("upstroke.toml"))
    };
    check_pin_adapters(&config.pins, builtin_adapter, &config_path())?;
    let chains: Vec<ResolvedChain> = plan
        .tasks
        .iter()
        .map(|t| route::resolve(t, &config))
        .collect();
    let gates_from_config = config.gates.is_some();
    let gates = match &config.gates {
        Some(configured) => configured
            .iter()
            .map(|g| ShellGate {
                name: g.name.clone(),
                cmd: g.cmd.clone(),
                timeout: g.timeout,
                shell: config.shell,
            })
            .collect(),
        None => gates::derive(&opts.config_root, config.shell),
    };
    Ok(Analysis {
        plan,
        config,
        chains,
        gates,
        gates_from_config,
        warnings: all_warnings,
    })
}

/// Whether this build ships an adapter for `agent`.
///
/// Injected into the checks below rather than called from them, so the guards
/// can be tested against agents that do and do not exist without waiting for
/// the registry to grow one.
pub fn builtin_adapter(agent: &str) -> bool {
    agent::by_id(agent).is_some()
}

fn adapter_list() -> String {
    agent::ADAPTERS
        .iter()
        .map(|a| a.id())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A pin naming an agent with no adapter must fail the same way in `validate`
/// and `run`; otherwise the preview promises a binding the run then refuses at
/// pre-flight (§18).
///
/// Currently unreachable through `upstroke.toml` alone — `config::load` rejects
/// any pin whose (agent, model) is absent from the catalog, and every catalog
/// agent has an adapter as of step 9. It stays because that is a coincidence of
/// today's table, not a property: §13 says the catalog ships ahead of support
/// (Aider models are catalogued in v0.2 before its adapter lands), and the
/// moment it does, this is what stops a preview from promising them.
fn check_pin_adapters(
    pins: &[config::Pin],
    has_adapter: impl Fn(&str) -> bool,
    config_path: &Path,
) -> Result<(), UpstrokeError> {
    for pin in pins {
        if !has_adapter(&pin.agent) {
            return Err(UpstrokeError::Config {
                path: config_path.to_path_buf(),
                message: format!(
                    "pin for tier `{}` names agent `{}`, which has no adapter in this build \
                     (available: {})",
                    pin.tier,
                    pin.agent,
                    adapter_list()
                ),
            });
        }
    }
    Ok(())
}

pub fn run(opts: &ValidateOptions) -> Result<Report, UpstrokeError> {
    let analysis = analyze(opts)?;
    let mut warnings = analysis.warnings;
    // Zero-spend preview of the §14 gate pre-flight: warn, never refuse.
    gates::preview_resolution(&analysis.gates, &opts.config_root, &mut warnings);
    // Who would judge the work (§11.2–§11.3), against the adapters this binary
    // ships. A run asks the same question of the adapters its own harness
    // holds, which in production is the same set — so the preview cannot
    // promise a reviewer the run would then refuse.
    let reviews = review::plan_for(
        &analysis.plan,
        &analysis.chains,
        &analysis.config,
        builtin_adapter,
        &mut warnings,
    )?;
    let rows = analysis
        .plan
        .tasks
        .iter()
        .zip(&analysis.chains)
        .enumerate()
        .map(|(index, (task, chain))| {
            let second = reviews.second_opinion.get(index).and_then(Option::as_ref);
            render::to_row(task, chain.clone(), second)
        })
        .collect();
    let (observations, run_id) = latest_run_observations(
        &opts.config_root,
        !analysis.config.pools.is_empty(),
        &mut warnings,
    );
    Ok(Report {
        rows,
        warnings,
        strategy: render::strategy_echo(&analysis.config),
        capacity: render::capacity_echo(&analysis.config, &observations, run_id.as_deref()),
        review: render::review_echo(&reviews),
        effort: render::effort_echo(&analysis.config),
        gates: analysis.gates.iter().map(|g| g.name.clone()).collect(),
        gates_from_config: analysis.gates_from_config,
        plan: analysis.plan,
    })
}

/// §13's observations, without executing anything: fold the latest run in this
/// repository, if there is one.
///
/// A missing or unreadable run is not an error here. `validate` describes a
/// plan; a broken run directory beside it is somebody else's problem, and
/// refusing to preview a plan over one would be a strange trade.
/// `has_pools` short-circuits the whole fold. With no pools connected the
/// capacity block is one line and the observations are never consulted, so
/// parsing an entire run's log for it is work with no reader — and `validate`
/// is the fast, zero-spend iteration loop §18 puts on day one.
fn latest_run_observations(
    repo_root: &Path,
    has_pools: bool,
    warnings: &mut Vec<String>,
) -> (capacity::Observations, Option<String>) {
    let none = || (capacity::Observations::default(), None);
    if !has_pools {
        return none();
    }
    let Some(run_id) = crate::rundir::latest_run(repo_root) else {
        return none();
    };
    let events_path = crate::rundir::public_dir(repo_root, &run_id).join("events.jsonl");
    let mut ignored = Vec::new();
    match crate::events::read_all(&events_path, &mut ignored) {
        Ok(events) => (capacity::observe(&events), Some(run_id)),
        // A run that exists but cannot be folded is not "no run" — and
        // `read_all`'s refusal ("the log has been rewritten…") is exactly the
        // loud error the event-log design exists to produce, so swallowing it
        // and reporting an empty repository hid two things at once.
        Err(error) => {
            warnings.push(format!(
                "run {run_id} exists but its event log could not be folded for self-metered \
                 spend ({error}); the capacity block below rests on rate-limit signals alone"
            ));
            none()
        }
    }
}

impl Report {
    /// The rendered preview.
    ///
    /// The surface stays here — it is the one every caller names, and the one
    /// `effects/wrappers.toml` classifies under this module — while the table
    /// it produces is `render::report`.
    pub fn render(&self) -> String {
        render::report(self)
    }

    pub fn write_normalized_json(&self, path: &Path) -> Result<(), UpstrokeError> {
        let json = serde_json::to_string_pretty(&self.plan).map_err(|e| UpstrokeError::Parse {
            message: format!("serializing normalized plan: {e}"),
        })?;
        fs::write(path, json + "\n").map_err(|source| UpstrokeError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::env;
    use std::io::{self, ErrorKind, Write};
    use std::sync::{Mutex, OnceLock};

    fn opts(plan: impl Into<PathBuf>) -> ValidateOptions {
        let hermetic_root =
            env::temp_dir().join(format!("upstroke-validate-hermetic-{}", std::process::id()));
        fs::create_dir_all(&hermetic_root).expect("hermetic root");
        ValidateOptions {
            plan_path: plan.into(),
            config_path: None,
            config_root: hermetic_root,
            engine_limits: config::EngineLimits::Fresh,
            pools_path: Some({
                // A real, empty pools file: an explicit `--pools` that does not
                // exist is a hard error, and `None` would reach for the
                // operator's own `~/.upstroke/pools.toml`.
                // Created once: identical for every caller, and rewriting one
                // shared path from parallel tests truncates it under a reader.
                static PATH: OnceLock<PathBuf> = OnceLock::new();
                PATH.get_or_init(|| {
                    let dir = env::temp_dir()
                        .join(format!("upstroke-validate-nopools-{}", std::process::id()));
                    fs::create_dir_all(&dir).expect("scratch dir");
                    let path = dir.join("pools.toml");
                    fs::write(
                        &path,
                        "# no pools
",
                    )
                    .expect("empty pools file");
                    path
                })
                .clone()
            }),
        }
    }

    /// A scratch repo root of its own, so a test that rewrites its inputs
    /// cannot be read half-written by another running beside it.
    fn scratch_root(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("upstroke-validate-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch root");
        dir
    }

    /// [`opts`], rooted in `root` rather than in the shared hermetic directory.
    fn opts_in(root: &Path, plan: &str) -> ValidateOptions {
        let mut opts = opts(plan);
        opts.config_root = root.to_path_buf();
        opts
    }

    /// The name [`Corpus::of`] gives its `index`-th attempt on this thread.
    ///
    /// Shared with the allocator rather than restated, so the witness that
    /// pre-creates a collision cannot drift from the rule it is testing.
    fn candidate(tag: &str, index: usize) -> PathBuf {
        env::temp_dir().join(format!(
            "upstroke-validate-{tag}-{}-{:?}-{index}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    thread_local! {
        /// The attempt counter, **per thread rather than per process**.
        ///
        /// The thread id is already in the name, so a process-wide counter
        /// separates nothing a thread-local one does not — and it costs two
        /// things. It couples parallel tests' names to each other's allocation
        /// rate, and it makes
        /// [`a_corpus_steps_over_a_name_already_taken_and_leaves_it_alone`]
        /// non-deterministic: that witness reads the value its own next call
        /// will use, and a test running beside it could consume that value
        /// first, leaving the witness green with no collision ever driven.
        static NEXT_INDEX: Cell<usize> = const { Cell::new(0) };
    }

    /// The next attempt index on this thread, advancing it.
    fn next_index() -> usize {
        NEXT_INDEX.with(|next| {
            let index = next.get();
            next.set(index + 1);
            index
        })
    }

    /// The next attempt index on this thread, **without** advancing it.
    fn peek_index() -> usize {
        NEXT_INDEX.with(Cell::get)
    }

    /// Names [`Corpus::of`] — and [`Leftover::plant`], which steps in lockstep
    /// with it — try before giving up. Each attempt takes a fresh index, so the
    /// only way to burn one is a directory a *previous* process left at the
    /// same pid, thread id and index — and a run leaves at most as many per
    /// thread as it built guards on it, which is single digits here. 64 is two
    /// orders of magnitude past that and still bounded, and it is not the cap
    /// that catches a broken temp directory: any error other than
    /// `AlreadyExists` fails on the first attempt.
    const ATTEMPTS: usize = 64;

    /// A directory holding [`crate::plan::corpus`] that **owns** its tree.
    ///
    /// The corpus is inline, but [`run`] reads its plan from a path, so these
    /// tests still need files on disk. §12's hermetic rule is what shapes the
    /// type, and `engine::topology::prelock::tests::Scratch` is the precedent
    /// it copies: the same three decisions, for the same reasons that file
    /// records after a `fn scratch(&str) -> PathBuf` handed back a path nothing
    /// owned and leaked 5050 roots.
    ///
    /// **The name reads no clock.** A wall clock would make every test that
    /// builds one depend on ambient time, which §12 forbids: a host set before
    /// the epoch would panic each of them before it reached any validation
    /// behaviour. What replaces it is the precedent's naming — the pid and
    /// `std::thread::current().id()`, which is how that file keeps two live
    /// fixtures apart — plus [`NEXT_INDEX`], which makes the name unique within
    /// a process rather than leaving that to an assumption about how the
    /// harness schedules tests.
    ///
    /// **Every one of those components resets with the process, so the name is
    /// reproducible across runs and the allocator has to expect a collision.**
    /// A run killed before its guards dropped leaves directories that a later
    /// run under a reused pid will name again. [`Corpus::of`] creates its leaf
    /// with [`fs::create_dir`] rather than `create_dir_all`, so a name already
    /// taken is refused rather than adopted — §8's rule that a check followed
    /// by a write is not exclusive — and then **steps over it**: `AlreadyExists`
    /// takes the next index, never the directory. It is not adopted, it is not
    /// deleted (deleting a directory this process did not create is
    /// `scratch_root`'s anti-pattern, which C-006 exists for), and it is not
    /// written into. Any other error still fails on the first attempt, naming
    /// the candidate.
    ///
    /// **The guard exists before the first fallible write.** Constructing it
    /// after the four writes leaves the window the reviewer named: the temp
    /// filesystem fills, a write panics, no guard exists, the partial directory
    /// stays. [`Corpus::of`] creates the directory, builds `Self`, and writes
    /// through it.
    ///
    /// [`Corpus::plan`] can only be reached from a value, and a value only
    /// exists once every plan is written, so no caller sees a path before the
    /// file under it is complete — [`opts`]'s pools file is written inside its
    /// `OnceLock` for the same reason.
    struct Corpus {
        dir: PathBuf,
    }

    impl Corpus {
        fn new() -> Self {
            Self::of("corpus", &crate::plan::corpus::PLANS)
        }

        /// [`Corpus::new`] with the tag and the plan table named, so this
        /// guard's own witnesses can drive a write that fails and then look for
        /// what it left under a tag nothing else uses.
        fn of(tag: &str, plans: &[(&str, &str)]) -> Self {
            let mut tried = 0usize;
            let dir = loop {
                let candidate = candidate(tag, next_index());
                tried += 1;
                match fs::create_dir(&candidate) {
                    Ok(()) => break candidate,
                    // Step over it. Never adopt a directory this process did
                    // not create, and never delete one either: that is
                    // `scratch_root`'s anti-pattern, which C-006 exists for.
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        assert!(
                            tried < ATTEMPTS,
                            "no free corpus directory in {tried} names on this thread; \
                             the last one tried was {}",
                            candidate.display()
                        );
                    }
                    Err(error) => {
                        panic!("corpus directory {}: {error}", candidate.display())
                    }
                }
            };
            // Before the writes, not after: from here on every exit out of this
            // function is an exit out of a live guard.
            let corpus = Self { dir };
            for (name, text) in plans {
                fs::write(corpus.dir.join(name), text).expect("corpus plan");
            }
            corpus
        }

        /// The path of one plan, by the file name it carried under `fixtures/`.
        fn plan(&self, name: &str) -> PathBuf {
            self.dir.join(name)
        }
    }

    impl Drop for Corpus {
        fn drop(&mut self) {
            reclaim("corpus directory", &self.dir);
        }
    }

    /// Remove `dir`, and say so if that fails — on every path.
    ///
    /// Gone is the end state, whoever got there first: `NotFound` is success,
    /// not a failure to report. Any other error is the leak the guards exist to
    /// close, and it is reported on both exits. On the ordinary exit the report
    /// is a panic, which fails the test. While a panic is already travelling it
    /// is a line on stderr instead, because a second panic out of a destructor
    /// aborts the process and replaces the test's own failure report with
    /// nothing at all — the leak would cost the diagnosis of whatever actually
    /// broke. `eprintln!` is not that line: it panics on a write error, which is
    /// the abort again, and it is a denied macro in this file. `writeln!` to
    /// stderr returns the write's error instead, and that is matched rather
    /// than discarded, the way `rundir::scratch_tree`'s reporter matches its
    /// own. There is genuinely nothing to do with it — the channel that would
    /// carry a complaint is the one that just failed.
    fn reclaim(what: &str, dir: &Path) {
        let failure = match fs::remove_dir_all(dir) {
            Ok(()) => return,
            Err(error) if error.kind() == ErrorKind::NotFound => return,
            Err(error) => error,
        };
        let message = format!("the {what} {} was not reclaimed: {failure}", dir.display());
        if std::thread::panicking() {
            match writeln!(io::stderr(), "{message}") {
                Ok(()) => {}
                Err(_reporting_failed) => {}
            }
        } else {
            panic!("{message}");
        }
    }

    /// A corpus of one plan: enough for a witness whose subject is the guard's
    /// reclamation rather than anything the plan says.
    const ONE_PLAN: [(&str, &str); 1] = [("one.md", "## One\n")];

    /// Every directory [`Corpus::of`] made in this process under `tag`.
    ///
    /// A prefix rather than a whole name: what the guard's witnesses assert is
    /// that nothing of theirs survives, and the pid keeps a parallel suite's
    /// other processes out of the answer. A search that matched nothing would
    /// make an "it left nothing" assertion vacuous, so every caller pairs it
    /// with a live guard it must find.
    fn residue(tag: &str) -> Vec<PathBuf> {
        let prefix = format!("upstroke-validate-{tag}-{}-", std::process::id());
        let mut found: Vec<PathBuf> = fs::read_dir(env::temp_dir())
            .expect("the temp directory lists")
            .map(|entry| entry.expect("a temp directory entry reads").path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
            })
            .collect();
        found.sort();
        found
    }

    /// Whether `path` is gone — as distinct from "could not tell".
    ///
    /// `Path::exists` folds every error into `false`, so a permission error or
    /// an unreadable parent reads as absence, and a witness for reclamation
    /// would pass on the strength of a stat it never completed. `NotFound` is
    /// the only answer that means gone; anything else fails the test that
    /// asked. `symlink_metadata` rather than `metadata`, so a dangling symlink
    /// is a thing that is there rather than a thing that is not.
    fn is_gone(path: &Path) -> bool {
        match fs::symlink_metadata(path) {
            Ok(_) => false,
            Err(error) if error.kind() == ErrorKind::NotFound => true,
            Err(error) => panic!("could not tell whether {} is gone: {error}", path.display()),
        }
    }

    /// A directory this test planted to stand in for a killed run's leftover,
    /// reclaimed by **the test's own guard** rather than by the [`Corpus`]
    /// under test — which must leave it alone, and the witness that plants one
    /// asserts exactly that.
    ///
    /// Plainly not a `Corpus`: it holds no plans, and it is planted with the
    /// index the next `Corpus::of` on this thread will try, not consumed from
    /// it. A panic anywhere in the test that owns one still reclaims it.
    struct Leftover {
        dir: PathBuf,
    }

    impl Leftover {
        /// Plant the directory the next [`Corpus::of`] on this thread will try
        /// first.
        ///
        /// The name is as predictable as the guard's, so a name already taken —
        /// an actual leftover from an earlier run, which is the very thing this
        /// stands in for — must not fail the test: it is stepped over the way
        /// `Corpus::of` steps, and **in lockstep with it**. `peek_index` shows
        /// the next index without consuming it and `Corpus::of` consumes one
        /// per attempt, so a skip here consumes one too. The invariant the
        /// witness rests on — that after planting, the next `Corpus::of` on this
        /// thread selects precisely the planted directory — is asserted rather
        /// than assumed, because a witness that got it wrong would go green
        /// having driven no collision at all.
        fn plant(tag: &str) -> Self {
            let mut tried = 0usize;
            let dir = loop {
                let candidate = candidate(tag, peek_index());
                tried += 1;
                match fs::create_dir(&candidate) {
                    Ok(()) => break candidate,
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                        // The index the guard would have skipped too.
                        next_index();
                        assert!(
                            tried < ATTEMPTS,
                            "no free name to plant in {tried} attempts on this thread; \
                             the last one tried was {}",
                            candidate.display()
                        );
                    }
                    Err(error) => panic!("planting {}: {error}", candidate.display()),
                }
            };
            let planted = Self { dir };
            assert_eq!(
                candidate(tag, peek_index()),
                planted.dir,
                "the next Corpus::of on this thread would not try the planted directory, \
                 so no collision would be driven"
            );
            planted
        }
    }

    impl Drop for Leftover {
        fn drop(&mut self) {
            reclaim("planted leftover", &self.dir);
        }
    }

    /// A [`Corpus`] directory with its write permission taken away, so the
    /// guard's own removal fails for a real reason, and the test's guard that
    /// puts the permission back and reclaims the tree when the test ends —
    /// on the panic path as well.
    ///
    /// Unix only, and the doc comments on the two witnesses that use it say
    /// what that leaves undriven.
    #[cfg(unix)]
    struct Unwritable {
        dir: PathBuf,
    }

    #[cfg(unix)]
    impl Unwritable {
        /// Take write permission away from `dir`, so nothing inside it can be
        /// unlinked and `remove_dir_all` fails with `PermissionDenied`.
        ///
        /// The prerequisite is checked rather than assumed, because mode bits
        /// bind an ordinary user and not a privileged one, and a witness that
        /// passed for the wrong reason under root would be worse than one that
        /// fails with a diagnostic (§12).
        fn new(dir: PathBuf) -> Self {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o500))
                .expect("the corpus directory takes new mode bits");
            let held = Self { dir };
            match fs::create_dir(held.dir.join("probe")) {
                Err(error) if error.kind() == ErrorKind::PermissionDenied => held,
                Ok(()) => panic!(
                    "this witness needs a user the mode bits bind: a create inside a 0o500 \
                     directory succeeded (running as root?), so no genuine removal failure \
                     can be driven here"
                ),
                Err(error) => panic!("probing {}: {error}", held.dir.display()),
            }
        }
    }

    #[cfg(unix)]
    impl Drop for Unwritable {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            // Put the permission back first, or the reclamation below fails
            // for the very reason this type exists to cause.
            match fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700)) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) if std::thread::panicking() => match writeln!(
                    io::stderr(),
                    "the mode bits on {} were not restored: {error}",
                    self.dir.display()
                ) {
                    Ok(()) => {}
                    Err(_reporting_failed) => {}
                },
                Err(error) => {
                    panic!(
                        "the mode bits on {} were not restored: {error}",
                        self.dir.display()
                    )
                }
            }
            reclaim("unwritable corpus directory", &self.dir);
        }
    }

    /// The guard reclaims its tree on every exit — the ordinary one and the
    /// unwind, which is the exit a failing assertion in any test below takes.
    ///
    /// Each path is recorded from inside the scope that made it rather than
    /// re-derived out here: a witness that restates [`Corpus::of`]'s naming
    /// rule agrees with whatever that rule happens to be and proves nothing
    /// about it.
    ///
    /// The panic hook is deliberately **not** silenced for the second half.
    /// The hook is process-global and this suite runs in parallel, so a test
    /// that takes it, installs a no-op and restores it can interleave with
    /// another doing the same and leave the process with a no-op hook for good
    /// — every later panic anywhere in the suite losing its message. The few
    /// lines this prints cost less than that.
    #[test]
    fn a_corpus_directory_is_reclaimed_on_every_exit_including_an_unwind() {
        let ordinary = {
            let corpus = Corpus::new();
            let plan = corpus.plan("sample-plan.md");
            assert!(plan.is_file(), "the corpus wrote no sample plan");
            corpus.dir.clone()
        };
        assert!(
            is_gone(&ordinary),
            "the corpus directory {} outlived its guard on the ordinary exit",
            ordinary.display()
        );

        let recorded = Mutex::new(None);
        let unwound = std::panic::catch_unwind(|| {
            let corpus = Corpus::new();
            *recorded
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(corpus.dir.clone());
            // The shape of a real failure: an assertion about the run that does
            // not hold, raised with the guard still in scope.
            assert!(
                !corpus.plan("sample-plan.md").is_file(),
                "a deliberate failure, mid-test"
            );
        });
        assert!(
            unwound.is_err(),
            "the closure was supposed to unwind, so nothing about the panic path was measured"
        );
        let path = recorded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("the closure recorded its directory before it panicked");
        assert!(
            is_gone(&path),
            "the corpus directory {} survived the unwind",
            path.display()
        );
    }

    /// A construction that fails partway through its own setup leaves nothing
    /// behind.
    ///
    /// This is the window a guard built *after* the writes cannot cover, and it
    /// is the failure the review named: the directory exists, some plans are in
    /// it, a later write fails, and with no live guard the partial tree stays.
    /// The failing write is a real one rather than an injected fault — a plan
    /// name whose parent directory was never created, so `fs::write` fails with
    /// `NotFound` on every platform.
    ///
    /// The first half is the control, and it is not optional: [`residue`]
    /// matches on a name prefix, so a naming change alone would make "it left
    /// nothing" pass while proving nothing. The control fails first if the
    /// search cannot see a live guard of this shape.
    #[test]
    fn a_corpus_that_fails_midway_through_its_own_setup_leaves_nothing_behind() {
        const TAG: &str = "raii-setup";
        // `deeper/` is never created, so the second write fails where the first
        // succeeded: the directory is there, one plan is in it, and reclaiming
        // both is what the guard has to do from inside its own constructor.
        const PARTIAL: [(&str, &str); 2] = [("one.md", "## One\n"), ("deeper/two.md", "## Two\n")];

        let live = Corpus::of(TAG, &ONE_PLAN);
        assert_eq!(
            residue(TAG),
            vec![live.dir.clone()],
            "the search cannot see a live corpus under this tag, so its absence would prove nothing"
        );
        drop(live);
        assert!(
            residue(TAG).is_empty(),
            "the control corpus outlived its guard: {:?}",
            residue(TAG)
        );

        let failed = std::panic::catch_unwind(|| Corpus::of(TAG, &PARTIAL));
        assert!(
            failed.is_err(),
            "the setup was supposed to fail on the second plan"
        );
        assert!(
            residue(TAG).is_empty(),
            "a corpus that failed partway through its own setup left {:?}",
            residue(TAG)
        );
    }

    /// A name already taken is **stepped over**: not adopted, not deleted, not
    /// written into.
    ///
    /// This is review pass 3's own reproduction turned into a test. Every
    /// component of the name resets with the process, so a run killed before
    /// its guards dropped leaves directories a later run under a reused pid
    /// names again; exclusive creation then turned that into a hard failure
    /// rather than a step aside. The reviewer pre-created the predicted path at
    /// `dbdce08` and `sample_plan_renders_expected_table` exited 101 with
    /// `AlreadyExists`, before reaching any validation behaviour.
    ///
    /// The collision is **exact rather than probable**. [`candidate`] is the
    /// allocator's own name function, not a restatement of it, and
    /// [`peek_index`] reads the index this thread will use next without
    /// advancing it — so the directory pre-created here is precisely the one
    /// the next [`Corpus::of`] tries first. That is what [`NEXT_INDEX`] being
    /// per thread buys: a process-wide counter could be advanced by a test
    /// running beside this one, and this witness would then go green with no
    /// collision ever driven.
    ///
    /// The leftover carries a file, because "stepped over" has to mean the
    /// directory survives with its contents rather than merely surviving.
    ///
    /// The leftover is a [`Leftover`], planted with the same step-over the
    /// guard has and owned by a guard of the test's own: an earlier run's
    /// actual leftover at this name cannot fail the witness for the scenario
    /// it exists to cover, and a panic anywhere below still reclaims what was
    /// planted — by the test, never by the `Corpus`. Two are planted, so that
    /// the planter's own step-over and its lockstep with `Corpus::of` are
    /// driven on every run rather than only when an earlier run happened to
    /// leave something: the second plant finds the first's name taken, steps
    /// past it consuming that index, and asserts the next `Corpus::of` will
    /// now try the second — which it then does.
    #[test]
    fn a_corpus_steps_over_a_name_already_taken_and_leaves_it_alone() {
        const TAG: &str = "raii-taken";
        const LEFTOVER: &str = "left by a run that never dropped its guard\n";

        let first = Leftover::plant(TAG);
        let taken = Leftover::plant(TAG);
        assert_ne!(
            first.dir, taken.dir,
            "the second plant did not step past the first"
        );
        let marker = taken.dir.join("not-ours.md");
        fs::write(&marker, LEFTOVER).expect("the leftover's contents");

        let corpus = Corpus::of(TAG, &ONE_PLAN);
        assert_ne!(
            corpus.dir, taken.dir,
            "the guard adopted the directory it found rather than stepping over it"
        );
        assert!(
            corpus.plan("one.md").is_file(),
            "the guard stepped aside but wrote no corpus"
        );
        assert!(
            marker.is_file(),
            "the guard removed a directory it did not create"
        );
        assert_eq!(
            fs::read_to_string(&marker).expect("the leftover still reads"),
            LEFTOVER,
            "the guard wrote over a file it did not create"
        );

        drop(corpus);
        assert!(
            marker.is_file(),
            "the guard removed the leftover on its way out"
        );
        assert!(
            !is_gone(&first.dir),
            "the guard removed a leftover it never even collided with"
        );
        // Both leftovers reclaim themselves when this test ends, on every
        // exit. That it is the test's guards and not the `Corpus` doing so is
        // the assertion just above.
    }

    /// A directory already gone when the guard drops is **not** a failed
    /// reclamation.
    ///
    /// Gone is the end state the guard exists to reach, whoever got there
    /// first, and a guard that panicked over it would fail a test whose
    /// cleanup had already succeeded. The tree is reclaimed out from under the
    /// live guard through the very call the guard will use, so its own removal
    /// then finds `NotFound` for a real reason rather than an injected one —
    /// and nothing is reported.
    #[test]
    fn a_corpus_directory_already_gone_is_not_a_failed_reclamation() {
        let quiet = std::panic::catch_unwind(|| {
            let corpus = Corpus::of("raii-gone", &ONE_PLAN);
            fs::remove_dir_all(&corpus.dir).expect("the tree reclaims early");
        });
        assert!(
            quiet.is_ok(),
            "the guard reported an already-reclaimed tree as a failure: {:?}",
            quiet
                .err()
                .and_then(|e| e.downcast_ref::<String>().cloned())
        );
    }

    /// A reclamation that **genuinely** fails is reported, not discarded.
    ///
    /// `Drop` cannot return, so the alternative to reporting is silence — and
    /// silence here is the same leak the guard exists to close, with nothing to
    /// say it happened. The failure is real: the corpus directory has its write
    /// permission taken away, so unlinking the plan inside it is refused and
    /// `remove_dir_all` returns `PermissionDenied`; the panic that carries the
    /// report is caught here rather than failing this test, and the tree is
    /// then shown to be genuinely still there before the test's own guard puts
    /// the permission back and reclaims it.
    ///
    /// **Unix only, and this is an honest gap rather than a portable witness.**
    /// The failure the review named is a Windows one — a process holding a plan
    /// file open without delete sharing — and there is no portable way to drive
    /// a removal failure. Mode bits are the Unix way. The Windows held-handle
    /// case is **not driven by any test** in this file; on Windows this witness
    /// is absent from the suite rather than passing for the wrong reason.
    #[cfg(unix)]
    #[test]
    fn a_corpus_directory_that_cannot_be_reclaimed_is_reported_rather_than_discarded() {
        const TAG: &str = "raii-reported";
        let cleanup = Mutex::new(None);
        let reported = std::panic::catch_unwind(|| {
            let corpus = Corpus::of(TAG, &ONE_PLAN);
            *cleanup
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(Unwritable::new(corpus.dir.clone()));
            // The corpus drops here, on the ordinary exit, and cannot unlink
            // its plan.
        })
        .expect_err("the guard discarded a failed reclamation");

        let message = reported
            .downcast_ref::<String>()
            .map_or_else(String::new, Clone::clone);
        assert!(
            message.contains("was not reclaimed") && message.contains(TAG),
            "the report must name the directory it could not reclaim: {message}"
        );

        let held = cleanup
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .expect("the closure armed the test's own cleanup before the guard dropped");
        let dir = held.dir.clone();
        assert!(
            !is_gone(&dir),
            "the report was for a tree that is not there, so the failure was not genuine"
        );
        drop(held);
        assert!(
            is_gone(&dir),
            "the test's own guard did not reclaim {}",
            dir.display()
        );
    }

    /// A reclamation that fails **while a panic is already travelling** is
    /// reported without a second panic, and the primary failure is the one
    /// that arrives.
    ///
    /// The other half of the report. On this path the report is a line on
    /// stderr rather than a panic, because a second panic out of a destructor
    /// mid-unwind aborts the process. This witness runs in-process and asserts
    /// two things: that the primary panic's payload comes back intact, and that
    /// the tree the guard could not reclaim is genuinely still there for the
    /// test's own guard to take back. What it cannot assert is the stderr line
    /// itself — no in-process hook captures it — and if the destructor ever
    /// regressed to a second panic, this test would not fail so much as take
    /// the whole binary down with an abort, which is loud in its own way.
    ///
    /// Unix only, for the reason the witness above gives.
    #[cfg(unix)]
    #[test]
    fn a_reclamation_that_fails_during_an_unwind_is_reported_without_a_second_panic() {
        const PRIMARY: &str = "the primary failure this witness keeps observable";
        let cleanup = Mutex::new(None);
        let caught = std::panic::catch_unwind(|| {
            let corpus = Corpus::of("raii-unwind-unreclaimable", &ONE_PLAN);
            *cleanup
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(Unwritable::new(corpus.dir.clone()));
            panic!("{PRIMARY}");
        })
        .expect_err("the closure was supposed to unwind");

        // Reached at all only because the destructor did not panic a second
        // time.
        let message = caught
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| caught.downcast_ref::<&str>().map(|m| (*m).to_owned()))
            .unwrap_or_default();
        assert_eq!(
            message, PRIMARY,
            "the destructor's own report displaced the primary panic, so the failure a test \
             would be diagnosing is the one that got lost"
        );

        let held = cleanup
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .expect("the closure armed the test's own cleanup before it panicked");
        let dir = held.dir.clone();
        assert!(
            !is_gone(&dir),
            "the guard reclaimed the tree after all, so no failure was reported on this path"
        );
        drop(held);
        assert!(
            is_gone(&dir),
            "the test's own guard did not reclaim {}",
            dir.display()
        );
    }

    #[test]
    fn the_captured_set_names_every_file_an_analysis_reads() {
        // Completeness is the property, and it is the one an incomplete capture
        // silently loses: a caller comparing two equal captures concludes
        // nothing moved, so anything outside the comparison is free to move.
        // The plan, the repo config, the pools file, and the three worktree
        // files the gate derivation consults are the whole set.
        let root = scratch_root("capturedset");
        let plan = root.join("plan.md");
        fs::write(&plan, "## One\n<!-- upstroke: id=t1 depends= -->\n").expect("plan");
        let mut options = opts_in(&root, plan.to_str().expect("utf-8 path"));
        options.config_path = Some(root.join("upstroke.toml"));

        let captured = CapturedInputs::capture(&options);
        let mut expected = vec![plan, root.join("upstroke.toml")];
        expected.push(options.pools_path.clone().expect("the fixture pools file"));
        expected.extend(GATE_DERIVATION_INPUTS.iter().map(|name| root.join(name)));
        assert_eq!(captured.paths(), expected);
    }

    #[test]
    fn an_analysis_is_parsed_out_of_the_captured_plan_not_a_second_read_of_it() {
        // The plan is an input like any other, and it was the one an earlier
        // capture left out. Same interleaving as the config's: capture, let the
        // file become something else for exactly as long as the parse takes,
        // restore it. What comes back has to describe the captured plan.
        let root = scratch_root("capturedplan");
        let plan = root.join("plan.md");
        fs::write(&plan, "## One\n<!-- upstroke: id=t1 depends= -->\n").expect("captured plan");
        let options = opts_in(&root, plan.to_str().expect("utf-8 path"));
        let captured = CapturedInputs::capture(&options);

        fs::write(
            &plan,
            "## One\n<!-- upstroke: id=t1 depends= -->\n\
             ## Two\n<!-- upstroke: id=t2 depends=t1 -->\n",
        )
        .expect("the transient plan");
        let analysis = analyze_captured(&captured, &options).expect("the captured plan analyses");
        assert_eq!(
            analysis
                .plan
                .tasks
                .iter()
                .map(|t| t.id.as_str().to_owned())
                .collect::<Vec<_>>(),
            vec!["t1"],
            "the transient plan was parsed in place of the captured one"
        );

        fs::write(&plan, "## One\n<!-- upstroke: id=t1 depends= -->\n").expect("restored");
        assert_eq!(
            CapturedInputs::capture(&options),
            captured,
            "and the excursion leaves no trace for a confirmation to find"
        );
    }

    #[test]
    fn a_gate_derivation_input_is_part_of_the_captured_set() {
        // `gates::derive` takes a directory, so these three are captured rather
        // than consumed — which makes it worth proving they are genuinely
        // inputs, and that a change to one of them is a change the capture sees.
        let root = scratch_root("capturedgates");
        let plan = root.join("plan.md");
        fs::write(&plan, "## One\n<!-- upstroke: id=t1 depends= -->\n").expect("plan");
        let options = opts_in(&root, plan.to_str().expect("utf-8 path"));

        let bare = CapturedInputs::capture(&options);
        let analysis = analyze_captured(&bare, &options).expect("analysis");
        assert!(
            analysis.gates.is_empty(),
            "a repo of no recognised shape derives no gates: {:?}",
            analysis.gates
        );

        fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("a rust repo now");
        let shaped = CapturedInputs::capture(&options);
        assert_ne!(shaped, bare, "the capture must see the worktree change");
        let analysis = analyze_captured(&shaped, &options).expect("analysis");
        assert_eq!(
            analysis
                .gates
                .iter()
                .map(|g| g.name.clone())
                .collect::<Vec<_>>(),
            vec!["check".to_owned(), "test".to_owned()],
            "and the change is one the derivation acts on"
        );
    }

    #[test]
    fn a_pin_without_an_adapter_fails_validate_not_just_run() {
        // Every catalogued agent has an adapter as of step 9, so the guard is
        // driven directly rather than through a config file it can no longer be
        // reached from. §13 ships the catalog ahead of adapter support, which is
        // when this fires for real.
        let pins = vec![config::Pin {
            tier: crate::ir::Tier::Frontier,
            agent: "aider".to_owned(),
            model: "qwen-3-coder".to_owned(),
            effort: None,
        }];
        let err = check_pin_adapters(&pins, builtin_adapter, Path::new("upstroke.toml"))
            .expect_err("preview must not promise a binding run would refuse");
        let message = err.to_string();
        assert!(message.contains("no adapter"), "got: {message}");
        assert!(
            message.contains("claude-code") && message.contains("copilot"),
            "lists what is available: {message}"
        );

        // And it passes what this build really does ship.
        let pins = vec![config::Pin {
            tier: crate::ir::Tier::Frontier,
            agent: "copilot".to_owned(),
            model: "gpt-5.3-codex".to_owned(),
            effort: None,
        }];
        assert!(
            check_pin_adapters(&pins, builtin_adapter, Path::new("upstroke.toml")).is_ok(),
            "copilot gained an adapter in step 9"
        );
    }

    #[test]
    fn the_preview_shows_who_reviews_without_promising_a_binary_it_cannot_probe() {
        // §18: `validate` and `--dry-run` execute nothing, so they cannot check
        // that a named reviewer is installed. Saying "would be, if installed"
        // is the difference between a plan and a promise.
        let root = env::temp_dir().join(format!("upstroke-validate-review-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        let plan = root.join("plan.md");
        fs::write(
            &plan,
            "## Rotate the signing key\n\
             <!-- upstroke: id=rotate kind=implement depends= paths=src/auth/** -->\n\n\
             ## Note it down\n<!-- upstroke: id=note kind=docs depends=rotate -->\n",
        )
        .expect("plan");
        let cfg = root.join("upstroke.toml");
        fs::write(
            &cfg,
            "[[routing.overrides]]\npaths = [\"src/auth/**\"]\nsecond_opinion = \
             \"different-vendor\"\n",
        )
        .expect("config");
        let mut o = opts("unused");
        o.plan_path = plan;
        o.config_path = Some(cfg);
        let rendered = run(&o).expect("validate").render();

        assert!(
            rendered.contains("review: claude-code/claude-opus-5"),
            "{rendered}"
        );
        assert!(rendered.contains("if installed"), "{rendered}");
        assert!(
            rendered.contains("1 task(s) also require a second opinion"),
            "{rendered}"
        );
        // The per-task decision belongs in the row that explains what this
        // task's paths bought it — and only on the task whose paths matched.
        let rotate = rendered
            .lines()
            .find(|l| l.starts_with("rotate"))
            .expect("row");
        assert!(
            rotate.contains("[second opinion: copilot/gpt-5.3-codex]"),
            "{rotate}"
        );
        let note = rendered
            .lines()
            .find(|l| l.starts_with("note"))
            .expect("row");
        assert!(!note.contains("second opinion"), "{note}");
    }

    #[test]
    fn the_preview_echoes_resolved_role_tier_pin_and_disabled_review_effort() {
        let root = env::temp_dir().join(format!("upstroke-validate-effort-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        let corpus = Corpus::new();
        let cases = [
            (
                "defaults",
                "",
                "effort: implementation=by tier (small=low, mid=medium, frontier=high), review=high",
            ),
            (
                "pin-fallback",
                "[routing]\nreview = { tier = \"small\" }\n\n\
                 [[pins]]\ntier = \"small\"\nagent = \"claude-code\"\n\
                 model = \"claude-haiku-4-5\"\neffort = \"max\"\n",
                "effort: implementation=by tier (small=max, mid=medium, frontier=high), review=max",
            ),
            (
                "other-role-values",
                "[routing.effort]\nimplementation = \"low\"\nreview = \"xhigh\"\n",
                "effort: implementation=low, review=xhigh",
            ),
            (
                "configured-role-values",
                "[routing.effort]\nimplementation = \"xhigh\"\nreview = \"max\"\n",
                "effort: implementation=xhigh, review=max",
            ),
            (
                "review-disabled",
                "[routing]\nreview = { enabled = false }\n",
                "effort: implementation=by tier (small=low, mid=medium, frontier=high), review=disabled",
            ),
        ];

        for (name, config, expected) in cases {
            let cfg = root.join(format!("{name}.toml"));
            fs::write(&cfg, config).expect("config");
            let mut o = opts(corpus.plan("sample-plan.md"));
            o.config_path = Some(cfg);
            let rendered = run(&o).expect("validate").render();
            let actual = rendered
                .lines()
                .find(|line| line.starts_with("effort:"))
                .expect("effort line");
            assert_eq!(actual, expected, "case {name}:\n{rendered}");
        }
    }

    #[test]
    fn the_capacity_block_estimates_without_probing_and_never_reads_unknown_as_full() {
        let dir = env::temp_dir().join(format!("upstroke-validate-pools-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let pools = dir.join("pools.toml");
        fs::write(
            &pools,
            "[pools.claude-max]\nkind = \"subscription-window\"\nagent = \
             \"claude-code\"\nwindow = \"5h\"\nweekly = true\nsources = [\"signals\", \"self\", \
             \"local-logs\"]\nprofile = \"personal\"\n",
        )
        .expect("pools");
        let corpus = Corpus::new();
        let mut o = opts(corpus.plan("sample-plan.md"));
        o.pools_path = Some(pools);
        let rendered = run(&o).expect("validates").render();

        assert!(rendered.contains("claude-max"), "rendered:\n{rendered}");
        assert!(!rendered.contains("capacity: not connected"));
        assert!(rendered.contains("window=5h"), "rendered:\n{rendered}");
        // D2's seam is echoed even though nothing acts on it.
        assert!(
            rendered.contains("profile=personal"),
            "rendered:\n{rendered}"
        );
        // §13's conservatism, visible: an unmeasured pool reads as unknown, and
        // the block says that is not the same as full.
        assert!(
            rendered.contains("claude-max: unknown [unknown]"),
            "rendered:\n{rendered}"
        );
        assert!(rendered.contains("not full"), "rendered:\n{rendered}");
        // A source the estimate did not read must not pass as accounted for.
        assert!(
            rendered.contains("local-logs") && rendered.contains("not read in v0.1"),
            "rendered:\n{rendered}"
        );
        // §18: this command executes nothing, and says which side of that line
        // it is on rather than letting a preview read as a promise.
        assert!(rendered.contains("never probes"), "rendered:\n{rendered}");
        assert!(rendered.contains("read-only"), "rendered:\n{rendered}");
        assert!(
            rendered.contains("no run in this repository yet"),
            "rendered:\n{rendered}"
        );
    }

    #[test]
    fn derived_gates_appear_in_the_preview() {
        let root = env::temp_dir().join(format!("upstroke-validate-gates-{}", std::process::id()));
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("Cargo.toml"), "[package]\nname='x'\n").expect("marker");
        let corpus = Corpus::new();
        let mut o = opts(corpus.plan("sample-plan.md"));
        o.config_root = root;
        let report = run(&o).expect("validates");
        let rendered = report.render();
        assert!(
            rendered.contains("gates: check, test [derived]"),
            "rendered:\n{rendered}"
        );

        // Hermetic root with no markers: no gates, still explicit.
        let report = run(&opts(corpus.plan("sample-plan.md"))).expect("validates");
        assert!(report.render().contains("gates: none"));
    }

    #[test]
    fn sample_plan_renders_expected_table() {
        let corpus = Corpus::new();
        let report = run(&opts(corpus.plan("sample-plan.md"))).expect("sample plan validates");
        let rendered = report.render();

        assert!(rendered.contains("api-design"));
        assert!(
            rendered.contains("frontier(annotation)"),
            "rendered:\n{rendered}"
        );
        assert!(
            rendered.contains("mid(annotation)"),
            "min clip shows as annotation source"
        );
        assert!(rendered.contains("min=mid clipped the chain start"));
        assert!(rendered.contains("paths: src/api/**"));
        assert!(rendered.contains("small(default)=claude-code/claude-haiku-4-5(preview)"));
        assert!(rendered.contains("capacity: not connected"));
        assert!(rendered.contains("ok: 4 tasks, no cycles"));
    }

    #[test]
    fn bare_plan_validates_via_heuristics() {
        let corpus = Corpus::new();
        let report = run(&opts(corpus.plan("bare-plan.md"))).expect("bare plan validates");
        let rendered = report.render();
        assert!(rendered.contains("ok: 5 tasks, no cycles"));
        assert!(rendered.contains("design-the-search-index-schema"));
    }

    #[test]
    fn cyclic_plan_fails_naming_the_cycle() {
        let corpus = Corpus::new();
        let err = run(&opts(corpus.plan("cyclic-plan.md"))).expect_err("cycle must fail");
        let message = err.to_string();
        assert!(message.contains("dependency cycle"), "got: {message}");
        assert!(message.contains("a -> c -> b -> a"), "got: {message}");
    }

    #[test]
    fn unknown_depends_fails_clearly() {
        let dir = env::temp_dir().join(format!("upstroke-validate-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("unknown-dep.md");
        fs::write(&plan, "## One\n<!-- upstroke: id=one depends=ghost -->\n").expect("write plan");
        let mut o = opts("x");
        o.plan_path = plan;
        let err = run(&o).expect_err("unknown dep must fail");
        let message = err.to_string();
        assert!(message.contains("unknown id `ghost`"), "got: {message}");
    }

    #[test]
    fn duplicate_ids_fail() {
        let dir = env::temp_dir().join(format!("upstroke-validate-dup-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("dup.md");
        fs::write(
            &plan,
            "## One\n<!-- upstroke: id=same -->\n\n## Two\n<!-- upstroke: id=same depends= -->\n",
        )
        .expect("write plan");
        let mut o = opts("x");
        o.plan_path = plan;
        let err = run(&o).expect_err("duplicate ids must fail");
        assert!(err.to_string().contains("duplicate task id `same`"));
    }

    #[test]
    fn steps_plan_validates_via_ordered_list_fallback() {
        let corpus = Corpus::new();
        let report = run(&opts(corpus.plan("steps-plan.md"))).expect("steps plan validates");
        let rendered = report.render();
        assert!(rendered.contains("ok: 4 tasks, no cycles"));
        assert!(rendered.contains("design-the-limiter-interface-and-storage-schema"));
    }

    #[test]
    fn artifact_needed_from_a_non_dependency_warns() {
        let dir = env::temp_dir().join(format!("upstroke-wiring-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("wiring.md");
        fs::write(
            &plan,
            "## Design\n<!-- upstroke: id=d out=contract depends= -->\n\n\
             ## Build\n<!-- upstroke: id=b needs=contract depends= -->\n",
        )
        .expect("write plan");
        let mut o = opts("x");
        o.plan_path = plan;
        let report = run(&o).expect("wiring problems warn, not fail");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("`b` needs artifact `contract` produced by `d`")),
            "warnings: {:?}",
            report.warnings
        );

        // The sample plan wires artifacts along its dependency chain — silent.
        let corpus = Corpus::new();
        let clean = run(&opts(corpus.plan("sample-plan.md"))).expect("sample validates");
        assert!(clean.warnings.is_empty(), "warnings: {:?}", clean.warnings);
    }

    #[test]
    fn unrecognized_plan_format_names_available_adapters() {
        let dir = env::temp_dir().join(format!("upstroke-sniff-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let plan = dir.join("plan.json");
        fs::write(&plan, "{\"tasks\": []}\n").expect("write file");
        let mut o = opts("x");
        o.plan_path = plan;
        let err = run(&o).expect_err("json must not sniff as markdown");
        assert!(err.to_string().contains("no plan adapter recognizes"));
    }

    #[test]
    fn emit_json_round_trips_through_the_ir() {
        let corpus = Corpus::new();
        let report = run(&opts(corpus.plan("sample-plan.md"))).expect("sample plan validates");
        let dir = env::temp_dir().join(format!("upstroke-emit-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        let json_path = dir.join("plan.normalized.json");
        report
            .write_normalized_json(&json_path)
            .expect("write json");

        let text = fs::read_to_string(&json_path).expect("read back");
        let plan: Plan = serde_json::from_str(&text).expect("json matches the IR");
        assert_eq!(plan.tasks.len(), 4);
        assert_eq!(plan.source.adapter, "markdown");
        assert_eq!(plan.tasks[2].min_tier, Some(crate::ir::Tier::Mid));
    }
}
