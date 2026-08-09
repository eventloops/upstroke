// tactus — headless orchestration engine for AI coding agents.
// Copyright (C) 2026 Cameron Lambert. Licensed under the GNU AGPL v3 only;
// see LICENSE, or <https://www.gnu.org/licenses/>. Commercial licences are
// available for use the AGPL does not permit — see README.md.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use tactus::engine::{self, RunOutcome};
use tactus::interaction::InteractionMode;
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
            print!("{}", report.render());
            match report.outcome() {
                RunOutcome::Complete => Ok(ExitCode::SUCCESS),
                // §12: parked is neither clean nor broken. Distinguishable so
                // CI can gate on it without parsing prose.
                RunOutcome::Parked => Ok(ExitCode::from(EXIT_PARKED)),
                RunOutcome::Halted => anyhow::bail!(
                    "run halted at task `{}`",
                    report.halted_at.as_deref().unwrap_or("?")
                ),
            }
        }
    }
}
