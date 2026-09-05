# `src/status.rs`

Extended notes for [`src/status.rs`](../../src/status.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

`upstroke status` — the run, folded out of its own log (DESIGN.md §15).

Status is a pure read: it opens no branch, spawns no agent, and takes no
lock. Everything it shows is derived by replaying `events.jsonl` through
the same [`RunState::apply`](crate::events::RunState::apply) the engine
writes through, so a running engine and a watching operator are looking at
one computation rather than two that ought to agree.

The plan comes from the run's own `plan.normalized.json` rather than from
the plan file on disk: §5 freezes a plan at run start, and status should
describe the run that happened even if the source plan has since moved on.

## `#![allow(clippy::disallowed_methods, clippy::disallowed_types)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `mod render;`

The view and the per-event lines, which reach nothing and so restore the
effect denials this module allows.

## `pub struct RunStatus {`

One run, as read back from disk.

## `pub struct RunStatus` › `pub running: bool,`

Whether an engine is driving this run *and* the run has not recorded
that it finished — the two halves of "still going", which the lock
alone does not answer.

## `pub struct RunStatus` › `pub held: bool,`

Whether anything holds the run's lock, finished or not.

Kept beside `running` rather than folded into it because a process
claiming a run that already ended is real and worth saying: `resume`
takes the lock and holds it across a dozen git subprocesses before it
writes `run_resumed`. During that window the run has an owner and an
outcome at the same time, and an operator asking `status` deserves
both.

## `pub struct RunStatus` › `pub interrupted: u32,`

Attempts that were in flight when a previous process stopped.

## `impl RunStatus` › `pub fn report(&self) -> RunReport {`

The same projection a run writes to `report.json`.

## `impl RunStatus` › `pub fn interrupted_run(&self) -> bool {`

Whether this run stopped without recording that it had finished — the
signature of a kill, a power loss, or an aborting error.

## `fn husk_answer(repo_root: &Path, wanted: &str) -> Option<UpstrokeError> {`

What `status` says about a husk id, or `None` if `wanted` names no husk.

Read-only end to end, and it never resolves a husk into a run: the answer
is the refusal, carrying which of the three kinds of husk this is, its
reason and its private locator. The authorized private root is the default
one, which is the root a read-only command is configured with.

## `pub fn load(repo_root: &Path, run_id: Option<&str>) -> Result<RunStatus, UpstrokeError> {`

Load a run: the newest one, or any unambiguous id prefix.

## `pub fn load(repo_root: &Path, run_id: Option<&str>) -> Result<RunStatus, UpstrokeError> {` › `Err(error) => return Err(husk_answer(repo_root, wanted).unwrap_or(error)),`

`startup_census`: "status is read-only: it ignores husks and,
asked explicitly for a husk id, reports an unstarted husk that
the next write command reclaims, a retained husk with its reason
and locator, or a possibly committed run whose public log has no
valid committed first line".

## `pub fn load(repo_root: &Path, run_id: Option<&str>) -> Result<RunStatus, UpstrokeError> {` › `let running = held && replayed.state.finished.is_none();`

Two questions, not one. The lock says whether a process has claimed this
run; the log says whether the run still has anywhere to go. `running`
needs both, for the same reason `interrupted_run` below does: `resume`
claims the lock before it writes anything, so a budget-stopped run has an
owner for as long as that resume takes to get going. Reading the lock
alone made those seconds render as `run in progress`, dropping the stop
reason, the parked list, and the `resume --budget` line the operator is
there to find.

## `pub fn load(repo_root: &Path, run_id: Option<&str>) -> Result<RunStatus, UpstrokeError> {` › `let interrupted = if running {`

Settled in memory only: status is a pure read and must not write to a
run it is merely looking at. A resume records the same settlement as
events instead.

And only for a run nothing is driving. An attempt in flight under a live
engine has not been interrupted — it is working — so settling it here
would report a running attempt as a failure and the whole run as halted.
`status` is the only window into a run that holds its own terminal, so
that reading is worse than no reading at all.

## `pub fn render(status: &RunStatus) -> String {`

The whole view: what happened, what it cost, and what it is waiting for.

The view itself is the private `render` child's; this is the public
surface it is reached through.

## `pub fn describe(event: &Event) -> String {`

One human line per event, for `--follow`.

Delegates to the private `render` child, beside the view it belongs with:
both turn a fold of the log into text and neither touches anything.

## `pub fn follow(`

The source retains the lock and resume ordering protocol required by §10.

Stream a run's events, from the beginning and then as they arrive.

Starting from the beginning is deliberate: `--follow` on a run already in
progress should show how it got here, not drop the reader into the middle
of a story. Reads only whole lines, so a follower attached to a live engine
never sees half an event. Returns once the run records that it is done —
or, once nothing is driving the run any more, after `max_idle_polls` with
nothing new, so a follower attached to a run whose engine has died gives up
instead of waiting forever.

## `let running = rundir::is_running(&status.paths.public);`

The idle budget is not a timeout on silence. A whole attempt —
the agent's thinking, its tool calls, the gates, the review —
folds into a single `attempt_finished`, so a healthy run says
nothing for minutes at a time; giving up on one would drop the
live view mid-run. The budget exists only to release a terminal
attached to an engine that has died, so it starts counting when
the run's lock does not.

One syscall per poll, asked plainly. This used to need a cheaper
variant of its own, because the check waited out a contention
grace every time the answer was yes — which on a healthy run is
every poll. The lock now answers exactly, so there is no cheaper
question to ask.

## `if terminal && !rundir::is_running(&status.paths.public) {`

A resume owns the lock before it can append RunResumed. A follower
that sees the previous epoch's RunFinished in that window must wait
for the marker rather than treating historical terminal state as the
current process's result.

## `fn stable_event_bytes_with(`

The source retains the sampling protocol required by §10.

Pair event bytes with a stable liveness observation. A dead snapshot is
trusted only after an identical second read and a second dead probe; this
prevents status from reading `attempt_started`, observing the conductor
release its lock after writing the settlement, and then inventing an
interrupted attempt from the stale prefix.

## `mod tests` › `fn status_asked_for_a_husk_id_names_which_husk_it_is() {`

`load` composes `resolve_run_id`'s refusal with `rundir::husk_report`,
and a composition nobody drives is the shape `PR4-CONF-008` was: both
halves were tested and their join was not. So this asks `status` itself.

## `fn status_asked_for_a_husk_id_names_which_husk_it_is()` › `let status = std::process::Command::new("git")`

A real repository, because the husk answer takes this repository's
key over its canonical common git dir.

## `fn the_ledger_keeps_worker_and_review_spend_apart()` › `assert!(ledger.contains("per-pool drain:"), "{ledger}");`

§13's second currency, beside the dollars and derived from the same
attempt records.

## `fn answers_and_defects_render_without_quoting_the_operator()` › `let line = describe(&event(EventBody::QuestionAnswered {`

The operator's words are an instruction to the agent, not something
status needs to echo into a terminal it does not control.
