# `src/review.rs`

Extended notes for [`src/review.rs`](../../src/review.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Review (DESIGN.md §11.2–§11.3): read-only worker profiles judge the
engine-captured diff against the task's acceptance criteria. A judgement is
authoritative only when the complete trimmed answer is one `json`-labelled
fence containing one verdict object; prose and examples cannot approve.

Two things make this more than a second opinion from the same model: a
reviewer sees the *diff* rather than the implementer's account of it
(invariant 3), and its prompt is explicitly anti-sycophantic — its job is
to find reasons to fail, not to agree. Unparseable output earns exactly
one re-ask; after that the attempt fails, because a reviewer that cannot
answer in the required shape has not reviewed anything.

Everything a reviewer is shown — the diff, and any artifacts — was
written by an agent, so it is quoted as data behind a fence the payload
cannot close and labelled untrusted. Parsing is deliberately fail-closed:
a mangled answer costs a re-ask and then a failure, and never falls back
to some earlier passing-looking object in the reply.

### A list of passes, not one reviewer

§11.5 generalizes review "from a single pass into a **list of passes, each
with a lens and a pass rule**", and §11.3's cross-vendor second opinion is
the first user of that shape: on blast-radius paths a second reviewer from a
different *model family* judges the same diff, and **both verdicts must
pass**. [`ReviewPlan`] resolves which passes a task gets; [`Lens`] is what
distinguishes them.

The passes are independent on purpose. Neither reviewer is told the other's
verdict — a second opinion that has already read "the first reviewer passed
this" is an agreement machine, which is the same failure the anti-sycophancy
instruction exists to prevent.

§11.5's security lens joins [`Lens`] in v0.2, and it is **not** just another
entry: its ladder dispatch differs deliberately, because a security finding
that enters the retry-until-it-passes loop is a finding being laundered into
a commit. It goes to an `Unblock` question instead. Nothing here should make
that harder to add — which is why a lens is an enum with behaviour hanging
off it rather than a bool.

## `#![allow(clippy::disallowed_methods, clippy::disallowed_macros)]`

LEGACY-EFFECT: this module is in the **frozen legacy section** of
`effects/allowlist.toml`, which carries its justification and the condition
under which the section shrinks. `decisions.effect_site_inventory.mechanism` (2).

## `pub const MAX_DIFF_BYTES: usize = 1024 * 1024;`

Largest complete diff one review pass accepts. Silently omitting files is
never a review: work above this bound is refused before model spend and must
be split into a smaller task.

## `pub enum Lens {`

What one review pass is looking for, and how its artifacts are named.

v0.2 adds `Security` here (§11.5) — with a different ladder dispatch, since
a security finding must never enter the retry loop.

## `pub enum Lens` › `Acceptance,`

§11.2: does this change meet its acceptance criteria without breaking
anything? The pass every reviewed task gets.

## `pub enum Lens` › `SecondOpinion,`

§11.3: the same diff, judged independently by a different model family.

## `impl Lens` › `pub fn name(self) -> &'static str {`

Short id used in profile names, event records, and the ledger.

## `impl Lens` › `fn file_suffix(self) -> &'static str {`

Suffix distinguishing this pass's on-disk artifacts. The acceptance pass
keeps the bare names it has had since step 6, so a run directory reads
the same way whether or not a second opinion was configured.

## `impl Lens` › `fn preamble(self) -> &'static str {`

Prepended to the prompt. The acceptance pass adds nothing: it *is* the
baseline the rest of the prompt already describes.

## `pub struct PassBinding {`

Which agent and model a pass runs on. Plain data so the run record can carry
it (§15) and a resume can honour what actually judged this run's code.

## `pub struct ReviewPass {`

One resolved pass: a lens and the binding that will apply it.

## `pub struct ReviewPlan {`

Which passes each task gets, resolved once before any agent is spawned.

Resolved up front rather than per attempt so that pre-flight can probe every
agent that might judge this run (step-6 finding #10: a reviewer that cannot
be built must never silently degrade the run to gates-only) and so the run
record can pin what its verification standard was.

`second_opinion` is aligned to `plan.tasks` by index rather than keyed by
task id, which is safe for the same reason `Progress` is: a resume refuses
outright when the plan hash or the resolved chains moved, so the task list
this was built against and the one it is read back against are the same list.

## `pub struct ReviewPlan` › `pub enabled: Option<bool>,`

Whether verification was deliberately enabled when this plan was
frozen. `Option` is intentional: schema-3 replay must distinguish an
old/malformed record that omitted the field from an explicit `false`.

## `pub struct ReviewPlan` › `pub alternative_available: Option<bool>,`

Whether the frozen plan deliberately retained an anti-self-review
alternative. Absence of the binding is legitimate, but absence of this
marker is not: otherwise a truncated record silently weakens review.

## `pub struct ReviewPlan` › `pub pass_timeout_secs: Option<u64>,`

Independent wall-clock budget for each pass, including its one
verdict-format re-ask. Seconds keep the event record plain and stable.
This complete-review contract begins at event schema 3. A schema-2
binary would ignore this field *and* still truncate the prompt at 60
KiB, so allowing it to resume could accept a partial review. The schema
boundary makes that downgrade a refusal instead.

## `pub struct ReviewPlan` › `pub primary: Option<PassBinding>,`

`None` ⟺ `[routing] review = { enabled = false }`. Anything else that
fails to resolve is an error, never an empty plan.

## `pub struct ReviewPlan` › `pub alternative: Option<PassBinding>,`

A different-family binding at the review tier, where this build can
reach one. Used *only* to stop a task being reviewed by the model that
wrote it; absent on a single-vendor install, which warns instead.

## `pub struct ReviewPlan` › `pub second_opinion: Vec<Option<PassBinding>>,`

Per task, aligned with `plan.tasks`: the §11.3 second opinion this
task's paths asked for.

## `impl ReviewPlan` › `pub fn pass_timeout(&self) -> Result<Duration, UpstrokeError> {`

Validate and materialize the timeout recorded for this run. A corrupt
zero must fail closed on resume rather than disabling supervision.

## `impl ReviewPlan` › `pub fn agents(&self) -> Vec<&str> {`

Every agent that could be asked to judge something — the set pre-flight
must probe, deduped and stable.

## `impl ReviewPlan` › `pub fn required_agents(&self) -> Vec<&str> {`

Agents whose absence is fatal: everything except the opportunistic
[`Self::alternative`], which degrades to a warning (see
[`Self::drop_alternative`]).

## `impl ReviewPlan` › `pub fn drop_alternative(&mut self) {`

Give up on the anti-self-review rebind — the alternative agent would not
probe. Reviews still happen; some may be same-model.

## `impl ReviewPlan` › `pub fn self_review_warning(`

Which tasks will be judged by the model that wrote them, and why nothing
prevented it — or `None` when the rebind is available or nothing is at
risk.

Called from two places on purpose. Resolution reaches it when no
cross-family model has an adapter at all; **pre-flight reaches it again
after a probe failure drops the alternative**, which is the case that
actually happens to people. A real build always ships the Copilot
adapter, so resolution alone would never fire this — the warning would
have been dead code in every shipped binary.

## `impl ReviewPlan` › `pub fn passes_for(&self, index: usize, implementer: &PassBinding) -> Vec<ReviewPass> {`

The ordered passes for the task at `index`. See [`passes_for`].

## `pub struct ReviewBindings<'a> {`

The three bindings pass selection actually reads, for one task.

**Ask for what you read.** [`passes_for`] consumes exactly `primary`,
`alternative` and this task's `second_opinion` — never the enabled flag, the
timeout, or the other tasks' entries. Naming that as a type is what lets one
rule serve two shapes: a schema-3 [`ReviewPlan`] indexed by task position,
and a schema-4 [`crate::topology::registry::FrozenTaskSpec`] that resolved
its own second opinion at freeze time. The alternative was a driver-side
re-derivation of the rebind rule, and `wrong_internal_assumption` is 48.3%
of this project's classified findings — a second implementation of a rule
with two interacting cases is exactly the shape that produces them.

## `pub struct ReviewBindings<'a>` › `pub primary: Option<&'a PassBinding>,`

The reviewer configured for every task in the run.

## `pub struct ReviewBindings<'a>` › `pub alternative: Option<&'a PassBinding>,`

The anti-self-review fallback, where the run retained one.

## `pub struct ReviewBindings<'a>` › `pub second_opinion: Option<&'a PassBinding>,`

This task's §11.3 second opinion, where its paths asked for one.

## `impl<'a> ReviewBindings<'a>` › `pub fn of_plan(plan: &'a ReviewPlan, index: usize) -> Self {`

The run-level plan's answer for the task at `index`.

## `pub fn passes_for(bindings: ReviewBindings<'_>, implementer: &PassBinding) -> Vec<ReviewP…`

The ordered passes for one task, given the binding the implementer is
actually running on.

Two rules meet here, and the order matters:

1. A task with a configured second opinion keeps its primary reviewer
   **unrebound**. Rebinding it would let both passes resolve to the same
   different-family model, and Anthropic-written code would lose its
   Anthropic review entirely — strictly worse than the self-review the
   rebind exists to prevent.
2. Otherwise the primary rebinds when it would be the *same model* that
   wrote the code. Exact `(agent, model)` equality, not family similarity:
   `claude-sonnet-5` reviewed by `claude-opus-5` is a genuine second look,
   and rebinding it would spend cross-vendor capacity on half the tasks in a
   run for no verification gain.

## `pub fn obliged_lenses(bindings: ReviewBindings<'_>) -> Vec<Lens> {`

The lenses one task's frozen plan **obliges**, in the order it obliges them.

[`passes_for`]'s answer, projected to its lenses — not a second reading of
its two rules. The projection is exact because the obligation is
**invariant in the implementer**: the rebind of rule 2 chooses *which
binding* applies the acceptance lens and never whether that lens runs, and
rule 1 turns on a configured second opinion rather than on who wrote the
code. So the set of lenses a record owes can be asked without knowing what
ran, which is what lets the fold ask it —
[`crate::events::AttemptRecord`] carries the passes and not the binding
that produced them.

The stand-in implementer is `primary` for that reason: any value gives the
same lenses, and using one that is certainly present keeps the call total.
[`the_obliged_lenses_do_not_depend_on_who_implemented`] measures the
invariance rather than asserting it here.

## `pub fn plan_for(`

Resolve every task's review passes (§11.2, §11.3).

`has_adapter` is injected rather than read from the registry so the engine
can ask about the adapters its own harness holds — which under test is not
the built-in set — and so `validate` and `run` reach the same answer.

Failure is asymmetric, and deliberately so. An explicitly configured
`second_opinion` that cannot resolve is an **error**: the operator asked for
two model families on their blast-radius paths, and quietly giving them one
is step-6 finding #10 all over again. The implicit anti-self-review rebind
merely **warns**, because nobody asked for it and refusing would make
upstroke unusable on a single-vendor install.

## `cfg.overrides.iter().find(|ov| {`

`is_some`, not a match on the one variant that exists today:
§11.5 adds a security lens to this key, and a new variant should
arrive as a compile error where it needs handling — not as a
silently-ignored override here.

## `if let Some(index) = demanded.iter().position(Option::is_some) {`

Contradictory config: one key says judge nothing, another says judge
twice. Only an error where it would actually change what runs.

## `let primary = match cfg.pins.iter().find(|p| p.tier == tier) {`

The same rules the router uses: a pin for the tier, else the catalog's
example binding.

## `let primary_family = catalog::lookup(&primary.agent, &primary.model).map(|e| e.family);`

Every binding above comes from the catalog (pins are validated against it
at load), so this is belt-and-braces — but without a family there is no
way to tell "different" from "same", and guessing is how a reviewer ends
up quietly paired with itself.

## `if let Some(warning) = resolved.self_review_warning(plan, chains, tier) {`

The carried step-6 item, now visible: say when a task will be judged by
the model that wrote it and nothing in this build can prevent it.

## `pub struct ReviewSubject<'a> {`

What a review pass reads about the task under review.

**Three fields, and [`ReviewCx`] used to take a whole `ir::Task` to reach
them.** `materialize_prompt` — the only thing in this module's review path
that touches the task at all — quotes the title, the body and the acceptance
criteria, and nothing else.

The wider field could not be shared. The schema-4 driver holds a
`FrozenTaskSpec` from the frozen registry and no `ir::Task` anywhere:
synthesising one would mean inventing an id, a kind and a dependency list
the reviewer never reads, and a conversion that fabricates fields is free to
drift from the plan it claims to represent. Asking for what is read removes
the question — the same narrowing `OpenGeneration` made for the rebuild
family, and for the same reason.

## `pub struct ReviewSubject<'a>` › `pub title: &'a str,`

The task's one-line title.

## `pub struct ReviewSubject<'a>` › `pub body: &'a str,`

Its body, which may be empty.

## `pub struct ReviewSubject<'a>` › `pub acceptance: &'a [String],`

Its acceptance criteria, which may be empty.

## `impl<'a> ReviewSubject<'a>` › `pub fn of(task: &'a Task) -> Self {`

The subject of a legacy plan's task.

## `pub struct ReviewCx<'a>` › `pub lens: Lens,`

Which pass this is (§11.5). Decides the prompt preamble and the names of
this review's artifacts on disk.

## `pub struct ReviewCx<'a>` › `pub artifacts: &'a [(String, String)],`

Artifacts the reviewer should judge against (conventions brief first).

## `pub struct ReviewCx<'a>` › `pub decisions: &'a [String],`

What the operator has already settled about this task (§12). A question
parks a task precisely because its acceptance criteria turn on a
decision the repository cannot supply, so a judge that cannot see the
answer will look for it, fail to find it, and reject the change for
having obeyed it.

## `pub struct ReviewCx<'a>` › `pub settings_dir: &'a Path,`

Where this review's permission settings are materialized. Outside the
workspace (§15 split), so the reviewer cannot read the description of
its own sandbox.

## `pub struct ReviewCx<'a>` › `pub reviews_dir: &'a Path,`

Where the verdict transcripts land — also outside the workspace, since
they are agent-authored.

## `pub struct ReviewCx<'a>` › `pub stem: String,`

Unique file stem for this task's review artifacts, attempt included —
step 7 reviews the same task more than once and each verdict is the
evidence for its own retry.

## `pub struct ReviewInvocations {`

The two identities one review pass can spend.

Two, because the packet's role set has two members for a review —
`decisions.admission_and_leases.permits.invocation_identity`: "role in
{worker, gate(n), **review_pass(n)**, **review_reask(n)**}". A pass that
answers unparseably earns exactly one re-ask, and that re-ask is a second
process with its own identity rather than a second run of the first.

Built by the caller rather than here, because which *form* they take is the
caller's: a task's review is the attempt form and an integration
transaction's is the sequence form (which has no worker).

## `pub struct ReviewInvocations` › `pub pass: InvocationId,`

The verdict.

## `pub struct ReviewInvocations` › `pub reask: InvocationId,`

The one format-only re-ask, if the verdict could not be parsed.

## `pub enum ReviewResult {`

What a review attempt produced. A reviewer that could not run at all is
NOT a rejection of the change: the engine has to tell "the code is wrong"
apart from "the judge was unavailable", or a rate-limited pool reads as a
failed task and the retry ladder punishes the implementer for it.

## `pub struct ReviewOutcome` › `pub invocations: u32,`

How many agent invocations it took (2 means the re-ask was needed).

## `pub struct ReviewOutcome` › `pub transcript: PathBuf,`

The transcript the verdict (or the give-up) actually came from.

## `impl ReviewPass` › `pub fn profile(&self, effort: Effort) -> WorkerProfile {`

The read-only profile this pass runs under. Named for its lens and its
model, so an event log and a ledger both say which judgement is whose.
Effort is a parameter rather than a field on [`PassBinding`] for a
specific reason: `passes_for` decides the §11.3 rebind by comparing a
binding with the implementer's, and a binding carrying an effort the
implementer's descriptor does not would make that comparison always
false — silently retiring the check that stops a model reviewing its own
work. The comparison is about identity; effort is not identity.

## `pub fn profile_for(agent: &str, model: &str, name: &str, effort: Effort) -> WorkerProfile…`

A read-only profile bound to the same rung the reviewer is configured for.

## `pub fn run_review(`

Run one review pass through `runner`.

### Errors

Only what makes the *evidence* unusable — an oversized or opaque diff. A
reviewer that could not run is [`ReviewResult::Unavailable`], not an error:
the engine has to tell "the code is wrong" from "the judge was
unavailable".

## `let full_prompt = materialize_prompt(cx)?;`

Validate the complete evidence before permission files are written or an
adapter can build/spawn a model command. An incomplete review is no
review, so large tasks fail closed rather than losing early paths.

## `let settings_path = match cx.adapter.materialize_permissions(`

Reviewers run nothing: no gate commands, no edit tools (§20).

## `let prompt = match (invocation, &resume) {`

The re-ask only gets to be terse if the reviewer's context survives.
Without a session to resume it has never seen the diff, and a
verdict from an agent that read nothing is worthless — so re-send
the whole prompt rather than asking it to invent an answer.

## `gate_cmds: Vec::new(),`

Reviewers run nothing, so there is nothing to allow (§20). This
is the same empty list handed to `materialize_permissions` above
— an agent whose permissions ride on argv reads it from here.

## `let request = crate::runner::review_request(`

A reviewer is an agent CLI, so it is slotted and `host-v1` gives it
its agent's credential location (`ExecutionRole::Review`). The
workspace is the read-only candidate snapshot the caller resolved,
and it is the runner that puts the process there — the adapter no
longer can.

The prompt still arrives the way the adapter says: `stdin_payload`
is delivery policy (a CLI that takes the prompt as an argument
returns nothing here), and the spec is what carries those bytes to
the child.

## `if let Ok(outcome) = cx.adapter.parse(&output) {`

The model may already have spent tokens. Parse only to retain
any spend it reported; without a durable transcript its verdict
cannot be accepted.

## `let answer = outcome.detail.clone().unwrap_or_default();`

A verdict is read even from a failed invocation — a reviewer that
answered and then crashed still told us something.

## `if outcome.status != OutcomeStatus::Completed {`

The reviewer never ran properly: re-asking an exhausted pool or a
hung process just spends again for the same result.

## `Ok(ReviewOutcome {`

§11.2: one re-ask, then it counts as a failure. The reviewer ran and
answered — it just never answered in a shape that means anything — so
this is a genuine no-pass, not an outage.

## `fn materialize_prompt(cx: &ReviewCx<'_>) -> Result<String, …` › `prompt.push_str(cx.lens.preamble());`

What distinguishes this pass from the others, if anything (§11.5). It
leads, because it frames everything below it.

## `fn materialize_prompt(cx: &ReviewCx<'_>) -> Result<String, …` › `if !cx.decisions.is_empty() {`

Above the fence, and framed as instruction rather than data: unlike
everything below, this came from the operator, and a criterion that
reads "the policy the operator chose" is unjudgeable without it.

## `fn materialize_prompt(cx: &ReviewCx<'_>) -> Result<String, …` › `for (name, content) in cx.artifacts {`

Everything below is agent-authored: the artifacts were written by an
earlier task's agent and the diff by the very agent under review. It is
quoted as data, with a fence the payload cannot close, and labelled as
untrusted so instructions smuggled inside it are not obeyed.

## `pub(crate) enum CompleteDiffError {`

Explain why a diff cannot receive a complete review, if it exceeds the
fail-closed input limit.

The engine checks this before dispatch and turns it into a settled policy
failure. `materialize_prompt` repeats the check as a last line of defence
for direct callers, which still receive a refusal rather than a truncated
review.

## `pub fn parse_verdict(text: &str) -> Option<Verdict> {`

Parse the one authoritative verdict envelope.

The complete trimmed reply must be exactly one `json`-labelled Markdown
fence whose body is exactly one JSON object. This deliberately rejects
useful-looking prose, quoted examples, bare JSON, extra fences, and trailing
commentary: none of those forms proves that the reviewer meant the object
as its verdict. Unparseable output earns the one §11.2 re-ask instead.

## `pub fn parse_verdict(text: &str) -> Option<Verdict>` › `let normalized = text.trim().replace("\r\n", "\n");`

Normalise the line ending emitted by Windows CLIs, but do not otherwise
rewrite the answer: the wrapper itself is part of the authority boundary.

## `fn verdict_from_json(candidate: &str) -> Option<Verdict>` › `let pass = value.get("pass")?.as_bool()?;`

`pass` is mandatory: without it there is no verdict, only prose.

## `fn verdict_from_json(candidate: &str) -> Option<Verdict>` › `needs_human: value`

§12: the reviewer may decline to judge. Absent, or anything but a
literal `true`, means it judged — escalating to a human on a
malformed field would let sloppy output park tasks.

## `mod tests` › `fn host() -> HostRunner {`

The boundary these tests run on: the real host one, because a review
test that mocked the runner would stop proving that a review is a
process.

## `mod tests` › `fn review_ids() -> ReviewInvocations {`

One review pass's two identities, in the legacy engine's own scope —
task 0, attempt 1, pass 0. Written here rather than taken from a
generator so the test names what it is asserting about.

## `mod tests` › `struct NeverInvokedAdapter;`

Any adapter contact is a test failure. Used to prove evidence-size
refusal happens before permission materialization, command build, or
model spend.

## `fn build(&self, _run: &TaskRun) -> Result<crate::runner::Co…` › `let (marker, delay_ms) = if invocation == 0 {`

0.4 and 0.75 of `verdict_reask_uses_the_remaining_pass_deadline`'s
3s budget. The ratios are what the test is about and they are
unchanged; only the absolute scale moved, and it moved because
0.4 of *one* second left 599ms for a process spawn. See that
test for the measurement.

## `fn a_mangled_final_verdict_never_falls_back_to_an_earlier_p…` › `for botched in [`

The reviewer echoes the requested shape, then fails the change but
botches the JSON. Falling back to the echo would commit a rejected
change; the only safe answer is None, which earns the re-ask.

## `fn a_refusal_quoting_the_template_is_not_a_pass()` › `let text = "I was unable to complete this review: the diff appears truncated. For \`

The old bare-JSON fallback turned this exact reply into pass=true.

## `fn needs_human_is_read_only_from_a_literal_true()` › `let asked = parse_verdict(`

§12's escalation channel. Absent means "I judged it"; a sloppy
non-boolean must not park a task either.

## `fn the_prompt_teaches_needs_human_without_offering_it_as_an…` › `assert!(`

The schema must still be unparseable, or a model echoing it would
produce an authoritative-looking verdict (step-6 finding 4).

## `fn the_operators_answer_reaches_the_judge_as_a_decision()` › `let task = task();`

§12's loop, closed. A task parks because its acceptance criteria
turn on something the repository cannot settle; the worker is handed
the answer and complies. A judge that never sees it goes looking for
the decision, finds no trace, and rejects the change for obeying an
instruction it was not shown — re-raising the same question forever.

## `fn the_operators_answer_reaches_the_judge_as_a_decision()` › `assert!(prompt.contains("re-litigate"), "{prompt}");`

It has to outrank the reviewer's own taste, or the anti-sycophancy
stance above simply argues with the operator instead of the diff.

## `fn the_operators_answer_reaches_the_judge_as_a_decision()` › `let mut bare = cx;`

And it is the operator's answer that earns this framing, not any
text near it: with nothing settled, none of it appears.

## `fn broad_diffs_keep_every_file_in_the_review_prompt()` › `let filler = "+let unchanged_context = 1;\n".repeat(2_500);`

This is larger than the old 60 KiB truncation threshold but safely
below the complete-review limit. Both ends must survive.

## `mod tests` › `struct RecordingRunner {`

A runner that writes down which identity each review process carried.

## `mod tests` › `fn the_one_format_reask_is_its_own_invocation_not_a_second_run_of_the_first() {`

The re-ask is a second process with the packet's *other* review role.

`decisions.admission_and_leases.permits.invocation_identity` gives a
review two role members — "{worker, gate(n), **review_pass(n)**,
**review_reask(n)**}" — so the one format-only re-ask a pass is allowed
carries `review_reask(n)`, not a second run of `review_pass(n)`. The
expected values are written from that sentence.

## `fn the_one_format_reask_is_its_own_invocation_not_a_second_…` › `timeout: Duration::from_secs(30),`

Generous, so the pass really performs both invocations rather
than running out of clock the way the deadline test wants it to.

## `fn verdict_reask_uses_the_remaining_pass_deadline()` › `timeout: Duration::from_millis(3000),`

Three seconds, not one, and the two child delays scale with it
(1200ms and 2250ms — the same 0.4 and 0.75 of the budget).

The property under test is a ratio: the first invocation fits,
and the re-ask does *not* fit in what the first one left. At one
second that held with 599ms of slack for a process spawn —
measured on an idle box, invocation 1 takes 401ms of the 1000ms
— and a saturated machine eats 599ms spawning a test binary
easily. When it does, the first invocation exhausts the whole
budget, `run_review` returns before invocation 2, and this test
fails with `invocations: 1` while asserting exactly the right
thing. Observed twice under load and 41 times green idle.

Scaling the clock is not weakening the assertion: the same
re-ask still must not fit. Witnessed by mutation — giving the
re-ask `cx.timeout` instead of the remaining budget still fails
this test at the 3s scale, exactly as it did at 1s.

## `fn quoted_fences_in_the_diff_cannot_close_the_block()` › `let diff = "diff --git a/README.md b/README.md\n+++ b/README.md\n \`

A markdown file whose content is itself a fenced block — the exact
shape that used to break out of the reviewer's ```diff fence.

## `fn quoted_fences_in_the_diff_cannot_close_the_block()` › `assert!(prompt.contains("diff"), "fence escalated: {prompt}");`

The fence around the diff must be longer than any run inside it.

## `mod tests` › `fn binding(agent: &str, model: &str) -> PassBinding {`

---------------------------------------------------------------------
§11.3/§11.5: the pass list
---------------------------------------------------------------------

## `mod tests` › `fn plan_with(second: Option<PassBinding>) -> ReviewPlan {`

Primary at frontier, a reachable OpenAI alternative, one task.

## `fn a_task_reviewed_by_its_own_author_rebinds_to_another_fam…` › `let plan = plan_with(None);`

The step-6 carried item: at the frontier rung both binders resolve
identically, so without this the reviewer IS the implementer.

## `fn a_different_model_from_the_same_family_is_left_alone()` › `let plan = plan_with(None);`

sonnet-written code judged by opus is a genuine second look. Rebinding
it would spend cross-vendor capacity on most of a run for nothing.

## `fn without_an_alternative_the_primary_stands_even_when_it_w…` › `let mut plan = plan_with(None);`

Single-vendor install: `plan_for` has already warned about this. The
review still happens — refusing would make upstroke unusable without a
second CLI installed.

## `fn a_second_opinion_adds_a_pass_and_suppresses_the_rebind()` › `let plan = plan_with(Some(binding("copilot", "gpt-5.3-codex")));`

The trap: rebinding the primary here would resolve BOTH passes to
copilot/gpt-5.3-codex, and opus-written code would lose its Anthropic review
entirely — strictly worse than the self-review being avoided.

## `fn the_probe_set_separates_required_agents_from_the_optiona…` › `let plan = plan_with(Some(binding("copilot", "gpt-5.3-codex")));`

The alternative is opportunistic, so its probe may fail without
taking the run down; everything else is load-bearing.

## `mod tests` › `fn scratch_config(name: &str, body: &str) -> Config {`

---------------------------------------------------------------------
plan_for: what each task's passes resolve to, before anything is spawned
---------------------------------------------------------------------

## `mod tests` › `fn no_pools() -> std::path::PathBuf {`

An explicit pools path with no pools in it, built once.

A real, empty file rather than an absent one: an explicit pools path
that does not exist is a hard error, and `None` would reach for the
operator's own `~/.upstroke/pools.toml`. Created once because every caller
wants the same bytes, and rewriting one shared path from parallel tests
truncates it under a reader — `name` above is unique per test, this was
the one file they all shared.

## `mod tests` › `fn auth_plan(cfg: &Config) -> (Plan, Vec<ResolvedChain>) {`

A one-task plan whose paths can match an override.

## `fn a_second_opinion_that_cannot_resolve_refuses_and_says_wh…` › `let cfg = scratch_config(`

Step-6 finding #10's posture. The operator asked for two families;
silently giving them one is the failure that finding exists to stop.

## `fn a_single_vendor_build_warns_that_a_task_will_review_itse…` › `let cfg = scratch_config(`

The visible half of the step-6 carried item: the run continues, but
it says the check is weaker than it looks.

## `fn a_single_vendor_build_warns_that_a_task_will_review_itse…` › `let mut quiet = Vec::new();`

With the second vendor present there is nothing to warn about.

## `fn a_task_that_never_runs_at_the_review_tier_is_not_warned_…` › `let cfg = scratch_config(`

Only a chain that can actually reach the reviewer's own binding is a
self-review risk; warning about the rest is noise that trains people
to ignore the warning that matters.

## `fn review_disabled_and_a_second_opinion_asked_for_is_a_cont…` › `let cfg = scratch_config(`

One key says judge nothing, the other says judge twice. Picking a
winner silently would be the engine deciding how much verification
the operator meant.

## `fn a_pinned_review_tier_still_gets_a_cross_family_partner()` › `let cfg = scratch_config(`

A pin fixes the primary; the second opinion is chosen relative to
whatever the pin landed on, not to the catalog's default.

## `fn the_recorded_plan_survives_the_wire()` › `let mut plan = plan_with(Some(binding("copilot", "gpt-5.3-codex")));`

It rides on `run_started`, so a resume reads back exactly what the
run resolved (§15).

## `fn the_recorded_plan_survives_the_wire()` › `let empty: ReviewPlan = serde_json::from_str("{}").expect("absent field defaults");`

A log written before step 9 has no such field at all.

## `fn the_second_opinion_prompt_is_independent_and_says_so()` › `assert!(prompt.contains("find reasons this change should NOT be accepted"));`

Whatever the lens adds, the step-6 guards still hold.

## `fn the_second_opinion_prompt_is_independent_and_says_so()` › `let mut plain = cx;`

And the acceptance pass is unchanged by any of it.

## `mod tests` › `fn the_obliged_lenses_do_not_depend_on_who_implemented() {`

**The obliged lenses do not depend on who implemented the change.**

[`obliged_lenses`] hands `passes_for` the primary binding as a stand-in
implementer, and the whole of why that is sound is that the answer does
not vary in that argument: rule 2 rebinds *who applies* the acceptance
lens and never *whether it runs*, and rule 1 turns on a configured
second opinion rather than on the author. Measured over the cross
product rather than argued, because the fold now judges a candidate's
success against this answer without having the implementer's binding to
hand.

The implementers include the primary itself — which triggers the rebind
— and the alternative, so the case that would differ if the claim were
wrong is in the grid rather than adjacent to it.

## `fn the_obliged_lenses_do_not_depend_on_who_implemented()` › `("second alone", None, None, Some(&second)),`

No primary is `None ⟺ review disabled`, and nothing else can
resurrect a pass — not even a configured second opinion.
