# `src/interaction.rs`

Extended notes for [`src/interaction.rs`](../../src/interaction.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Interaction model (DESIGN.md §12): questions are events, delivery is
pluggable, and answers arrive through a seam rather than a hard-coded
prompt.

Two traits keep §8's split honest. A [`Notifier`] only *delivers* — it can
fail, be missing, or be a phone, and the run survives either way. An
[`AnswerSource`] is where an answer comes *back* from; v0.1 ships the
attached terminal, and step 8 replaces it with an event-log reader backing
`upstroke answer <id>` without anything else moving.

Every question is also written to `questions/<id>.json` (§15) the moment it
is raised. That file — not the terminal output — is the contract a
notifier, a dashboard, or a future UI reads: the engine stays headless and
panes are thin clients over its record.

## `#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
carries this module's review clause -- effects only inside site-taking APIs,
no writable handle returned. `decisions.effect_site_inventory.mechanism` (2).

## `pub enum InteractionMode {`

`[interaction] mode` (§17).

## `pub enum InteractionMode` › `Never,`

CI: nothing may block on a human. Questions degrade to parked-task
reporting and the exit status says so (§12).

## `pub enum InteractionMode` › `OnBlock,`

Ask only once the runnable frontier is empty — the precise definition
of a hard block (§12).

## `impl InteractionMode` › `pub fn interactive(self) -> bool {`

Whether a human may be asked at all.

## `pub struct QuestionRecord {`

A question plus whatever came back. Serialized to `questions/<id>.json` at
raise time and rewritten when answered, so a reader that arrives late still
sees the whole exchange.

## `pub struct QuestionRecord` › `pub answer: Option<Answer>,`

`None` while open.

## `pub fn write_question(dir: &Path, record: &QuestionRecord) -> Result<(), UpstrokeError> {`

§15: `questions/<question-id>.json`, the payload notifiers and UIs read.

Through `RunDir.WriteQuestionPayload`. Behaviour-neutral: the same bytes at
the same path, now named by the site the frozen inventory gives them.

## `pub fn answer_path(dir: &Path, id: &QuestionId) -> PathBuf {`

Where `upstroke answer` leaves an answer for a running or future engine.

Answers arrive as *files*, not as lines appended to `events.jsonl`. Keeping
the log single-writer is what makes it safe to reason about at all: two
processes appending concurrently is a portability question (and on Windows
a sharing question) that a directory of one-file-per-answer simply does not
raise. The engine ingests the file and emits the `question_answered` event
itself, so the log still records every answer — the file is transport, the
event is the record.

## `pub fn write_answer(dir: &Path, id: &QuestionId, answer: &Answer) -> Result<(), UpstrokeE…`

Write an answer atomically.

Temp file plus rename, because the engine may be reading this directory at
any moment. That atomicity is what lets [`read_answer`] be strict: it
refuses a file it cannot parse rather than skipping it, which is only a
safe policy because a half-written file can never be observed. A corrupt
one therefore means something outside upstroke wrote here, and silently
ignoring what might be an operator's answer is worse than stopping to say
so.
Through `Answer.StageWrite` then `Answer.PublishRename` — the two sites the
frozen inventory gives the answer command. Same two steps, same bytes.

## `pub fn read_answer(dir: &Path, id: &QuestionId) -> Result<Option<Answer>, UpstrokeError> {`

Read an answer if one has been left. `None` simply means not yet.

`Answer.Ingest` — a read-only observation, which is why it performs no
effect and is still a site: the inventory names it and a site nothing calls
cannot be shown to execute.

## `pub fn render_question(question: &Question) -> String {`

The human-facing form of a question. `context` is passed through verbatim:
whoever built the question owns quoting and labelling any agent-authored
text inside it, exactly as `review.rs` does with a diff.

## `pub trait Notifier {`

§8 `Notifier` — delivery only. Answers never come back through this trait,
which is what lets a run outlive its notifier.

## `pub struct CliNotifier;`

Announces a question on stderr as soon as it is raised (§12: eagerly, at
detection). One line — the full text belongs in the prompt at the hard
block and in the question file, not repeated in the middle of a run.

## `pub fn notifiers_for(ids: &[String], warnings: &mut Vec<String>) -> Vec<&'static dyn Noti…`

Resolve `[interaction] notify = [...]` to delivery channels. An id that
resolves to nothing warns rather than silently dropping notifications —
believing you configured an alert you never get is worse than having none.

## `pub trait AnswerSource {`

Where an answer comes from. Step 8 adds an event-log implementation behind
`upstroke answer <id>`; the engine does not change when it does.

## `pub trait AnswerSource` › `fn resolve(&self, question: &Question) -> Result<Answer, UpstrokeError>;`

Called only at a hard block (§12), never mid-frontier.

## `pub struct UnattendedAnswers;`

CI and every other detached context: nobody is there. Note this returns
`Unanswered`, not `Declined` — the task parks rather than failing, and the
run's exit status reports it (§12).

## `pub struct TerminalAnswers;`

§12's attached-terminal channel. Degrades to `Unanswered` whenever stdin is
not a terminal, so a run piped from a file or a service manager parks
instead of hanging on a read that will never return.

## `fn resolve(&self, question: &Question) -> Result<Answer, Up…` › `Ok(0) => Ok(Answer::Unanswered),`

EOF: the terminal went away mid-run. Park, do not fail.

## `pub struct EventLogAnswers<'a> {`

§19's hard block, for a run nobody is sitting in front of.

A detached but interactive run — `nohup`, a service unit, `upstroke run &` —
has no terminal to prompt at, but it is not CI either: a human is expected,
just not right now. So it waits for `upstroke answer` to leave a file, which
is what "hard block (interactive)" means when the block cannot be a prompt.

The budget is shared across every question this run asks rather than per
question, because it exists to bound how long a forgotten run holds a
workspace and a branch hostage — a per-question budget would multiply by the
number of questions and defeat that.

## `pub struct EventLogAnswers<'a>` › `remaining: Mutex<Duration>,`

Wait left across all questions. `Mutex` only because `resolve` takes
`&self`; the engine is single-threaded.

## `impl<'a> EventLogAnswers<'a>` › `pub const DEFAULT_POLL: Duration = Duration::from_secs(5);`

Poll often enough to feel responsive, rarely enough to be free.

## `impl<'a> EventLogAnswers<'a>` › `pub fn new(dir: PathBuf, budget: Duration, sleeper: &'a dyn Sleeper) -> Self {`

The waiting itself is injected, so a test can exercise a bounded wait
without spending it.

## `pub fn interpret(question: &Question, raw: &str) -> Answer {`

Interpret one typed line.

Empty deliberately means *parked*, not *declined*: a stray Enter must not
fail a task and block its dependents. Failing requires typing it.

## `pub(crate) fn answer_for_option(question: &Question, choice: usize) -> Option<Answer> {`

Resolve one rendered, 1-indexed option without losing the action encoded by
engine-authored terminal choices. `Question.options` predates typed option
records, so the final option on every non-clarification question is the
frozen decline action; treating its label as ordinary guidance would retry
the task the operator explicitly chose to give up on.

## `pub fn answers_for<'a>(`

Pick the answer channel for a mode and the situation the run is actually in.

§12 lists two v0.1 channels, and which one applies is not a mode question
alone: `on_block` at an attached terminal means *prompt*, and the identical
config detached means *wait for `upstroke answer`*. Deciding it here rather
than in the engine keeps that distinction where the channels live.

A zero budget collapses the detached case back to parking immediately,
which is what an operator who does not want a run holding a workspace asks
for with `wait_on_block_secs = 0`.

## `pub trait Sleeper {`

Waiting, injectable so tests never actually sleep.

## `pub const DEFAULT_DEFER_BACKOFF: Duration = Duration::from_secs(60);`

First wait after a rate-limited attempt. Windows reset on the order of
hours (§13), but without the capacity engine there is no reset time to read
— so this backs off rather than pretending to know one.

## `pub const MAX_DEFER_BACKOFF: Duration = Duration::from_secs(600);`

Cap on the wait. Past this, waiting longer is worse than asking a human.

## `pub fn defer_backoff(base: Duration, round: u32) -> Duration {`

Doubling backoff, capped. `round` counts consecutive waits where deferred
tasks were the *only* runnable work.

## `fn an_empty_line_parks_but_skip_declines()` › `assert_eq!(interpret(&question(), "\n"), Answer::Unanswered);`

A stray Enter must never fail a task and block its dependents.

## `fn a_number_picks_an_option_and_anything_else_is_free_text()` › `assert_eq!(`

Out of range is not silently clamped — it is the user's words.

## `fn the_answer_channel_follows_the_mode_and_the_situation()` › `assert_eq!(`

The test harness runs detached, so an interactive mode here resolves
to the waiting channel rather than a prompt nobody would see. That
is the §19 case a terminal-only implementation silently degraded to
CI behaviour.

## `fn the_answer_channel_follows_the_mode_and_the_situation()` › `let immediate = answers_for(InteractionMode::OnBlock, dir, Duration::ZERO, &idle);`

A zero budget is an explicit "do not hold the workspace": still not
a prompt, but it gives up immediately.

## `fn answers_survive_the_trip_through_a_file()` › `let leftovers: Vec<String> = std::fs::read_dir(&dir)`

Nothing partial is left behind for the engine to trip over.

## `fn a_detached_run_waits_for_an_answer_file_then_gives_up()` › `let dir = std::env::temp_dir().join(format!("upstroke-answer-wait-{}", std::process::id()…`

§19's "hard block (interactive)" for a run with no terminal: it
waits for `upstroke answer` rather than degrading to CI behaviour.

## `fn a_detached_run_waits_for_an_answer_file_then_gives_up()` › `let counting = CountingSleeper::default();`

Nothing ever arrives: the budget bounds the wait rather than the
run holding its workspace forever.

## `fn a_detached_run_waits_for_an_answer_file_then_gives_up()` › `assert_eq!(`

The budget is shared across questions, not granted per question, so
a run with many open questions cannot multiply its own deadline.

## `fn a_detached_run_waits_for_an_answer_file_then_gives_up()` › `let arriving = ArrivingSleeper {`

An answer that lands during the wait is picked up.

## `mod tests` › `struct ArrivingSleeper {`

Stands in for an operator running `upstroke answer` mid-wait.

## `fn unattended_parks_rather_than_declining()` › `let answer = UnattendedAnswers.resolve(&question()).expect("resolve");`

The distinction is the whole CI story: Declined fails the task,
Unanswered parks it and the exit status reports it (§12).
