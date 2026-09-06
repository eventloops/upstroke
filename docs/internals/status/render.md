# `src/status/render.rs`

Extended notes for [`src/status/render.rs`](../../../src/status/render.rs).

These notes preserve the module comments after the status repairs. Item headings quote source lines for navigation.

## Module

The settled view and the one-line event descriptions (DESIGN.md §15).

The half of `status` that touches nothing. It takes a `RunStatus` the parent
has already folded out of the log and returns a `String`, and it turns one
`Event` into one line for `--follow`. What is left in the parent is
everything that does reach the world — reading the log, probing the lock,
and streaming to a sink — so the two halves have one reason to change each
(CODING_STANDARDS.md §3).

Splitting the rendering out did not change what it renders: the parent's
`render` and `describe` are the public surface and delegate here, and this
module is private.

# What a `--follow` line promises

Each line is a contract with an operator, not a debug aid: it says what
happened, to which task, and what the engine decided next. A failure names
its reason and the transition recorded with it; a decline names its halt
policy; a halted run names the task it halted at. Two things hold for every
line, whatever the log carries. It is **one line** — every field printed
here is on-disk data (CODING_STANDARDS.md §8), and a failure reason quotes
an agent's stderr, so the assembled text passes through [`one_line`] before
it is returned. And it is **exhaustive** — the `match` over `EventBody` has
no wildcard arm, so a variant this module does not know is a build error
rather than an event that renders as nothing.

The line is product surface and its contract is `DESIGN.md` §18 (the CLI
surface), which this module implements; a change to what a line says is a
change to that section in the same pull request (CODING_STANDARDS.md §13).

# Why the effect denials are restored here

`status` carries a module-level allow of `clippy::disallowed_methods` and
`clippy::disallowed_types`, recorded in the **frozen legacy section** of
`effects/allowlist.toml` — earned by `follow`, which writes to an
`io::Write` sink, and by the husk fixtures, which build run directories with
raw `fs` and a `git` subprocess. Lint levels descend through the module
tree, so that allowance would reach this file for free.

It has no business doing so. Nothing below writes a file, starts a process,
or streams to a sink: the view is accumulated into a `String` through
`std::fmt::Write`, whose `write_fmt` is a different `DefId` from the denied
`io::Write::write_fmt` and is not an effect. Restoring the two denials makes
an effect added here a build error rather than something the parent's
allowance quietly covers — and it is why this file needs no allowlist row of
its own, since an allowance is what that file records and this module takes
none.

## `use std::fmt::Write as _;`

The remaining `write!` calls append suffixes through String's fmt::Write
implementation, which cannot return Err. Their discarded results are Ok (§7).

## `pub(super) fn render(status: &RunStatus) -> String {`

The settled view, assembled: the report and its ledger, then the trailing
lines that say whether it is still moving and what it is waiting for.

The `state:` line is one of four readings of two facts — whether anything
holds the run's lock (`held`) and whether the log records a finish — which
the parent folds into `running` (held and unfinished) and `interrupted_run`
(unheld and unfinished). Held and finished is a claim on an ended run, said
as such; unheld and finished adds nothing to the outcome the report has
already printed, so it says nothing.

## `if status.running {`

Liveness first among the trailing lines, because it decides whether any
of the above is still moving.

## `out.push(format_args!(`

Finished, and somebody has claimed it anyway — a `resume` between
taking the lock and writing `run_resumed`. The outcome above is still
this run's outcome; it may just not be the last word for long.

## `pub(super) fn describe(event: &Event) -> String {`

One line per event: the time of day out of the record's own timestamp,
with the zone the record wrote (`Z` for everything this engine writes), then
the body.

The time is the record's, not the reader's: a `--follow` at 16:03 local in
UTC+2 shows `14:03:07Z`, and the suffix is what says so. A timestamp not in
the calendar, clock, and suffix form accepted by [`clock_of`] is printed
whole. Leap-second values retain their date too, since abbreviating one
would hide information needed to check it against the leap-second schedule.

## `if !data.discarded.is_empty() {`

Recorded so that someone reading the run tomorrow can see that
work was thrown away; a follower reading it today deserves the
same.

## `EventBody::AttemptFinished {`

The outcome, then each decision the settlement carries, in the
order the engine made them. Each half of the settlement renders on
its own, so no pairing of transition and parking is a shape this
arm has to know about: a parked escalation reads "escalating past
…; parked on question …", and a pairing no writer produces today
still prints both facts rather than dropping one.

## `EventBody::LadderRetry { task, data, .. } => format!("{task}: {}", describe_retry(data)),`

The legacy standalone forms of the decisions above, spelt by the
same helpers so the two wire shapes cannot drift apart.

## `EventBody::DeferWaitElapsed { data } => {`

Integer milliseconds preserve the wire's precision even beyond
f64's exact range. A public caller's submillisecond remainder is
truncated, matching the persisted format's precision.

## `EventBody::QuestionAnswered { data } => match &data.answer {`

Three answers, three lines. A decline carries the halt policy
frozen with it, and that is what this line reports — the policy, not
a transition: the task failure a decline causes is its own later
event (`task_failed`, which `resume` appends for a log that stopped
before it), the answer names no task, and a question may park more
than one. A question no channel could reach a person with was not
answered at all.

## `None => "was not recorded",`

Only a log older than schema 3, which requires the
policy on every decline.

## `let outcome = match data.outcome {`

Spelt here rather than through `Debug`: a derived `Debug` is a
Rust identifier, not a contract, and a halted run's line has to
name the task it halted at, which is what the operator acts on.

## `let mut line = format!("run finished: {outcome}");`

The two fields are read together: a halt names its task or
says the record did not, and a task named on a run that did not
halt is shown as the oddity it is rather than as a halt.

## `fn attempt_outcome(record: &AttemptRecord) -> String {`

What one settled attempt's record says happened.

"Passed" is the record's own claim of success — no failure and every
review pass passed, the facts [`AttemptRecord::is_successful`] reads, in
the same order — and not `failure.is_none()`. A review that rejected the
code or never reached a verdict is named by pass and model whether or not
the engine also recorded a failure for it: in production it always does
(`engine::attempt::evaluate_review` writes a `ReviewFailed` or
`ReviewUnavailable` failure beside the pass's outcome), so the line a run
produces is `failed — review failed: …; review \`review\` (model)
rejected it`. A record carrying the outcome and no failure is rendered as
it reads, "was not approved". Schema-3 validation refuses this inconsistent
shape, and schema-4 success checks reject it through `is_successful`.
Public describe renders the supplied event without validating a log.

## `fn describe_transition(transition: &AttemptTransition) -> String {`

The ladder decision one failed attempt settled with.

## `fn describe_task_failure(data: &TaskFailed) -> String {`

The terminal decision, in the one spelling both wire shapes use, carrying
the transition's own reason: beside an attempt record it repeats the
attempt's reason when the coordinator copied it, and shows the difference
when a log carries two, which schema-3 validation admits (it requires the
kinds to agree and says nothing about the reasons). The `Debug` of the kind
is the log's `snake_case` spelt as a Rust identifier; a `Display` on
`FailureKind` belongs to `src/ladder.rs` (SWEEP-RENDER-011).

## `fn halt_suffix(halts_run: bool) -> &'static str {`

What a task's terminal failure means for the rest of the run, which is the
fact an operator watching one acts on.

## `fn clock_of(ts: &str) -> Option<(&str, &str)> {`

The clock and suffix of a calendar-valid RFC 3339 timestamp with seconds
in `00..=59`. Other text, including leap-second values, stays whole.

Month lengths and Gregorian leap years keep malformed dates visible.
Fractions need digits, and a zone is required with no trailing text.
This is an abbreviation rule, not event validation: it does not consult
the historical leap-second schedule, so those values keep their date.
Every Option propagation below selects the whole-timestamp fallback,
including the checked slices. Absence never becomes an error (§7).

## `let value = digits`

Every call below requests two or four digits, so the accumulated
value is at most 9999 and both arithmetic operations fit u32.

## `let mut index = 19;`

The suffix: an optional fraction, then `Z` or a signed `HH:MM` offset,
and nothing after it.

## `Some((ts.get(11..19)?, ts.get(19..)?))`

Every byte checked above is ASCII, so both boundaries are char
boundaries and the two `get`s below cannot fail.
