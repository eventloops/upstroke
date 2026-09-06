# `src/status.rs`

Extended notes for [`src/status.rs`](../../src/status.rs).

These notes preserve the module comments after the status repairs. Item headings quote source lines for navigation.

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

## `mod render;`

The view and the per-event lines, which reach nothing and so restore the
effect denials this module allows.

## `pub struct RunStatus {`

One run, as read back from disk.

## `pub running: bool,`

Whether an engine is driving this run *and* the run has not recorded
that it finished — the two halves of "still going", which the lock
alone does not answer.

## `pub held: bool,`

Whether anything holds the run's lock, finished or not.

Kept beside `running` rather than folded into it because a process
claiming a run that already ended is real and worth saying: `resume`
takes the lock and holds it across a dozen git subprocesses before it
writes `run_resumed`. During that window the run has an owner and an
outcome at the same time, and an operator asking `status` deserves
both.

## `pub interrupted: u32,`

Attempts that were in flight when a previous process stopped.

## `pub fn report(&self) -> RunReport {`

The same projection a run writes to `report.json`.

## `pub fn interrupted_run(&self) -> bool {`

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

## `Err(error) => return Err(husk_answer(repo_root, wanted).unwrap_or(error)),`

`startup_census`: "status is read-only: it ignores husks and,
asked explicitly for a husk id, reports an unstarted husk that
the next write command reclaims, a retained husk with its reason
and locator, or a possibly committed run whose public log has no
valid committed first line".

## `let running = held && replayed.state.finished.is_none();`

Two questions, not one. The lock says whether a process has claimed this
run; the log says whether the run still has anywhere to go. `running`
needs both, for the same reason `interrupted_run` below does: `resume`
claims the lock before it writes anything, so a budget-stopped run has an
owner for as long as that resume takes to get going. Reading the lock
alone made those seconds render as `run in progress`, dropping the stop
reason, the parked list, and the `resume --budget` line the operator is
there to find.

## `let interrupted = if running {`

Settled in memory only: status is a pure read and must not write to a
run it is merely looking at. A resume records the same settlement as
events instead.

## `let interrupted = if running {`



## `let interrupted = if running {`

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

Renders the supplied event without folding or validating a log. Recorded
control characters are made safe for a terminal; malformed timestamps and
leap-second values retain their full date. A supplied duration is shown at
millisecond precision. See DESIGN.md §18 for the output contract.

## `pub fn follow(`

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

## `let running = rundir::is_running(&status.paths.public);`



## `let running = rundir::is_running(&status.paths.public);`

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

Pair event bytes with a stable liveness observation. A dead snapshot is
trusted only after an identical second read and a second dead probe; this
prevents status from reading `attempt_started`, observing the conductor
release its lock after writing the settlement, and then inventing an
interrupted attempt from the stale prefix.

## `#[test]`

`load` composes `resolve_run_id`'s refusal with `rundir::husk_report`,
and a composition nobody drives is the shape `PR4-CONF-008` was: both
halves were tested and their join was not. So this asks `status` itself.

## `let status = std::process::Command::new("git")`

A real repository, because the husk answer takes this repository's
key over its canonical common git dir.

## `assert!(ledger.contains("per-pool drain:"), "{ledger}");`

§13's second currency, beside the dollars and derived from the same
attempt records.

## `let line = describe(&event(EventBody::QuestionAnswered {`

The operator's words are an instruction to the agent, not something
status needs to echo into a terminal it does not control.

## `fn record(failure: Option<events::FailureRecord>, reviews: Vec<ReviewRecord>) -> AttemptRecord {`

One settled record, varying only what a test is about.

## `#[test]`

The pre-repair line for every answer was "q-1 answered via terminal":
a decline, which fails the affected task and may halt the run, and a
question no channel could deliver both read as an answer.

## `let decline = answered(Answer::Declined, Some(halts_run));`

The coordinator writes the answer before either task failure.
A crash here leaves this prefix as the entire durable truth.

## `let unnamed = finished(RunOutcome::Halted, None);`

The parser admits both inconsistent shapes; each is shown as what
the record says rather than folded into a halt or a silence.

## `#[test]`

The record production writes for a rejected review carries both the
pass's `Failed` outcome and a `ReviewFailed` failure whose reason is
`review failed: …`; an unavailable reviewer carries `Unavailable` and a
failure that `engine::attempt::review_failure` maps from the reviewer's
outcome status: rate-limited to `RateLimited`, a timeout to `Timeout`,
and any other unavailability to `ReviewUnavailable`. The line names the
pass and the model beside the reason on that shape — the one a run
produces — and not only on a record with the outcome alone.

## `let gates = describe(&finished(`

A gate failure has no review to name, and says nothing about one.

## `(`

Above f64's exact-integer range, milliseconds still survive.

## `(Duration::MAX, "18446744073709551615.999", u32::MAX),`

Public describe also accepts values beyond the wire's u64 ms.

## `let standalone = describe(&event(EventBody::TaskFailed {`

Independent events own equal transition snapshots, so exercise
both wire shapes with the same failure policy and reason.

## `let atomic = describe(&finished(`

Public describe can receive either parking shape; its
transition must retain reason B beside attempt reason A.

## `#[test]`

"Passed" follows `AttemptRecord::is_successful`, not `failure.is_none()`:
the grid below is every combination of the two facts that predicate
reads, and the line says "passed" exactly where the predicate says so.
The pre-repair code answered "passed" for a record with no failure and
a review that rejected it.

## `#[test]`

The transition and the parking are two halves of one settlement, and
the line renders each on its own. The pre-repair code rendered a parked
attempt's transition only when it was an escalation, so a parking
beside a `Fail` — a shape no writer produces today — dropped the task's
failure from the line.

## `let parked_pass = describe(&finished(`

A parking with no failure and no transition says what the record
says, rather than inventing a "policy refusal" for it.

## `#[test]`

Every field on a line is on-disk data, and a failure reason quotes an
agent's stderr. The pre-repair line carried the reason verbatim, so a
newline in it split one event across lines of `--follow` and an escape
sequence in it reached the terminal.

## `let parked = describe(&event(EventBody::TaskParked {`

Not only the reason: the guarantee is on the assembled line, so a
field the arm did not think of is covered too.

## `let plain = describe(&finished(record(None, Vec::new()), None, None));`

And a line with nothing to change is the line as assembled.

## `#[test]`

Abbreviation checks calendar dates, clock ranges, and the complete
suffix. Leap-second records retain their date because this renderer
does not consult the historical leap-second schedule. Event.ts is an
unconstrained String and parsing validates nothing about it.
