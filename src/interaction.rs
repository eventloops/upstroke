//! Interaction model (DESIGN.md §12): questions are events, delivery is
//! pluggable, and answers arrive through a seam rather than a hard-coded
//! prompt.
//!
//! Two traits keep §8's split honest. A [`Notifier`] only *delivers* — it can
//! fail, be missing, or be a phone, and the run survives either way. An
//! [`AnswerSource`] is where an answer comes *back* from; v0.1 ships the
//! attached terminal, and step 8 replaces it with an event-log reader backing
//! `tactus answer <id>` without anything else moving.
//!
//! Every question is also written to `questions/<id>.json` (§15) the moment it
//! is raised. That file — not the terminal output — is the contract a
//! notifier, a dashboard, or a future UI reads: the engine stays headless and
//! panes are thin clients over its record.

use std::fmt;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::TactusError;
use crate::ir::{Answer, Question, QuestionId};
use crate::ulid;
use crate::util;

/// `[interaction] mode` (§17).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionMode {
    /// CI: nothing may block on a human. Questions degrade to parked-task
    /// reporting and the exit status says so (§12).
    Never,
    /// Ask only once the runnable frontier is empty — the precise definition
    /// of a hard block (§12).
    #[default]
    OnBlock,
    OnMilestone,
}

impl InteractionMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "never" => Some(Self::Never),
            "on_block" => Some(Self::OnBlock),
            "on_milestone" => Some(Self::OnMilestone),
            _ => None,
        }
    }

    /// Whether a human may be asked at all.
    pub fn interactive(self) -> bool {
        self != Self::Never
    }
}

impl fmt::Display for InteractionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Never => "never",
            Self::OnBlock => "on_block",
            Self::OnMilestone => "on_milestone",
        })
    }
}

/// A question plus whatever came back. Serialized to `questions/<id>.json` at
/// raise time and rewritten when answered, so a reader that arrives late still
/// sees the whole exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionRecord {
    pub question: Question,
    /// `None` while open.
    pub answer: Option<Answer>,
}

impl QuestionRecord {
    pub fn open(question: Question) -> Self {
        Self {
            question,
            answer: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.answer.is_none()
    }
}

pub fn new_question_id() -> QuestionId {
    QuestionId(format!("q-{}", ulid::ulid()))
}

/// §15: `questions/<question-id>.json`, the payload notifiers and UIs read.
pub fn write_question(dir: &Path, record: &QuestionRecord) -> Result<(), TactusError> {
    let path = dir.join(format!(
        "{}.json",
        util::filename_component(record.question.id.as_str())
    ));
    util::write_json(&path, record)
}

/// The human-facing form of a question. `context` is passed through verbatim:
/// whoever built the question owns quoting and labelling any agent-authored
/// text inside it, exactly as `review.rs` does with a diff.
pub fn render_question(question: &Question) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "question {} [{}] — parks: {}",
        question.id,
        question.kind,
        question
            .affected_tasks
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(out, "{}", question.context.trim());
    for (index, option) in question.options.iter().enumerate() {
        let _ = writeln!(out, "  {}) {option}", index + 1);
    }
    out
}

/// §8 `Notifier` — delivery only. Answers never come back through this trait,
/// which is what lets a run outlive its notifier.
pub trait Notifier {
    fn id(&self) -> &'static str;
    fn ask(&self, question: &Question) -> Result<(), TactusError>;
}

/// Announces a question on stderr as soon as it is raised (§12: eagerly, at
/// detection). One line — the full text belongs in the prompt at the hard
/// block and in the question file, not repeated in the middle of a run.
pub struct CliNotifier;

impl Notifier for CliNotifier {
    fn id(&self) -> &'static str {
        "cli"
    }

    fn ask(&self, question: &Question) -> Result<(), TactusError> {
        let first_line = question
            .context
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("(no context)");
        eprintln!(
            "question {} [{}]: {} — parking {}; the run continues",
            question.id,
            question.kind,
            util::head(first_line, 160),
            question
                .affected_tasks
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        Ok(())
    }
}

static CLI_NOTIFIER: CliNotifier = CliNotifier;

/// Resolve `[interaction] notify = [...]` to delivery channels. An id that
/// resolves to nothing warns rather than silently dropping notifications —
/// believing you configured an alert you never get is worse than having none.
pub fn notifiers_for(ids: &[String], warnings: &mut Vec<String>) -> Vec<&'static dyn Notifier> {
    let mut chosen: Vec<&'static dyn Notifier> = Vec::new();
    for id in ids {
        match id.as_str() {
            "cli" => chosen.push(&CLI_NOTIFIER),
            "desktop" => warnings.push(
                "[interaction] notify `desktop` is not available in this build; questions are \
                 announced on the CLI and written to the run's questions/ directory"
                    .to_owned(),
            ),
            "telegram" | "slack" => warnings.push(format!(
                "[interaction] notify `{id}` arrives in v0.2; using the CLI notifier"
            )),
            other => warnings.push(format!(
                "unknown [interaction] notifier `{other}` (known: cli)"
            )),
        }
    }
    if chosen.is_empty() {
        chosen.push(&CLI_NOTIFIER);
    }
    chosen
}

/// Where an answer comes from. Step 8 adds an event-log implementation behind
/// `tactus answer <id>`; the engine does not change when it does.
pub trait AnswerSource {
    fn id(&self) -> &'static str;
    /// Called only at a hard block (§12), never mid-frontier.
    fn resolve(&self, question: &Question) -> Result<Answer, TactusError>;
}

/// CI and every other detached context: nobody is there. Note this returns
/// `Unanswered`, not `Declined` — the task parks rather than failing, and the
/// run's exit status reports it (§12).
pub struct UnattendedAnswers;

impl AnswerSource for UnattendedAnswers {
    fn id(&self) -> &'static str {
        "unattended"
    }

    fn resolve(&self, _question: &Question) -> Result<Answer, TactusError> {
        Ok(Answer::Unanswered)
    }
}

/// §12's attached-terminal channel. Degrades to `Unanswered` whenever stdin is
/// not a terminal, so a run piped from a file or a service manager parks
/// instead of hanging on a read that will never return.
pub struct TerminalAnswers;

impl AnswerSource for TerminalAnswers {
    fn id(&self) -> &'static str {
        "terminal"
    }

    fn resolve(&self, question: &Question) -> Result<Answer, TactusError> {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            return Ok(Answer::Unanswered);
        }
        eprint!("\n{}{PROMPT_LEGEND}", render_question(question));
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            // EOF: the terminal went away mid-run. Park, do not fail.
            Ok(0) => Ok(Answer::Unanswered),
            Ok(_) => Ok(interpret(question, &line)),
            Err(error) => {
                eprintln!("could not read an answer ({error}); leaving the task parked");
                Ok(Answer::Unanswered)
            }
        }
    }
}

const PROMPT_LEGEND: &str = "answer (a number picks an option, `skip` fails this task, empty \
                             leaves it parked): ";

/// Interpret one typed line.
///
/// Empty deliberately means *parked*, not *declined*: a stray Enter must not
/// fail a task and block its dependents. Failing requires typing it.
fn interpret(question: &Question, raw: &str) -> Answer {
    let text = raw.trim();
    if text.is_empty() {
        return Answer::Unanswered;
    }
    if matches!(
        text.to_ascii_lowercase().as_str(),
        "skip" | "decline" | "fail" | "abandon"
    ) {
        return Answer::Declined;
    }
    if let Ok(choice) = text.parse::<usize>()
        && (1..=question.options.len()).contains(&choice)
        && let Some(option) = question.options.get(choice - 1)
    {
        return Answer::Answered {
            text: option.clone(),
        };
    }
    Answer::Answered {
        text: text.to_owned(),
    }
}

/// Pick the v0.1 answer channel for a mode.
pub fn answers_for(mode: InteractionMode) -> &'static dyn AnswerSource {
    static TERMINAL: TerminalAnswers = TerminalAnswers;
    static UNATTENDED: UnattendedAnswers = UnattendedAnswers;
    if mode.interactive() {
        &TERMINAL
    } else {
        &UNATTENDED
    }
}

/// Waiting, injectable so tests never actually sleep.
pub trait Sleeper {
    fn sleep(&self, duration: Duration);
}

pub struct RealSleeper;

impl Sleeper for RealSleeper {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// First wait after a rate-limited attempt. Windows reset on the order of
/// hours (§13), but without the capacity engine there is no reset time to read
/// — so this backs off rather than pretending to know one.
pub const DEFAULT_DEFER_BACKOFF: Duration = Duration::from_secs(60);
/// Cap on the wait. Past this, waiting longer is worse than asking a human.
pub const MAX_DEFER_BACKOFF: Duration = Duration::from_secs(600);

/// Doubling backoff, capped. `round` counts consecutive waits where deferred
/// tasks were the *only* runnable work.
pub fn defer_backoff(base: Duration, round: u32) -> Duration {
    base.saturating_mul(2u32.saturating_pow(round.min(16)))
        .min(MAX_DEFER_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{QuestionKind, TaskId};

    fn question() -> Question {
        Question {
            id: QuestionId::from("q-TEST"),
            kind: QuestionKind::Unblock,
            affected_tasks: vec![TaskId::from("fix-obo")],
            context: "Every rung failed on the same assertion.".to_owned(),
            options: vec!["retry on frontier".to_owned(), "skip the task".to_owned()],
        }
    }

    #[test]
    fn an_empty_line_parks_but_skip_declines() {
        // A stray Enter must never fail a task and block its dependents.
        assert_eq!(interpret(&question(), "\n"), Answer::Unanswered);
        assert_eq!(interpret(&question(), "   \n"), Answer::Unanswered);
        for typed in ["skip", "SKIP", "decline", "fail", "abandon"] {
            assert_eq!(
                interpret(&question(), typed),
                Answer::Declined,
                "typed: {typed}"
            );
        }
    }

    #[test]
    fn a_number_picks_an_option_and_anything_else_is_free_text() {
        assert_eq!(
            interpret(&question(), "1\n"),
            Answer::Answered {
                text: "retry on frontier".to_owned()
            }
        );
        assert_eq!(
            interpret(&question(), "2"),
            Answer::Answered {
                text: "skip the task".to_owned()
            }
        );
        // Out of range is not silently clamped — it is the user's words.
        assert_eq!(
            interpret(&question(), "7"),
            Answer::Answered {
                text: "7".to_owned()
            }
        );
        assert_eq!(
            interpret(&question(), "use base64 cursors\n"),
            Answer::Answered {
                text: "use base64 cursors".to_owned()
            }
        );
    }

    #[test]
    fn a_question_with_no_options_still_takes_free_text() {
        let mut q = question();
        q.options.clear();
        assert_eq!(
            interpret(&q, "1"),
            Answer::Answered {
                text: "1".to_owned()
            },
            "no options to index into, so `1` is the answer itself"
        );
    }

    #[test]
    fn rendering_names_the_id_kind_and_parked_tasks() {
        let rendered = render_question(&question());
        assert!(rendered.contains("q-TEST"));
        assert!(rendered.contains("unblock"));
        assert!(rendered.contains("fix-obo"), "the human sees what parked");
        assert!(rendered.contains("1) retry on frontier"));
    }

    #[test]
    fn questions_are_written_where_a_ui_can_read_them() {
        let dir = std::env::temp_dir().join(format!("tactus-questions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");

        let mut record = QuestionRecord::open(question());
        write_question(&dir, &record).expect("write open question");
        assert!(record.is_open());
        let path = dir.join("q-TEST.json");
        let text = std::fs::read_to_string(&path).expect("question file");
        let back: QuestionRecord = serde_json::from_str(&text).expect("round-trips");
        assert_eq!(back.question.id.as_str(), "q-TEST");
        assert!(back.answer.is_none());

        record.answer = Some(Answer::Answered {
            text: "retry on frontier".to_owned(),
        });
        write_question(&dir, &record).expect("rewrite answered");
        let text = std::fs::read_to_string(&path).expect("question file");
        let back: QuestionRecord = serde_json::from_str(&text).expect("round-trips");
        assert_eq!(
            back.answer,
            Some(Answer::Answered {
                text: "retry on frontier".to_owned()
            }),
            "the whole exchange survives for a late reader"
        );
    }

    #[test]
    fn question_ids_are_unique_and_filename_safe() {
        let a = new_question_id();
        let b = new_question_id();
        assert_ne!(a, b);
        assert!(a.as_str().starts_with("q-"));
        assert_eq!(util::filename_component(a.as_str()), a.as_str());
    }

    #[test]
    fn modes_parse_and_only_never_is_non_interactive() {
        assert_eq!(
            InteractionMode::parse("never"),
            Some(InteractionMode::Never)
        );
        assert_eq!(
            InteractionMode::parse("on-block"),
            Some(InteractionMode::OnBlock),
            "hyphen and underscore both accepted"
        );
        assert_eq!(
            InteractionMode::parse("ON_MILESTONE"),
            Some(InteractionMode::OnMilestone)
        );
        assert_eq!(InteractionMode::parse("sometimes"), None);
        assert!(!InteractionMode::Never.interactive());
        assert!(InteractionMode::OnBlock.interactive());
        assert_eq!(InteractionMode::default(), InteractionMode::OnBlock);
        assert_eq!(
            answers_for(InteractionMode::Never).id(),
            "unattended",
            "CI never waits on a human"
        );
        assert_eq!(answers_for(InteractionMode::OnBlock).id(), "terminal");
    }

    #[test]
    fn unknown_notifiers_warn_and_the_cli_is_always_reachable() {
        let mut warnings = Vec::new();
        let chosen = notifiers_for(&["cli".to_owned(), "desktop".to_owned()], &mut warnings);
        assert_eq!(chosen.iter().map(|n| n.id()).collect::<Vec<_>>(), ["cli"]);
        assert!(
            warnings.iter().any(|w| w.contains("desktop")),
            "a channel that does nothing must say so: {warnings:?}"
        );

        let mut warnings = Vec::new();
        let chosen = notifiers_for(&["carrier-pigeon".to_owned()], &mut warnings);
        assert_eq!(
            chosen.len(),
            1,
            "a run never loses its last delivery channel"
        );
        assert!(warnings.iter().any(|w| w.contains("carrier-pigeon")));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let base = Duration::from_secs(60);
        assert_eq!(defer_backoff(base, 0), Duration::from_secs(60));
        assert_eq!(defer_backoff(base, 1), Duration::from_secs(120));
        assert_eq!(defer_backoff(base, 3), Duration::from_secs(480));
        assert_eq!(defer_backoff(base, 4), MAX_DEFER_BACKOFF);
        assert_eq!(
            defer_backoff(base, u32::MAX),
            MAX_DEFER_BACKOFF,
            "no overflow, no absurd wait"
        );
        assert_eq!(
            defer_backoff(Duration::ZERO, 9),
            Duration::ZERO,
            "tests can opt out of waiting entirely"
        );
    }

    #[test]
    fn unattended_parks_rather_than_declining() {
        // The distinction is the whole CI story: Declined fails the task,
        // Unanswered parks it and the exit status reports it (§12).
        let answer = UnattendedAnswers.resolve(&question()).expect("resolve");
        assert_eq!(answer, Answer::Unanswered);
    }
}
