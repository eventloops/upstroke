// tactus — headless orchestration engine for AI coding agents.
// Copyright (C) 2026 Cameron Lambert. Licensed under the GNU AGPL v3 only;
// see LICENSE, or <https://www.gnu.org/licenses/>. Commercial licences are
// available for use the AGPL does not permit — see README.md.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tactus::engine;
use tactus::validate::{self, ValidateOptions};

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
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
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

fn run() -> anyhow::Result<()> {
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
            Ok(())
        }
        Command::Run {
            plan,
            dry_run,
            config,
        } => {
            if dry_run {
                let report = validate::run(&validate_options(plan, config)?)?;
                print!("{}", report.render());
                println!("dry run: no agents executed, nothing spent");
                return Ok(());
            }
            let repo_root = std::env::current_dir().context("resolving current directory")?;
            let report = engine::run(&engine::RunOptions {
                plan_path: plan,
                config_path: config,
                pools_path: None,
                repo_root,
                attempt_timeout: engine::DEFAULT_ATTEMPT_TIMEOUT,
            })?;
            print!("{}", report.render());
            if let Some(task) = &report.halted_at {
                anyhow::bail!("run halted at task `{task}`");
            }
            Ok(())
        }
    }
}
