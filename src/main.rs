// tactus — headless orchestration engine for AI coding agents.
// Copyright (C) 2026 Cameron Lambert. Licensed under the GNU AGPL v3 only;
// see LICENSE, or <https://www.gnu.org/licenses/>. Commercial licences are
// available for use the AGPL does not permit — see README.md.

use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use tactus::answer::{self, Reply};
use tactus::engine::{self, RunOutcome};
use tactus::interaction::{InteractionMode, RealSleeper};
use tactus::status;
use tactus::validate::{self, ValidateOptions};

/// §12: a run that ends with tasks parked on unanswered questions completed
/// neither cleanly nor in error. CI has to be able to tell the difference, so
/// it gets its own status.
const EXIT_PARKED: u8 = 2;

#[derive(Parser)]
#[command(name = "tactus", version, about = "Conductor for AI coding agents")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a plan, resolve routing, and print the task table (no execution)
    Validate {
        /// Path to the plan file (annotated or bare markdown)
        plan: PathBuf,
        /// Write plan.normalized.json (the IR) to the current directory
        #[arg(long)]
        emit_json: bool,
        /// Repo config path (default: ./tactus.toml, optional)
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Execute a plan sequentially: run branch, agent per task, commit per task
    Run {
        /// Path to the plan file (annotated or bare markdown)
        plan: PathBuf,
        /// Everything except agents: parse, route, and print the preview at
        /// zero spend
        #[arg(long)]
        dry_run: bool,
        /// Repo config path (default: ./tactus.toml, optional)
        #[arg(long)]
        config: Option<PathBuf>,
        /// Override [interaction] mode; `never` is the CI setting — questions
        /// park their tasks and the run reports them instead of waiting
        #[arg(long, value_enum)]
        interaction: Option<Interaction>,
    },
    /// Continue a run that was interrupted or ended with tasks parked
    Resume {
        /// Run id, or any unambiguous prefix of one
        run_id: String,
        /// Repo config path (default: the one the run recorded)
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, value_enum)]
        interaction: Option<Interaction>,
    },
    /// Show a run: what happened, what it cost, and what it is waiting for
    Status {
        /// Run id or prefix; omit for the most recent run
        run_id: Option<String>,
        /// Stream events as they are appended, ending when the run finishes
        #[arg(long)]
        follow: bool,
    },
    /// Answer a question a run is parked on (§12)
    Answer {
        /// Question id, or any unambiguous prefix of one
        question_id: String,
        /// Pick one of the question's numbered options
        #[arg(long, conflicts_with_all = ["text", "decline"])]
        option: Option<usize>,
        /// Answer in your own words
        #[arg(long, conflicts_with = "decline")]
        text: Option<String>,
        /// Give up on the task; its dependents will be blocked
        #[arg(long)]
        decline: bool,
    },
}

/// CLI spelling of [`InteractionMode`], so CI does not have to edit
/// `tactus.toml` to stop a run waiting on a human.
#[derive(Clone, Copy, ValueEnum)]
enum Interaction {
    Never,
    OnBlock,
    OnMilestone,
}

impl From<Interaction> for InteractionMode {
    fn from(value: Interaction) -> Self {
        match value {
            Interaction::Never => Self::Never,
            Interaction::OnBlock => Self::OnBlock,
            Interaction::OnMilestone => Self::OnMilestone,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// One construction point so `validate` and `run --dry-run` can never drift
/// into previewing different things.
fn validate_options(plan: PathBuf, config: Option<PathBuf>) -> anyhow::Result<ValidateOptions> {
    Ok(ValidateOptions {
        plan_path: plan,
        config_path: config,
        config_root: std::env::current_dir().context("resolving current directory")?,
        pools_path: None,
    })
}

fn run() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Validate {
            plan,
            emit_json,
            config,
        } => {
            let report = validate::run(&validate_options(plan, config)?)?;
            if emit_json {
                let path = PathBuf::from("plan.normalized.json");
                report
                    .write_normalized_json(&path)
                    .with_context(|| format!("writing {}", path.display()))?;
                println!("wrote {}", path.display());
            }
            print!("{}", report.render());
            Ok(ExitCode::SUCCESS)
        }
        Command::Run {
            plan,
            dry_run,
            config,
            interaction,
        } => {
            if dry_run {
                let report = validate::run(&validate_options(plan, config)?)?;
                print!("{}", report.render());
                println!("dry run: no agents executed, nothing spent");
                return Ok(ExitCode::SUCCESS);
            }
            let repo_root = std::env::current_dir().context("resolving current directory")?;
            let mut opts = engine::RunOptions::new(plan, repo_root);
            opts.config_path = config;
            opts.interaction = interaction.map(Into::into);
            let report = engine::run(&opts)?;
            finish(&report)
        }
        Command::Resume {
            run_id,
            config,
            interaction,
        } => {
            let repo_root = std::env::current_dir().context("resolving current directory")?;
            let mut opts = engine::ResumeOptions::new(run_id, repo_root);
            opts.config_path = config;
            opts.interaction = interaction.map(Into::into);
            let report = engine::resume(&opts)?;
            finish(&report)
        }
        Command::Status { run_id, follow } => {
            let repo_root = std::env::current_dir().context("resolving current directory")?;
            let run = status::load(&repo_root, run_id.as_deref())?;
            if follow {
                // History first, then live events: dropping a reader into the
                // middle of a run tells them less than showing how it got here.
                status::follow(
                    &run,
                    &RealSleeper,
                    Duration::from_millis(500),
                    IDLE_POLLS_BEFORE_GIVING_UP,
                    &mut std::io::stdout(),
                )?;
                // Re-read: the run has moved since the summary would have been
                // computed, and the closing summary is the useful one.
                let settled = status::load(&repo_root, Some(&run.run_id))?;
                print!("{}", status::render(&settled));
            } else {
                print!("{}", status::render(&run));
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Answer {
            question_id,
            option,
            text,
            decline,
        } => {
            let repo_root = std::env::current_dir().context("resolving current directory")?;
            let reply = match (option, text, decline) {
                (_, _, true) => Reply::Decline,
                (Some(choice), _, _) => Reply::Option(choice),
                (_, Some(text), _) => Reply::Text(text),
                // Nothing given: show the question and read one line, so the
                // common case is `tactus answer <id>` and then just type.
                (None, None, false) => Reply::Text(prompt_for_answer(&repo_root, &question_id)?),
            };
            let recorded = answer::answer(&repo_root, &question_id, reply)?;
            println!(
                "recorded an answer to {} on run {}",
                recorded.question_id, recorded.run_id
            );
            if recorded.run_is_live {
                println!("that run is live; it will pick this up and un-park the task");
            } else {
                println!(
                    "continue the run with:\n    tactus resume {}",
                    recorded.run_id
                );
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Roughly two minutes of silence before a follower concludes nothing more is
/// coming — long enough to ride out a slow agent turn, short enough that a
/// follower attached to a dead engine does not hang a terminal.
const IDLE_POLLS_BEFORE_GIVING_UP: u32 = 240;

fn finish(report: &engine::RunReport) -> anyhow::Result<ExitCode> {
    print!("{}", report.render());
    match report.outcome() {
        RunOutcome::Complete => Ok(ExitCode::SUCCESS),
        // §12: parked is neither clean nor broken. Distinguishable so CI can
        // gate on it without parsing prose.
        RunOutcome::Parked => Ok(ExitCode::from(EXIT_PARKED)),
        RunOutcome::Halted => anyhow::bail!(
            "run halted at task `{}`",
            report.halted_at.as_deref().unwrap_or("?")
        ),
    }
}

/// Show the question, then take one line.
fn prompt_for_answer(repo_root: &std::path::Path, question_id: &str) -> anyhow::Result<String> {
    eprint!("{}", answer::show(repo_root, question_id)?);
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprint!("answer (a number picks an option, empty aborts): ");
    }
    let mut line = String::new();
    // Read to end rather than one line so a piped answer can span lines; the
    // interpreter trims and treats the whole thing as the operator's words.
    stdin
        .lock()
        .take(64 * 1024)
        .read_to_string(&mut line)
        .context("reading an answer from stdin")?;
    Ok(line)
}
