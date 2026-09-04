//! Extended notes: `docs/internals/main.md`

// LEGACY-EFFECT: this module is in the frozen legacy section of
// `effects/allowlist.toml`, which carries its justification.
#![allow(
    clippy::disallowed_macros,
    clippy::disallowed_methods,
    clippy::disallowed_types
)]
#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the CLI binary is the §13 output surface: stdout carries results, stderr carries diagnostics"
)]

use std::io::{BufRead, IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use upstroke::answer::{self, Reply};
use upstroke::capacity;
use upstroke::connect;
use upstroke::engine::{self, RunOutcome};
use upstroke::error::UpstrokeError;
use upstroke::export::{self, Format as ExportFormat};
use upstroke::interaction::{InteractionMode, RealSleeper};
use upstroke::status;
use upstroke::validate::{self, ValidateOptions};

const EXIT_PARKED: u8 = 2;

const EXIT_BUDGET: u8 = 3;

#[derive(Parser)]
#[command(name = "upstroke", version, about = "Conductor for AI coding agents")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Connect {
        #[arg(long)]
        force: bool,
        #[arg(long)]
        pools: Option<PathBuf>,
    },
    Capacity {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        pools: Option<PathBuf>,
    },
    Validate {
        plan: PathBuf,
        #[arg(long)]
        emit_json: bool,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Run {
        plan: PathBuf,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, value_enum)]
        interaction: Option<Interaction>,
        #[arg(long)]
        budget: Option<f64>,
    },
    Resume {
        run_id: String,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, value_enum)]
        interaction: Option<Interaction>,
        #[arg(long)]
        budget: Option<f64>,
    },
    Status {
        run_id: Option<String>,
        #[arg(long)]
        follow: bool,
    },
    ExportDecisions {
        run_id: String,
        #[arg(long, value_enum, default_value_t = ExportFormat::Jsonl)]
        format: ExportFormat,
    },
    Answer {
        question_id: String,
        #[arg(long, conflicts_with_all = ["text", "decline"])]
        option: Option<usize>,
        #[arg(long, conflicts_with = "decline")]
        text: Option<String>,
        #[arg(long)]
        decline: bool,
    },
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandClass {
    Write,
    ReadOnly,
}

const fn command_class(command: &Command) -> CommandClass {
    match command {
        Command::Run { .. } | Command::Resume { .. } => CommandClass::Write,
        Command::Connect { .. }
        | Command::Capacity { .. }
        | Command::Validate { .. }
        | Command::Status { .. }
        | Command::ExportDecisions { .. }
        | Command::Answer { .. } => CommandClass::ReadOnly,
    }
}

mod containment {
    use super::{Command, CommandClass, command_class};
    use upstroke::error::UpstrokeError;

    pub struct Contained(());

    pub fn establish(
        command: &Command,
        join_ambient_job: impl FnOnce() -> Result<(), UpstrokeError>,
    ) -> anyhow::Result<Contained> {
        match command_class(command) {
            CommandClass::Write => join_ambient_job()?,
            CommandClass::ReadOnly => {}
        }
        Ok(Contained(()))
    }
}

use containment::Contained;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn validate_options(plan: PathBuf, config: Option<PathBuf>) -> anyhow::Result<ValidateOptions> {
    Ok(ValidateOptions {
        plan_path: plan,
        config_path: config,
        config_root: std::env::current_dir().context("resolving current directory")?,
        pools_path: None,
        engine_limits: upstroke::config::EngineLimits::Fresh,
    })
}

fn run() -> anyhow::Result<ExitCode> {
    run_wired(Cli::parse().command, &mut upstroke::agent::proc::NoHooks)
}

fn run_wired(
    command: Command,
    hooks: &mut dyn upstroke::agent::proc::SpawnHooks,
) -> anyhow::Result<ExitCode> {
    dispatch(command, || {
        upstroke::runner::host::start_write_command(hooks)
    })
}

fn dispatch(
    command: Command,
    join_ambient_job: impl FnOnce() -> Result<(), UpstrokeError>,
) -> anyhow::Result<ExitCode> {
    let contained = containment::establish(&command, join_ambient_job)?;
    execute(command, contained)
}

fn execute(command: Command, _contained: Contained) -> anyhow::Result<ExitCode> {
    match command {
        Command::Connect { force, pools } => {
            let report = connect::run(&connect::ConnectOptions {
                pools_path: pools,
                force,
            })?;
            print!("{}", connect::render_report(&report));
            if report.refused() {
                return Ok(ExitCode::FAILURE);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Capacity { config, pools } => {
            let report = capacity::report(
                &capacity::CapacityOptions {
                    config_path: config,
                    pools_path: pools,
                    repo_root: std::env::current_dir().context("resolving current directory")?,
                },
                &engine::BuiltinAdapters,
            )?;
            print!("{}", report.render());
            Ok(ExitCode::SUCCESS)
        }
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
            budget,
        } => {
            if dry_run {
                let report = validate::run(&validate_options(plan, config)?)?;
                print!("{}", report.render());
                if let Some(budget) = budget {
                    println!(
                        "budget: ${budget:.2} would cap this run; nothing is spent in a dry run"
                    );
                }
                println!("dry run: no agents executed, nothing spent");
                return Ok(ExitCode::SUCCESS);
            }
            let repo_root = std::env::current_dir().context("resolving current directory")?;
            let mut opts = engine::RunOptions::new(plan, repo_root);
            opts.config_path = config;
            opts.interaction = interaction.map(Into::into);
            opts.budget_usd = budget;
            let report = engine::run(&opts)?;
            finish(&report)
        }
        Command::Resume {
            run_id,
            config,
            interaction,
            budget,
        } => {
            let repo_root = std::env::current_dir().context("resolving current directory")?;
            let mut opts = engine::ResumeOptions::new(run_id, repo_root);
            opts.config_path = config;
            opts.interaction = interaction.map(Into::into);
            opts.budget_usd = budget;
            let report = engine::resume(&opts)?;
            finish(&report)
        }
        Command::Status { run_id, follow } => {
            let repo_root = std::env::current_dir().context("resolving current directory")?;
            let run = status::load(&repo_root, run_id.as_deref())?;
            if follow {
                status::follow(
                    &run,
                    &RealSleeper,
                    Duration::from_millis(500),
                    IDLE_POLLS_BEFORE_GIVING_UP,
                    &mut std::io::stdout(),
                )?;
                let settled = status::load(&repo_root, Some(&run.run_id))?;
                print!("{}", status::render(&settled));
            } else {
                print!("{}", status::render(&run));
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::ExportDecisions { run_id, format } => {
            let repo_root = std::env::current_dir().context("resolving current directory")?;
            let loaded = export::load(&repo_root, &run_id)?;
            for warning in loaded.warnings {
                eprintln!("warning: {warning}");
            }
            export::write(&loaded.rows, format, &mut std::io::stdout())?;
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
                    "continue the run with:\n    upstroke resume {}",
                    recorded.run_id
                );
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

const IDLE_POLLS_BEFORE_GIVING_UP: u32 = 240;

fn finish(report: &engine::RunReport) -> anyhow::Result<ExitCode> {
    print!("{}", report.render());
    match report.outcome() {
        RunOutcome::Complete => Ok(ExitCode::SUCCESS),
        RunOutcome::Parked => Ok(ExitCode::from(EXIT_PARKED)),
        RunOutcome::BudgetExceeded => Ok(ExitCode::from(EXIT_BUDGET)),
        RunOutcome::Halted => anyhow::bail!(
            "run halted at task `{}`",
            report.halted_at.as_deref().unwrap_or("?")
        ),
    }
}

fn prompt_for_answer(repo_root: &std::path::Path, question_id: &str) -> anyhow::Result<String> {
    eprint!("{}", answer::show(repo_root, question_id)?);
    let stdin = std::io::stdin();
    let mut line = String::new();
    if stdin.is_terminal() {
        eprint!("answer (a number picks an option, empty aborts): ");
        stdin
            .lock()
            .read_line(&mut line)
            .context("reading an answer from stdin")?;
    } else {
        stdin
            .lock()
            .take(64 * 1024)
            .read_to_string(&mut line)
            .context("reading an answer from stdin")?;
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use clap::CommandFactory;

    use super::*;

    const DISPATCH: &[(&str, &[&str], CommandClass)] = &[
        ("connect", &["upstroke", "connect"], CommandClass::ReadOnly),
        (
            "capacity",
            &["upstroke", "capacity"],
            CommandClass::ReadOnly,
        ),
        (
            "validate",
            &["upstroke", "validate", "plan.md"],
            CommandClass::ReadOnly,
        ),
        ("run", &["upstroke", "run", "plan.md"], CommandClass::Write),
        (
            "resume",
            &["upstroke", "resume", "01ABCDEF"],
            CommandClass::Write,
        ),
        ("status", &["upstroke", "status"], CommandClass::ReadOnly),
        (
            "export-decisions",
            &["upstroke", "export-decisions", "01ABCDEF"],
            CommandClass::ReadOnly,
        ),
        (
            "answer",
            &["upstroke", "answer", "q1", "--decline"],
            CommandClass::ReadOnly,
        ),
    ];

    const ABSENT_PLAN: &str = "/upstroke-pr4-no-such-plan-33f1a9/plan.md";

    #[test]
    fn every_dispatch_arm_is_classified_by_the_packets_rule() {
        let declared: BTreeSet<String> = Cli::command()
            .get_subcommands()
            .map(|sub| sub.get_name().to_owned())
            .collect();
        let tabled: BTreeSet<String> = DISPATCH
            .iter()
            .map(|(name, _, _)| (*name).to_owned())
            .collect();
        assert_eq!(
            declared, tabled,
            "the dispatch and this table name different commands"
        );
        assert_eq!(declared.len(), 8, "eight subcommands");
        assert_eq!(DISPATCH.len(), 8, "eight rows, one per subcommand");

        for (name, argv, expected) in DISPATCH {
            let cli = Cli::try_parse_from(*argv)
                .unwrap_or_else(|error| panic!("`{name}` does not parse from {argv:?}: {error}"));
            assert_eq!(
                command_class(&cli.command),
                *expected,
                "`{name}` is classified against the packet's rule"
            );
        }

        let writes: Vec<&str> = DISPATCH
            .iter()
            .filter(|(_, _, class)| *class == CommandClass::Write)
            .map(|(name, _, _)| *name)
            .collect();
        assert_eq!(
            writes,
            vec!["run", "resume"],
            "the census names the write commands: `every topology write command (run, resume)`"
        );
        assert_eq!(
            DISPATCH
                .iter()
                .filter(|(_, _, class)| *class == CommandClass::ReadOnly)
                .count(),
            6
        );
    }

    #[test]
    fn the_dry_run_preview_is_classified_with_its_arm() {
        let dry = Cli::try_parse_from(["upstroke", "run", "plan.md", "--dry-run"]).expect("parse");
        assert_eq!(command_class(&dry.command), CommandClass::Write);
        let wet = Cli::try_parse_from(["upstroke", "run", "plan.md"]).expect("parse");
        assert_eq!(command_class(&wet.command), CommandClass::Write);
    }

    #[test]
    fn the_commands_that_spawn_outside_a_run_are_named_and_counted() {
        let outside: Vec<&str> = DISPATCH
            .iter()
            .filter(|(name, _, _)| matches!(*name, "connect" | "capacity"))
            .map(|(name, _, _)| *name)
            .collect();
        assert_eq!(outside, vec!["connect", "capacity"]);
        assert_eq!(outside.len(), 2, "two commands, and this is the count");
        for name in &outside {
            let row = DISPATCH
                .iter()
                .find(|(n, _, _)| n == name)
                .expect("a named command is in the table");
            assert_eq!(row.2, CommandClass::ReadOnly);
        }
    }

    #[test]
    fn a_write_command_refuses_before_any_effect_when_containment_fails() {
        let argv = ["upstroke", "run", ABSENT_PLAN, "--dry-run"];

        let refused = dispatch(Cli::try_parse_from(argv).expect("parse").command, || {
            Err(UpstrokeError::Refused {
                message: "the ambient Job Object could not be established (simulated failure)"
                    .to_owned(),
            })
        })
        .expect_err("a write command whose ambient job cannot be established must refuse");
        let refused = format!("{refused:#}");
        assert!(
            refused.contains("ambient Job Object"),
            "the refusal must diagnose the ambient job: {refused}"
        );
        assert!(
            !refused.contains(ABSENT_PLAN),
            "the command reached its arm before containment: {refused}"
        );

        let reached = dispatch(Cli::try_parse_from(argv).expect("parse").command, || Ok(()))
            .expect_err("the arm then fails on its own, on the plan");
        let reached = format!("{reached:#}");
        assert!(
            reached.contains(ABSENT_PLAN),
            "with containment established the arm must run: {reached}"
        );
        assert!(
            !reached.contains("ambient Job Object"),
            "a successful join must not be reported as a refusal: {reached}"
        );
    }

    #[test]
    fn every_write_command_establishes_containment_and_no_read_only_one_does() {
        let mut argvs: Vec<(Vec<&str>, CommandClass)> = DISPATCH
            .iter()
            .map(|(_, argv, class)| (argv.to_vec(), *class))
            .collect();
        argvs.push((
            vec!["upstroke", "run", "plan.md", "--dry-run"],
            CommandClass::Write,
        ));
        assert_eq!(argvs.len(), 9, "eight subcommands and the dry-run preview");

        let mut joined = 0_usize;
        let mut skipped = 0_usize;
        for (argv, class) in &argvs {
            let command = Cli::try_parse_from(argv).expect("parse").command;
            let mut calls = 0_usize;
            let contained = containment::establish(&command, || {
                calls += 1;
                Ok(())
            });
            assert!(
                contained.is_ok(),
                "a successful join must not refuse {argv:?}"
            );
            match class {
                CommandClass::Write => {
                    assert_eq!(calls, 1, "{argv:?} did not join the ambient job");
                    joined += 1;
                }
                CommandClass::ReadOnly => {
                    assert_eq!(calls, 0, "{argv:?} joined the ambient job");
                    skipped += 1;
                }
            }

            let command = Cli::try_parse_from(argv).expect("parse").command;
            let outcome = containment::establish(&command, || {
                Err(UpstrokeError::Refused {
                    message: "the ambient Job Object could not be established (simulated)"
                        .to_owned(),
                })
            });
            assert_eq!(
                outcome.is_err(),
                *class == CommandClass::Write,
                "{argv:?}: a failed join must stop exactly the write commands"
            );
        }
        assert_eq!(joined, 3, "`run`, `resume`, and the dry-run preview");
        assert_eq!(skipped, 6, "the six read-only subcommands");
    }

    #[test]
    fn the_cli_write_path_runs_the_real_containment_step() {
        use upstroke::runner::host::containment_establishments;

        let before = containment_establishments();
        let write = Cli::try_parse_from(["upstroke", "run", ABSENT_PLAN, "--dry-run"])
            .expect("parse")
            .command;
        let reached = run_wired(write, &mut upstroke::agent::proc::NoHooks)
            .expect_err("the arm then fails on its own, on the plan");
        assert_eq!(
            containment_establishments(),
            before + 1,
            "the CLI's write path did not establish containment through the real step"
        );
        let reached = format!("{reached:#}");
        assert!(
            reached.contains(ABSENT_PLAN),
            "with containment established the arm must run: {reached}"
        );

        let mark = containment_establishments();
        let read_only = Cli::try_parse_from(["upstroke", "validate", ABSENT_PLAN])
            .expect("parse")
            .command;
        let _ = run_wired(read_only, &mut upstroke::agent::proc::NoHooks);
        assert_eq!(
            containment_establishments(),
            mark,
            "a read-only command established the coordinator's containment"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_cli_write_command_refuses_when_the_real_containment_step_refuses() {
        use upstroke::agent::proc::SpawnHooks;
        use upstroke::topology::effects::{Injection, SubEffectPoint};

        struct RefuseAmbientJoin;
        impl SpawnHooks for RefuseAmbientJoin {
            fn point(&mut self, point: SubEffectPoint) -> Injection {
                if point == SubEffectPoint::AmbientJobJoined {
                    Injection::Error
                } else {
                    Injection::Proceed
                }
            }
        }

        let write = Cli::try_parse_from(["upstroke", "run", ABSENT_PLAN, "--dry-run"])
            .expect("parse")
            .command;
        let refused = run_wired(write, &mut RefuseAmbientJoin)
            .expect_err("a CLI write command whose ambient join refuses must refuse");
        let refused = format!("{refused:#}");
        for fragment in ["ambient", "INV-18", "No process was spawned"] {
            assert!(
                refused.contains(fragment),
                "the CLI's refusal must say `{fragment}`: {refused}"
            );
        }
        assert!(
            !refused.contains(ABSENT_PLAN),
            "the CLI reached its arm although containment refused: {refused}"
        );
    }

    #[test]
    fn the_cli_wires_the_real_containment_step_into_dispatch() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"),
        )
        .expect("read this file");
        let source = source.replace("\r\n", "\n");
        let production = source
            .split("\n#[cfg(test)]\n")
            .next()
            .expect("the production region");
        assert!(
            production.len() < source.len(),
            "the split found no test module, so this census is reading the whole file"
        );

        let code: String = production
            .lines()
            .map(|line| {
                match line
                    .match_indices("//")
                    .find(|(at, _)| *at == 0 || !line[..*at].ends_with(':'))
                {
                    Some((at, _)) => &line[..at],
                    None => line,
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("let _ = upstroke::runner::host::start_write_command"),
            "the comment strip left a doc comment's text in the code region"
        );

        assert_eq!(
            code.matches("start_write_command(").count(),
            1,
            "the CLI names the containment step more than once, so one of the calls is \
             somewhere no test drives"
        );

        for name in ["fn run() -> anyhow::Result<ExitCode> {", "fn run_wired("] {
            let start = code
                .find(name)
                .unwrap_or_else(|| panic!("`{name}` is gone from src/main.rs"));
            let body = &code[start..];
            let end = body.find("\n}\n").expect("the function ends");
            let body = &body[..end];
            assert!(
                !body.contains("Ok("),
                "`{name}` constructs a Result of its own; it is a delegation, and the only \
                 reason to write one is to answer success without asking: {body}"
            );
            if name.starts_with("fn run()") {
                assert!(
                    body.contains("run_wired("),
                    "`run` no longer goes through the wiring the tests drive: {body}"
                );
            }
        }
    }

    #[test]
    fn a_read_only_command_does_not_join_the_ambient_job() {
        for argv in [
            vec!["upstroke", "validate", ABSENT_PLAN],
            vec!["upstroke", "capacity", "--config", ABSENT_PLAN],
        ] {
            let command = Cli::try_parse_from(&argv).expect("parse").command;
            assert_eq!(command_class(&command), CommandClass::ReadOnly);
            let outcome = dispatch(command, || {
                panic!("a read-only command joined the ambient job: {argv:?}")
            });
            assert!(
                outcome.is_err(),
                "the fixture relies on this arm failing on its own input"
            );
        }
    }

    #[cfg(windows)]
    const AMBIENT_LATCH_RECORD: &str = "UPSTROKE_PR4_CLI_LATCH_RECORD";

    #[cfg(windows)]
    #[test]
    #[ignore = "subprocess helper"]
    fn cli_ambient_latch_helper() {
        fn note(stage: &str, record: &std::path::Path, observed: &mut Vec<String>) {
            observed.push(format!(
                "{stage} {}",
                i32::from(upstroke::agent::proc::ambient_job_established())
            ));
            std::fs::write(record, observed.join("\n")).expect("record the observation");
        }

        let Some(record) = std::env::var_os(AMBIENT_LATCH_RECORD) else {
            return;
        };
        let record = PathBuf::from(record);
        let mut observed = Vec::new();
        note("start", &record, &mut observed);

        let read_only = Cli::try_parse_from(["upstroke", "validate", ABSENT_PLAN])
            .expect("parse")
            .command;
        let _ = run_wired(read_only, &mut upstroke::agent::proc::NoHooks);
        note("read-only", &record, &mut observed);

        let write = Cli::try_parse_from(["upstroke", "run", ABSENT_PLAN, "--dry-run"])
            .expect("parse")
            .command;
        let _ = run_wired(write, &mut upstroke::agent::proc::NoHooks);
        note("write", &record, &mut observed);
    }

    #[cfg(windows)]
    #[test]
    fn a_write_command_establishes_the_ambient_job_and_a_read_only_command_does_not() {
        let record = std::env::temp_dir().join(format!(
            "upstroke-pr4-cli-ambient-latch-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&record);
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["cli_ambient_latch_helper", "--ignored", "--nocapture"])
            .env(AMBIENT_LATCH_RECORD, &record)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("spawn the ambient-latch helper");
        let written = std::fs::read_to_string(&record).ok();
        let _ = std::fs::remove_file(&record);
        assert!(
            status.success(),
            "the ambient-latch helper died; it had recorded {written:?}"
        );
        let written = written.unwrap_or_else(|| {
            panic!(
                "the helper wrote nothing to {}: it exited 0 without running, which is what a \
                 libtest filter that matches no test does",
                record.display()
            )
        });
        let observed: Vec<&str> = written.lines().collect();
        assert_eq!(
            observed,
            vec!["start 0", "read-only 0", "write 1"],
            "the child's latch at three points: `start 0` or this test's premise is gone and the \
             rest of it says nothing; `read-only 0` or a read-only command established the \
             coordinator's ambient job; `write 1` or a write command ran without joining it \
             (INV-18)"
        );
    }
}
