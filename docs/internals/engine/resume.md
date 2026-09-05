# `src/engine/resume.rs`

Extended notes for [`src/engine/resume.rs`](../../../src/engine/resume.rs).

The code is the authority for what it does. This file preserves the migrated prose;
the concurrency protocol also remains at its source sites under standards §10 and §13.
Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `#![allow(clippy::disallowed_methods)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `pub(super) fn resume_harness_inner_on(`

The same resume, on an explicit boundary. See [`super::run_harness_on`], and
[`super::coordinator::run_harness_inner_on`] for why `_contained` is a
parameter: a resume is a write command too, and the ambient job it needs is
the one no facade used to establish.

## `let mut header_warnings = Vec::new();`

Read-only, and before either lock. §14's refusals must reach the operator
without a worktree lease and without a `run.lock` file behind them, and
the config is one of them — so the two things deciding *how* to read
today's config have to be read first: the schema this run was recorded
at, which chooses between refusing an impossible ceiling and warning
about one, and where the run's config lives.

Only that. The authoritative whole-log read is below, under the locks,
and everything the resume actually acts on comes from there. This read
decides what to refuse before taking anything.

## `let mut run_opts = RunOptions::new(`

The run knows its own plan and config; the CLI may override the config
but never the plan, which is frozen (§5).

## `let limits = config::EngineLimits::for_resume(header_schema);`

This run's ceilings were fixed when it started. Today's `[engine]` keys
are read for the same reason today's gates are — the file is what is
here — but a value this engine cannot honour is a statement about some
future run, not an instruction to this one, so it warns rather than
stranding a run whose only fault is that someone edited a file it reads.

## `let workspace = Workspace::open(&opts.repo_root)?;`

The first effect of the command.

## `let _lock = RunLock::acquire(&public)?;`

Claimed before anything is acted on, so two resumes cannot race each
other into the same branch. The lock sits beside the ops surface, which
is the only half of the run directory known this early: where the private
half went is recorded in `run_started`, which the authoritative read
below is about to establish.

## `if started.plan_path != header.plan_path || started.config_path != header.config_path {`

`run_started` is the first line of an append-only log, so the read under
the lease must agree with the one before it about where this run's plan
and config live. If it does not, something rewrote history while we were
waiting for the lease, and the pre-lock refusals were answered about
files this run never named.

## `let analysis = validated.confirm_under_lease(`

Adopted only now, and only against the schema the authoritative read
settled on: a resume that raced an appended schema upgrade must not run
on a reading derived from the header it saw first.

## `let recorded_gates = events::recorded_gates(&events).cloned();`

Usually `run_started`'s, but a log too old to carry them there may have
had them established by an earlier resume instead — which is what stops
the re-derivation repeating, and drifting, on every resume after that.

## `let Preflight {`

Re-probes agents and re-reads config, exactly as a fresh run does —
except for the two things that are facts about *this run* rather than
about today's machine: who reviews it and what verifies it. Both come
from the record (see `preflight_with_recorded`), so a resume continues
the run it is resuming rather than starting a differently-judged one on
the same branch.

## `let names_now: Vec<String> = gates.iter().map(|gate| gate.name.clone()).collect();`

A log from before the gate record, resumed for the first time — the
only case with nothing to rebuild from, since this resume writes down
what it settles on and the next one is ordinary.

It still recorded gate *names*, which is not enough to rebuild the
gates but is enough to say something better than "anything may have
changed": if the names have moved, that is proof rather than
suspicion, and if they have not, the only undetectable edit left is a
command behind an unchanged name.

## (end of file)

Both empty: the run recorded no gates and none resolve today, so
there is nothing a command could have hidden behind. Saying "may have
been verified differently" here would be a false alarm on every
gateless run, and a warning that cries wolf on the harmless case is
one nobody reads on the harmful one.

## `if analysis.plan.source.hash != started.plan_hash {`

The plan is frozen. A different hash means the file moved under the run,
so every task index in the log — which is what `Progress` is keyed by —
may now mean a different task.

## `Some(events::RunOutcome::Parked | events::RunOutcome::BudgetExceeded) | None => {}`

Ended parked, at a budget, or never ended at all — all three are
exactly what resume is for. A budget stop in particular is *designed*
to be resumable: `--budget` re-derives the ceiling (see
`ResumeOptions::budget_usd`), so raising it and continuing is one
command rather than a new run and a lost branch.

## `let defect_questions: BTreeSet<QuestionId> = replayed`

`question_answered`, its design-defect record, and a declined task's
failure predate atomic parking and are three durable appends. Preserve
every crash prefix so a closed question can never strand its task in
AwaitingInput with no legal way to answer it again.

## `let paths = match &opts.private_root {`

Resolve the recorded private root before touching the worktree so a
killed engine's durable snapshot registrations are reclaimed first.

## `let recorded_head = last_committed_sha(&replayed.events).unwrap_or(started.base_sha.clone());`

§15's check, before anything is discarded: if HEAD moved, refusing has
to leave the operator's tree exactly as they left it.

## `let mut adopted = None;`

A schema-3 successful settlement durably names the exact commit object
that passed review. Recovery may publish that object from its pin, or
finish recording it when HEAD already advanced. Subject/parent matching
is intentionally insufficient: another commit can share both while
containing arbitrary bytes.

## `for interrupted in replayed.state.interrupted_attempts() {`

A pin with no successful settlement is from a crash between preparing
the object and appending AttemptFinished. It has no authority to move
HEAD and is removed with an expected-old-value CAS before retrying.

## `let discarded = workspace.uncommitted_summary()?;`

Crash residue: a dead agent's half-written edits. §14 rolls a failed
attempt back to the last commit, and an attempt that never reported is
no different — the session that would have explained these edits is
gone, so nothing can verify them.

## `let sleeper = harness.sleeper.unwrap_or(&RealSleeper);`

Where the agent-authored half lives is a fact about the run, not about
today's defaults. A resume under a different HOME — a service account, a
container, the no-home fallback — would otherwise scatter the rest of
this run's transcripts into a second private root while `status` went on
pointing at the first. An explicit override still wins, for a private
root that has genuinely moved.

## `let prior_signals = capacity::observe(&replayed.events).exhausted;`

§13's ground-truth signals, folded from this run's own log before its
state is moved into the scheduler — what the earlier process learned
about the pools, which a resumed run's snapshot must not forget.

## `budgets,`

Re-derived from today's config and flags, deliberately (see
`ResumeOptions::budget_usd`): raising the ceiling and resuming is the
one-command recovery a budget stop is supposed to have.

## `exhausted_pools: prior_signals.keys().cloned().collect(),`

Seeded from the log so a resume neither re-announces an outage the
previous process recorded nor swallows a fresh one.

## `if let Some((task, message)) = adopted {`

The `task_committed` the dead process never got to must be the first
append after its successful settlement. Schema 3 treats that adjacency
as part of the exact prepared-commit binding, so unrelated legacy answer
repairs cannot interpose and poison the log.

## `if effective_schema < events::SCHEMA_VERSION {`

A legacy run cannot have its opening event rewritten without violating
append-only history. This no-op event is the current downgrade boundary:
schema-1 binaries do not know its tag, while schema-2 binaries reject a
transition to schema 3 before applying their old partial-review contract.

## `for record in &run.state.questions {`

A crash between `question_answered` and the payload rewrite leaves a
file that still reads as open, which `upstroke answer` would accept a
second answer against — one no engine can ever ingest, because the
question is already closed in the log. The log is what is authoritative;
make the payloads agree with it again.

## `let interrupted = run.state.interrupted_attempts();`

Write the `attempt_finished` the dead process never got to.

Recorded rather than settled in memory, because a settlement only a
reader performs is lost the moment someone else replays the log: the
ledger line vanishes and, worse, the rung's refunded allowance vanishes
with it, so a later resume would think the attempt had been spent.

## `run.emit(EventBody::RunResumed {`

Applying this is what drops every session and wakes deferred work — the
§14 pairing, enforced by the same fold a replay uses rather than by this
function remembering to do it.

## `gates: recorded_gates.is_none().then(|| gates.clone()),`

Only when this resume is the one that had to settle the question.
Where the log already answers it, re-stating the answer would put
the same fact in two places that a later change could pull apart.

## `run.emit_capacity_snapshot(&prior_signals)?;`

§14 takes a capacity snapshot at pre-flight, and §15 makes a resume
re-establish everything a fresh run establishes. A resume that skipped it
would leave the log claiming the pools looked, hours later, exactly as
they did when the run began.

## `fn render_names(names: &[String]) -> String {`

A gate name list, for a message.

## `fn last_committed_sha(events: &[events::Event]) -> Option<String> {`

The sha the run's record ends at — what HEAD must still be.

## `fn unrecorded_commit(`

The task an interrupted run committed without living long enough to record.

The shape is narrow, which is what makes it safe to act on: the log must
*end* at an attempt that passed, for a task that never reached `Done`. No
other event can follow, because the process that would have written one is
the process that died. Returns the task and the message the engine would
have used, so the caller can confirm the commit really is the one it is
about to adopt rather than trusting the log's shape alone.
