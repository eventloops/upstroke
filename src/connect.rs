//! `tactus connect` (DESIGN.md §13, §18): discover the agent CLIs on this
//! machine and write `~/.tactus/pools.toml`.
//!
//! **Invariant 2 is the one to watch here.** Connect subprocesses the vendors'
//! own CLIs and parses what they print. No HTTP, no token ever handled, no
//! credential file read — a vendor CLI talking to its own vendor is the design,
//! not a leak, and it is the same posture §9 sets for plan importers.
//!
//! Two things this deliberately does not do:
//!
//! - **It never invents a profile.** §13 wants `connect` to enumerate
//!   credential profiles, not just binaries, so that one vendor can back
//!   several pools. There is no vendor registry of profiles to enumerate — the
//!   mechanism is a config-directory environment variable, not a list — so v0.1
//!   writes one pool per agent and leaves `profile` for the operator to add by
//!   hand. See [`crate::capacity`]'s module docs for the v0.2 sketch.
//! - **It never clobbers.** §17 calls the pools file hand-editable, and it is
//!   the file that says which subscriptions exist. An existing file that
//!   differs is printed and the command exits asking for `--force`; an
//!   identical one reports "unchanged" and rewrites nothing.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use crate::agent::{AuthState, Discovery};
use crate::capacity::{Pool, PoolKind, Source};
use crate::engine::{AdapterSource, BuiltinAdapters};
use crate::error::TactusError;
use crate::util;

#[derive(Debug, Clone, Default)]
pub struct ConnectOptions {
    /// Where to write. `None` takes `~/.tactus/pools.toml`; tests always set
    /// it, so no test can reach the operator's real pools file.
    pub pools_path: Option<PathBuf>,
    /// Overwrite an existing file that differs.
    pub force: bool,
}

/// What `connect` did, so the CLI can render it and a test can assert on it
/// without parsing prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrote {
    /// The file did not exist, or `--force` replaced one that differed.
    Written,
    /// Configures exactly what is already there. Compared over settings rather
    /// than bytes — see [`settings_of`].
    Unchanged,
    /// An existing file differs and `--force` was not given.
    Refused,
}

#[derive(Debug)]
pub struct ConnectReport {
    pub path: PathBuf,
    pub outcome: Wrote,
    /// The file `connect` produced — written, or merely proposed when it
    /// refused to clobber.
    pub content: String,
    /// One entry per registered adapter, in registry order.
    pub agents: Vec<AgentReport>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct AgentReport {
    pub agent: String,
    /// `Err` means this agent contributed no pool. It never aborts the others:
    /// a machine with Claude Code and no Copilot is the normal case, not a
    /// broken one.
    pub outcome: Result<Discovery, String>,
    pub pool: Option<Pool>,
}

/// Discover, render, and write — the whole command.
pub fn run(opts: &ConnectOptions) -> Result<ConnectReport, TactusError> {
    run_with(
        opts,
        &BuiltinAdapters,
        crate::agent::ADAPTERS.iter().map(|a| a.id()),
    )
}

/// The injectable form: `adapters` supplies the implementations and `ids` the
/// registry order, so a test can drive scripted discovery with no CLI on the
/// machine at all.
pub fn run_with<'a>(
    opts: &ConnectOptions,
    adapters: &dyn AdapterSource,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<ConnectReport, TactusError> {
    let path = match &opts.pools_path {
        Some(path) => path.clone(),
        None => util::user_tactus_dir()
            .map(|dir| dir.join("pools.toml"))
            .ok_or_else(|| TactusError::Refused {
                message: "cannot find a home directory to write ~/.tactus/pools.toml into — pass \
                          --pools <path> to say where it should go"
                    .to_owned(),
            })?,
    };

    let mut warnings = Vec::new();
    let mut agents = Vec::new();
    for id in ids {
        let Some(adapter) = adapters.get(id) else {
            continue;
        };
        // Probe first: §14 already treats a missing or broken binary as a
        // refusal to start, and discovery on a CLI that cannot even report its
        // version would be reading tea leaves.
        let discovered = adapter.probe().and_then(|_| adapter.discover());
        match discovered {
            Ok(discovery) => {
                // D1's cross-check, at the moment the roster's provenance is
                // being written into the file. Today `models` is empty on both
                // adapters and this never fires — the header says as much —
                // but the day a CLI grows enumeration, `connect` is where a
                // stale catalog entry should first be caught.
                let missing = crate::catalog::missing_from(id, &discovery.models);
                if !missing.is_empty() {
                    warnings.push(format!(
                        "{id} does not advertise catalogued model(s): {}. Cross-family review \
                         binds to catalogued names, so one this CLI rejects fails at runtime — \
                         upgrade tactus or pin a model it lists.",
                        missing.join(", ")
                    ));
                }
                let pool = pool_for_agent(id, &discovery);
                agents.push(AgentReport {
                    agent: id.to_owned(),
                    outcome: Ok(discovery),
                    pool: Some(pool),
                });
            }
            Err(error) => {
                warnings.push(format!("{id}: no pool written — {error}"));
                agents.push(AgentReport {
                    agent: id.to_owned(),
                    outcome: Err(error.to_string()),
                    pool: None,
                });
            }
        }
    }

    let content = render(&agents);
    let existing = fs::read_to_string(&path).ok();
    let outcome = match existing {
        Some(existing) if settings_of(&existing) == settings_of(&content) => Wrote::Unchanged,
        Some(_) if !opts.force => Wrote::Refused,
        _ => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|source| TactusError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            util::write_text(&path, &content)?;
            Wrote::Written
        }
    };

    Ok(ConnectReport {
        path,
        outcome,
        content,
        agents,
        warnings,
    })
}

/// The settings a pools file actually carries — comments and blank lines
/// dropped.
///
/// "Differs" has to mean *differs in what it configures*, not "differs in
/// bytes". The header names the write date, so a byte comparison would make
/// two runs a second apart look like a conflict: every re-`connect` would
/// refuse, and the only way past it would be `--force`, which is exactly the
/// flag that discards hand edits. A refusal an operator is trained to bypass
/// protects nothing.
///
/// The other direction holds too: an operator who edits only a comment has
/// changed no setting, and being told their file conflicts would be noise.
fn settings_of(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// One pool per (agent × discovered account) — today exactly one per agent,
/// because nothing enumerates credential profiles (see the module docs).
fn pool_for_agent(agent: &str, discovery: &Discovery) -> Pool {
    // §13's default where the CLI could not say: Copilot's post-Jun-2026
    // billing is credits, and everything else that reports nothing is treated
    // as a subscription window — the shape whose estimator is the most
    // conservative of the two. The rendered file carries a comment saying so,
    // because a default the operator cannot see is a guess wearing a fact's
    // clothes.
    let kind = discovery.shape.unwrap_or(match agent {
        "copilot" => PoolKind::Credits,
        _ => PoolKind::SubscriptionWindow,
    });
    // §13's trust order, minus the sources v0.1 does not read: writing
    // `local-logs` into a fresh file would promise interactive-usage awareness
    // that has not been built. An operator who wants it recorded can add it —
    // the parser accepts it and the estimate says it is unread.
    Pool::discovered(
        default_pool_name(agent),
        kind,
        agent,
        vec![Source::Signals, Source::SelfMetered],
    )
}

/// Stable, human-meaningful pool names, so a hand edit and a re-`connect`
/// converge on the same file rather than accumulating duplicates.
fn default_pool_name(agent: &str) -> &str {
    match agent {
        "claude-code" => "claude-max",
        other => other,
    }
}

/// Render the pools file: §17's shape, plus a header saying who wrote it, when,
/// and where the model roster came from.
fn render(agents: &[AgentReport]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Written by `tactus connect` v{} on {}.\n\
         #\n\
         # Pools are user-level (§17): they describe YOUR subscriptions, not this repo. The file\n\
         # is hand-editable and `tactus connect` will not overwrite your edits without --force.\n\
         #\n\
         # Model roster provenance: catalog {}, the static capability table shipped with this\n\
         # binary. Neither agent CLI offers non-interactive model enumeration as of this writing,\n\
         # so nothing here was cross-checked against what your installed CLI actually accepts.\n\
         #\n\
         # `profile` selects between several accounts on one vendor (§13). It is parsed, shown by\n\
         # `tactus capacity`, and acted on by nothing in v0.1 — add it when v0.2 wires it up.",
        env!("CARGO_PKG_VERSION"),
        util::rfc3339_utc_now(),
        env!("CARGO_PKG_VERSION"),
    );

    for report in agents {
        out.push('\n');
        match (&report.outcome, &report.pool) {
            (Ok(discovery), Some(pool)) => {
                let _ = writeln!(out, "# {}: {}", report.agent, describe_auth(discovery.auth));
                for note in &discovery.notes {
                    let _ = writeln!(out, "#   {note}");
                }
                if discovery.shape.is_none() {
                    let _ = writeln!(
                        out,
                        "#   kind below is a default, not something detected — change it if your \
                         plan differs"
                    );
                }
                out.push_str(&render_pool(pool));
            }
            _ => {
                let _ = writeln!(
                    out,
                    "# {}: not usable on this machine, so no pool was written for it.",
                    report.agent
                );
            }
        }
    }
    out
}

fn describe_auth(auth: AuthState) -> String {
    match auth {
        AuthState::Authenticated => "signed in".to_owned(),
        AuthState::NotAuthenticated => {
            "NOT signed in — log in with the vendor's own CLI before running".to_owned()
        }
        // Never rendered as "not connected": see `AuthState`.
        AuthState::Unknown => "auth state could not be determined".to_owned(),
    }
}

fn render_pool(pool: &Pool) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[pools.{}]", pool.name);
    let _ = writeln!(out, "kind = \"{}\"", pool.kind);
    let _ = writeln!(out, "agent = \"{}\"", pool.agent);
    if let Some(window) = pool.window {
        let _ = writeln!(
            out,
            "window = \"{}\"",
            crate::capacity::render_duration(window)
        );
    }
    if pool.weekly {
        let _ = writeln!(out, "weekly = true");
    }
    let sources: Vec<String> = pool.sources.iter().map(|s| format!("\"{s}\"")).collect();
    let _ = writeln!(out, "sources = [{}]", sources.join(", "));
    let _ = writeln!(out, "safety_margin = {:.2}", pool.safety_margin);
    let _ = writeln!(
        out,
        "reserve = {:.2}                     # headroom kept for your own interactive sessions",
        pool.reserve
    );
    out
}

/// What the CLI prints.
pub fn render_report(report: &ConnectReport) -> String {
    let mut out = String::new();
    for agent in &report.agents {
        match (&agent.outcome, &agent.pool) {
            (Ok(discovery), Some(pool)) => {
                let _ = writeln!(
                    out,
                    "{}: {} — pool `{}` [{}]",
                    agent.agent,
                    describe_auth(discovery.auth),
                    pool.name,
                    pool.kind
                );
                for note in &discovery.notes {
                    let _ = writeln!(out, "  {note}");
                }
            }
            (Err(error), _) => {
                let _ = writeln!(out, "{}: skipped — {error}", agent.agent);
            }
            (Ok(_), None) => {}
        }
    }
    for warning in &report.warnings {
        let _ = writeln!(out, "warning: {warning}");
    }
    match report.outcome {
        Wrote::Written => {
            let _ = writeln!(out, "wrote {}", report.path.display());
        }
        Wrote::Unchanged => {
            let _ = writeln!(out, "unchanged: {}", report.path.display());
        }
        Wrote::Refused => {
            let _ = writeln!(
                out,
                "{} already exists and differs from what connect would write. That file is \
                 hand-editable (§17), so it is not overwritten silently.\n\nWhat connect would \
                 write:\n{}\nRe-run with --force to replace it.",
                report.path.display(),
                indent(&report.content)
            );
        }
    }
    out
}

fn indent(text: &str) -> String {
    text.lines().map(|line| format!("  {line}\n")).collect()
}

/// A refusal to clobber is not an error the operator can fix by retrying, and
/// exit status is how a script tells the difference.
impl ConnectReport {
    pub fn refused(&self) -> bool {
        self.outcome == Wrote::Refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentAdapter, Caps, ProcessOutput, TaskRun};
    use crate::ir::Outcome;
    use std::path::Path;
    use std::process::Command;

    /// A scripted stand-in, so these tests run on a machine with no agent CLI
    /// installed at all.
    struct FakeAdapter {
        id: &'static str,
        discovery: Option<Discovery>,
    }

    impl AgentAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            self.id
        }

        fn probe(&self) -> Result<Caps, TactusError> {
            if self.discovery.is_none() {
                return Err(TactusError::Agent {
                    message: "binary not found on PATH".to_owned(),
                });
            }
            Ok(Caps {
                version: "0.0.0-fake".to_owned(),
                json_output: true,
                session_resume: true,
                cost_reporting: true,
                read_only_mode: true,
                acp: false,
                model_list: false,
            })
        }

        fn build(&self, _run: &TaskRun) -> Result<Command, TactusError> {
            unreachable!("connect never spawns an attempt")
        }

        fn parse(&self, _out: &ProcessOutput) -> Result<Outcome, TactusError> {
            unreachable!("connect never parses an attempt")
        }

        fn discover(&self) -> Result<Discovery, TactusError> {
            self.discovery.clone().ok_or_else(|| TactusError::Agent {
                message: "binary not found on PATH".to_owned(),
            })
        }
    }

    struct Machine {
        adapters: Vec<FakeAdapter>,
    }

    impl AdapterSource for Machine {
        fn get(&self, id: &str) -> Option<&dyn AgentAdapter> {
            self.adapters
                .iter()
                .find(|a| a.id == id)
                .map(|a| a as &dyn AgentAdapter)
        }
    }

    fn machine() -> Machine {
        Machine {
            adapters: vec![
                FakeAdapter {
                    id: "claude-code",
                    discovery: Some(Discovery {
                        auth: AuthState::Authenticated,
                        models: Vec::new(),
                        shape: Some(PoolKind::SubscriptionWindow),
                        notes: vec!["auth method `subscription`".to_owned()],
                    }),
                },
                // Installed nowhere: the normal single-vendor machine.
                FakeAdapter {
                    id: "copilot",
                    discovery: None,
                },
            ],
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tactus-connect-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir.join("pools.toml")
    }

    fn connect(path: &Path, force: bool) -> ConnectReport {
        run_with(
            &ConnectOptions {
                pools_path: Some(path.to_path_buf()),
                force,
            },
            &machine(),
            ["claude-code", "copilot"],
        )
        .expect("connect runs")
    }

    #[test]
    fn a_missing_agent_skips_its_pool_without_taking_the_others_with_it() {
        let path = scratch("partial");
        let report = connect(&path, false);
        assert_eq!(report.outcome, Wrote::Written);
        let written = fs::read_to_string(&path).expect("file");
        assert!(written.contains("[pools.claude-max]"), "{written}");
        assert!(
            !written.contains("[pools.copilot]"),
            "no pool for a CLI that is not installed: {written}"
        );
        assert!(
            written.contains("# copilot: not usable"),
            "and it says why: {written}"
        );
        assert!(
            report.warnings.iter().any(|w| w.contains("copilot")),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn what_connect_writes_parses_back_into_the_pools_it_describes() {
        // The round trip is the whole contract: a file this command writes must
        // be one `config::load` accepts, or `tactus capacity` reports on
        // something `connect` cannot produce.
        let path = scratch("roundtrip");
        connect(&path, false);
        let mut warnings = Vec::new();
        let hermetic = path.parent().expect("parent").to_path_buf();
        let cfg = crate::config::load(None, &hermetic, Some(&path), &mut warnings)
            .expect("the written file parses");
        assert_eq!(cfg.pools.len(), 1);
        let pool = &cfg.pools[0];
        assert_eq!(pool.name, "claude-max");
        assert_eq!(pool.kind, PoolKind::SubscriptionWindow);
        assert_eq!(pool.agent, "claude-code");
        assert_eq!(pool.sources, [Source::Signals, Source::SelfMetered]);
        assert_eq!(pool.safety_margin, crate::capacity::DEFAULT_SAFETY_MARGIN);
        assert_eq!(pool.reserve, crate::capacity::DEFAULT_RESERVE);
        assert_eq!(pool.profile, None, "connect never invents a profile");
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn an_existing_file_that_differs_is_never_clobbered() {
        // §17 says the file is hand-editable, so silently overwriting a hand
        // edit destroys the operator's own record of their subscriptions.
        let path = scratch("clobber");
        let mine = "[pools.claude-max]\nkind = \"subscription-window\"\nagent = \
                    \"claude-code\"\nprofile = \"work\"\n";
        fs::write(&path, mine).expect("hand-written file");

        let report = connect(&path, false);
        assert_eq!(report.outcome, Wrote::Refused);
        assert!(report.refused());
        assert_eq!(
            fs::read_to_string(&path).expect("still there"),
            mine,
            "the hand-written file is untouched"
        );
        let rendered = render_report(&report);
        assert!(rendered.contains("--force"), "{rendered}");
        assert!(
            rendered.contains("[pools.claude-max]"),
            "it shows what it would have written: {rendered}"
        );

        // --force is the escape hatch, and it really does replace.
        let forced = connect(&path, true);
        assert_eq!(forced.outcome, Wrote::Written);
        assert!(
            !fs::read_to_string(&path)
                .expect("file")
                .contains("profile = \"work\""),
            "--force replaces"
        );
    }

    #[test]
    fn re_connecting_an_unchanged_machine_reports_unchanged_rather_than_a_conflict() {
        // The header names the write date, so a byte comparison would call
        // every second run a conflict — and the only way past a conflict is
        // `--force`, the flag that discards hand edits. A refusal an operator
        // is trained to bypass protects nothing, so the comparison is over
        // settings, not bytes.
        let path = scratch("idempotent");
        connect(&path, false);
        let first = fs::read_to_string(&path).expect("file");

        let again = connect(&path, false);
        assert_eq!(again.outcome, Wrote::Unchanged, "{:?}", again.outcome);
        assert_eq!(
            fs::read_to_string(&path).expect("file"),
            first,
            "nothing changed, so nothing was rewritten — including the date it says it was written"
        );

        // And a comment-only hand edit is still no change: comments carry no
        // settings, so being told they conflict would be noise.
        fs::write(&path, format!("# my own note\n{first}")).expect("annotate");
        assert_eq!(connect(&path, false).outcome, Wrote::Unchanged);
    }

    #[test]
    fn a_cli_that_lists_models_is_cross_checked_against_the_catalog() {
        // D1's guard. It cannot fire against a real CLI today — neither
        // enumerates models — so it is driven through a scripted discovery
        // that does, which is the shape the check exists for.
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "copilot",
                discovery: Some(Discovery {
                    auth: AuthState::Authenticated,
                    // A roster that has moved on without the catalog.
                    models: vec!["gpt-6".to_owned()],
                    shape: Some(PoolKind::Credits),
                    notes: Vec::new(),
                }),
            }],
        };
        let report = run_with(
            &ConnectOptions {
                pools_path: Some(scratch("crosscheck")),
                force: false,
            },
            &machine,
            ["copilot"],
        )
        .expect("connect runs");
        let warning = report
            .warnings
            .iter()
            .find(|w| w.contains("does not advertise"))
            .unwrap_or_else(|| panic!("expected a cross-check warning: {:?}", report.warnings));
        assert!(
            warning.contains("gpt-5.3-codex"),
            "names the frontier slug the second opinion depends on: {warning}"
        );
    }

    #[test]
    fn an_undetectable_plan_shape_takes_a_default_and_says_so() {
        // The Copilot case: §13 gives it two billing shapes and the CLI
        // distinguishes neither. A default is fine; a silent default is not.
        let machine = Machine {
            adapters: vec![FakeAdapter {
                id: "copilot",
                discovery: Some(Discovery::unknown().with_note("no auth query exists")),
            }],
        };
        let path = scratch("shape");
        let report = run_with(
            &ConnectOptions {
                pools_path: Some(path),
                force: false,
            },
            &machine,
            ["copilot"],
        )
        .expect("connect runs");
        assert!(
            report.content.contains("kind = \"credits\""),
            "{}",
            report.content
        );
        assert!(
            report.content.contains("kind below is a default"),
            "the default is visible in the file: {}",
            report.content
        );
        assert!(
            report
                .content
                .contains("auth state could not be determined"),
            "unknown auth never renders as 'not connected': {}",
            report.content
        );
    }

    #[test]
    fn discovery_against_the_real_claude_binary_when_present() {
        // §13's discovery is a claim about a real CLI, so it is checked against
        // one where the machine has it — and skipped cleanly where it does not,
        // which is the shape every other binary-touching test here takes.
        let Ok(caps) = crate::agent::claude::ClaudeCodeAdapter.probe() else {
            eprintln!("skipped: no claude on PATH");
            return;
        };
        let discovery = crate::agent::claude::ClaudeCodeAdapter
            .discover()
            .expect("discovery never fails on a CLI that probes");
        // Whatever it answers, it must be one of the three states and it must
        // explain itself — including when the answer is "could not tell".
        assert!(
            !discovery.notes.is_empty(),
            "discovery always says how it worked it out"
        );
        assert!(
            discovery.models.is_empty() || caps.model_list,
            "models may only be reported by a CLI whose --help advertises listing"
        );
    }
}
