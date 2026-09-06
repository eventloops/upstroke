# `src/engine/preflight.rs`

Extended notes for [`src/engine/preflight.rs`](../../../src/engine/preflight.rs).

The code is the authority for what it does. This file preserves the migrated prose;
the concurrency protocol also remains at its source sites under standards §10 and §13.
Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `pub(super) struct Preflight {`

Everything `run` and `resume` both establish before an agent is spawned.

Shared so the two cannot drift: §15 requires a resume to re-probe agents
and re-check gates, and the surest way to guarantee it performs the same
checks as a fresh run is for there to be one function that performs them.

## `pub(super) struct Preflight` › `pub(super) review_pass_timeout: Duration,`

Each pass gets this independent frozen allowance. It comes from the
review plan rather than today's config on resume.

## `pub(super) struct Preflight` › `pub(super) gates: Vec<GateSummary>,`

The effective gates, in the one shape everything else projects from —
the record, the permission grants, and the report all read this rather
than walking `analysis.gates` again, so they cannot drift apart.

## `pub(super) struct Preflight` › `pub(super) budgets: config::Budgets,`

§17's ceilings with `--budget` folded in and validated — computed at
pre-flight so a bad flag refuses before the run branch exists.

## `pub(super) struct Recorded {`

What a resume takes from the run's own record instead of from today's
machine (§15). Empty for a fresh run, which has no record to take from.

## `pub(super) struct Recorded` › `pub(super) reviews: Option<ReviewPlan>,`

Who judges this run's code. `None` for a log written before step 9.

## `pub(super) struct Recorded` › `pub(super) gates: Option<Vec<GateSummary>>,`

What verifies it. `None` for a log written before the gate record.

## `pub(super) struct Recorded` › `pub(super) legacy_review_timeout_missing: bool,`

The legacy record identifies the reviewers but predates schema 3's
explicit per-pass timeout. Its first complete-review resume must choose
and serialize that missing part of the verification identity.

## `pub(super) struct Recorded` › `pub(super) gates_from_config: bool,`

Whether those gates came from `[[gates]]` rather than the repo's shape.

Travels with them, and read only when `gates` is `Some`: it is a label
*on the recorded list*, so leaving it to be re-derived would have the
run's own report and a later `status` disagree about the same gates —
the drift this record exists to stop, one field short of stopped.

## `pub(super) struct Recorded` › `pub(super) routing: Option<RecordedRouting>,`

The run's routing structure plus the first snapshot that names every
resolved rung binding. Present only on resume.

## `pub(super) struct Validated {`

The pure half of pre-flight: the part that only reads files.

It exists because of *when* it can run rather than what it does. §14's
read-only refusals — the plan parsing, the graph, the routing chains, and
every `[engine]` ceiling — have to land before the first effect of a write
command, which is the worktree lease. Nothing here spawns a process, takes a
lock, or writes a byte, so it can run before that lease and refuse there.

The other half of pre-flight — probing agents, resolving gate programs —
genuinely inspects the machine and stays behind the lease.

`analysis` is what `inputs` says, and only what `inputs` says: the capture is
the source [`validate::analyze_captured`] parses, not a fingerprint taken
alongside a second read. That is the whole point of the type. A snapshot
beside an independent read proves nothing about what was validated — bytes
that change and change back leave two equal snapshots either side of a
validation performed on the value in between — so there is one read, and this
is it.

## `pub(super) struct Validated` › `inputs: validate::CapturedInputs,`

Every file the analysis came out of: the plan, the repo config, the pools
file, and the worktree files the gate derivation reads.

## `pub(super) fn validate_inputs(`

Capture every input, then validate that capture.

Callers run this **before** the worktree lease, and again under it. See
[`Validated`] and [`Validated::confirm_under_lease`].

## `let inputs = validate::CapturedInputs::capture(&validate_opts);`

Capture first, then parse the capture. Not "snapshot, then read": the
ordering is not the point, having a single read is.

## `let analysis = validate::analyze_captured(&inputs, &validate_opts)?;`

§14: plan parses cycle-free, config loads, chains resolve.

## `impl Validated` › `pub(super) fn confirm_under_lease(`

Adopt an analysis, now that the lease is held.

The pre-lock check buys the ordering: a refusal reaches the operator
without a lock file, a run directory, or a branch behind it. What it
cannot buy is that the files did not move in the window between it and
the lease, and a run that executed inputs nothing ever checked would be a
worse defect than the ordering it fixed.

So the lease-holder captures and validates again, and adopts *that*
analysis rather than the pre-lock one, on the condition that the two
captures agree. The re-validation is not redundant with the comparison:
it is what puts the gate derivation — the one input `analyze` reaches the
filesystem for rather than parsing out of the capture — behind the lease,
where the worktree is this run's and a read of it is a fact about it. The
comparison is what makes the pre-lock refusal mean something: it says the
question answered before the lease was asked about these bytes.

Every retry does the whole thing — capture, validate that capture,
confirm it against the previous one — so an adopted analysis is always
one whose own bytes were seen twice with nothing in between.

`limits` is passed again rather than remembered because a resume derives
it from a header it read before the lock; if the authoritative read
disagrees, the reading has to be redone under the one that counts.

Bounded, because inputs being rewritten faster than they can be read
twice is a broken machine rather than a race worth waiting out, and
looping forever there would hold the lease while doing so.

## `impl Validated` › `let Self {`

The pre-lock analysis is deliberately dropped rather than returned on
agreement: it answered "may this start", out of a worktree that was
still anybody's. What survives it is its capture, which is the thing
the reading taken under the lease has to agree with.

## `pub(super) fn preflight_with_recorded(`

Pre-flight, with whatever a previous process already resolved for this run.

Both halves of §14's verification — who reviews and what gates — are read
from the record on resume rather than re-derived, for one reason stated
twice: they are facts about the *run*, not about today's machine. A CLI
installed or removed since the run started must not change who judges it,
and a `upstroke.toml` edited since — including by an implementer, in the very
workspace it edits — must not change what verifies it. A live run already
works this way by construction, holding one analysis in memory for its whole
length; this is what makes a resume the same run rather than a new one
wearing its branch.

`None` on either means the log predates that record and said nothing. Both
re-derive in that case rather than inherit an empty value, because an empty
review plan reads as "review is off" and an empty gate list reads as "there
was nothing to pass" — each would finish the run less verified than it began.
The caller warns; only it knows which absence it is looking at.

`analysis` arrives already validated, from [`validate_inputs`] run before
the worktree lease and confirmed under it — see [`Validated`]. Everything
from here on may inspect the machine.

## `if let Some(routing) = recorded.routing.as_ref() {`

Bindings are execution identity just like reviewers and gates. Restore
them before resolving reviewers or probing agents: probing today's pin
and only swapping later would let a harmless config edit refuse a resume
on an agent this run was never going to use.

## `if let Some(record) = &recorded.gates {`

The recorded gates replace the re-derived ones *here*, before anything
reads them — so the pre-flight resolution below, the `Bash(<cmd>)` grants
the workers get, the prompt that names their allowed commands, and the
report all describe the gates this run actually verifies against. One
substitution point rather than a comparison the rest of the function
could forget about.

## `None => review::plan_for(`

Resolved against the adapters *this harness* holds, not the built-in
registry: the harness is what can actually spawn something, and
asking the wrong one would let a preview's answer stand in for a
capability the run does not have.

## `events::validate_review_identity(&review_plan, analysis.plan.tasks.len(), &opts.plan_path)?;`

A legacy record is not trustworthy merely because its missing marker
fields can be filled. Validate the complete inherited identity before
probing an adapter or dispatching any paid work; otherwise a malformed
schema-2 pass list can run once and only be rejected after it has been
appended as schema 3.

## `let required = review_plan.required_agents();`

Probe every agent the chains reference; a missing binary is a refusal
to start, not a task failure (§19). The capabilities are kept, not
discarded: §11.4's same-rung retry resumes a session only where the
adapter says it can.

Reviewers are probed on the same footing as implementers — step-6
finding #10 — but in two classes. Everything the config *asked* for is
required. The anti-self-review alternative was upstroke's own idea, so a
machine that cannot run it loses the upgrade rather than the run.

Resume draws the line in the same place. Requiring the alternative there
— on the grounds that a run should keep one verification standard — would
refuse to continue over a reviewer that may never have judged anything,
and the per-attempt record already names who judged each attempt, so the
ledger stays honest either way. A loud warning beats a dead run.

## `let tier = analysis`

Now say WHICH tasks. Resolution could not: a shipped binary
always has the Copilot adapter, so the only way the rebind
actually goes missing is right here, and naming the tasks is
the difference between a note and something actionable.

## `if !analysis.gates.is_empty() {`

Effective gates come from the shared analysis (single derivation point
with `validate`), or from the record above. §14 pre-flight: the shell and
every gate command must resolve before any agent tokens are spent — and
on a resume that check runs against the *recorded* gates, so a machine
that cannot run what this run verifies against says so plainly instead of
quietly proceeding.

Per gate rather than per config: a recorded gate carries the shell it ran
under, and nothing requires every gate in a list to share one.

## `let budgets = effective_budgets(analysis.config.budgets, opts.budget_usd)?;`

Here, with the other pre-flight refusals, rather than where the ceiling
is first read: `--budget 0` must not create a branch and a run directory
before discovering it cannot spend anything (§14 — pre-flight refuses
before any agent token is spent, and before the workspace is touched).

## `fn effective_budgets(`

`[budgets]` with `--budget` folded in.

The flag overrides `run_usd` only. `task_usd` has no flag because a
per-task ceiling is a property of how the plan is shaped, not of one
invocation — and a single `--budget` that quietly moved both would be
impossible to reason about at the ledger afterwards.

## `if let Some(limit) = flag {`

Validated through the same check `[budgets]` uses. A flag that overrides
a validated key must not be a way around the validation: `--budget 0` and
`--budget -5` both stop the run before it spends anything, and
`--budget nan` silently never fires at all — three different broken
behaviours behind one mistyped number, where the config key refuses all
three at load.

## `pub(super) fn repo_relative(repo_root: &std::path::Path, path: &std::path::Path) -> String {`

A path as the run record should carry it: relative to the repo root where
possible, so the record survives the repository being moved or cloned
somewhere else before a resume.

## `pub(super) fn chain_summaries(analysis: &Analysis) -> Vec<ChainSummary> {`

The resolved chain per task, as it stood at this moment.

## `fn restore_recorded_routing(`

Validate the rung index space and restore the exact bindings the run began
with. Structural changes still refuse: an existing `Progress.rung` cannot be
interpreted against a different tier list. Binding-only changes warn and
continue with the snapshot, matching gates and effort.

## `fn gate_summaries(analysis: &Analysis) -> Vec<GateSummary> {`

The effective gates, in full, as they stood at this moment.

## `pub(super) fn gates_differ(recorded: &[GateSummary], now: &[GateSummary]) -> Option<String> {`

What today's config would gate with, against what the run recorded — `None`
when they agree.

This is a **warning**, not a refusal: the run continues under the gates it
recorded, and the operator's edit simply does not apply to it. Saying so is
still worth a line, because an edit that silently does nothing is how
somebody concludes the gate is broken.

Matching is by whole gate, then paired up by name, which is what makes the
message survive the shapes a by-name lookup got wrong: duplicate names are
legal in `[[gates]]`, so `find`-by-name silently answers for the wrong entry
— reporting an edit nobody made, or finding every name present and claiming
a reorder when a gate was added.

## `pub(super) fn gates_differ(recorded: &[GateSummary], now: &[GateSummary]) -> Option<String> {` › `let mut unmatched: Vec<&GateSummary> = now.iter().collect();`

Whole-gate multiset difference: what the record has that today lacks, and
the reverse. Anything appearing in both cancels, however many times.

## `pub(super) fn gates_differ(recorded: &[GateSummary], now: &[GateSummary]) -> Option<String> {` › `return Some(`

Same gates, listed in a different order. Worth a line — the record is
what runs, and the order it runs in decides which failure a task sees
first — but not the same claim as a changed command.

## `pub(super) fn gates_differ(recorded: &[GateSummary], now: &[GateSummary]) -> Option<String> {` › `let once = |gates: &[&GateSummary], name: &str| {`

A name in exactly one dropped and one added gate is one gate edited, not
one removed and an unrelated one added. Only when it is unambiguous:
with duplicates, "which `check` became which" has no answer worth
guessing at, so both sides are reported plainly instead.

## `fn changes_between(recorded: &GateSummary, now: &GateSummary) -> String {`

How one gate's recorded form and its form in today's config differ.
