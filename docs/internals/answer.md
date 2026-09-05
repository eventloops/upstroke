# `src/answer.rs`

Extended notes for [`src/answer.rs`](../../src/answer.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

`upstroke answer <question-id>` — the cross-process answer channel (§12).

A question is raised by one process and answered by a person who may be
nowhere near it: at another terminal, hours later, or after the run has
already ended. So the command does not talk to the engine at all. It writes
the answer beside the question, and whichever engine is or will be driving
that run picks it up — a live one on its next scheduler turn, or the next
`upstroke resume`.

That indirection is what makes §12's promise ("a run survives its
notifier") true of answers as well as delivery.

## `#![allow(clippy::disallowed_methods)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `pub enum Reply {`

What the operator said, before it is turned into an [`Answer`].

## `pub enum Reply` › `Option(usize),`

Pick option N, 1-indexed as the question renders them.

## `pub enum Reply` › `Decline,`

Give up on the task; its dependents will be blocked (§19).

## `pub struct Answered {`

Where an answer landed, for the caller to report.

## `pub struct Answered` › `pub run_is_live: bool,`

Whether a live engine is expected to pick this up on its own.

## `pub fn answer(repo_root: &Path, wanted: &str, reply: Reply) -> Result<Answered, UpstrokeError> {`

Record an answer to a question, found by id or unambiguous prefix.

## `pub fn answer(repo_root: &Path, wanted: &str, reply: Reply) -> Result<Answered, UpstrokeError> {` › `if let Some(existing) = &record.answer {`

Answering twice is a mistake worth catching rather than a last-write-
wins race: the first answer may already have driven a retry, and the
second would look like it had an effect it cannot have.

## `pub fn answer(repo_root: &Path, wanted: &str, reply: Reply) -> Result<Answered, UpstrokeError> {` › `format!("1-{}", record.question.options.len())`

Not `1..N`: every operator of this tool reads
Rust, where that is the range that excludes N.

## `pub fn answer(repo_root: &Path, wanted: &str, reply: Reply) -> Result<Answered, UpstrokeError> {` › `if answer == Answer::Unanswered {`

An empty reply means "leave it parked" at a prompt (§12), and typing
nothing into this command almost certainly means the same — but here it
would write a file the engine then ingests as an answer, which is not
what the operator asked for.

## `pub fn show(repo_root: &Path, wanted: &str) -> Result<String, UpstrokeError> {`

Render the question for an operator deciding what to say.

## `fn seed(repo: &Path, run: &str, id: &str)` › `std::fs::write(`

A run only exists once its log records a committed `run_started`:
`find_question` scans committed directories, so a fixture that wrote
only a question would be answering a question in a husk. Written by
hand rather than serialized, so the fixture pins the wire rather than
agreeing with whatever the writer happens to produce.

## `fn an_option_number_preserves_the_option_action()` › `seed(&repo, "02RUN", "q-2");`

Out of range is refused rather than silently clamped — the operator
meant a specific option.

## `fn an_empty_answer_is_refused_rather_than_written()` › `let repo = scratch("empty").join("repo");`

At a prompt, empty means "leave it parked". Written to a file it
would instead be ingested as an answer that changes nothing.

## `fn a_question_is_answered_once()` › `let questions = rundir::public_dir(&repo, "01RUN").join("questions");`

Simulate the engine having ingested the first answer and rewritten
the payload with it.
