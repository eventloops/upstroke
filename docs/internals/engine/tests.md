# `src/engine/tests.rs`

Extended notes for [`src/engine/tests.rs`](../../../src/engine/tests.rs).

## `#![allow(clippy::disallowed_methods, clippy::disallowed_types)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `EditFile,`

Simulates an agent that edits the workspace and succeeds.

## `EditTest,`

Simulates an agent that writes real test code.

## `LargeEdit,`

Produces a complete diff larger than the review input boundary.

## `OpaqueEdit,`

Produces a binary patch whose changed bytes cannot be semantically
reviewed from the unified diff.

## `IgnoredGateInput,`

Adds an ignored, uncommitted input that a gate could observe in the
worker tree but that is absent from the staged review candidate.

## `FrozenCandidate,`

Lets the test mutate the authoritative index after capture and
records which immutable tree the reviewer actually received.

## `JamCleanupAfterReview,`

After the reviewer workspace exists, plants a stale lock in the
common git directory so the later authoritative cleanup fails.

## `LargeEditQuestionWriteFailure,`

Produces the same oversized diff, then makes the question payload
directory unwritable so parking preparation fails deterministically.

## `NoEdit,`

Simulates a lying agent: success report, no changes.

## `SpawnError,`

Command construction succeeds, but the worker executable cannot be
spawned — the shape of an agent CLI removed, renamed, or self-updated
out from under a run that has already passed pre-flight.

## `Error,`

Simulates an agent-side failure.

## `RateLimited,`

Simulates the pool being exhausted.

## `AskQuestion,`

Edits, then stops and asks the operator a question (§12).

## `Exit,`

Kills the whole process partway through the attempt, leaving the
on-disk shape a `kill -9` or a power loss leaves: a dirty working
tree and an `attempt_started` with no `attempt_finished`.

## `const CRASH_EXIT_CODE: i32 = 42;`

Distinctive so the parent can tell a deliberate death from a panic,
which would also exit non-zero.

## `Unparseable,`

Prose with no verdict block: drives the re-ask path.

## `RateLimited,`

The judge itself could not run.

## `SpawnError,`

Command construction succeeds, but the reviewer executable cannot
be spawned.

## `NeedsHuman,`

§12: the reviewer declines to judge and asks for a person.

## `struct FakeAdapter {`

Scripted stand-in for a real CLI. `build` performs the "agent edit"
directly (test-only shortcut) and returns a trivial command; `parse`
reports the scripted outcome. Read-only profiles are review
invocations and answer with a verdict, exercising the real
command → stdout → parse → verdict path.

Both scripts are consumed per invocation and the final entry repeats,
so a one-element script behaves exactly like the fixed adapter did.

## `id: &'static str,`

Which agent this stands in for. Cross-vendor tests (§11.3) need two
ids, because "a different model family" is unreachable otherwise.

## `probe_error: Option<&'static str>,`

Simulates a CLI that is installed but broken, for the pre-flight
probe classes: required agents refuse the run, the opportunistic
cross-family one only warns.

## `reports_cost: bool,`

Whether this route reports spend. Copilot's does not.

## `const REVIEW_MARKER: &str = "UPSTROKE-FAKE-REVIEW";`

Marker the fake's review command prints so `parse` can tell a review
invocation from an implementation one.

## `fn copilot(reviews: Vec<ReviewBehavior>) -> Self {`

The second vendor. It only ever reviews in these tests, so it needs
no effects script.

## `fn unpriced(mut self) -> Self {`

Stands in for the Copilot route, which has no JSON envelope and so
reports no spend at all (§13).

## `fn reviews_run(&self) -> usize {`

How many review invocations this adapter was asked for.

## `fn cross_vendor(`

A machine with both CLIs installed: claude-code implements and gives the
acceptance verdict, copilot gives the §11.3 second opinion. Each adapter
keeps its own review script and counter, so a test can say what each
vendor answered and check which of them was asked at all.

## `fn scripted<T: Copy>(script: &[T], index: usize, fallback: T) -> T {`

The last scripted entry repeats forever.

## `return Ok(shell_spec(&format!("echo {REVIEW_MARKER}")));`

No `current_dir`: the runner puts the process in
`RunnerRequest.workspace`, which is `run.workspace`.

## `let _ = fs::write(`

Half-finished edits first, then die without
unwinding — no destructors, no flush of anything the
engine has not already synced. That is what makes
this a faithful stand-in for a kill rather than a
tidy shutdown, and it happens at a deterministic
point instead of racing a signal.

## `return Ok(CommandSpec::new(`

Return before editing anything: this attempt never gets to
run, so it must not leave the workspace looking as though it
did.

## `fn materialize_permissions(`

Delegate to the real generator so the engine's permission wiring is
exercised, not stubbed out.

## `let index = self`

`build` already consumed this invocation's slot.

## `Effect::EditFile`

`Exit` never reaches here — `build` ends the process.

## `| Effect::SpawnError`

`SpawnError` never reaches here either: the runner fails to
spawn it, so nothing is parsed.

## `copilot: Option<FakeAdapter>,`

`None` is the single-vendor machine — which is also the shape that
makes a cross-family reviewer unresolvable.

## `struct ScriptedAnswers {`

Answers handed out in order; anything past the script is unanswered,
which is exactly how a detached terminal behaves.

## `fn shell_spec(script: &str) -> CommandSpec {`

Shared with the production path so tests exercise the same shell
invocation (including its Windows quoting) rather than a parallel one.

## `fn seed(repo: &Path, plan: &str, config: Option<&str>) {`

Replace the plan and config, then commit so the tree is clean.

## `opts.defer_backoff = Duration::ZERO;`

Tests must never actually wait — not out a rate limit, and not at a
hard block either. The test harness has no terminal, so an
interactive mode resolves to the waiting answer channel; without a
zero budget every parking test would sit out the real one.

## `fn no_pools() -> PathBuf {`

An explicit pools path with no pools in it.

A real, empty file rather than an absent one: an explicit `--pools` that
does not exist is a hard error now, and `None` would reach for the
operator's real `~/.upstroke/pools.toml` — which no test may touch.
An empty pools file, created once for the whole test process.

Every test routes through here, and this used to *rewrite* the file on
each call — one shared path truncated and rewritten while other threads
were reading it. The content is the same for every caller, so there is
nothing to rewrite: build it once and hand back the path.

## `fn private_root_for(repo: &Path) -> PathBuf {`

A scratch stand-in for `~/.upstroke`, so tests never touch the real one.

A *sibling* of the repo, never a directory inside it. That is not
tidiness: §14's rollback is `git clean -fd`, which deletes untracked
directories — a private root inside the workspace would have its
transcripts and verdicts destroyed by the first failed attempt. The
same reasoning is why production puts it under the user's home.

## `fn resume_options(repo: &Path, run_id: &str) -> ResumeOptions {`

Resume options matching [`options`], for the same reasons.

## `fn paths_of(repo: &Path, run_id: &str) -> RunPaths {`

The paths a test's run wrote to.

## `#[derive(Default)]`

---- a returned legacy append error ------------------------------------
Fails the **third** legacy append of a live run, by returning an error at
`Event.LegacyAppend`'s `Written` point.

The third, and the number is load-bearing rather than arbitrary:
`run_harness_inner_on` emits `run_started` and then the capacity snapshot —
two appends — *before* `drain_and_report` is called at all, so a fault at
either tests the startup path and never reaches the branch both findings are
about. The third is the first append inside `drain()`, which is where
`production_effect`'s "it reports and stops" and the partial report live.
`a_returned_legacy_append_error_still_leaves_the_partial_report` checks that
the two startup appends really did land, so if this number ever stops being
the right one it fails loudly instead of passing for the wrong reason.

## `#[test]`

A returned append error **stops the run** — it is not swallowed and carried
on from (`PR5-CONF-010`).

`production_effect` says "the legacy engine's handling of a returned append
error is unchanged — **it reports and stops**". The shipped code did;
nothing required it to. Replacing `Run::emit`'s `?` with an arm that pushed
a warning and returned `Ok` **survived the whole suite**: every append
failure the suite injected targeted an `EventLog` a test had built directly,
and `emit` reached `EventLog::append`, which hard-codes `NoEventHooks`, so
no fixture could make a **live `Run`**'s append fail at all.

The two axes this crosses are *whose* `EventLog` fails and *what observes
the failure*. `src/events/log/tests.rs` holds the first constant at "a log
the test owns" and varies the second exhaustively; the census beside those
tests reads the coordinator's source and can see that the branch returns,
but not that the error ever gets to it. What varies here is the log: it is
the live run's own, reached through `engine::run_with`, and the assertion is
on the value the *caller* receives.

## `#[test]`

…and the partial report is written beside the log on the way out
(`PR5-CONF-011`).

Deleting `drain_and_report`'s partial `finish()` and `rundir::write_report`
survived the whole suite, for the same reason and one branch further on.

**This is the legacy path and the repair must not generalize.**
`coordinator_integration.append_error_protocol` forbids exactly the
opposite for schema-4 — "no report, status, question payload, or cleanup is
derived from the poisoned fold" and "still performs no retry, **report from
memory**, cleanup, or fold mutation". So the assertion below is on the
legacy `report.json` only, and nothing here is asserted of the topology
coordinator.

Held constant with the sibling above: the same fault, at the same append, in
the same fixture. What varies is what is examined — the caller's return
value there, the run directory here.

## `let runs = opts.repo_root.join(".upstroke").join("runs");`

The run directory the failed run created, found by its own log rather
than by a run id the caller never received.

## `let log = fs::read_to_string(public.join("events.jsonl")).expect("the log");`

The premise: the fault fired *inside* `drain()`, not during startup.
`run_started` and the capacity snapshot are appends 1 and 2 and both must
have landed whole; the third is the one that failed, and its Written
error-return arm leaves a torn prefix rather than a complete line.

## `#[test]`

---- step 1-6 behaviour, unchanged by the ladder ----------------------

## `assert!(`

Per task: implementer 0.01 + reviewer 0.05 (§11.2 reviews every
attempt), so both spends are accounted for.

## `let paths = paths_of(&repo, &report.run_id);`

Rewind to the exact atomic settlement, then add dirty residue to
model death before ordinary post-attempt cleanup. Replay must retain
both the paid ledger line and the question, discard the residue, and
never dispatch another worker for the known-oversized identity.

## `Some("[routing]\ntest = { chain = [\"small\"], attempts_per = 1 }\n"),`

One rung, one attempt: the provenance failure is what is under
test, not the ladder's reaction to it.

## `seed(`

A gate that creates a file: residue must never reach a commit nor
survive the task.

## `assert!(`

§15 split: the file describing an agent's own sandbox is not
somewhere that agent can read.

## `let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);`

A reviewer that would REJECT everything: if review still ran, the
task would never commit.

## `const FRONTIER_AUTH_PLAN: &str = "## Rotate the signing key\n\`

---- step 9: cross-vendor review (§11.3) -----------------------------
A plan whose one task runs at frontier and touches `src/auth/**`, so
both step-9 mechanisms are in play: its implementer binds to the same
model as the reviewer, and its paths can match a `second_opinion`
override.

## `const FRONTIER_ONLY_CONFIG: &str =`

Same task, no override — the implicit anti-self-review path.

## `let repo = temp_engine_repo("secondopinion");`

§11.3: both verdicts must pass. And the primary must NOT rebind here
even though it matches the implementer — rebinding would resolve both
passes to copilot/gpt-5.3-codex and drop the Anthropic review entirely, which
is worse than the self-review the rebind exists to prevent.

## `assert_eq!(t1.review_cost_usd, Some(0.10), "0.05 per pass");`

Both reviewers' spend lands in the review column, not the worker's.

## `let repo = temp_engine_repo("secondopinionfail");`

The point of two passes: the one that says no decides, even when the
first already approved.

## `let repo = temp_engine_repo("shortcircuit");`

Passes short-circuit like gates do (§11.1): once one has said no, a
second opinion on the same diff changes nothing and costs a frontier
invocation to learn it.

## `let repo = temp_engine_repo("selfreview");`

The item carried since step 6: both binders resolve `frontier`
identically, so without the rebind the reviewer IS the implementer.

## `let source = cross_vendor(`

The claude adapter's review script says FAIL and the copilot one says
PASS, so a committed task proves which of them was actually asked.

## `let repo = temp_engine_repo("noneedtorebind");`

A mid-tier implementer judged by the frontier reviewer is already a
genuine second look, so nothing rebinds. Triggering on family
similarity instead of exact identity would send most of a run
cross-vendor for no verification gain.

## `let repo = temp_engine_repo("nosecondfamily");`

Step-6 finding #10's posture: the operator asked for two model
families on their blast-radius paths. Quietly giving them one is the
failure that finding exists to prevent, so this refuses instead.

## `let repo = temp_engine_repo("selfreviewwarn");`

The implicit rebind is upstroke's own idea, not the operator's, so a
single-vendor machine loses the upgrade rather than the run — but it
is told, because a verification property that quietly is not there is
exactly what step 6 objected to.

## `let repo = temp_engine_repo("brokencopilot");`

Installed but broken is different from absent, and the two probe
classes have to agree about which is which: the opportunistic
reviewer only warns.

## `assert!(`

And it names the tasks. Resolution cannot reach this warning — a
shipped binary always has the Copilot adapter, so the only way the
rebind really goes missing is a probe failure, and a warning that
never fires for a real user is not a warning.

## `let repo = temp_engine_repo("brokenrequired");`

Same machine, same breakage — but now a `second_opinion` names it, so
it is load-bearing rather than opportunistic.

## `let repo = temp_engine_repo("resumereviewers");`

Who judged this run is a fact about the run, not about today's
machine — step-8 finding #8's lesson on `private_dir`. Re-deriving it
would let a CLI installed since the run began become the judge for
the back half, leaving one run with two verification standards.

The work left over has to be work the rebind would OTHERWISE claim,
or this proves nothing: the task resumed onto is at frontier, where
the implementer and the reviewer are the same model.

## `let source = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);`

First process: no copilot on the machine, and the agent changes
nothing — so t1 exhausts its chain and parks, still unbuilt.

## `let later = cross_vendor(`

Second process: copilot has appeared since. The record still rules,
so the retry is judged by the model the run started with.

## `let resumed = resume_answering(&repo, &run_id, Effect::EditFile);`

`resume_answering` exposes only the Claude fake. If pre-flight probes
today's Copilot pin before restoring the record, this refuses before
the behavioral assertions below can run.

## `let repo = temp_engine_repo("maxparallelrefusal");`

The config refusal is only worth having if it lands before the run has
done anything an operator must undo. Pre-flight loads the config ahead of
the run id, the run directory, the run lock, and the branch — so a
ceiling this engine cannot honour leaves the repository exactly as it
was, rather than a husk under `.upstroke/runs` that `latest_run` then
reports on in place of the real one.

## `fn worktree_lock_path(repo: &Path) -> PathBuf {`

Where this repository's worktree lease file would be, held or not.

## `let repo = temp_engine_repo("ceilingbeforelease");`

The ordering claim itself, tested where cleanup cannot fake it.

A run that took the lease, *then* read the config, then tidied up on its
way out would leave a repository indistinguishable from this one — so an
end-state assertion proves nothing about when the config was read. These
two do:

 (a) The lock file is created by acquisition and is never removed by
     release, by design (a killed engine must leave nothing to clear by
     hand, and the OS releases the hold; the file stays). Its absence is
     therefore proof that acquisition never happened, not proof that it
     was undone.

 (b) With a competing holder already on the lease, an acquisition-first
     order cannot produce a config error at all: the acquisition is what
     fails, and the operator is told another process owns the worktree —
     the wrong diagnosis for a file they can fix in five seconds. Move
     `validate_inputs` back below `WorktreeLock::acquire_in` and this
     half fails no matter what any cleanup does afterwards.

## `let source = fake(Effect::EditFile);`

(a) Uncontended: the refusal must not have created the lease file.

## `let competitor = WorktreeLock::acquire(&repo).expect("a competing holder takes the lease");`

(b) Contended: the config error must still be the one that comes back.

## `let (repo, run_id) = parked_run("resumeceilingbeforelocks");`

The same claim for the other write command, which takes two locks rather
than one. `max_per_agent = 0` is the ceiling to test a resume's ordering
with: the legacy reading below softens `max_parallel > 1` for a run that
is already sequential, but a limit with no meaning at all is refused for
fresh runs and resumes alike, so this refusal is genuinely about *when*.

## `let lease = worktree_lock_path(&repo);`

The fixture run created both lock files. Remove them, so that a file
found afterwards is evidence about this resume rather than history.

## `let competitor = WorktreeLock::acquire(&repo).expect("a competing holder takes the lease");`

And with the lease already held, so that an acquisition-first order
could only ever answer with contention.

## `fn config_with_repairs(repairs: u32) -> String {`

A config with a distinguishable, harmless ceiling in it.

`max_merge_repairs` is the right knob for the tests below: it is kept
verbatim by every reading, it loads without refusing, and its value is
visible on the `Analysis` — so "which bytes produced this analysis" has a
direct answer rather than an inferred one.

## `let repo = temp_engine_repo("confirmunderlease");`

The pre-lock check answers "may this start", from files the worktree did
not yet belong to this run. Adopting *that* analysis afterwards would
execute an answer about bytes that no longer exist, so what the lease
holder adopts is an analysis it captured and validated itself — on the
condition that the two captures agree about what it was reading.

## `fs::write(&config, config_with_repairs(5)).expect("the config before the lease");`

(a) A change that is still there at the lease is adopted, not papered
    over with the pre-lock reading. Nothing here is refused: the point is
    that a stale-adopting implementation returns 5 and a re-validating
    one returns 7, and only one of those is the config the run will hold.

## `fs::write(&config, config_with_repairs(5)).expect("the config before the lease");`

(b) And if the change is one this engine must refuse, it refuses — under
    the lease rather than never. A run whose config was checked in an
    earlier life and replaced since is a run executing something nothing
    ever validated, which is the whole defect.

## `fs::write(&config, config_with_repairs(5)).expect("A");`

(c) The A-to-B-to-A interleaving, end to end. The excursion is invisible
    to both captures — which is precisely why the analysis may not come
    from a read taken beside them. It comes from the capture itself, so
    what is adopted is A whether or not B ever existed.

## `let repo = temp_engine_repo("gatesunderlease");`

The one input `analyze` still reads from the filesystem rather than out
of the capture: `gates::derive` is handed a directory. So the derivation
has to happen where the worktree is this run's — which means the adopted
analysis cannot be the pre-lock one, and the files it looks at have to be
in the captured set so that a change to them is a change this confirmation
notices.

## `fs::write(repo.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("a rust repo now");`

The repo becomes a Rust repo between the check and the lease.

## `fn volatile_strings(repo: &Path, run_id: &str) -> Vec<String> {`

What two runs of one fixture can never share: their run id, and the two
absolute paths their identity is built out of.

## `for path in [private_root_for(repo), repo.to_path_buf()] {`

Longest first, so a path that contains another is replaced whole.

## `fn replace_exact_runs(`

Replace every maximal run of `member` that is exactly `len` long.

Length-exact and maximal so that content-derived digests keep their meaning:
a 16-character plan hash and a 64-character normalized-plan digest are facts
two identical fixtures must agree on, and only the 40-character Git object
names and the 26-character ULIDs are unshareable.

## `fn canonicalize_json(value: &mut serde_json::Value, volatile: &[String]) {`

One JSON value with everything two runs of one fixture legitimately differ
by replaced by a token, in place.

## `if matches!(key.as_str(), "ts" | "duration_ms" | "duration") {`

Wall-clock, not meaning. Everything else — cost, usage,
effort, tier, model, reviewer, session, rung — is compared.

## `fn canonical_trace(events: &[events::Event], repo: &Path, run_id: &str) -> Vec<String> {`

One run's log as a semantic trace two runs of the same fixture can be
compared by.

Whole event bodies, not kinds. A config key that changed which reviewer
judged the retry, what effort it ran at, how long a review was allowed, or
which rung the ladder resumed on would leave the sequence of event kinds and
the run's outcome untouched — so comparing those alone would pass on exactly
the reinterpretation this exists to rule out.

## `fn canonical_projection(report: &RunReport, repo: &Path, run_id: &str) -> String {`

The run's report and every task record in it, canonicalized the same way.

`warnings` is dropped rather than compared: saying what today's config
contains is the one thing the two arms are *supposed* to differ by, and the
assertions about it are separate and explicit.

## `const LEGACY_RESUME_LIMITS: &str = "\n[engine]\nmax_parallel = 2\nmax_merge_repairs = 7\n\`

All four §17 ceilings, written the way an operator waiting for the parallel
engine would write them — `max_parallel` included, and above 1.

## `const LEGACY_RESUME_NO_LIMITS: &str = "\n# no [engine] ceilings in this arm\n";`

What the control arm appends instead.

An edit rather than nothing: §14 rolls an interrupted run's uncommitted
paths back, so an arm that left `upstroke.toml` untouched would record no
discard while the other recorded one — a difference about which fixture
edited a file, not about what the ceilings did. Both arms edit it; only one
says anything.

## `#[derive(Clone, Copy)]`

Which resume shape a legacy-limits fixture exercises.

## `Parked,`

The ordinary case: a parked run answered and carried to the end.

## `InterruptedAttempt,`

A crash prefix: the log ends inside an attempt that never settled, so
the resume has to record the interrupted settlement, refund the rung,
and retry before it can finish. This is where an unacted-on ceiling
would be most tempting to act on, because it is the only path that
re-decides how much of the ladder is left.

## `struct LegacyArm {`

One arm of a legacy-limits comparison: everything two resumes of the same
fixture must agree about, plus the warnings they are allowed to differ by.

## `trace: Vec<String>,`

Every event, whole, with the tokens two runs cannot share replaced.

## `projection: String,`

The report and its task records, canonicalized the same way.

## `tree: String,`

The tree the run committed. Content-addressed, so it is directly
comparable across two repositories whose commits can never share a sha.

## `fn legacy_resume_pair(tag: &str, fixture: LegacyFixture) -> Vec<LegacyArm> {`

Run one legacy fixture twice — once with the four ceilings in today's
config, once without — and hand back what each resume did.

## `for (fixture, tag) in [`

The keys are new; the run is not. A resume that reads all four — the one
a fresh run refuses among them — must continue exactly as it would have
without them, and say so rather than act on them.

Proved against a control resume of the identical fixture rather than by
reading the resume path: "the engine ignores these fields" is a claim
about every line of that path, and only a comparison covers all of them.

And compared *semantically*, not by event kinds and an outcome. A ceiling
that changed which reviewer judged the retry, what effort it ran at, how
long the review was allowed, which rung the ladder resumed on, or what
the attempt cost would leave the sequence of event kinds and the final
outcome identical — so a comparison that could not see those would pass
on exactly the reinterpretation it exists to rule out. Three comparisons
together close that: every event body, the report and its task records,
and the tree the run actually committed.

## `let mut open = 0i32;`

Sequential all the way through, stated as a property of the log
rather than of the outcome: one attempt open at a time is what
`max_parallel = 2` would have changed if anything acted on it.

## `EventBody::AttemptFinished { .. } | EventBody::AttemptInterrupted { .. } => {`

Both settlements close one: a crashed attempt's recovery
record is as much an end as a finished one.

## `for key in [`

Every one of the four is named, and only where it was written.

## `let repo = temp_engine_repo("oldlogresume");`

`RunStarted.reviews` is #[serde(default)] so a step-8 log still
parses — but the default is an EMPTY plan, which every later reader
cannot tell apart from `review = { enabled = false }`.

## `let paths = paths_of(&repo, &first.run_id);`

Rewrite run_started as a pre-step-9 process would have written it.

## `let later = source(vec![Effect::EditFile], vec![ReviewBehavior::Fail]);`

A reviewer that rejects everything: if review still runs, nothing can
commit. If the absent field read as "review disabled", it commits —
verification gone without a word, which is step-6 finding #10.

## `let repo = temp_engine_repo("outagerecord");`

Step-6 finding #8's distinction, carried into the ledger: a judge
that never ran said nothing about the code, and recording it as a
plain "did not pass" puts a rejection against a model that never read
the diff.

## `assert!(committed(&report, "t1"), "{report:?}");`

And the ladder treated it as an outage: deferred, then committed.

## `let repo = temp_engine_repo("partialcost");`

The Copilot route bills nothing back (§13), so a two-pass review
shows one reviewer's spend. Presenting that as the total is exactly
what `render_ledger` says is worse than no ledger at all.

## `let repo = temp_engine_repo("reviewtrail");`

An escalated task can be judged on one rung by one model and on the
next by another. `review_cost_usd` sums every attempt, so a list
scoped to the final attempt would read as though it explained a total
it does not cover.

## `let source = cross_vendor(`

Mid fails review, escalates to frontier, which passes. The frontier
rung is self-review, so its pass rebinds to the other family.

## `let repo = temp_engine_repo("passtranscripts");`

Two reviewers, two records. The acceptance pass keeps the bare name
it has had since step 6, so a run directory reads the same way
whether or not a second opinion was configured.

## `assert!(`

A clean pass still passes.

## `let failure = review_failure(review::ReviewResult::Unavailable {`

A rate-limited or hung judge must not read as "your code is wrong",
or the ladder retries the implementer for an outage.

## `let prompt = materialize_prompt(`

Missing input: say so plainly rather than pointing at nothing.

## `fs::write(`

Present input: content is inlined.

## `#[test]`

---- step 7: the ladder in the engine ---------------------------------

## `let repo = temp_engine_repo("resume");`

§21 definition-of-done (b). The gate demands a file only the second
attempt writes, so recovery is real rather than scripted around.

## `let files = git_in(&repo, &["show", "--name-only", "--format=", "HEAD"]);`

§14: a resumed retry keeps the tree, so the commit carries BOTH
attempts' work rather than only the last one's.

## `let repo = temp_engine_repo("escalate");`

§21 definition-of-done (c).

## `assert_eq!(runs[0].model, "claude-haiku-4-5");`

The adapter's own record, not just the report echoing what the
engine intended: the second attempt really was dispatched to the
higher rung's model.

## `let repo = temp_engine_repo("park");`

§21 definition-of-done (d) and invariant 6: t1 exhausts its chain
and parks; the independent t3 must still commit.

## `let source = source(`

t1 fails; every later attempt (t3's) edits and passes.

## `let path = repo`

The question is on disk where a notifier, `upstroke answer`, or a UI
can read it — that file is the contract, not the terminal output.

## `Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),`

A single attempt on a single rung: if the rate limit spent it,
the task could never commit.

## `assert!(committed(&report, "t3"));`

Independent work ran while t1 waited, and the answer resumed it.

## `assert!(`

Parking rolled the tree back, so the session's account of what it
wrote no longer matches the repository (§14 pairs resume with tree
retention). The retry therefore starts fresh and carries the whole
task again, with the operator's answer as an instruction.

## `let runs = source.adapter.runs();`

Invocation order across the whole run, not just this task: t1 asks
(0), the independent t3 proceeds while t1 is parked (1), then t1
retries once the answer arrives (2). That interleaving is the point
of invariant 6, so the retry is the third invocation.

## `let repo = temp_engine_repo("ci");`

§12: `interaction = "never"` degrades questions to parked-task
reporting, and the outcome is distinguishable from both a clean run
and a halt.

## `let repo = temp_engine_repo("noloop");`

Without this the hard block spins: ask, get nothing, ask again.

## `let reviews = paths_of(&repo, &report.run_id).reviews();`

The re-ask actually happened, and both sides are on record.

## `fn raw_object_after(line: &str, key: &str) -> Option<String> {`

The raw text of the JSON object that follows `key` in `line`.

Byte-exact and order-preserving, which is the whole point: a `serde_json::Value`
round-trip re-emits an object with its keys sorted, and a claim about *what the
log holds* cannot be made from a value that has been through one. Tracks string
state so a brace inside a gate's error text does not end the object early.

## `#[test]`

**The legacy record did not change when schema 4's did.**

`FailureRecord::detail` exists for schema 4, whose settlement transitions have no
feedback field. The record builder is shared with `coordinator.rs`, and the first
version of the repair copied the value in unconditionally — so the **live**
`upstroke run` path began writing an 8-KiB gate tail onto the legacy wire and into
`report.json`, once per failed attempt, duplicating the `ladder_retry` copy that
already held it. The 2026-08-26 re-review of `c2c0294` found it as finding A.
`classify::FeedbackCarrier` is the repair; this is what holds it.

**The fixture is real, and it is compared byte for byte.** `PRE_CHANGE_FAILURE` below
is the exact `"failure"` object `610106b` — the commit before the field existed — wrote
for this scenario, captured by running it there. The earlier version of this test
quoted it *elided* in prose and then asserted three key names and
`reason.starts_with(...)`, so a changed reason suffix still passed. The round-3 review
of `bf927f3` said so, and it was right: the body and the decision record both claimed a
byte comparison this test did not make.

**One
residual difference that is stated rather than hidden**: `detail` serializes as an
explicit null, because `skip_serializing_if` is not available here. Schema 4's strict
door decides "unknown field" by asking the record which keys it claims back, so a
field that decodes from `"detail":null` and re-encodes to nothing makes the door
refuse every failed attempt's settlement — measured, and held by
`an_explicit_null_detail_survives_the_strict_door` above.

So the claim is exact: **strip `,"detail":null` and the bytes are the pre-change
bytes.** Not "similar", and not asserted by rebuilding the expected value from the
observed one — the strip must fire, and what is left must match key for key.

## `const PRE_CHANGE_FAILURE: &str = concat!(`

The exact bytes `610106b` wrote for this scenario's failed attempt. Captured by
building a worktree at that commit and running the same gate; not derived from
anything this build produces.

## `let bytes =`

**The log's own bytes, sliced out of the line.** Not
`serde_json::to_string(failure)`: a `Value` round-trip sorts keys, so
the field order the comparison is about would be the map's rather than
the struct's, and the pre-change fixture's `,"detail":null` suffix
would never appear where it actually appears.

## `assert_eq!(`

**Byte for byte against the captured fixture.** Not a key-set check and not a
`starts_with`: every byte of `kind`, `origin` and `reason` must be what
`610106b` wrote, so a changed reason — a different gate name, a reworded
prefix, an extra suffix — fails here instead of passing a prefix predicate.

## `let object = failure.as_object().expect("the failure is an object");`

The point of the whole finding: the tail is not on the record. Asserted as an
**explicit null and not merely a falsy read** — `failure["detail"].is_null()`
answers true for an absent key too, so it could not tell "the field is here
and empty" from "the field is gone", which are different wires.

## `if event`

And it is still delivered — by the carrier that owns it here.

## `let report: serde_json::Value = serde_json::from_str(`

`report.json` is the other reader of these records, and the reason
`LadderRetry` holds the text instead: it "should not grow one per attempt".

## `let runs = source.adapter.runs();`

The retry was still told. Delivery is the whole reason the field exists on the
other engine, and removing it here must not cost the legacy one anything.

## `#[test]`

**The strict door's precondition, checked over the record that gained a field.**

Schema 4 refuses an unknown field in a transaction payload (`refusals[24]`) by
decoding a record, re-encoding it, and reporting any key the input carried that
the record did not claim back. That is exact **only** while every embedded record
serializes each field it deserializes — no `skip_serializing_if` — and
`topology::events::strict`'s own documentation says so.

`events::tests::a_known_null_survives_the_strict_door_and_an_unknown_null_does_not`
checks that precondition, and checks it over `AttemptRecord`'s own optional fields:
its fixture has `failure: None`, so **no `FailureRecord` appears in the payload at
all**. Adding `skip_serializing_if` to `FailureRecord::detail` therefore leaves that
test green while breaking the door for every failed attempt — measured 2026-08-26,
which is why `decisions/2026-08-26-durable-retry-feedback.md`'s argument for a plain
`#[serde(default)]` needed a witness rather than a paragraph.

This is that witness, one record deeper. It lives here because
`src/topology/events.rs` is frozen and this needs no change to it.

## `failure.insert("detail".to_owned(), serde_json::Value::Null);`

Explicit, not absent: an absent key is the older-log case and is covered by
`a_log_predating_the_detail_field_folds_and_resumes`. What is under test here
is the key being *present* and null, which is what this build writes.

## `#[test]`

**Both of §11.4's feedback sources reach the durable record.**

§11.4 names exactly two: "failure feedback (gate log or `required_changes`)
goes back to the *same rung*". `AttemptFailure::feedback` unifies them —
`classify::gate_failure` puts the 8-KiB tail there, and
`attempt::review_failure` puts the reviewer's required changes there — and
`classify::attempt_record` is the one production construction that copies
the pair onto the wire.

It copied `{kind, origin, reason}` and dropped the feedback. `reason` is the
human-facing summary, so a resumed run could say *that* a gate failed and
nothing about **what it printed**: the retry ran on attempt 1's prompt and
could repeat the same defect while spending another attempt. The 2026-08-26
frontier review of `75da796` raised it as finding 2 and it is
`PR7-FEEDBACK-NOT-DURABLE-IN-SCHEMA-4`; the field is authorised by
`decisions/2026-08-26-durable-retry-feedback.md`.

Asserted on **content**, not on `is_some()`: the claim is that the next
worker is told what this one was told to fix, and a record carrying the
wrong string satisfies a presence check exactly as well as the right one.

## `fn durable_detail(failure: &crate::ladder::AttemptFailure) -> Option<String> {`

One failure's `detail` as the wire carries it, through the production
builder rather than around it.

## `feedback: super::classify::FeedbackCarrier::AttemptRecord,`

The schema-4 carrier, because that is the path under test: this
helper exists to assert the field is written at all.

## `assert_eq!(`

Multi-line questions survive, because the prompt asks for it last.

## `let reply = "The retry feedback says I can use the UPSTROKE-QUESTION: marker if I am \`

The engine hands the agent this marker in every fresh prompt, and
the empty-diff feedback names it verbatim — so an agent mentioning
it before asking is the expected shape, not a corner case. Taking
the first occurrence would hand the operator the agent's reasoning
with the question buried at the end.

## `let quoting = "I will end with the UPSTROKE-QUESTION: marker if I get stuck.";`

`detail` carries the agent's partial output on every failure path,
and that output routinely quotes the prompt. Reading the marker
before the status would turn a rate limit into a parked question —
silently defeating "RateLimited defers rather than burning an
attempt", and losing the timeout's transcript-tail feedback.

## `let mut asked = fake_outcome(`

A genuine question on a completed run still parks the task.

## `let repo = temp_engine_repo("haltpark");`

t1 parks on a question, t2 fails terminally under the default halt
policy. Asking about t1 afterwards spends the operator's attention
on an answer no attempt can consume, and a decline would relabel
`halted_at` with t1 — sending triage at the wrong task.

## `let source = source(`

t1 asks and parks; t2 changes nothing and parks on chain exhaustion.

## `let answers = ScriptedAnswers::new(vec![`

Declining t1 fails it, which halts the run under the default policy.
The second answer must never be consumed: t2's question cannot be
asked once nothing can act on the reply.

## `assert!(`

The distinguishing assertion. Unguarded, t2's question would be
asked, answered, and flipped back to Pending — where `next_ready`
refuses it because the run has halted, so it would surface as
`Skipped` with the operator's answer sitting unused on disk.

## `fn replay_of(repo: &Path, run_id: &str) -> crate::status::RunStatus {`

---- step 8: the event log is the state ------------------------------
Fold a run's log the way `status` and `resume` do.

## `struct Scenario {`

One path through the ladder, for the live-equals-replay property.

## `plan: Option<&'static str>,`

Overrides the default two-task plan where a scenario needs path
hints or a particular tier.

## `second_opinion: Option<Vec<ReviewBehavior>>,`

`Some` puts a second vendor on the machine (§11.3).

## `fn assert_live_equals_replay(repo: &Path, live: &RunState, report: &RunReport) {`

The property the whole design rests on: a live run and a replay of its
own log are the same computation, not two that happen to agree.

Asserted on `RunState` rather than on the report, because the report is
a lossy projection — it drops `feedback`, `resume_next`, `session`, and
the rung a task is standing on, which are exactly the fields a resume
depends on being right.

## `let strip = |report: &RunReport| {`

Warnings are the one field deliberately excluded. They are
diagnostics of the *process* — what this invocation noticed about a
missing notifier or a discarded working tree — not facts about the
run, so a later reader legitimately has different ones. Anything
that genuinely belongs to the run is an event instead (a discarded
tree, for instance, rides on `run_resumed`).

## `let scenarios = vec![`

One scenario per branch the engine can take, so the equality is
exercised against commits, retries, escalations, deferrals, parks,
answers, and a halt — not just the happy path.

## `Scenario::new(`

§11.3: two review passes per attempt, so `AttemptRecord.reviews`
carries more than one entry through serialize → deserialize. The
list replaced a scalar pair in step 9; this is what proves the
new shape survives the wire.

## `Scenario::new(`

And the same with the second reviewer rejecting, so a `false`
verdict on a non-final pass replays too.

## `Scenario::new(`

The anti-self-review rebind: the acceptance pass runs on a model
no chain rung names, so the record has to carry the binding
rather than let a replay re-derive it.

## `Scenario::new(`

Step 10's two new branches. `budget_exceeded` folds into a
run-level field and `capacity_snapshot` folds into nothing —
opposite shapes, and both have to come back the same on replay.

## `Scenario::new(`

And the ApproveSpend park, whose fold depends on the escalation
having landed *before* the park — the ordering D3 turns on.

## `if cross_vendor_scenario {`

A cross-vendor scenario that quietly resolved to one pass would
still replay identically and prove nothing about the shape this
step introduced. Check the run did what the scenario claims
before trusting the equality below.

## `let repo = temp_engine_repo("abortlog");`

The engine dying between the agent's edits and a verdict is §19's
"engine crash" row. Nothing gets to write a tidy ending, so the log
has to be enough on its own.

## `let source = source(vec![Effect::EditFile], vec![ReviewBehavior::Pass]);`

Run it through, then rewind the log by hand: reproducing a real
abort at exactly this point needs a failure the fake adapter cannot
raise, and the on-disk shape is what this test is actually about.

## `let paths = paths_of(&repo, &report.run_id);`

Now truncate the log to the moment before the attempt reported, the
exact on-disk shape a kill leaves, and confirm it still folds.

## `let nothing: [f64; 0] = [];`

Observed as `total: $-0.0000` in the ledger of a run whose first
attempt was still in flight. This first assertion is the diagnosis,
kept because it is the whole reason `total_of` exists: if std ever
folds from `+0.0`, the helper can go.

## `let spent = vec![`

And the fold change cannot have moved a real total: `+0.0` preserves
every value a cost can be.

## `fn task_report_costing(worker: Option<f64>, review: Option<f64>) -> TaskReport {`

A report carrying nothing but the two cost columns.

## `let repo = temp_engine_repo("livestatus");`

The settlement above, inverted. A run an engine is still driving has
a dangling attempt at every instant, exactly like a killed one — so
settling unconditionally reports a working attempt as a failure and
the whole run as halted. `status` is the only window into a run that
holds its own terminal, and a window that lies is worse than none.

## `let text = fs::read_to_string(paths.events()).expect("log");`

Rewind to mid-attempt: the shape a live engine's log has the whole
time it is working, not only the shape a kill leaves behind.

## `let stopped = replay_of(&repo, &report.run_id);`

With nothing holding the run, that shape still means interrupted —
and `t2` really is blocked, because on an ended run a dependency that
never finished never will.

## `let lock = RunLock::acquire(&paths.public).expect("simulate a live engine");`

Now hold the lock the way a working engine does — through the same
`RunLock` a run takes, not a hand-rolled `flock` on the same path.
Which primitive holds a run is `rundir`'s to decide, and a test that
reaches around it is testing a lock nothing else uses.

## `assert!(out.contains("t2: queued"), "{out}");`

The one the dependency-free pair could not catch: `t2` is waiting on
a task that is working, which is what `Queued` means. Reading that as
`Blocked` tells the operator a dependency failed when it is running.

## `let repo = temp_engine_repo("resumetrunc");`

Decision 3, end to end: the attempt shows up in the ledger, the
rung's allowance does not, and the task completes on the retry.

## `Some("[routing]\nimplement = { chain = [\"small\"], attempts_per = 1 }\n"),`

One attempt on one rung: if the interrupted attempt had been
counted, the task could never commit.

## `let text = fs::read_to_string(paths.events()).expect("log");`

Rewind the record to mid-attempt and put the tree back the way a
dead agent would have left it.

## `assert!(`

The residue is gone and the branch is linear.

## `let repo = temp_engine_repo("crashkill");`

The real thing: a separate process is driven into an attempt and
dies inside it, exactly as `kill -9` or a power cut would.

## `assert!(`

What a kill leaves: a dirty tree and an attempt that never reported.

## `assert!(`

The lock died with the process, so nothing has to be cleared by hand.

## `let rendered = crate::status::render(&before);`

And the summary line says what happened rather than claiming an
outcome. A killed run replays into `Complete` — nothing halted it, no
budget stopped it, nothing is parked — so the ledger used to be
followed by `run complete: 1 task(s) committed` and then, one line
later, `state: interrupted`. Two adjacent lines contradicting each
other about a run that died mid-attempt with work left undone.

## `assert!(rendered.contains("skipped (run interrupted)"), "{rendered}");`

Its unreached tasks were not skipped because the run *halted* — that
is a different ending, and one an operator acts on differently.

## `#[test]`

Spawned by `killing_a_run_mid_attempt_leaves_a_resumable_record`.
Ends its own process on purpose, which is why it must never run as part
of the ordinary suite.

## `let source = source(`

t1 commits; the process dies inside t2's first attempt.

## `std::process::exit(0);`

Only reachable if the adapter never got a second invocation, which
would mean this test is not exercising what it claims to.

## `let repo = temp_engine_repo("answerresume");`

§21's definition-of-done (d) across processes: the run ends parked,
a person answers with `upstroke answer` while nothing is running, and
the resume picks the answer up.

## `let recorded = crate::answer::answer(`

Nothing is running; the answer is written by the CLI path.

## `let runs = source.adapter.runs();`

This adapter is fresh for the resume, so its first invocation is
t1's retry — the one the answer released. t2 runs after it.

## `let repo = temp_engine_repo("midrun");`

Invariant 6 at its most useful: the operator answers from elsewhere
while other work is still going, and the task is released on the
next scheduler turn rather than at the end of the run.

## `let report = run_harness(`

Nobody is reachable through the answer *channel* at all: if the
sweep did not exist, t1 could only ever end parked.

## `struct AnsweringViaFile {`

Stands in for an operator running `upstroke answer` in another terminal
while the run is still going: it writes the file and tells the engine
nobody replied, so only the sweep can find it.

## `let repo = temp_engine_repo("blocked");`

The chain is listed backwards on purpose: a single pass in plan
order would settle `late` before `mid` was known to be blocked, and
report it as merely skipped.

## `let repo = temp_engine_repo("unblock");`

Blocked is a *view*, not recorded state — which is what lets an
answer make a whole chain runnable again on resume.

## `let repo = temp_engine_repo("terminate");`

The drain loop's termination argument, executed: an adapter that
never succeeds, a pool that never returns, and a channel nobody
answers. Every branch of the loop fires and the run still ends.

## `fn resume_err(repo: &Path, run_id: &str) -> String {`

---- step 8: resume refuses rather than guessing -----------------------

## `const PARKED_RUN_CONFIG: &str = "[interaction]\nmode = \"never\"\n\n\`

The base config every parked-run fixture starts from: one rung, one
attempt, no interaction — so a task that cannot pass parks immediately.

## `fn parked_run(tag: &str) -> (PathBuf, String) {`

A run that ends parked — the resumable shape every refusal test starts
from, so each one isolates exactly the thing it breaks.

## `fn parked_run_with_config(tag: &str, config: &str) -> (PathBuf, String) {`

As [`parked_run`], with the config spelled out — for the tests that need
a `[[gates]]` section in the record.

One recipe, not two: the chains check runs before anything gate-related,
so a copy whose `[routing]` line drifted from the original would fail
these tests on "routing has changed" and point at the wrong thing.

## `let (repo, run_id) = parked_run("headmoved");`

§15's HEAD check. Something committed after the run stopped, so the
log no longer describes what is on the branch.

## `let (repo, run_id) = parked_run("chainmoved");`

`Progress.rung` is an index into the chain; re-resolving a different
chain would point it at another tier without saying so.

## `fn parked_run_with_gate(tag: &str, cmd: &str) -> (PathBuf, String) {`

[`parked_run`], with one `[[gates]]` entry — the resumable shape for the
gate tests, which need a recorded gate to diverge from.

## `fn gate_config(cmd: &str) -> String {`

[`PARKED_RUN_CONFIG`] plus a `check` gate running `cmd`.

## `fn resume_answering(repo: &Path, run_id: &str, effect: Effect) -> RunReport {`

Resume and answer the question the parked task is waiting on, so the
task actually runs again and its gates actually execute.

## `let (repo, run_id) = parked_run_with_gate("gaterecorded", "git --version");`

The load-bearing test for the whole gate record, and behavioural
rather than textual: the recorded gate passes, today's config would
fail, and the task commits — which it can only do if the gate that
actually executed came from the log.

This is the self-hosting hazard from the gate-config record, closed
at the point
that matters. The workspace an implementer edits contains the very
upstroke.toml its gates come from, so an edited gate must not become
the standard for what follows. Refusing would also have stopped the
weakened gate running, but it would have stopped the *run* too, and
a legitimately-committed config edit would have left it unresumable.

## `fs::write(`

`git` still resolves at pre-flight, so nothing refuses before the
gate runs — it just exits non-zero when it does.

## `let warning = resumed`

And the operator learns their edit did not take effect here, rather
than concluding the gate is broken when it never ran.

## `assert_eq!(resumed.gates, ["check"]);`

The report describes the gates that ran, not the ones on disk.

## `let (repo, run_id) = parked_run_with_gate("gate_record_lenient", "git --version");`

`design/15`: gates are taken from the record, not re-derived — and not
refused over. Today's file has changed since the run started, and
changed into shapes a fresh run refuses: a zero timeout, a second
entry repeating the first's name, and a key no entry has. The record
decides what runs, so the resume goes through, says what it saw, and
runs the recorded gate — which is the only way `t1` can commit.

This is the composition the config-level tests cannot see: which
reading `resume.rs` chooses from the log, and that the record is what
then executes. Pass 2 on PR #150 showed a parser-only downgrade wrong
in both directions; this test and its twin below are the two
directions.

## `let (repo, run_id) = parked_run_with_gate("gate_record_absent", "git --version");`

The other direction. A log from before the gate record has nothing to
substitute: this resume settles the run's gates from today's file, so
today's file governs, and `timeout_sec = 3600` is a gate that would
really run at the 600 s default — the harm a warning cannot prevent.
So the resume refuses, before any effect, naming the key.

## `let after = fs::read_to_string(paths.events()).expect("the log after");`

Refused before any effect: the log is byte-identical to what it was.

## `fs::write(repo.join("upstroke.toml"), gate_config("git --version")).expect("edit config");`

And with the key spelt right the same log resumes and, having no
record, settles today's gate and runs it.

## `let (repo, run_id) = parked_run_with_gate("gatelabel", "git --version");`

`gates` came from the record but `gates_from_config` did not, so the
run's own report and a later `status` disagreed about the same list:
`finish()` read today's analysis while `RunReport::from_state` read
the record. The doc above `from_state` promises those two cannot
drift, and this is the one field that still let them.

## `fs::write(repo.join("upstroke.toml"), PARKED_RUN_CONFIG).expect("edit config");`

`[[gates]]` deleted, so today's flag would be false and today's
derivation empty — the temp repo has no project marker.

## `let replayed = replay_of(&repo, &run_id).report();`

The other half of the same promise: a reader replaying the log agrees.

## `let (repo, run_id) = parked_run_with_gate("gateunmoved", "git --version");`

The success path, with a non-empty gate list — the direction a false
positive would break. Every other gate test edits the config, so
without this one an over-eager comparison (order, whitespace, a
re-derived timeout) would warn on every ordinary resume unnoticed.

## `let gate = |name: &str, cmd: &str| GateSummary {`

`[[gates]]` does not require unique names, so the obvious by-name
lookup answers for the wrong entry: it reports edits nobody made,
and — worse — finds every name present and concludes "reordered"
when a gate was added. Each case here produced a false sentence
before the comparison paired whole gates instead of names.

## `let added =`

A duplicate name added. The record's `check` is present and unchanged,
so nothing was edited and nothing reordered — one gate appeared.

## `let removed = gates_differ(&[check.clone(), gate("check", "cargo clippy")], only_check)`

One of two same-named gates removed. Pairing by name would report
`check` as edited from one command to the other; both are real
entries and neither changed.

## `let edited = gates_differ(only_check, &[gate("check", "true")]).expect("a difference");`

An unambiguous single-name edit still reads as one edit.

## `let renamed = gates_differ(only_check, &[gate("verify", "cargo test")]).expect("a difference");`

A rename is two facts, and saying so beats guessing which gate the
operator meant to rename into which.

## `let reshelled = gates_differ(`

Shell and timeout are recorded because they decide what a command
means and how long it has to mean it — `true` always passes under sh
and is not a program at all under cmd.

## `let other = gate("test", "cargo test");`

Same gates, different order: a difference worth a line, but not the
same claim as a changed command.

## `let (repo, run_id) = parked_run_with_gate("oldgatelog", "git --version");`

A v0.1 log recorded gate names and nothing else. Refusing would
strand every run written before the record over a field it could
never have carried, so resume re-derives — and uses the one thing
such a log *does* have. A moved name is proof the standard changed,
not a suspicion, and the warning says which.

## `fs::write(`

Re-derivation must be a real re-derivation, or this test would pass
against a resume that ignored today's config entirely.

## `let (repo, run_id) = parked_run_with_gate("oldgateestablish", "git --version");`

Without this, the pre-record population never gains a record: every
resume re-derives, so a gate weakened between two of them is adopted
silently — the exact substitution the record exists to prevent,
surviving in the one population that could not carry it.

Behavioural, and it takes two resumes to show: the first establishes
`git --version`, the gate is then weakened to something that fails,
and the second must still commit. It can only do that by running the
gate the first resume wrote down.

## `let first = resume_answering(&repo, &run_id, Effect::NoEdit);`

First resume: nothing to rebuild from, so it re-derives and says so.
`Effect::NoEdit` leaves the task parked, so there is a second resume
to make.

## `let paths = paths_of(&repo, &run_id);`

It wrote down what it settled on.

## `fs::write(`

Now weaken the gate, exactly as an implementer editing the workspace
would. Under the old behaviour the second resume re-derived and
adopted this.

## `assert!(`

And it is an ordinary record-bearing resume now: it warns about the
difference rather than about the log's age.

## `let (repo, run_id) = parked_run("oldgatelessslog");`

The run recorded no gates and none resolve today, so no command can
have hidden behind an unchanged name. A warning here would fire on
every gateless pre-record run, and one that cries wolf on the
harmless case is not read on the harmful one.

## `#[test]`

---- step 8: status and the ledger -------------------------------------

## `rendered.contains("per-pool drain: no pool is connected"),`

No pools file in these tests, so no attempt names a pool — and
the ledger says exactly that rather than showing a blank column
that reads as "nothing was spent".

## `assert!(`

The ledger totals are the run's, derived from the log rather than
carried over from the process that wrote it.

## `let paths = paths_of(&repo, &report.run_id);`

Holding the lock on a run that has already recorded its finish does
not make it live again. It says a process has claimed the run —
which is what a `resume` looks like before it writes anything — and
leaves the outcome above alone. A live run is covered by
`a_live_run_reads_as_running_rather_than_halted`, which truncates the
log so that the run genuinely has somewhere left to go.

## `let repo = temp_engine_repo("private");`

The §15 split, and the reason the private root cannot be inside the
repo: §14's rollback is `git clean -fd`, which would delete it.

## `let adapters = source(`

The first attempt fails, so a rollback happens before the second.

## `assert!(paths.events().starts_with(&repo));`

The ops surface stays where §15 documents it.

## `let in_repo = repo.join(".upstroke").join("runs").join(&report.run_id);`

And nothing agent-authored is reachable from the repo.

## `fn strip_run_started_field(paths: &RunPaths, field: &str) {`

---- step 8.1: the seams either side of the log ------------------------
Drop a field from a log's `run_started` — the shape a log written before
that field existed has.

Selects the event by its tag rather than by line number: `run_started`
is first today, and a helper that hard-codes that would silently rewrite
an unrelated event the day something precedes it.

## `fn rewrite_run_started_as_schema_one(paths: &RunPaths, absent: &[&str]) {`

Rewrite the opening event into the exact compatibility shape a
schema-1 binary wrote: selected top-level fields absent and no per-chain
binding snapshot. Used only by downgrade/resume regressions.

## `fn rewrite_run_started_as_schema_two(paths: &RunPaths) {`

Rewrite a current start into the shape written immediately before the
complete-review contract: schema 2 and no per-pass timeout field.

## `fn truncate_log_before(paths: &RunPaths, event: &str) {`

Rewind a log to just before the named event — the shape a process
killed at that instant leaves behind.

## `fn truncate_log_after(paths: &RunPaths, event: &str) {`

Rewind a log through the named event — the shape a process killed
immediately after its durable transition leaves behind.

## `let repo = temp_engine_repo("adoptcommit");`

§14 commits, reads the sha back, scrubs the tree, and only then
appends `task_committed`. A process killed inside those three git
calls leaves the branch one commit past its own log — which is what
foreign history looks like too. Refusing would tell the operator to
reset away a commit that already passed its gates and its review,
and to spend the attempt a second time.

## `truncate_log_before(&paths, "task_committed");`

The commit is on the branch; the log stops just short of it.

## `let question_id = QuestionId::from("q-before-success");`

Model an earlier answered question whose DesignDefect append was
interrupted. It is unrelated to the later successful settlement,
but resume still owes the repair after closing that settlement.

## `let repo = temp_engine_repo("adoptforeign");`

Exact object identity, not a plausible subject, is the authority.

## `let repo = temp_engine_repo("privatedir");`

Which private root a run used is a fact about that run. Recomputing
it from today's environment — another HOME, a service account, the
no-home fallback — would scatter the rest of its transcripts
somewhere `status` never looks.

## `truncate_log_before(&recorded, "attempt_finished");`

Stop before the successful settlement, so this is a genuinely
interrupted attempt rather than a settled prepared commit whose pin
a real process would still retain.

## `let mut resume = resume_options(&repo, &run_id);`

No override, so the resume has to read the location off the record.

## `let repo = temp_engine_repo("stalepayload");`

The engine emits `question_answered` and then rewrites the payload
beside it. A crash in between leaves a file that still reads as
open — and `upstroke answer` will accept a second answer against it,
one no engine can ever ingest, because the log has already closed
the question.

## `let retry = source(vec![Effect::NoEdit], vec![ReviewBehavior::Pass]);`

The retry fails the same way, so the run ends parked on a *second*
question with the first one answered in the log.

## `let questions = rundir::public_dir(&repo, &run_id).join("questions");`

Rewind the payload to what a crash mid-ingest leaves.

## `let repo = temp_engine_repo("husk");`

Nothing is on the record until the first event lands. A failure in
that window would otherwise leave a run directory with no
`events.jsonl`, and since run ids sort newest-last it becomes what a
bare `upstroke status` reports on — "no event log here" for a run that
never began, shadowing the real latest one.

## `git_in(&repo, &["branch", "upstroke"]);`

Git stores refs as paths, so a branch literally named `upstroke` is a
file where `upstroke/run-<id>` needs a directory: branch creation
cannot succeed.

## `struct BacklogAnswers {`

An operator working a backlog: they answer some other parked question
out of band, reply to this one at the prompt, and then walk away — so a
dropped answer never gets a second chance.

## `let repo = temp_engine_repo("backlog");`

Both channels can produce an answer on one scheduler turn. The sweep
must not swallow the reply the operator typed: it closed a different
question, and discarding this one throws away words a person sat and
wrote — words nothing will ask for again.

## `let source = source(`

Both tasks fail into a question, then both succeed once released.

## `let repo = temp_engine_repo("nullanswer");`

`sweep_answers` reports whether anything *changed*, and the drain
loop trusts that to mean it made progress. A file the sweep reads
but declines to apply — `unanswered`, which `upstroke answer` refuses
to write but a hand-edit produces — must not read as progress: that
branch terminates only because it closes the question it fires for.
A regression here hangs this test rather than failing it.

## `struct LockReleasingSleeper {`

Holds a run's lock and lets go after a set number of sleeps — an engine
that finishes while a follower is waiting on it.

## `let repo = temp_engine_repo("followlive");`

A whole attempt — the agent's thinking, its tool calls, the gates,
the review — folds into one `attempt_finished`, so a healthy run
says nothing for minutes at a time. The idle budget exists to
release a terminal attached to a dead engine; spending it on a live
one drops the operator's view mid-run.

## `let text = fs::read_to_string(paths.events()).expect("log");`

Drop the ending, so `follow` idles rather than stopping at it.

## `crate::status::follow(&loaded, &sleeper, Duration::ZERO, 1, &mut out).expect("follow");`

A budget of one idle poll: without the liveness check this returns
after two sleeps, whatever the run is doing.

## `fn pools_file(repo: &Path, content: &str) -> PathBuf {`

---- step 10: pools, budgets, and spend approval (§13) ------------------
A pools file beside the repo — never `~/.upstroke`, which is the
operator's, and never inside the workspace, where §14's `git clean -fd`
would delete it.

## `let repo = temp_engine_repo("budgetstop");`

The one-fold property, on the branch step 10 added: the stop is an
event, `RunState::apply` is what turns it into state, and a replay of
the log lands on the same state the live run held.

## `assert_eq!(report.outcome(), RunOutcome::BudgetExceeded, "{report:?}");`

Each task costs 0.06 (0.01 implementer + 0.05 review), so the ceiling
is crossed after the first and the second task is refused before it
spawns anything.

## `let events = events_of(&repo, &report.run_id);`

Exactly once: the scheduler stops scheduling on the first stop, so a
second would describe a spawn that never happened.

## `assert!(matches!(task(&report, "t2").status, TaskRunStatus::Skipped));`

Nothing after t1 ran, and the untouched tasks settle as skipped.

## `let source = source(`

Fails on the first rung, so a second attempt is asked for — and
refused, because this task has already spent past its own ceiling.

## `let repo = temp_engine_repo("budgetresume");`

D4's whole point: a budget stop is recoverable in one command,
because budgets are re-derived at resume rather than inherited.

## `let repo = temp_engine_repo("approvespend");`

D3, end to end. The engine escalates FIRST and then asks, so an
approved task un-parks already standing on the frontier rung with a
fresh allowance — and `answer_question` needs no ApproveSpend arm.

## `let tiers: Vec<&str> = task(&report, "t1")`

The approved attempt really ran on the frontier rung with the
allowance the escalation reset — not a re-run of the mid rung.

## `assert_eq!(report.outcome(), RunOutcome::Halted, "{report:?}");`

Through `ingest_answer`'s existing Declined path — the one place that
owns the halt policy, with no ApproveSpend special case beside it.

## `let repo = temp_engine_repo("frontierstart");`

§12's target is silent escalation. A task the operator deliberately
routed to frontier in config was not escalated onto it silently, and
asking anyway trains people to approve without reading.

## `let drain = &report.pool_drain;`

§13's second currency in the ledger, folded from the same records the
dollar column comes from.

## `let events = events_of(&repo, &report.run_id);`

And §14's pre-flight snapshot is on the record — folding to nothing,
which `assert_live_equals_replay` elsewhere is what proves.

## `let repo = temp_engine_repo("poolexhausted");`

§13 source 1 made real: the signal is ground truth, and the estimator
that reads it back must never let a self-metered figure talk it up.

## `let signal_at = events`

A fold that stops at the signal reads the pool as exhausted, at the
top confidence rank — the signal is ground truth about that moment.

## `let settled = capacity::estimate(&cfg.pools, &capacity::observe(&events));`

But the whole log has the pool serving an attempt afterwards, so the
signal is retired rather than standing forever. Reporting `exhausted`
here — on the same line that reports the attempts it served — was the
shape the review caught.

## `let repo = temp_engine_repo("budgetflag");`

`[budgets] run_usd = 0.0` is a hard error at load. The flag that
overrides it must not be a way around that: zero and negative both
stopped the run before it spent anything, and NaN silently never
fired at all.

## `let branch = git_in(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);`

And refused at pre-flight, before a branch or a run directory exists.

## `let repo = temp_engine_repo("approvalfeedback");`

Every other question's answer is guidance for the next attempt. An
ApproveSpend answer is a yes/no about money whose meaning was already
consumed by the un-park, and `feedback_section` frames feedback as
"an instruction from a person… it takes precedence over your earlier
assumptions" — which is not a thing to tell a coding agent about a
billing decision.

## `assert!(`

An Unblock answer still does, because there it really is guidance.

## `let repo = temp_engine_repo("cannedoption");`

The options a question carries are the engine's instructions to the
operator: "retry this task with guidance you type below", "answer in
your own words". `upstroke answer <id> --option 1` resolved to that
sentence and pushed it as human feedback — so it reached the
implementer framed as "an instruction from a person", and once §12's
decisions were routed to the judge as well, it reached the reviewer
as "a decision from a person… a change that departs from it is a
defect however well argued". There is no diff that satisfies a
sentence about where to type, so an honest judge rejects every
attempt until the ladder is spent.

## `let source = source(`

Ask, then answer by picking the first option verbatim — what
`--option 1` writes.

## `let runs = source.adapter.runs();`

The retry happened — the answer still un-parks the task.

## `for review in runs`

And nothing reached the judge as an operator decision either.

## `let source = source(`

Down for three attempts, then back.

## `let repo = temp_engine_repo("budgetdirty");`

§14 keeps the working tree for a resumed same-rung retry, because
that retry re-gates the *cumulative* diff. The ceiling is checked at
the top of the same loop, so a budget reached between the two
returns to the operator with a rejected attempt's edits still staged
in their repository — and staged changes follow `git switch` onto
whatever branch is visited next. Observed on a real repository:
run 01KZNMR59E5ATC9MBYY29WZB6E left two files staged after exit 3.

Keeping them buys nothing even in principle: `run_resumed` discards
every uncommitted path and clears `session`/`resume_next` on every
task, so the retry those edits were preserved for cannot use them.

## `opts.budget_usd = Some(0.05);`

Enough for the first attempt, not for the retry that attempt asks
for — so the stop lands exactly between the two.

## `let mut resume_opts = resume_options(&repo, &stopped.run_id);`

And the run is still exactly as resumable as it was before.

## `fn priced_and_unpriced_attempts() -> TaskReport {`

One attempt that reported its spend and one that did not — the shape a
kill/resume leaves, and a mixed-route ladder too.

## `fn empty_report() -> RunReport {`

A `RunReport` with nothing in it, for tests that care about one field.

## `let mut task = task_report_costing(None, None);`

§13's rule, on the line an operator actually reads. The ledger has
always shown `—` for a route that reports no dollars, and the review
half of this line has said `$?` since step 9 — but the worker half
used `unwrap_or(0.0)`, so a codex-implemented task printed
`gpt-5.6-sol $0.0000` above a ledger row reading `—`. One run, two
answers, and the wrong one is the one that looks precise.

## `task.attempts = vec![AttemptRecord {`

The attempt that actually ran, as a route reporting no dollars
records it. Without this the task has no attempts at all, which is a
different thing entirely — nothing ran, so nothing is missing, and
the ledger correctly prints `—` rather than a floor.

## `assert!(`

And the same rule one level up. `total_cost_usd` is an `f64`, so it
cannot distinguish a zero sum from an unreported one — the floor has
to be carried beside it. Measured on run 01KZRTZ9ZKKF1YS7MVT4350X7M,
where a codex-implemented task made `total $0.1561` read as complete
while the worker's real spend was unknown.

## `let row = ledger`

Here every attempt was unpriced, so the worker column is `—`, which
already says "unreported" — `partial` leaves it alone rather than
decorating it into `—?`.

## `let mut mixed = priced_and_unpriced_attempts();`

The `?` belongs on a figure that exists but is short: two attempts,
one priced and one not. That is what a resumed run looks like after
the engine was killed inside the first attempt, and what a mixed
ladder looks like when one rung reports and another does not.

## `let mut priced = report;`

And a route that does report keeps its figure.

## `let text = r#"{`

`report.json` is a projection for whoever reads the run afterwards,
and `TaskRunStatus` is `pub` and `Deserialize` because that reader
may be someone else's program. Every variant added to a serde-tagged
enum with no fallback is a hard `unknown variant` error in every
consumer built against an older version — one unreadable status makes
the entire report unreadable.

`running`, `Queued` and `Running` did that to anything compiled
against 0.0.1, and that break is published and cannot be taken back.
This is so the next variant is not another one.

## `assert!(`

And everything the reader *can* understand still arrives intact.

## `let task = Task {`

`Running` says of itself that only a live `status` produces it, and
the arm that built it consulted `in_flight` alone — while the arm
directly below, for `Queued`, guards on `running`. What actually held
the promise was a guarantee made one function away: `settle` turns
every `Pending` into `Skipped` before `task_report` sees it when the
run has ended, so the only way in is `Deferred`, which is recorded
after an attempt finishes and therefore never has anything in flight.

Unreachable is not the same as impossible, and the distance between
the promise and the code keeping it is the whole hazard: a dangling
`in_flight` is what any error out of `run_attempt` leaves behind, and
`drain_and_report` writes a partial `report.json` on exactly that
path. One reordering away, that file reads `t1: running now — attempt
2 on mid` beside a top-level `"running": false`, and outlives the
process that wrote it. So the invariant is stated where it is relied
upon.

## `let repo = temp_engine_repo("resumewindow");`

`resume` takes the run's lock and then does a dozen git subprocesses
— branch checks, a switch, a discard — before it writes
`run_resumed`. Deriving liveness from the lock alone made that whole
window read as a live run: `status` printed `run in progress: N
task(s) committed so far` and returned early, dropping the stop
reason, the parked list, and the `resume --budget` line an operator
at a budget stop is running `status` to find.

The lock answers who has claimed the run. Whether the run still has
anywhere to go is a question only its log answers.

## `let repo = temp_engine_repo("budgetjam");`

Handing back a clean tree was added *before* the ceiling was
recorded, with a `?` on it. So a `git reset --hard` that failed for
any of the ordinary reasons — a locked index, a read-only path, a
hook that exits non-zero — took the whole budget stop with it: no
`budget_exceeded` event, `budget_stop` left `None`, exit 1 with a git
error where CI was gating on exit 3, and a `resume --budget` with no
stop to get past. The tidying is a courtesy; the ceiling is the run's
account of why it stopped.

The fake reviewer plants a stale lock only after its exact candidate
worktree exists. This reaches the budget-stop cleanup without using
ordinary gate residue to pierce the new workspace isolation.

## `assert!(`

And it says so rather than leaving the operator to find the mess.

## `let repo = temp_engine_repo("budgetdecline");`

A decline routes through `fail_task`, which sets `halted_at`, and
halted outranks budget in `outcome()`. A decline sitting on disk when
the ceiling hits would have relabelled the stop as a task failure —
exit 1 where CI was gating on exit 3 to raise the ceiling.

## `#[derive(Debug, Clone)]`

---------------------------------------------------------------------------
PR4: every process the legacy engine starts goes through the Runner
---------------------------------------------------------------------------
One process the engine asked a runner to execute.

## `stdin: String,`

What the child receives on stdin. The adapter says *whether* a prompt
is delivered this way (`AgentAdapter::stdin_payload`); the spec is what
carries the bytes, and the runner is what writes them.

## `struct RecordingRunner {`

A real [`HostRunner`](crate::runner::host::HostRunner) that writes down what
it was asked to run.

It delegates rather than stubs: a recorder that returned canned output
would prove the engine *called* something and nothing about the run still
working. Every assertion below is therefore made about a run that actually
committed its tasks.

## `fn program_stem(program: &str) -> String {`

The file stem of a program path, however it was spelled.

## `#[test]`

`decisions.pr_sequence[5].scope`: "probes, workers, gates, reviews go
through the Runner", and `invariants_preserved`: "legacy engine behavior
unchanged (**the legacy engine does not run the shell probe**)".

One run of two tasks, each with two gates and one review pass, driven
through a recorder wrapped around the real host runner. What it establishes,
in the order the contract asks for it:

1. **Every** process the run started is a Runner request — the count is the
   run's shape (2 workers + 4 gates + 2 reviews), so a process that had
   gone round the seam would leave the count short and a process that had
   been added would leave it long.
2. The identities are the packet's first form in the **legacy generation**,
   written out by hand, and they are unique.
3. **No `probe(shell)` request**, and the recorder can see one — the same
   recorder is handed a real shell probe afterwards and the count moves
   from 0 to 1, so the zero is a measurement rather than a blind spot.
4. **Authoritative Git never crosses the boundary** (DESIGN.md:612): the
   run made commits, and not one recorded request is a `git` process.

## `let expected_ids = vec![`

(1) and (2): the exact identities of the run, in order, written from the
plan's shape and the packet's grammar rather than read back from the
engine. Two tasks at plan positions 0 and 1, one attempt each, generation
0 because the legacy engine has none, and inside each attempt the worker,
then gate 0 and gate 1, then review pass 0.

## `use crate::runner::ExecutionRole;`

The roles, and what each buys. R3, via `ExecutionRole::is_slotted`: a
worker and a review take an {agent, pool} pair; a gate does not, because
a gate is repository-controlled code and runs no agent CLI — which is
also why it is handed no agent.

## `let worker_stdin = &seen`

The prompt still reaches the child the way the adapter says it should.
`stdin_payload` is delivery policy and the spec is what carries those
bytes, so the two routed agent processes must be carrying them: a worker
gets the materialized task prompt, a reviewer gets the verdict prompt,
and a gate — which is a shell command, not an agent — gets nothing.

## `let shell_probes = |seen: &[RoutedProcess]| {`

(3) The clause, and the control that makes it a measurement. The run has
a recorded shell and ran four gate commands through it, and still never
asked that shell to `exit 0` through the Runner — `gates::shell_available`
is a PATH check and stays one.

## `assert!(`

(4) Authoritative Git never crosses the boundary. The run committed both
tasks — every one of those commits was git work — and not one process
the runner was asked to execute is git.

## `let worker = seen`

The workspace is the runner's to set, and it set a different one for the
worker than for the gates and the review: an attempt edits the repo,
while gates and reviewers judge the frozen candidate snapshot.

`same_path`, not `==`: the runner is given the workspace root the run
resolved and this test holds the `temp_dir()` name it created the repo
under, and those are two spellings of one directory on any host whose
temp directory is reached through a symlink (macOS: `/var` →
`/private/var`) or whose user directory has an 8.3 short name (Windows
CI: `RUNNER~1` for `runneradmin`). Comparing the directories rather than
the strings is also what makes the `!` case below mean anything — an
inequality between two spellings holds for free.

## `#[test]`

Every identity a *retried* attempt with two review passes and a re-ask
assigns, recorded from production rather than constructed by the test.

`the_legacy_engine_routes_every_process_through_the_runner` above records a
run whose every task runs once, with one review pass and no re-ask — so the
three fields that vary *inside* an attempt's identity never vary in it:
`AttemptNumber`, the review pass index, and pass-versus-re-ask. Each of
those is a distinct call site in `engine::attempt`, and a call site that
passes a constant where it should pass its argument is invisible to a grid
of hand-built identities (`runner::tests::invocation_ids_are_unique_within_a_run…`
synthesizes its tuples; `review::tests::the_one_format_reask_is_its_own_invocation…`
is handed a correct pair). `invocation_identity` requires "unique per
process" and "a retry attempt has a new attempt number", and INV-20 makes
`review_pass(n)` and `review_reask(n)` distinct members.

So: one task that fails its first attempt and is retried, two gates, two
review passes from two families, and a first verdict the reviewer botches.
The expected list is written from the packet's grammar and this run's
shape.

## `vec![Effect::NoEdit, Effect::EditFile],`

The first attempt reports success and edits nothing, which fails
outcome sanity before any gate runs; the second does the work.

## `vec![ReviewBehavior::Unparseable, ReviewBehavior::Pass],`

The primary reviewer's first verdict is prose, so the pass spends
its one format-only re-ask and then answers.

## `"k0.g0.a1.worker.o0",`

Attempt 1: the worker alone — nothing downstream of a lying
agent runs.

## `"k0.g0.a2.worker.o0",`

Attempt 2 carries a *new attempt number* through every process
it starts, not only through the worker.

## `"k0.g0.a2.review_pass0.o0",`

Pass 0's verdict, and the one re-ask it is allowed — two
processes, two identities, and the second is not a second run of
the first.

## `"k0.g0.a2.review_pass1.o0",`

Pass 1 is the other family's, and it is pass *one*.

## `use std::collections::BTreeSet;`

Hostility as counts, so the list above cannot be satisfied by a run that
exercised fewer of the varying fields than it claims.

## `assert_eq!(`

`reviews_run` counts *invocations*, so the primary's two are the
verdict and its re-ask — the same two the identity list names.

## `#[test]`

A worker that cannot be spawned is an **infrastructure error**, not a task
failure — and the engine synthesizes no settlement for it.

`expected_failures_refusals[2]`: "a spawn failure uses the existing
runner/engine semantics: returned error; **no halting settlement is
synthesized**" (at integration, PR8's Deferred/Parked outcomes handle it).
Every fake worker in this file returns a spawnable shell command, and the
reviewer's spawn failures are converted to `ReviewUnavailable` *inside*
`run_review` before they ever reach the coordinator's error arm — so
nothing here exercised that arm, and converting it into `fail_task` would
have left the suite green while turning an outage into an attributed task
failure with a halt.

The oracle is threefold, because "returned error" alone would also be true
of a run that had recorded a failure first: the call returns `Err`, the
error is the runner's own spawn diagnostic, and the log carries
`attempt_started` with **no** settlement after it.

## `let run_id = rundir::latest_run(&repo).expect("the run created its directory");`

Nothing was settled: one attempt started, no attempt finished, no task
failed, and the ladder bought no second attempt.

## `#[test]`

The engine facade's public surface is the one the packet enumerates — no
wider.

`decisions.phase_zero_modules.visibility`: "pub(super) only where a sibling
or tests reference an item; **no new pub or pub(crate)**; public paths
unchanged", and `modules["src/engine/mod.rs"]` lists the facade item by
item: the five `pub use` groups, `pub fn run/run_with/run_harness`, and
`pub fn resume/resume_with/resume_harness`.

This slice added `run_harness_on` and `resume_harness_on`, which take the
boundary as a parameter. Inside the crate that is exactly right — it is how
`engine::tests` drives a recording runner. Public, it is a hole in
`invariants[22]` ("schema-1..3 runs are host-only and no run changes its
boundary or image between epochs"): a downstream crate could execute a
legacy run through a Docker or remote `Runner` with no `RunnerPolicy`
recorded and no refusal, and a later ordinary `resume` would move it back
on-host between epochs.

A `pub`/`pub(crate)` widening is invisible to every in-crate test — each
caller compiles either way — so this reads the facade's own text. The
expected sets are transcribed from the packet, not derived from the file.

## `let raw = include_str!("mod.rs");`

**The blanked region, not the raw file.** This read `include_str!` and
counted prose: the doc comment on `pub(crate) mod topology;` explains
that this census forbids `pub mod `, and that sentence *is* a `pub mod `
in the file — so the census failed on its own explanation.
`PR4-CENSUS-COMMENT-ORACLE`, and the same trick would have let any of the
six widenings be smuggled past by writing it in a comment.

## `let public_fns: BTreeSet<&str> = source`

Every `pub fn` at the facade's top level.

## `for widening in [`

And nothing else is exported by any other route.

## `"pub mod ",`

**`pub mod ` was missing from this list, and it is the route the
schema-4 surface actually took.** `pub mod topology;` stood here for
the whole slice: this census enumerated functions and re-exports, so
a whole subsystem reached the public path without moving any number
it counts. `create_run`, `Started::into_handle`,
`TopologyRun::resumed` and `step` were reachable by any downstream
caller of this library, which could then write schema-4 state that
this build's own recovery refuses — the frontier review of
`75da796`, finding 1. A census that forbids five widenings and not
the sixth is the shape of every fail-open needle this slice found.

## `let mut reexported: BTreeSet<&str> = BTreeSet::new();`

The re-exports, flattened. `pub use` is the other way a name reaches the
public path, and the packet enumerates these too.

## `"RunOptions",`

options

## `"RunReport",`

report

## `"AdapterSource",`

crate::agent

## `"AttemptRecord",`

crate::events

## `"AttemptFailure",`

crate::ladder

## `for private in ["fn run_harness_on(", "fn resume_harness_on("] {`

The boundary-taking helpers exist and are *not* public: this test would
pass just as well if they had been deleted, which is not what it is for.

## `fn public_facade_entry_points() -> Vec<&'static str> {`

The six public entry points of the facade, as the facade's own text spells
them.

Read from `mod.rs` rather than written out, so a seventh public entry point
cannot be added without appearing here — and therefore without being
classified by the two tests below, which cross this set against a table of
calls.

## `#[test]`

**Every** public way to become a write coordinator establishes containment
first — not only the CLI's.

`invariants[INV-18]` is "on Windows every host child is a member of the
**coordinator's** ambient kill-on-close Job Object *from creation*", and
`expected_failures_refusals[1]` is "ambient job cannot be created or joined
(Windows) → write command refuses at startup with a diagnostic". Neither
says "when the coordinator was started by `src/main.rs`".
`decisions.phase_zero_modules.modules["src/engine/mod.rs"]` freezes
`run/run_with/run_harness` and `resume/resume_with/resume_harness` as the
facade, so all six are supported entry points: a downstream crate calling
`engine::run_with` is a write coordinator, and until this test existed it
established nothing at all. A kill between `CreateProcessW` and private-job
assignment then left the suspended stub alive — the exact residue INV-18
exists to prevent — and an ambient failure could not produce the required
pre-effect refusal, because nothing attempted the join.

The oracle is [`crate::runner::host::containment_establishments`], which
counts on the calling thread: "this call established containment", not "some
earlier call in this process did". Each entry point is driven with input it
must refuse (an absent plan, an unknown run id) so the assertion is about
the entry point and not about a run.

The *class* is what is asserted: the table is crossed against the facade's
own `pub fn` list, so this cannot be satisfied by covering the two the
review named.

## `let mut driven: Vec<&str> = entry_points.iter().map(|(name, _)| *name).collect();`

The class, not the instance: every public entry point of the facade is
in this table, and every row of the table is one of them.

## `#[cfg(windows)]`

Windows has the other half of the same fact: the coordinator process
really is a member of an ambient job now. (Process-wide and latching,
so it corroborates the count above rather than replacing it.)

## `#[test]`

The other side of the same census: a public entry point that is *not* a
write coordinator does not establish containment, and there are six of them.

`crash_reconstruction` anchors the ambient job at "every **write** command",
and `src/main.rs`'s `command_class` is the CLI's half of that split
(`every_write_command_establishes_containment_and_no_read_only_one_does`,
which asserts `skipped == 6`, "the six read-only subcommands"). This is the
library's half, and the six rows below are **the functions those six arms
call** — one per read-only subcommand, so the two censuses count the same
six things from opposite ends and the distinction survives when the CLI is
not involved at all.

`connect` and `capacity` are the interesting rows — they *do* spawn agent
CLIs — and they are still not coordinators, so INV-18's "the
**coordinator's** ambient … Job Object" does not reach their children.
Protecting those is a stronger guarantee than the packet asks for; it is
recorded as `PR4A-SPAWN-WITHOUT-AMBIENT` in `reviews/FINDINGS.md` with an
owner, not done here.

## `std::iter::empty(),`

No ids: the seam exists so this test spawns nothing. What
is under test is the containment step, which happens
before any spawn or not at all.

## `#[test]`

Containment comes **before** the coordinator, and a failure to establish it
refuses the run before any effect.

`side_effect_vs_event_ordering` is "no events; ambient job before any
spawn", and `expected_failures_refusals[1]` is a refusal "at startup with a
diagnostic". The oracle is `src/main.rs`'s: the two outcomes are different
errors from different places. A refused containment names the ambient job
and *not* the plan; with containment established the coordinator runs and
fails on the plan instead. If establishment happened after the coordinator,
or not at all, the first call would carry the plan's error.

The step is a parameter here for the reason `dispatch` takes one: no machine
can make the real join fail on demand, and on Unix it cannot fail at all.
The seam is not a hole — `Contained`'s field is private to
`crate::runner::host::proof`, a module with no descendants, so a closure that
returns one has established containment.

## `#[test]`

The same ordering, for the other coordinator. A resume is a write command:
`startup_census` enumerates them "(run, resume)".

## `let looked_in = repo.display().to_string();`

The resume's own refusal names the run directory it looked in; the
containment refusal cannot, because it happens before the coordinator
resolves anything.
