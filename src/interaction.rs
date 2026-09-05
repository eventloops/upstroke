//! Extended notes: `docs/internals/interaction.md`

// Allowlist placement: the funnel section of `effects/allowlist.toml`, which
// carries this module's review clause. `effect_site_inventory.mechanism` (2).
#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]
#![expect(
    clippy::print_stderr,
    reason = "terminal question delivery and the answer prompt are this module's §13 output surface"
)]

use std::fmt;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::UpstrokeError;
use crate::ir::{Answer, Question, QuestionId, QuestionKind};
use crate::ulid;
use crate::util;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionMode {
    Never,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionRecord {
    pub question: Question,
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

pub fn write_question(dir: &Path, record: &QuestionRecord) -> Result<(), UpstrokeError> {
    crate::rundir::write_question_payload(
        dir,
        &util::filename_component(record.question.id.as_str()),
        record,
        &mut crate::rundir::NoHooks,
    )
}

pub fn answer_path(dir: &Path, id: &QuestionId) -> PathBuf {
    dir.join(format!("{}.json", util::filename_component(id.as_str())))
}

// The answer writer stages complete JSON before publishing it by rename; engine
// readers may poll concurrently and must never see a partial payload. Failed
// staging or publication can leave writer-owned .partial residue, which readers ignore.
pub fn write_answer(dir: &Path, id: &QuestionId, answer: &Answer) -> Result<(), UpstrokeError> {
    let component = util::filename_component(id.as_str());
    let hooks = &mut crate::rundir::NoHooks;
    crate::rundir::stage_answer(dir, &component, answer, hooks)?;
    crate::rundir::publish_answer(dir, &component, hooks)
}

pub fn read_answer(dir: &Path, id: &QuestionId) -> Result<Option<Answer>, UpstrokeError> {
    let component = util::filename_component(id.as_str());
    let path = answer_path(dir, id);
    match crate::rundir::ingest_answer(dir, &component, &mut crate::rundir::NoHooks)? {
        Some(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| UpstrokeError::Parse {
                message: format!("{}: {e}", path.display()),
            }),
        None => Ok(None),
    }
}

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

pub trait Notifier {
    fn id(&self) -> &'static str;
    fn ask(&self, question: &Question) -> Result<(), UpstrokeError>;
}

pub struct CliNotifier;

impl Notifier for CliNotifier {
    fn id(&self) -> &'static str {
        "cli"
    }

    fn ask(&self, question: &Question) -> Result<(), UpstrokeError> {
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

pub trait AnswerSource {
    fn id(&self) -> &'static str;
    fn resolve(&self, question: &Question) -> Result<Answer, UpstrokeError>;
}

pub struct UnattendedAnswers;

impl AnswerSource for UnattendedAnswers {
    fn id(&self) -> &'static str {
        "unattended"
    }

    fn resolve(&self, _question: &Question) -> Result<Answer, UpstrokeError> {
        Ok(Answer::Unanswered)
    }
}

pub struct TerminalAnswers;

impl AnswerSource for TerminalAnswers {
    fn id(&self) -> &'static str {
        "terminal"
    }

    fn resolve(&self, question: &Question) -> Result<Answer, UpstrokeError> {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            return Ok(Answer::Unanswered);
        }
        eprint!("\n{}{PROMPT_LEGEND}", render_question(question));
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        match stdin.read_line(&mut line) {
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

pub struct EventLogAnswers<'a> {
    dir: PathBuf,
    poll: Duration,
    remaining: Mutex<Duration>,
    sleeper: &'a dyn Sleeper,
}

impl<'a> EventLogAnswers<'a> {
    pub const DEFAULT_POLL: Duration = Duration::from_secs(5);

    pub fn new(dir: PathBuf, budget: Duration, sleeper: &'a dyn Sleeper) -> Self {
        Self::with_poll(dir, budget, Self::DEFAULT_POLL, sleeper)
    }

    pub fn with_poll(
        dir: PathBuf,
        budget: Duration,
        poll: Duration,
        sleeper: &'a dyn Sleeper,
    ) -> Self {
        Self {
            dir,
            poll,
            remaining: Mutex::new(budget),
            sleeper,
        }
    }
}

impl AnswerSource for EventLogAnswers<'_> {
    fn id(&self) -> &'static str {
        "event-log"
    }

    fn resolve(&self, question: &Question) -> Result<Answer, UpstrokeError> {
        if let Some(answer) = read_answer(&self.dir, &question.id)? {
            return Ok(answer);
        }
        let Ok(mut remaining) = self.remaining.lock() else {
            return Ok(Answer::Unanswered);
        };
        if remaining.is_zero() {
            return Ok(Answer::Unanswered);
        }
        eprintln!(
            "\n{}\nNobody is attached to this run, so it is waiting for an answer. From another \
             terminal:\n\n    upstroke answer {}\n",
            render_question(question),
            question.id
        );
        while !remaining.is_zero() {
            let wait = self.poll.min(*remaining);
            self.sleeper.sleep(wait);
            *remaining = remaining.saturating_sub(wait);
            if let Some(answer) = read_answer(&self.dir, &question.id)? {
                return Ok(answer);
            }
        }
        eprintln!(
            "no answer arrived for {}; the task stays parked and `upstroke resume` will pick up \
             an answer written later",
            question.id
        );
        Ok(Answer::Unanswered)
    }
}

pub fn interpret(question: &Question, raw: &str) -> Answer {
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
    if let Ok(choice) = text.parse::<usize>() {
        if let Some(answer) = answer_for_option(question, choice) {
            return answer;
        }
    }
    Answer::Answered {
        text: text.to_owned(),
    }
}

pub(crate) fn answer_for_option(question: &Question, choice: usize) -> Option<Answer> {
    let index = choice.checked_sub(1)?;
    let option = question.options.get(index)?;
    let is_decline = question.kind != QuestionKind::Clarify
        && question.options.len() >= 2
        && index + 1 == question.options.len();
    Some(if is_decline {
        Answer::Declined
    } else {
        Answer::Answered {
            text: option.clone(),
        }
    })
}

pub fn answers_for<'a>(
    mode: InteractionMode,
    answers_dir: PathBuf,
    wait_on_block: Duration,
    sleeper: &'a dyn Sleeper,
) -> Box<dyn AnswerSource + 'a> {
    if !mode.interactive() {
        return Box::new(UnattendedAnswers);
    }
    if std::io::stdin().is_terminal() {
        return Box::new(TerminalAnswers);
    }
    Box::new(EventLogAnswers::new(answers_dir, wait_on_block, sleeper))
}

pub trait Sleeper {
    fn sleep(&self, duration: Duration);
}

pub struct RealSleeper;

impl Sleeper for RealSleeper {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

pub const DEFAULT_DEFER_BACKOFF: Duration = Duration::from_secs(60);
pub const MAX_DEFER_BACKOFF: Duration = Duration::from_secs(600);

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
            Answer::Declined,
            "the numbered give-up option is the same action as typing `skip`"
        );
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
        let dir = std::env::temp_dir().join(format!("upstroke-questions-{}", std::process::id()));
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
    }

    #[test]
    fn the_answer_channel_follows_the_mode_and_the_situation() {
        let dir = std::env::temp_dir().join("upstroke-answers-for");
        let budget = Duration::from_secs(60);
        let idle = CountingSleeper::default();
        assert_eq!(
            answers_for(InteractionMode::Never, dir.clone(), budget, &idle).id(),
            "unattended",
            "CI never waits on a human"
        );
        assert_eq!(
            answers_for(InteractionMode::OnBlock, dir.clone(), budget, &idle).id(),
            "event-log"
        );
        let immediate = answers_for(InteractionMode::OnBlock, dir, Duration::ZERO, &idle);
        assert_eq!(immediate.id(), "event-log");
        assert_eq!(
            immediate.resolve(&question()).expect("resolve"),
            Answer::Unanswered
        );
    }

    #[test]
    fn answers_survive_the_trip_through_a_file() {
        let dir = std::env::temp_dir().join(format!("upstroke-answer-io-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let id = QuestionId::from("q-TEST");

        assert_eq!(
            read_answer(&dir, &id).expect("absent is not an error"),
            None
        );
        for answer in [
            Answer::Answered {
                text: "use base64 cursors".to_owned(),
            },
            Answer::Declined,
        ] {
            write_answer(&dir, &id, &answer).expect("write");
            assert_eq!(read_answer(&dir, &id).expect("read"), Some(answer));
        }
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .expect("list")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".partial"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn a_detached_run_waits_for_an_answer_file_then_gives_up() {
        let dir = std::env::temp_dir().join(format!("upstroke-answer-wait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");

        let counting = CountingSleeper::default();
        let answers = EventLogAnswers::with_poll(
            dir.clone(),
            Duration::from_secs(10),
            Duration::from_secs(2),
            &counting,
        );
        assert_eq!(
            answers.resolve(&question()).expect("resolve"),
            Answer::Unanswered
        );
        assert_eq!(counting.count(), 5, "10s budget in 2s polls");

        assert_eq!(
            answers.resolve(&question()).expect("resolve"),
            Answer::Unanswered
        );
        assert_eq!(counting.count(), 5, "the budget was already spent");

        let arriving = ArrivingSleeper {
            dir: dir.clone(),
            id: question().id,
            after: Mutex::new(2),
        };
        let answers = EventLogAnswers::with_poll(
            dir,
            Duration::from_secs(60),
            Duration::from_secs(1),
            &arriving,
        );
        assert_eq!(
            answers.resolve(&question()).expect("resolve"),
            Answer::Answered {
                text: "opaque cursors".to_owned()
            }
        );
    }

    #[derive(Default, Clone)]
    struct CountingSleeper(std::sync::Arc<Mutex<usize>>);

    impl CountingSleeper {
        fn count(&self) -> usize {
            self.0.lock().map(|c| *c).unwrap_or(0)
        }
    }

    impl Sleeper for CountingSleeper {
        fn sleep(&self, _duration: Duration) {
            if let Ok(mut count) = self.0.lock() {
                *count += 1;
            }
        }
    }

    struct ArrivingSleeper {
        dir: std::path::PathBuf,
        id: QuestionId,
        after: Mutex<usize>,
    }

    impl Sleeper for ArrivingSleeper {
        fn sleep(&self, _duration: Duration) {
            let Ok(mut remaining) = self.after.lock() else {
                return;
            };
            if *remaining == 0 {
                return;
            }
            *remaining -= 1;
            if *remaining == 0 {
                let _ = write_answer(
                    &self.dir,
                    &self.id,
                    &Answer::Answered {
                        text: "opaque cursors".to_owned(),
                    },
                );
            }
        }
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
        let answer = UnattendedAnswers.resolve(&question()).expect("resolve");
        assert_eq!(answer, Answer::Unanswered);
    }
}
