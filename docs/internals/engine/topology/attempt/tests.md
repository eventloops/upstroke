# `src/engine/topology/attempt/tests.rs`

Extended notes for [`src/engine/topology/attempt/tests.rs`](../../../../../src/engine/topology/attempt/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

`T-ATTEMPT`: five ordering clauses and nine tabled prefixes.

## `const WORKED: &[u8] = b"the agent edited this, and the capture stages it\n";`

The bytes an "agent" leaves in the task worktree.

A file the fixture repository does not already carry, so the blob it stages
is one this test can find in the index by name and one nothing else in the
object store could be.

## `fn agent_edits(worktree: &Path) {`

The engine never edits a file; an agent does. A fake runner runs nothing, so
the test writes what the worker would have written.

## `struct Process {`

Every invocation identity of an attempt, and its ledger and slot table.

A helper rather than a repeated block, because the two ledgers are
process-lifetime state that must be *one* per run: a test that built a fresh
[`SlotAssertion`] per call would assert a single slotted invocation against
an empty table and never see the overlap the assertion exists to catch.

## `impl Process` › `fn balances(&self) -> bool {`

The two process-end conditions, together.

## `macro_rules! context {`

The context one attempt runs in, built from disjoint fields of the run.

## `fn index_blobs(worktree: &Path) -> Vec<String> {`

The blob ids the task worktree's index holds.

## `fn unreachable_ephemeral_commits(base: &Path) -> Vec<String> {`

Every unreachable commit whose message is the ephemeral snapshot input's.

The message is `WorkspaceManager::snapshot_commit_tree`'s own, so this
identifies the object by what wrote it rather than by an id the killed child
never got to report. `--no-dangling` is already applied by
[`unreachable_objects`], so what comes back is exactly R27.

## `fn attempt_started_is_durable_before_any_spawn() {`

---------------------------------------------------------------------------
O23, O25, O26, O27 — the order of one clean attempt
---------------------------------------------------------------------------

## `fn attempt_started_is_durable_before_any_spawn() {`

**O23.** `attempt_started` is durable before the worker exists.

The runner records every request it is given, so "before any spawn" is
checkable directly: at the moment the first request arrives the log on disk
must already carry the event. It is asserted two ways for the reason O21's
test gives — the harness order proves the *sequence*, and reading the bytes
back proves the append was not merely issued first but landed first.

## `fn attempt_started_is_durable_before_any_spawn()` › `let ran = run.runner.ran();`

**The oracle.** Every record above is read after both the append and the
spawn, so a `start` that spawned first and appended afterwards leaves all
of them identical — measured, this test stayed green under exactly that
reordering. `durable_at_spawn` is the log as it stood *at the instant the
process was requested*, which is the only moment the clause is about.

## `fn every_process_of_an_attempt_is_recorded_reviewers_included() {`

**Every Runner process of an attempt is in the ledger, reviewers included.**

`permits.protocol`: "the invocation ledger records registered/completed/
cancelled" and R4 is "every Runner process registered exactly once, settled
exactly once". A review pass reaches the Runner through `run_review` with
the raw handle, so the worker and the gates were recorded and the reviewers
were not — the ledger balanced the whole time, because an unregistered
process is not an unsettled one. Balance is not the assertion; the **count**
is.

## `fn a_refused_gate_ends_the_set_and_its_cause_survives() {`

**A refused gate ends the gate set, and its cause survives.**

`gates::run_all` — the legacy authority — `return`s the first `GateFailure`
rather than running the rest. Two consequences, and this asserts both.

A refused gate must not buy the gates after it: the diff is already
rejected, and every later gate is spend on a verdict that cannot change.

And the **first cause must survive**. This fixture's second gate would exit
127 — a spawn-shaped failure. A driver that ran it and let it overwrite the
real cause would hand `ladder::next_step` an infrastructure failure where a
gate rejection happened, and the ladder prices those differently: one is the
implementer's, the other is not.

## `fn a_refused_gate_ends_the_set_and_its_cause_survives()` › `let second = plan.gates[0].clone();`

Two gates: the first refuses, the second would fail to spawn.

## `fn a_refused_gate_ends_the_set_and_its_cause_survives()` › `assert_eq!(`

**The gate's diagnostic reaches the ladder.** §11.1 makes the 8-KiB tail
the feedback §11.4 sends back to the same rung; the driver built its
`GateFailure` with `log_tail: String::new()`, so a rejected attempt was
retried knowing the exit code and nothing else.

## `fn a_refused_gate_ends_the_set_and_its_cause_survives()` › `assert!(`

And the gate is named, not numbered: an operator reading `gate 0` has to
count their own config to find out which one rejected the task.

## `fn capture_precedes_the_snapshots_and_every_snapshot_commits_before_its_intent() {`

**O25, O26 and O27** in one clean attempt, read off the shared harness.

One test rather than three, because the clauses are one chain and splitting
them would let a reordering pass two of the three: a snapshot taken before
the capture, an intent written before the commit, or a reviewer running on
the gate set's snapshot are all failures of *the same sequence*.

The last position asserted is the last `Snapshot.Remove`, which is what
makes O27 checkable inside this lane at all: `candidate.rs` owns the
commit-tree, so what this module owes is that nothing here reaches it and
that every judgement is finished before anything could.

O26 is asserted **per snapshot**, from a fence that advances past each
one's add. A comparison of first observations would be a comparison of the
gate set's triple and of nothing else — the two reviewer snapshots would
execute unasserted, and a reviewer path that wrote its intent before its
commit would pass. The count check above the loop is what makes the loop
exhaustive rather than merely repeated.

## `fn capture_precedes_the_snapshots_and_every_snapshot_commit…` › `let review_inputs = run.review_inputs();`

Built before the context borrows `run` mutably.

## `fn capture_precedes_the_snapshots_and_every_snapshot_commit…` › `let assessed = context!(run, process)`

Through the production phase, over the same diff the reviewers are
shown: a fixture-built `Assessment` could show the judge a diff the
cheap rungs never saw.

## `fn capture_precedes_the_snapshots_and_every_snapshot_commit…` › `&|pass| crate::review::ReviewInvocations {`

Caller-supplied, ordinal included: nothing pass-shaped is
minted inside `judge`, so PR8's merge verification can
supply its `SequenceIdentities` here without a redesign.

## `fn capture_precedes_the_snapshots_and_every_snapshot_commit…` › `let stage = run.must_order_of(STAGE, HookPhase::Before);`

O25: both capture sites, in order, before any snapshot effect.

## `fn capture_precedes_the_snapshots_and_every_snapshot_commit…` › `let snapshots = 1 + plan.reviewers.len();`

O26, once per snapshot rather than once per test. Three snapshots are
created here — one for the gate set and one per reviewer — and comparing
*first* observations compares only the gate set's triple: a reviewer
snapshot that wrote its intent before its ephemeral commit would leave
that triple untouched and pass. Each iteration takes its fence past the
previous snapshot's add, so the three positions it compares are that
snapshot's own.

## `fn capture_precedes_the_snapshots_and_every_snapshot_commit…` › `assert!(judgement.accepted());`

O27: everything judged, and nothing here is a commit-tree.

## `fn capture_precedes_the_snapshots_and_every_snapshot_commit…` › `assert_eq!(`

The tree the capture produced is the tree the snapshots were taken of.

## `fn gates_and_reviewers_run_on_fresh_exact_snapshots_and_never_in_the_task_worktree() {`

**`decisions.workspace_candidates.snapshots`.** Gates and reviewers execute
only in exact snapshots, one per role, never reused, and never in the task
worktree.

Four claims, and the fourth is the one a weaker test misses: "worker
worktrees and the staging worktree are **never** used for verification
processes". Every workspace the runner was given is checked against the task
worktree, so a `judge` that ran a gate in place would fail here rather than
merely producing a snapshot nobody used.

"Never reused" is checked by counting distinct workspaces, not by counting
snapshot adds: three adds that all returned one path would pass a count of
adds and fail this.

## `fn gates_and_reviewers_run_on_fresh_exact_snapshots_and_nev…` › `let review_inputs = run.review_inputs();`

Built before the context borrows `run` mutably.

## `fn gates_and_reviewers_run_on_fresh_exact_snapshots_and_nev…` › `let assessed = context!(run, process)`

Through the production phase, over the same diff the reviewers are
shown: a fixture-built `Assessment` could show the judge a diff the
cheap rungs never saw.

## `fn gates_and_reviewers_run_on_fresh_exact_snapshots_and_nev…` › `&|pass| crate::review::ReviewInvocations {`

Caller-supplied, ordinal included: nothing pass-shaped is
minted inside `judge`, so PR8's merge verification can
supply its `SequenceIdentities` here without a redesign.

## `fn gates_and_reviewers_run_on_fresh_exact_snapshots_and_nev…` › `let workspaces: Vec<PathBuf> = run`

**Where the processes actually ran, not where the judgement says they
ran.** This used to read `Verdict::workspace` from both lists, and
`Judgement.reviews` is now `Vec<ReviewRecord>` — a wire type, which has
no path to carry and should not grow one. The runner's record is the
better evidence anyway: it observes the request each process was spawned
with, so a `judge` that reported one workspace and spawned in another
fails here, and the old assertion could not have seen that.

## `fn gates_and_reviewers_run_on_fresh_exact_snapshots_and_nev…` › `let distinct: std::collections::BTreeSet<&PathBuf> = workspaces.iter().collect();`

One shared snapshot for the gate set; one fresh per reviewer.

## `fn gates_and_reviewers_run_on_fresh_exact_snapshots_and_nev…` › `assert!(`

Cleaned on completion: nothing survives the judgement.

## `fn gates_take_no_slot_and_the_worker_and_reviewers_do() {`

**`permits.agent_pool_slots`.** The worker and the reviewers take a slot
pair; the gates take none.

The exclusion is the whole content of the clause — "gate invocations and the
shell probe acquire no slot" — and a scheduler that gave a gate one would
halve the parallelism of every run without failing anything else. It is
asserted from **both** sides: [`is_slotted`] over each identity, and the
[`SlotAssertion`] refusing a gate outright.

## `fn gates_take_no_slot_and_the_worker_and_reviewers_do()` › `let assessed = context!(run, process)`

Through the production phase, over the same diff the reviewers are
shown: a fixture-built `Assessment` could show the judge a diff the
cheap rungs never saw.

## `fn gates_take_no_slot_and_the_worker_and_reviewers_do()` › `&|pass| crate::review::ReviewInvocations {`

Caller-supplied, ordinal included: nothing pass-shaped is
minted inside `judge`, so PR8's merge verification can
supply its `SequenceIdentities` here without a redesign.

## `fn gates_take_no_slot_and_the_worker_and_reviewers_do()` › `let ran = run.runner.ran();`

Every process really went through the run's Runner, with the identity
this attempt assigns and the seat its role gives it.

## `fn retained_tree(worktree: &Path) -> String {`

---------------------------------------------------------------------------
O24 — the retry
---------------------------------------------------------------------------

## `fn retained_tree(worktree: &Path) -> String {`

The retained cumulative tree of `worktree`, staged.

A retained generation "holds the retained cumulative tree", and that is what
its retry verifies against — not the base. Producing it here writes objects,
which is what `git write-tree` does and why `Worktree.Verify` may not run it
(`PR5-CONF-002`).

## `fn settle_retry(`

The first two steps of O24, which are [`settle::retry`]'s: the provisional
`{pipeline}` reservation and the **one** `Worktree.Verify` against the
retained cumulative tree.

Driven through the production seam — [`ManagedWorktrees`] over the run's
real [`WorkspaceManager`] — rather than through a double, because the join
between the clause's two owners is the thing under test. `attempt.rs` has no
retry entry point of its own, so a test that reached the append without
going through here would be covering a composition no coordinator can take.

## `fn authorized_plan(run: &Run, authorized: &AttemptStarted4) -> AttemptPlan {`

The plan this module appends, built from the event `settle::retry`
authorized.

Every field the fold checks is taken from that event rather than written as
a literal beside it. A plan that disagreed with the authorization would then
be the fold's refusal at the append, which is the whole point of the two
halves naming the same attempt.

## `fn a_retry_verifies_once_then_appends_then_spawns() {`

**O24, across both of its owners.** A retry verifies **once**, then appends
exactly the event that verification authorized, then spawns.

The clause is "reservation, worktree verification, `attempt_started`
(retry), spawn" and it has two owners. [`settle::retry`] takes the
`{pipeline}` reservation and performs the single `Worktree.Verify`;
[`AttemptContext::start`] appends and spawns. There is no retry entry point
on this side, so this test drives the join, and the order is asserted as
positions in the one harness list plus the runner's own log — a verify after
the append would let a retry start against a worktree nothing had looked at,
and a spawn before the append would put a paid-for process outside the log.

**The count of verifications is an assertion, not a detail.** A second
observation on the attempt side would be a second implementation of O24's
verification, and its refusal would be a pre-append failure — which
`permits.provisional_reservations` requires to cancel the reservation. But
the cancellation lives in the *first* verify's failure branch, and that
verify passed, so the branch is not taken: the reservation would be neither
converted nor cancelled. One observation is what makes the reservation's two
outcomes exhaustive, and `count_after(VERIFY)` is where that is checked.

The quiescence is `HoldsTree`, the retained generation's form of the check,
and the tree is deliberately made to differ from the base's so a
verification against `AtBase` could not pass in its place.

## `fn a_retry_verifies_once_then_appends_then_spawns()` › `let mark = run.mark();`

Steps one and two, which are `settle::retry`'s.

## `fn a_retry_verifies_once_then_appends_then_spawns()` › `let retry = authorized_plan(&run, &authorized);`

Steps three and four, which are this module's.

## `fn a_retry_verifies_once_then_appends_then_spawns()` › `assert_eq!(`

"Reused" as a fact about the worktree rather than as a tag any call
returned: nothing was removed after the mark, and the cumulative tree the
generation retained is still the one the worktree holds.

## `fn a_retry_verifies_once_then_appends_then_spawns()` › `let durable = run.emitter.durable_events();`

The two owners named the same attempt. This module builds its
`attempt_started` from `dispatched` and the plan, and `settle::retry`
built its own from the fold; the bytes on disk are what says they agree.

## `fn git_dir(worktree: &Path) -> PathBuf {`

The git directory of a linked worktree, asked of Git rather than derived.

`<worktree>/.git` is a **file** in a linked worktree and the administrative
directory it points at is elsewhere, so deriving the path here would be a
second implementation of `workspace_manager`'s own `git_dir_of`.

## `fn a_retry_whose_retained_worktree_fails_verification_closes_and_destroys_nothing() {`

**INV-06 / O24.** A retry whose retained worktree fails `Worktree.Verify`
closes the generation, cancels its reservation, and destroys nothing.

`decisions.workspace_candidates.generation` gives the failure two recoveries
and they are not interchangeable: "failing verification an OpenNoAttempt or
repair worktree is removed with force and recreated, **and a RetainedIdle
generation is closed with `generation_closed{WorktreeMissing}`**". A retry
that took the first branch would force-remove the worktree — and a retained
worktree's whole content is a cumulative tree that **no base can be re-cut
into**, which is what INV-06's "never recreated" protects — and would then
append `attempt_started(retry)` carrying `resume_session`, so the next
worker would run against an empty tree and be gated as if it were the
retained work. The append is durable before any caller sees the outcome, so
there is no later place to catch it.

The recovery is driven end to end here rather than only observed: the
closure `settle::retry` builds is appended through the same fold-checked
emitter every other event uses, so "the generation closes" is a transition
the fold accepted and not a struct this test looked at.

Six assertions, and each is a different way the destructive branch or a
stranded reservation would show: nothing was removed, nothing was appended
before the closure, no process was asked for, the tree the worktree holds is
byte-for-byte the tree it held, the generation ends `Closed` rather than
rebuilt, and the `{pipeline}` reservation is **cancelled** —
`permits.provisional_reservations` requires "cancellation on any pre-append
failure", and a retry entry point that refused *after* this verify passed
would leave it held with nobody to settle it.

The residue planted is `index.lock`, which is exactly what the interrupted
Git command in the failure sequence leaves, and it is the cheapest way into
a failing verify that does not itself disturb the thing being protected.

## `fn a_retry_whose_retained_worktree_fails_verification_close…` › `let lock = git_dir(&dispatched.worktree).join("index.lock");`

An interrupted Git command, which is the case `Worktree.Verify` exists
for and the one the failure sequence describes.

## `fn a_retry_whose_retained_worktree_fails_verification_close…` › `assert_eq!(`

It looked, which is what stops every assertion below being about a
function that returned without doing anything.

## `fn a_retry_whose_retained_worktree_fails_verification_close…` › `run.emitter`

The recovery itself, through the fold that has to accept it.

## `fn a_retry_whose_retained_worktree_fails_verification_close…` › `assert!(`

The work itself. The lock is what `Worktree.Verify` refused for, so it
comes off before the index can be written out — removing it is this
test's own act and not part of what the retry did.

## `fn a_refused_slot_acquisition_settles_the_registration_it_took() {`

**R4 / `permits.protocol`.** A slot acquisition the assertion refuses must
not leave the invocation registered.

"The invocation ledger records registered/completed/cancelled exactly once
and **balances at process end**", and [`InvocationLedger::balances`] states
that as "no entry is `Running`". The register happens before the pair is
asked for, so a refused acquisition that propagated straight out would
abandon a `Running` entry — and at process end that entry is
*indistinguishable* from a process this coordinator genuinely lost. A leak
check that cannot tell a bookkeeping mistake from a lost process reports
both or neither.

The refusal is driven the way a real one arrives: a pair is already held.
At `max_parallel = 1` [`SlotAssertion`] refuses rather than queues, which is
its whole purpose, so this is the refusal the substrate actually produces
rather than a synthetic error injected at the seam.

The held pair is deliberately **not** registered in the ledger, so the
ledger's own balance is a statement about the worker alone: after the
refusal nothing is running, one entry is cancelled, and none is completed.

## `fn a_refused_slot_acquisition_settles_the_registration_it_t…` › `let squatter = AttemptIdentities::new(ALPHA, GenerationId(9), AttemptNumber(9)).worker();`

Something else holds the one pair. `cancel_all_running` is not involved:
this invocation is in the slot table and not in the ledger.

## `fn a_refused_slot_acquisition_settles_the_registration_it_t…` › `assert_eq!(`

What actually happened, so the assertions above are about the state this
test claims to have driven: O23's append is durable and no process was
ever asked for.

## `fn attempt_kill_child() {`

---------------------------------------------------------------------------
T-ATTEMPT — the kills
---------------------------------------------------------------------------

## `fn attempt_kill_child() {`

The child every `T-ATTEMPT` kill test spawns.

One child with a site switch rather than six children, because every one of
them needs the same prefix built — a run, a dispatch, an attempt, and for
most of them a capture — and six copies of that prefix would be six chances
for one of them to build a different state than its name claims.

## `fn attempt_kill_child()` › `run.arm(STAGE, HookPhase::Before, Injection::Kill);`

Sub-prefix (a): the worker ran and no capture has begun.

## `fn attempt_kill_child()` › `let tree = retained_tree(&dispatched.worktree);`

The retry's own in-flight prefix, in the generation that retained,
built through both owners of O24 exactly as the parent test's
non-kill sibling does.

## `fn attempt_kill_child()` › `let _ = context!(run, process).capture(dispatched.site());`

In flight, and now killed inside it: the arming is at the capture
because `retry` itself must succeed for the generation to be
`InFlight { attempt: 2 }` when the coordinator dies.

## `fn attempt_kill_child()` › `run.arm(WRITE_TREE, HookPhase::After, Injection::Kill);`

Sub-prefix (b): the staged blob and tree objects exist and are
referenced only by the task worktree's index.

## `fn attempt_kill_child()` › `"after_snapshot_commit" => run.arm(SNAPSHOT_COMMIT, HookPhase::After, Injection::Kill),`

Sub-prefix (c), the after phase: the id was read and nothing durable
claims the commit.

## `fn attempt_kill_child()` › `"id_unread" => run.arm_point(`

Sub-prefix (c), the `IdUnread` point: the child exited with the
object written and the coordinator never recorded the id. Armed on
the shared harness, because a point is a real injection coordinate
and `IdUnread` supports `Kill` alone.

## `fn attempt_kill_child()` › `"after_snapshot_add" => run.arm(SNAPSHOT_ADD, HookPhase::After, Injection::Kill),`

Sub-prefix (d): the intent is durable and the snapshot worktree
registered, so its HEAD holds the ephemeral commit (R24).

## `fn attempt_kill_child()` › `let assessed = context!(run, process)`

Through the production phase, over the same diff the reviewers are
shown: a fixture-built `Assessment` could show the judge a diff the
cheap rungs never saw.

## `fn attempt_kill_child()` › `&|pass| crate::review::ReviewInvocations {`

Caller-supplied, ordinal included: nothing pass-shaped is
minted inside `judge`, so PR8's merge verification can
supply its `SequenceIdentities` here without a redesign.

## `fn adopted_generation(run: &Run) -> Dispatched {`

The dispatched generation of the child's run, rebuilt in the parent.

## `fn kill_during_attempt_settles_interrupted_and_redispatches_new_generation() {`

**`T-ATTEMPT`.** A kill during an attempt settles `attempt_interrupted`, and
the task is redispatched into a **new** generation.

Every clause of the tabled resume action is asserted: the terminal is
appended with the lease disposition its kind gives, the generation goes
`Closed`, the task returns `Pending`, the residue is discarded, and the next
dispatch opens generation 1 rather than reopening generation 0. The last is
the one that matters most — "later dispatch **new generation** (spend may
repeat)" — because a recovery that reused the generation would silently
claim the dead coordinator's unknown spend as its own.

## `fn kill_during_attempt_settles_interrupted_and_redispatches…` › `let next = run.dispatch(ALPHA, 1);`

The redispatch: a new generation, at the same base, and the fold accepts
it — which it would not if the old generation were still open.

## `fn kill_after_capture_leaves_index_referenced_objects_then_scrub_releases_them() {`

**`T-ATTEMPT`, sub-prefix (b).** The staged objects are referenced by the
task index while the worktree stands, and the forced scrub releases them to
R27.

Both halves are the claim. "Referenced only by the task worktree index (R9)"
is checked by `git fsck --unreachable` **not** reporting the blob — the index
is one of fsck's roots, so an object it holds is reachable — and the release
is the same query answering differently after the scrub. Asserting only the
second would pass for an object that was already unreachable before the
scrub ran.

## `fn kill_after_ephemeral_snapshot_commit_before_worktree_leaves_gc_owned_object() {`

**`T-ATTEMPT`, sub-prefix (c).** An ephemeral snapshot commit written before
any intent is Git's, and there is nothing to reclaim.

The object is identified by the message `snapshot_commit_tree` writes rather
than by an id, because the point of this prefix is that the coordinator died
without recording one. What is asserted beside its presence is the
*absence* of everything that would make it the engine's: no snapshot intent,
no snapshot worktree, and after the tabled recovery the object is still
there — "an ephemeral commit without a snapshot … is left to Git (nothing to
reclaim)". An engine that pruned it would be establishing authority over the
object store.

## `fn kill_at_snapshot_commit_id_unread_point_leaves_gc_owned_object() {`

**`T-ATTEMPT`, sub-prefix (c), the `IdUnread` point.**

The same durable residue as the test above and a different way of reaching
it: the child exited with the object written and the coordinator never read
the printed id. `Object.SnapshotCommitTree` exposes the point and
`SubEffectPoint::IdUnread` supports **`Kill` only** — it has no error-return
contract, and inventing one would be inventing a resume action nothing
tables.

What proves the kill landed *at the point* rather than somewhere else is
the child's own `unreachable!`: nothing else in that path is armed, so a
point that was never consulted would let `judge` finish and the child would
fail rather than die.

## `fn kill_at_snapshot_commit_id_unread_point_leaves_gc_owned_…` › `let refusal = run`

The point supports one mode, and arming the other is refused rather than
silently ignored — which is what stops a suite claiming coverage of an
error contract this point does not have.

## `fn kill_after_snapshot_add_reclaims_snapshot_and_releases_its_commit() {`

**`T-ATTEMPT`, sub-prefix (d).** A snapshot whose add completed is reclaimed
by its intent, and its ephemeral commit returns to R27.

The two states are asserted on either side of the reclaim: while the
snapshot stands its HEAD references the commit (R24), so fsck does not
report it; once the snapshot is removed nothing does, so fsck does. A test
that checked only the second would pass against a snapshot that never
referenced the commit at all.

## `fn kill_during_retry_attempt_closes_generation() {`

**`T-RETRY` meeting `T-ATTEMPT`.** A kill during a retry closes the
generation it was retrying.

The distinction this holds is the one `generation` draws: "a same-session
retry re-enters InFlight in the **same** generation", and an interruption of
it closes that generation rather than retaining it — "the generation does
*not* survive an interruption". So the recovered state is generation 0
`Closed` with attempt **2** named in the terminal, and the retained session
is gone with it.

## `fn halt_cancels_in_flight_attempt() {`

**ST-17.** "at Halted the same terminal is appended by cancellation".

Two things, and only the second needs constructing. The terminal is the
same `attempt_interrupted` — an interruption is a statement about a
coordinator, not a judgement of the work — with a detail that says the run
halted.

The in-flight *invocation* is built directly, because a synchronous
substrate cannot leave one any other way: `Runner::run` returns before the
coordinator can observe a halt, so a registration that never settled is
exactly the state a halt arriving **during** a run leaves, and the honest
way to test the cancellation is to put the ledgers in it. Both ledgers are
then required to balance, which is the process-end condition
`permits.protocol` states.

## `fn halt_cancels_in_flight_attempt()` › `let reviewer = started.identities.review_pass(0, 0);`

A reviewer whose completion never ran, holding the pair its role takes.

## `fn stage_elements() -> Vec<ResidueElement> {`

---------------------------------------------------------------------------
The `Internal` residue class of the two capture commands

`command_internal_sub_effects` gives this class two kinds of evidence and
both are here: **(i)** synthetic construction of every registered element,
each classifying `Internal` and recovering by the tabled action, and **(ii)**
a real-command kill-sampling record with the observed-class histogram. It is
deliberately *recovery-proven rather than execution-observed*: a killed
`git add` is not a hook point, so nothing can stand inside it and say "this
is what it left". A never-hit `Internal` does not fail; an **unclassifiable**
residue does.
---------------------------------------------------------------------------

## `fn stage_elements() -> Vec<ResidueElement> {`

The three elements `Object.CandidateStage` registers, planted one at a time.

Read off the frozen enum rather than written out, so an element added to the
site fails this until it is constructed — `bounded_grid`, the failure this
project has recorded three times, is a grid over the elements its author
remembered.

## `fn unstaged_work(worktree: &Path) {`

The half of an interrupted `git add` that is not an element: work in the
tree that the command had not finished staging.

`command_internal_sub_effects` defines the class as the elements "**with the
after-phase reference absent**", and the order in `classify_object_residue`
is that sentence's — the after-phase reference decides `After` first, and
only its absence lets residue decide `Internal`. For
`Object.CandidateStage` the after-phase reference is "an index that reflects
the working tree", so a worktree whose index is clean classifies `After`
however much R27 residue is lying around, and correctly: a `git add` that
finished is not one that was killed. Measured — a temporary object file
planted in a pristine worktree classifies `After`.

So every synthetic element is planted into a worktree that also carries
unstaged work, which is what a `git add` killed part-way through leaves.

## `fn plant_stage_residue(base: &Path, worktree: &Path, element: ResidueElement) {`

Plant one element of `Object.CandidateStage`'s residue in `worktree`.

The two object-store elements are R27 — Git's — and live in the **shared**
object directory, which is why they survive the scrub below while the
index lock does not. That difference is the point of planting them
separately rather than as one blob of "residue".

## `fn plant_stage_residue(base: &Path, worktree: &Path, elemen…` › `write_file(`

An orphan blob: written into the store and referenced by nothing.
Untracked on purpose — the index must not hold it, or it would be
reachable and the classifier would be right to ignore it.

## `fn synthetic_git_add_residue_unreferenced_objects_and_index_lock_then_forced_scrub_conver…`

**`T-ATTEMPT`, sub-prefix (b'), evidence (i).** Every residue element a
killed `git add` can leave, constructed, classified `Internal`, and
recovered by the tabled forced scrub.

**A repository per element.** Two of the three live in the *shared* object
store and are permanent until Git prunes them, so planting them in sequence
in one repository would leave the second element's slot carrying the first's
and a classifier that recognised only `UnreferencedObject` would answer
`Internal` for all three. Measured: it did.

Convergence is asserted for what the scrub owns and **not** for what it does
not. The lock leaves with the worktree's git dir; the orphan blob and the
temporary object file are R27 and stay, because "objects left unreferenced
by any of these prunings … are Git's" and an engine that pruned them would
be establishing authority over the object store, which `cleanup` forbids.

## `fn synthetic_git_add_residue_unreferenced_objects_and_index…` › `let target = ResidueTarget::new(&fixture.base).at(&worktree);`

Two controls, and the pair is what makes the `Internal` below mean
something. A classifier that answered `Internal` unconditionally
fails the first; one that ignored its element list and read only the
after-phase reference fails the second.

## `fn synthetic_git_add_residue_unreferenced_objects_and_index…` › `manager`

The tabled recovery: forced removal of the worktree, then its intent.

## `fn synthetic_git_add_residue_unreferenced_objects_and_index…` › `manager`

Idempotent, which `cleanup` requires of every reclaim.

## `fn synthetic_git_add_residue_unreferenced_objects_and_index…` › `let fixture = Fixture::created("synthetic-stage-all");`

And all three at once, which is the state a killed `git add` leaves.

## `const SAMPLED: [EffectSiteId; 2] = [STAGE, WRITE_TREE];`

---------------------------------------------------------------------------
Evidence (ii): the real-command kill-sampling record
---------------------------------------------------------------------------

## `const SAMPLED: [EffectSiteId; 2] = [STAGE, WRITE_TREE];`

The two commands `T-ATTEMPT`'s sub-prefix (b') names: "git add or write-tree
killed after writing objects and before publishing the index or cache-tree".

## `const SAMPLING_N: u32 = 8;`

The frozen sample count, per command.

`command_internal_sub_effects`: "the Git child of the site is killed at
uncontrolled points through the process funnel across N runs (N frozen per
site in the registry)". The claim each sample carries is *per sample* —
every observed residue classifies into exactly one class and recovers by the
classified action — and is not a coverage claim about the classes, which is
why N does not have to be large enough to hit `Internal`.

## `const HISTOGRAM: &str = "effects/attempt-residue-histogram.json";`

The observed-class histogram, which is a property of the machine and cannot
be pinned.

`effect_site_inventory.outputs` asks for "sampling N **and observed-class
histogram**" per site. Which class a sample lands in is a race between the
kill and Git, so it goes to a machine-varying evidence file rather than into
a byte-compared artifact — the same split, and for the same reason, as
`effects/residue-histogram.json`. This file is that one's `T-ATTEMPT`
sibling and is written to a **different path** so the two samplers cannot
overwrite each other's record.

## `struct Sample {`

One sample: what it ran, which rung its kill was aimed at, when the kill
actually fired, how the child ended, and what the classifier answered.

## `struct Sample` › `ran: Option<std::time::Duration>,`

The child's **own** duration, when it finished before the kill.

`None` when the kill got there first, which is the case this harness
wants. When every sample is `Some`, the schedule raced a number that
does not describe these runs, and these are the durations to rebuild it
from.

## `fn bulk(worktree: &Path) {`

Enough work in the worktree that the sampled command has a middle to be
killed in.

## `fn sampled_argv(site: EffectSiteId) -> Vec<String> {`

The exact argv the site's funnel runs.

Read from the funnel's own frozen lists, never transcribed: a funnel that
grew a flag beside a transcribed copy would leave this sampler killing a
stale command with every assertion here still green.

## `fn populate_for(site: EffectSiteId, worktree: &Path) {`

Populate a worktree for `site`, leaving it in the state the funnel would
find it in.

## `fn populate_for(site: EffectSiteId, worktree: &Path)` › `git(worktree, &["add", "-A"]);`

`write-tree` reads an index, so the bulk has to be in one.

## `fn sample_slot(generation: u32) -> crate::workspace_manager::Slot {`

A slot of the sampling fixture.

## `fn measure_budget(site: EffectSiteId, fixture: &Fixture) -> std::time::Duration {`

How long the same command takes when nothing kills it.

Measured in a **probe slot of its own**, which is then removed. Measuring it
in the worktree the next sample will kill in makes the probe *perform* the
command first, and the samples then classify a fixture artefact rather than
a kill — the "environment assumption in a test" class this project has
recorded.
A duration the sampled command plausibly takes, measured **warm**.

**The first invocation is the one that lies.** A cold worktree pays for a
filesystem cache miss and, on Windows CI, for an antivirus scan of files it
has just seen created — so a budget taken from run one is inflated relative
to every run that follows, and a schedule derived from it puts every kill
after its child has already exited. Measured: two consecutive
`test (windows-latest)` legs at `b07b8cc` in which **zero of sixteen**
sampled kills landed, on a commit that changed one line of a Markdown file.

So one run is discarded as warm-up and the median of the next three is
taken. The median rather than the mean because the failure mode is a single
outlier, and a mean carries an outlier's weight into the schedule that a
median discards.

## `fn measure_budget(site: EffectSiteId, fixture: &Fixture) ->…` › `const PROBE_SLOTS: [u32; 4] = [9_996, 9_997, 9_998, 9_999];`

Slots the probes use, distinct from every sampled run's.

## `fn measure_budget(site: EffectSiteId, fixture: &Fixture) ->…` › `median(&measured[1..])`

Discard the warm-up, then the median of what is left.

## `fn median(durations: &[std::time::Duration]) -> std::time::Duration {`

The median of a non-empty slice of durations.

## `fn sample(site: EffectSiteId) -> Vec<Sample> {`

Sample one command `SAMPLING_N` times and classify what each kill left.

**Self-healing against an unrepresentative probe, and only against that.**
The schedule races a measured duration, so a budget that does not describe
the runs it schedules puts every kill after its child has exited and the
sampling observes nothing. When that happens the runs themselves are the
better measurement — an unkilled run ran to completion, so its duration is
the true one — and the schedule is rebuilt from their median and retried
**once**.

Bounded at one retry on purpose. A second miss is not an unlucky probe; it
is the kill failing to land at all, which is a defect this harness exists to
report. The caller's vacuity refusal is what reports it, and nothing here
weakens that assertion — this only removes the case where it fires for an
environment rather than for a bug.

## `fn sample(site: EffectSiteId) -> Vec<Sample>` › `let observed: Vec<std::time::Duration> = first.iter().filter_map(|sample| sample.ran).col…`

Premise failed: no kill landed mid-run. Every run therefore completed, so
every `ran` is a full duration and their median is the budget the probe
should have produced.

## `fn sample(site: EffectSiteId) -> Vec<Sample>` › `return first;`

Nothing landed and nothing finished either: the schedule is not the
explanation, so there is nothing honest to recalibrate from. Hand
back the first pass and let the caller's vacuity refusal report it.

## `fn sample_once(`

One pass of `SAMPLING_N` runs against `budget`, taking slots from
`slot_base` so a retry never reuses the first pass's worktrees.

## `let deadline = std::time::Instant::now() + after;`

Sleep the schedule, but notice if the child finishes first — and
record its OWN duration when it does. Wall time to the reap would
include this sleep, so an over-long schedule would report itself
back as the number it should have been, and the recalibration below
would inherit exactly the error it exists to correct. Measured: it
did, on the first version of this fix.
Poll to the deadline WITHOUT shortening it. Noticing that the child
finished is a measurement; acting on it is not. Breaking out early
and killing there fires the kill sooner than the rung it was aimed
at, which the shape assertions below refuse — measured on the
Windows guest, where a kill fired at 40.3ms against a 48.5ms rung.

## `fixture`

The tabled recovery for every class this prefix can leave: forced
removal of the worktree, then its intent. Idempotent for `None` and
`After`, which is why one action covers all three.

## `fn sampled_git_add_and_write_tree_child_kills_every_residue_classified_and_recovered() {`

**`T-ATTEMPT`, sub-prefix (b'), evidence (ii).** Real `git add` and
`write-tree` children, killed at N uncontrolled points each; every observed
residue classifies into exactly one class and recovers by that class's
action.

### What is asserted and what is only recorded

The class counts are **not** asserted: which class a sample lands in is a
race between the kill and Git, so a suite that required `Internal` would be
red whenever the machine was fast. What is asserted is that every sample
classified into one of the three and recovered, and that `unclassified` is
zero — an unclassifiable residue is durable state no tabled action recovers,
and that is the failure this evidence exists to exclude.

### The oracles that a green completion does not also satisfy

A sampler whose kills all missed would still spawn `2 × N` children, still
classify a legal residue from each, still recover, and still write its
evidence file — of *completion* residue, filed under the kill's name. Three
things separate the two, and all three are here:

* **the ladder**, asserted per command: N kills aimed at N distinct,
  increasing points, because the clause says "killed at **uncontrolled
  points**" and one fixed delay is one point sampled N times;
* **every child fired at**, asserted per command and exactly, because
  `fired` is written inside `KillableGitChild::kill` and a kill that was
  skipped leaves no record to count;
* **at least one kill landing**, asserted over the sampling as a whole,
  because only a wait status distinguishes a killed child from a finished
  one.

The floor is over the whole sampling and not per command deliberately.
`git add` measures roughly **1 in 8** on this project's machines — the
budget probe writes the very blobs the samples then find already in the
object store, so a sample runs in about a fifth of the time its ladder was
scaled to — and a per-command floor would stand on a margin of one sample
and be red on the next machine. The per-command counts are recorded in the
evidence file so the margin stays visible without being load-bearing.

## `fn sampled_git_add_and_write_tree_child_kills_every_residue…` › `let counted = |wanted: ObjectResidue| -> u32 {`

The classifier's answers, tallied here rather than by the code under
test: a histogram that counted a class under the wrong name agrees
with itself, and only a second expression over the same list can see
it.

## `fn sampled_git_add_and_write_tree_child_kills_every_residue…` › `let failed: Vec<Option<i32>> = samples.iter().filter_map(|s| s.failed.map(Some)).collect(…`

The premise of every count below: a child that failed on its own left
the fixture's residue rather than the kill's.

## `fn sampled_git_add_and_write_tree_child_kills_every_residue…` › `let shape: Vec<&Sample> = samples`

The ladder, per command shape rather than per site label: the
contract names two *commands*, and two sites that sampled one shape
would leave two records intact.

## `fn sampled_git_add_and_write_tree_child_kills_every_residue…` › `for sample in &shape {`

A kill fired at every child, and no earlier than the rung it was
aimed at. `fired` is the clock read inside the kill, so deleting the
wait moves it and deleting the kill removes it.

## `fn sampled_git_add_and_write_tree_child_kills_every_residue…` › `let landed: usize = per_site`

The kill itself, over the sampling as a whole. Nothing else in this
harness changes when `KillableGitChild::kill` stops killing.

## `fn sampled_git_add_and_write_tree_child_kills_every_residue…` › `let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(HISTOGRAM);`

The evidence file `outputs` asks for, written and read back.

## `fn a_failing_gate_rejects_the_judgement_and_its_snapshot_is_still_cleaned() {`

A judgement is not a constant: a gate that fails rejects the attempt, and
its snapshot is still cleaned.

Without this, [`Judgement::accepted`] could `return true` and every other
test here would stay green — all of them drive a runner whose processes
succeed. The cleanup half matters as much: `snapshots` says they are
"cleaned on completion", and a completion is not the same thing as a pass.

## `fn a_failing_gate_rejects_the_judgement_and_its_snapshot_is…` › `let review_inputs = run.review_inputs();`

Built before the context borrows `run` mutably.

## `fn a_failing_gate_rejects_the_judgement_and_its_snapshot_is…` › `let assessed = context!(run, process)`

Through the production phase, over the same diff the reviewers are
shown: a fixture-built `Assessment` could show the judge a diff the
cheap rungs never saw.

## `fn a_failing_gate_rejects_the_judgement_and_its_snapshot_is…` › `&|pass| crate::review::ReviewInvocations {`

Caller-supplied, ordinal included: nothing pass-shaped is
minted inside `judge`, so PR8's merge verification can
supply its `SequenceIdentities` here without a redesign.

## `fn a_failing_gate_rejects_the_judgement_and_its_snapshot_is…` › `assert!(`

**The reviewers do not run, and that is a deliberate change.** This test
used to assert that they ran after the failing gate and passed, because
the old `judge` ran every gate and then every reviewer unconditionally.
The legacy engine does not: §11.2 is "a strong reviewer judges the diff
against the acceptance criteria **only once the cheap checks pass**", and
`run_attempt` guards its review block on `failure.is_none()`. Buying a
frontier invocation to judge a diff the gates have already refused is
spend for information the run cannot act on.

## `fn a_malformed_captured_id_is_a_git_error_naming_where_the_value_came_from() {`

A capture id that is not an object id is a Git error, not a refusal
(PR #130, pass 3).

`git write-tree`'s output and the recorded base are the engine's own
values, so a malformed one is the tool or the engine misbehaving. Reaching
`ObjectId::new` with a `?` made it `UpstrokeError::Refused`, which says a
caller offered something it should not have. Witnessed by restoring that
`?`: the error becomes `Refused` and the first assertion fails.
