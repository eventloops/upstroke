# `src/engine/topology/candidate/tests.rs`

Extended notes for [`src/engine/topology/candidate/tests.rs`](../../../../../src/engine/topology/candidate/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `const ENV_BASE: &str = "UPSTROKE_TEST_CAND_BASE";`

The env keys a kill child reads. Named rather than spelled twice: the
parent sets them and the child reads them, and a typo in either half is
a child that panics for the wrong reason and a parent that reads a
directory nothing wrote.

## `fn make_dir(path: &Path) {`

-----------------------------------------------------------------------
Fixtures

Every effect here goes through the funnel that owns its site, in tests
as in production: `src/engine/topology/**` carries no module-level allow
of a governed lint, so this module may not name `std::fs`'s writers or
`std::process::Command` at all. Git runs through the process funnel
(`crate::runner::container::view::fixtures`, which is where the crate
already keeps that helper), directories are made by the run-directory
funnel, and everything under the execution root is `WorkspaceManager`'s.
-----------------------------------------------------------------------

## `fn make_dir(path: &Path) {`

A scratch directory tree, made through the run-directory funnel.

`rundir::create_public_dir` is `RunDir.CreatePublicDir` — the crate's
directory-creation funnel — driven here with production's no-op
observer so a fixture contributes nothing to the coverage evidence.

## `fn drop_dir(path: &Path) {`

Remove a fixture tree, through the funnel that owns tree removal.

## `struct Fixture {`

A real repository, a real private root, a manager over both, and one
task worktree carrying a staged edit.

## `struct Fixture` › `base_sha: CommitSha,`

The commit the task worktree was created at.

## `struct Fixture` › `task: Slot,`

The task worktree's slot.

## `struct Fixture` › `tree_sha: CommitSha,`

The tree the worker produced, staged behind that worktree's index.

## `impl Fixture` › `fn at(root: PathBuf) -> Self {`

Build the repository, the private root and the task worktree.

`root` is created by the caller when it has to be predictable
across processes, and by this function otherwise.

## `fn at(root: PathBuf) -> Self` › `let blob = git_fixtures::git_ok(`

The worker's edit, written by `git` because this module may not
write a file itself. `hash-object -w --stdin` puts the blob in
the object store and `update-index --add --cacheinfo` puts it
behind *this worktree's* index, which is exactly the R9 state
`Object.CandidateStage` leaves.

## `impl Fixture` › `fn new(tag: &str) -> Self {`

A fixture under a fresh scratch root.

## `impl Fixture` › `fn judged(&self) -> JudgedTree {`

What a judged tree hands the sequence, for this fixture.

## `impl Fixture` › `fn divergent_tree_commit(&self, hooks: &mut Hooks) -> (CommitSha, CommitSha) {`

A commit on the same base whose **tree is different**.

The shape `verify_object`'s parent check cannot tell from the real
candidate: same parent, same everything the fold used to keep, and
content no gate ran against. Built by staging a second blob before
writing the tree, so the difference is real rather than a relabelled
sha.

## `fn divergent_tree_commit(&self, hooks: &mut Hooks) -> (CommitSha, CommitSha) {` › `let worktree = self.manager.slot_path(&self.task);`

A second entry behind the same index. The blob's bytes do not
matter; the *path* is what moves the tree, and moving the tree is
the whole point.

## `impl Fixture` › `fn sibling_commit(&self, hooks: &mut Hooks) -> CommitSha {`

A second commit on the same base, differing only in its message.

A *sibling*: `verify_object` now checks the parent, so a test that
wants to reach a later refusal needs an object that passes the
identity check without being the recorded candidate. The base itself
no longer serves — its parent is not the base.

## `impl Fixture` › `fn unreachable(&self) -> Vec<String> {`

Every unreachable object in the repository, per `git fsck`.

## `impl Fixture` › `fn is_unreachable(&self, object: &str) -> bool {`

Whether `object` is unreachable per `git fsck --unreachable`.

## `impl Fixture` › `fn unreachable_commits(&self) -> Vec<String> {`

Every unreachable object that is a commit.

## `impl Fixture` › `fn object_present(&self, object: &str) -> bool {`

Whether `object` is in this repository at all.

## `impl Fixture` › `fn run_refs(&self) -> Vec<(String, String)> {`

Every ref under this run's namespace.

## `impl Fixture` › `fn task_admin_dir(&self) -> PathBuf {`

The git administrative directory of the task worktree — where an
interrupted command's `index.lock` lands.

## `struct Journal {`

-----------------------------------------------------------------------
The journal: a real schema-4 log and the fold over it
-----------------------------------------------------------------------

## `struct Journal {`

[`CandidateJournal`] over a real `events.jsonl` and a real fold.

Not a recording double. The claims this module makes are about durable
bytes and about what the fold refuses, so a journal that recorded
intentions would leave both untested — and the "once" of
`kill_after_candidate_prepared_appends_candidate_created_once` is
literally a count of lines in a file.

## `impl Journal` › `fn open(private: &Path, base_sha: CommitSha, hooks: &Hooks) -> Self {`

A log carrying `run_started`, a dispatch and a started attempt —
and **not** its settlement.

That is `transaction_fault_matrix[T-CAND-OBJ]`'s durable state
exactly ("attempt_started only"; "attempt unsettled"), which is the
state the commit-tree and the pin happen in.
The step after them is `candidate_prepared`, which is the
settlement — there is no separate one to append first.

## `impl Journal` › `fn resume(private: &Path, hooks: &Hooks) -> Self {`

Reopen an existing log and replay it: `resume is
replay-then-continue, and there is no second path`.

## `impl Journal` › `fn count(&self, kind: &str) -> usize {`

How many committed lines of `kind` the log carries.

## `struct ArmedEffects {`

-----------------------------------------------------------------------
The hook bundle, with a phase this suite can arm
-----------------------------------------------------------------------

## `struct ArmedEffects {`

The git funnels, recording into the shared [`HookHarness`] and
answering an armed injection at a hook **phase**.

[`HookHarness::arm`] takes a [`SubEffectPoint`] and `hook()` answers
`Proceed` to `Before` and `After` unconditionally, so a phase can only
be armed by a double. The recording still goes to the shared harness —
a double that kept its own log would take every site it touched out of
the coverage evidence.

## `impl EffectHooks for ArmedEffects` › `fn refusal_cause(&self) -> Option<String> {`

Forwarded, so a poison the inner observer found is reported as poison
and not as a fault this double armed.

## `struct Trace(Arc<Mutex<Vec<String>>>);`

The order the funnels ran in, which is what an ordering clause is about.

The shared [`HookHarness`] counts executions and does not keep their
order — deliberately, because coverage is a set question. O28 to O31 are
order questions, so this records the sequence beside it rather than
instead of it.

## `impl Trace` › `fn reset(&self) {`

Forget everything recorded so far.

The fixture's own appends — the dispatch and the attempt start —
are not part of any clause O28 to O31 states, and an ordering
assertion that carried them would be an assertion about the
fixture's prologue.

## `impl Trace` › `fn order(&self, of_interest: &[EffectSiteId]) -> Vec<String> {`

The recorded sequence, with everything not in `of_interest` dropped.

Filtered rather than compared whole: a fixture that also creates an
execution root and writes an intent runs those funnels too, and an
assertion over the unfiltered list would be an assertion about the
fixture.

## `struct TracedEvents {`

The append funnel, tracing into the same order and recording into the
same harness.

Wrapped rather than reused directly because an ordering clause that
mentions an append — O29, O30, O31 all do — cannot be checked from a
trace that only sees the git funnels.

## `struct Hooks {`

The five-family bundle, with [`ArmedEffects`] in front of the git
families and the shared harness behind all five.

## `fn region() -> PathSet {`

-----------------------------------------------------------------------
The frozen inputs of the fold
-----------------------------------------------------------------------

## `fn run_started(base_sha: CommitSha) -> RunStarted4` › `enabled: Some(true),`

Enabled: this fixture's `candidate_prepared` records carry a
passed `review` pass, and a run that froze verification off
obliges none — a combination `plan_for` cannot produce, since
its disabled branch resolves no `primary` either.

## `fn candidate_kill_child() {`

=======================================================================
T-CAND-OBJ — the object, the pin, and what a resume does with neither
=======================================================================

## `fn candidate_kill_child() {`

The child of the three T-CAND-OBJ kill tests.

One child for three prefixes, because the setup up to the kill is the
same in all three and a second child is a second thing to keep in step
with this module.

`Injection::Kill` is `std::process::abort()` — a real process death,
chosen because the claim is *what a coordinator that runs no cleanup
leaves on disk*, and an early `return` would unwind and prove something
weaker. The `unreachable!` below is what fails the test if an injection
silently stops killing.

## `fn candidate_kill_child()` › `"after-prepared" => {`

T-CAND-REF: `candidate_prepared` durable, and the coordinator
dies before the candidates ref exists. The whole point of this
prefix is that the parent then resumes from the child's own
durable log.

## `fn spawn_kill_child(fixture: &Fixture, which: &str) -> ProcessOutput {`

Run [`candidate_kill_child`] against `fixture` at `which`, and hand back
what the dead child's process left.

The spawn goes through the process funnel — `Process.Spawn` is the site
that owns starting a process, in a test as in production.

## `fn assert_killed(output: &ProcessOutput, which: &str) {`

The child died where it was armed rather than panicking somewhere else.

`!success` alone does not say that: a child whose injection stopped
firing reaches `unreachable!`, panics, and exits non-zero too. The panic
message on stderr is what tells the two apart.

## `fn commit_body(fixture: &Fixture, object: &str) -> String {`

The commit `git cat-file` shows at `object`, as its raw header.

## `fn kill_after_commit_tree_before_pin_leaves_gc_owned_object_and_settles_interrupted() {`

T-CAND-OBJ (a), reached by killing between the commit-tree funnel's
return and the pin.

`durable_state` is "attempt_started only" and `authoritative_state` is
"attempt unsettled; the object is Git/GC-owned residue", so this asserts
three things: the object is **present**, it is **unreachable** per
`git fsck --unreachable` (R27 is a claim about reachability, not about
deletion), and the resume settles the attempt interrupted with nothing
to delete.

## `fn kill_after_commit_tree_before_pin_leaves_gc_owned_object_and_settles_interrupted() {` › `let journal = fixture.journal(&Hooks::new());`

The resume: `settle attempt interrupted`, and nothing to delete.

## `fn kill_at_commit_tree_id_unread_point_leaves_gc_owned_object() {`

T-CAND-OBJ (a) again, at the coordinate the packet names: the
parent-executed `IdUnread` point.

"the parent-executed IdUnread point lies between the child's exit and
the coordinator recording the id". So the durable outcome is the same
unreferenced object — and the *coordinator* never learned its id, which
is why this test identifies the commit by its content rather than by a
value anything recorded.

`IdUnread` supports `Kill` and nothing else
(`SubEffectPoint::modes`), so there is no error-return sibling to this
test and inventing one would invent a resume action nothing tables.

## `fn orphan_candidate_pin_removed_after_kill() {`

T-CAND-OBJ (b): the pin exists and `candidate_prepared` does not, so the
resume deletes the exact orphan pin expected-old and the object is again
Git's.

## `fn unpinned_object_never_adopted_on_resume() {`

`resume_action` (a) is "nothing to delete: the unpinned object is left
to Git (**never adopted**; decision:295)".

Adoption would be any of three things: a pin, a candidates ref, or a
`candidate_prepared` naming the commit. None of them happens, and the
object stays exactly where the interrupted run left it — present and
unreachable — through a full recovery.

## `fn unpinned_object_never_adopted_on_resume()` › `drop(unpinned);`

The witness is dropped without being pinned: the tabled outcome, not
a leak. Nothing else in this module can consume it.

## `fn unpinned_object_never_adopted_on_resume()` › `assert_eq!(`

And the run namespace is entitled to no *candidates* ref: nothing
durable names a candidate, so a ref that appeared for one would be
exactly the unexpected-ref refusal.

## `fn run_to_queued(fixture: &Fixture, hooks: &mut Hooks, journal: &mut Journal) -> CommitSha {`

=======================================================================
T-CAND-REF — the authoritative ref, the queue position, and the pin
=======================================================================

## `fn run_to_queued(fixture: &Fixture, hooks: &mut Hooks, journal: &mut Journal) -> CommitSha {`

The whole sequence from the pin onwards, on a live run.

Returns the fixture's judged candidate plus the hooks and journal it
ran through, so a test can assert on the order the funnels ran in and on
what the log holds.

## `fn a_substituted_prepared_pin_refuses_and_leaves_the_evidence() {`

**A substituted prepared pin is refused, and the evidence survives the
refusal.**

`DESIGN.md` §15's extended exact-identity rule: *"Any substituted or
symbolic pin, third branch SHA, changed branch identity, or mismatched
commit object refuses while preserving evidence."*

`recovery_for` read the pin as `pin.is_some()` and never compared its
target to the commit `candidate_prepared` recorded, and
`reclaim_after_creation` re-read the target and deleted **that** value
expected-old — a compare-and-swap comparing the ref to itself, which
cannot fail. So a pin moved from the recorded `C` to some `X` after the
settlement left a resume promoting `C`, appending
`task_candidate_created`, and then removing the substituted pin on the
way out: it succeeded, and it deleted the one ref that evidenced the
substitution. The `bf927f3` review's second P1.

Three claims, because "refuses while preserving evidence" is three
things: it refuses, it names both shas so the substitution is legible
from the error alone, and **nothing was appended, created or deleted** —
the pin is still at the substituted object for a person to look at.

## `fn a_substituted_prepared_pin_refuses_and_leaves_the_evidence() {` › `let unpinned = write_candidate_commit(&fixture.manager, &mut hooks, RUN_ID, fixture.judged())`

Reach the boundary honestly: commit, pin, `candidate_prepared`.

## `fn a_substituted_prepared_pin_refuses_and_leaves_the_evidence() {` › `let impostor = fixture.sibling_commit(&mut hooks);`

The substitution: the pin is moved to a real sibling commit on the
same base. A different *tree* is not needed — the point is that the
pin no longer names the recorded commit, and this must be caught by
the pin's own binding rather than by the tree check.

## `fn a_substituted_prepared_pin_refuses_and_leaves_the_evidence() {` › `let refused = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)`

(1) Recovery refuses, before any effect.

## `fn a_substituted_prepared_pin_refuses_and_leaves_the_evidence() {` › `let text = refused.to_string();`

(2) The refusal names both, so the substitution is legible from it.

## `fn a_substituted_prepared_pin_refuses_and_leaves_the_evidence() {` › `assert_eq!(`

(3) The evidence is intact: the pin is still at the impostor, the
    candidates ref was never created, and nothing was appended.

## `fn a_substituted_prepared_pin_refuses_and_leaves_the_evidence() {` › `let referenced = create_candidates_ref(&fixture.manager, &mut hooks, promoting)`

And the reclaim half refuses the same substitution rather than
deleting it, for a caller that reached it another way.

## `fn kill_after_candidate_prepared_appends_candidate_created_once() {`

T-CAND-REF's boundary reached by a real kill, then its `resume_action`
— and `task_candidate_created` lands **once**, however many times the
closure procedure runs.

The "once" is a count of committed lines in the log the dead process
wrote, not a count of calls: the fold refuses a second
`task_candidate_created` for a generation it has already closed, and a
closure procedure that did not read that would append a line the fold
then refuses on the next replay — a log that cannot be resumed.

## `fn kill_after_candidate_prepared_appends_candidate_created_once() {` › `let mut hooks = Hooks::new();`

The durable state of the boundary: the prepare landed, the queue
position did not, the pin is still holding the commit.

## `fn kill_after_candidate_prepared_appends_candidate_created_once() {` › `assert_eq!(`

**The tree came off the fold, and it is the tree the event recorded.**
`promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged`
builds its `PromotingCandidate` by hand, so it proves the *check* and
not the *value it checks against*: the fold retaining `base_sha` in
that field left it green.

**This is the resumed value, and it is the one that needed a witness.**
Production builds a `PromotingCandidate` in two places: `promote`
returns one carrying `judged.tree_sha` — the same value it has just
written into the event, so the comparison there cannot fail and
witnesses nothing — and `recovery_for` builds one from the fold,
where the number has been through a serialization, a replay and an
`apply`. Only the second can be wrong, and only the second is
asserted here.

## `fn kill_after_candidate_prepared_appends_candidate_created_once() {` › `assert!(`

"the closure procedure performs the same steps at any run end": run
it again. Every step reads the world first, so the second run appends
nothing, refuses nothing, and leaves the same refs.

## `fn kill_after_candidate_prepared_appends_candidate_created_once() {` › `let sibling = fixture.sibling_commit(&mut hooks);`

And the refusal the same sentence names: a ref present at another
SHA is not accepted as "already created".

## `fn pin_pruned_after_promotion() {`

O31, and `cleanup`'s "candidate-prepared pins pruned right after
promotion" — with the order the clauses give, observed rather than
assumed.

The trace is the assertion. O28 puts the commit object before the pin,
O29 the pin before `candidate_prepared`, O30 the candidates ref between
the two appends, and O31 the pin's pruning and the scrub after
`task_candidate_created` — which is one total order over six funnel
sites, and this is it.

## `fn pin_pruned_after_promotion()` › `"Event.Append".to_owned(),`

**candidate_prepared — the settlement itself, and there is
one.** This list carried a second `Event.Append` above this
one for an `attempt_finished(succeeded)` between the pin and
the prepare, annotated "the settlement, which is not this
module's". It was not the settlement and it should not have
been appended: `candidate_prepared` is the sole successful
settlement for a candidate-producing attempt, per
`design/26_design_merge_queue_protocol.md` §26, and
the 2026-08-27 ruling conformed the code to it. **Three
appends in this sequence, not four**, and the count is the
assertion — a build that re-introduced the pair would put the
fourth back and fail here.

## `fn pin_pruned_after_promotion()` › `"Event.Append".to_owned(),`

task_candidate_created.

## `fn pin_pruned_after_promotion()` › `assert_eq!(`

What is left: the authoritative ref, and nothing else.

## `fn pin_pruned_after_promotion()` › `assert!(`

`cleanup`: "candidates refs (R11) never pruned while the run can
resume". Nothing in this module deletes one — the site exists for
Complete finalization, and this sequence never reaches it.

## `fn pin_pruned_after_promotion()` › `assert_eq!(`

…and the expected-ref derivation says the same thing from the fold:
the candidates ref is expected, the pin no longer is.

## `fn promoting_completed_at_run_end() {`

ST-17: "a Promoting generation is always promoted before any
`run_finished`."

Executed as the fold's own refusal rather than as a claim about this
module: while the generation is `Promoting` the derived outcome is
`NotEnding`, so `run_finished` cannot be appended at all; the closure
procedure is what clears it.

## `fn promoting_completed_at_run_end()` › `let Some(promoting_again) = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA)`

The closure procedure, exactly as `resume_action` describes it.

## `fn worktree_removal_idempotent_after_candidate_created() {`

=======================================================================
T-SCRUB — the forced, contained, idempotent reclaim
=======================================================================

## `fn worktree_removal_idempotent_after_candidate_created() {`

`resume_action`: "idempotent contained forced removal of the worktree
and intent".

Idempotent both ways round: the promotion already scrubbed, and a resume
that re-runs the closure procedure scrubs again. Also `cleanup`'s "task
worktree scrubbed **only after** `task_candidate_created` is durable" —
asserted as an order, because a scrub that ran first would leave a
promotion with no worktree to verify against.

## `fn worktree_removal_idempotent_after_candidate_created()` › `let order = hooks.trace.order(&[`

The order `cleanup` states, from the trace: the append first.

## `fn worktree_removal_idempotent_after_candidate_created()` › `for round in 1..=2 {`

Again, and again through the funnel rather than by inspection.

## `fn worktree_removal_idempotent_after_candidate_created()` › `let escaping = Slot::Task {`

Containment, which is what makes an idempotent forced removal safe:
`refusal_condition` is "path outside execution root". Executed as a
refusal rather than asserted as a property of the happy path, because
an idempotent removal that had stopped checking would still pass
every assertion above.

## `fn worktree_removal_succeeds_with_index_lock_present() {`

`cleanup`: removal is forced "so Git administrative residue left by an
interrupted command (**index.lock**, …) never blocks reclaim — such
residue belongs to the worktree's row and leaves with it".

The control half is the first assertion. An `index.lock` that did not
actually block anything would make the rest of this test pass against a
removal that was never forced, which is the shape of the Windows
`File::open` test this project has already paid for.

## `fn worktree_removal_succeeds_with_index_lock_present()` › `git_fixtures::git_ok(`

Planted by `git config --file`, because this module may not write a
file itself. What the residue *is* is a file at that name: nothing in
the removal path reads its bytes, and Git's own index writer refuses
on its existence alone — which the control below executes.

## `fn worktree_removal_succeeds_with_index_lock_present()` › `let worktree = fixture.manager.slot_path(&fixture.task);`

Control: the residue really does block an index write.

## `fn worktree_removal_succeeds_with_index_lock_present()` › `let mut hooks = Hooks::new();`

The claim.

## `fn snapshot_residue_reclaimed() {`

`cleanup`: "snapshots pruned on completion and **reclaimed as
residue**", and `T-SCRUB`'s "snapshot intents reclaimed".

Both halves, because they are two mechanisms: a live coordinator removes
the snapshot it created, and a process that died holding one leaves an
intent that the next process's reclaim finds. The ephemeral commit each
snapshot created returns to R27 when its snapshot goes, which is the
object half of the same sentence.

## `fn snapshot_residue_reclaimed()` › `let gates = fixture`

One snapshot for the gate set, one for the reviewer: `snapshots` says
they are never reused across roles, so the reclaim has two rows.

## `fn snapshot_residue_reclaimed()` › `fixture`

Half one: the live removal.

## `fn snapshot_residue_reclaimed()` › `let reclaimed = fixture`

Half two: the reviewer's snapshot is left as a dead process would
leave it, and the next process's reclaim finds it by its intent.

## `fn snapshot_residue_reclaimed()` › `assert!(`

The object half: with no snapshot and no ref holding it, the
ephemeral commit is Git's again.

## `fn the_candidate_refs_are_the_names_the_packet_gives() {`

=======================================================================
The names, the refusals, and the window between the append and the prune
=======================================================================

## `fn the_candidate_refs_are_the_names_the_packet_gives() {`

The two ref names are durable identity: they are written into
`candidate_prepared` and a resume rebuilds them from the same inputs. So
they are pinned against literals rather than against the function that
builds them.

## `fn the_candidate_refs_are_the_names_the_packet_gives()` › `assert_eq!(run_namespace("01RUN"), "refs/upstroke/runs/01RUN/");`

The namespace's trailing separator, which is what keeps run `01RUN`
from owning run `01RUNNER`'s refs.

## `fn promotion_refuses_a_commit_that_is_not_in_the_repository() {`

`T-CAND-REF`'s "object missing": the candidates ref is created only for
a commit that is actually in the repository.

The refusal is what stops a resume from creating an authoritative ref
out of a record whose object a `git gc` already collected — which is a
reachable state precisely because the pin is what kept it alive.

## `fn promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged() {`

**Present is not the same as *is the candidate*.**

DESIGN.md §15 has `candidate_prepared` record the complete
attempt/base/commit/tree identity "so resume adopts only the judged
object". A promotion that asks only whether *something* is at that SHA
adopts whatever is there, and the two things that can be there are the
two this asserts: an object that is not a commit at all, and a commit
that is not on the generation's base. Both exist; neither is the judged
candidate; both must refuse before the ref is created.

The tree is deliberately **not** asserted, and it is not an oversight:
the fold keeps the candidate, the base and the paths, so a resume has no
recorded tree to compare against. `PR7-CANDIDATE-TREE-UNVERIFIED` in
`reviews/FINDINGS.md` §2 is that residue, recorded rather than papered
over.
**A commit on the recorded base, carrying a tree nobody judged, is refused.**

`DESIGN.md` §15: `candidate_prepared` records "exactly one complete
attempt/base/commit/tree identity … so resume adopts only that exact shape".
Recovery checked existence and parent. Both pass here — the impostor *is* a
commit and its parent *is* the base — and its tree is a tree no gate ran
against and no reviewer read. Adopting it would create the authoritative
candidate ref at that object and append `task_candidate_created`, which is
the whole of what the merge queue then trusts.

The sibling above refuses objects that are not commits, or commits that are
not on the base. Neither reaches this: the difference here is **content**,
and content is what the ladder judged.

Raised by the frontier re-review of `c2c0294` as finding B and carried
before that as `PR7-CANDIDATE-TREE-UNVERIFIED`. The repair is
`PreparedCandidate::tree_sha`, per-instance Class B approval 2026-08-26.

## `fn promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged() {` › `assert!(`

Both of the checks that existed pass, stated rather than assumed —
otherwise this test could be green because an *earlier* refusal fired.

## `fn promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged() {` › `tree: fixture.tree_sha.clone(),`

The durable record's tree — what the fold now retains.

## `fn promotion_refuses_a_commit_on_the_base_whose_tree_was_never_judged() {` › `assert_eq!(`

And nothing was appended or created on the way to refusing.

## `fn promotion_refuses_an_object_that_is_not_the_judged_candidate() {` › `let impostor = if present {`

Two real objects of the fixture's own repository. The tree is not
a commit; the base is a commit whose parent is not the base.

## `fn promotion_refuses_an_object_that_is_not_the_judged_candidate() {` › `assert!(`

The **production** presence predicate, not the fixture's: the
residue classifier asks `cat-file -e <sha>^{}`, which resolves any
object, so a tree answers "present" there. Asserting it here is
what makes this a test of identity rather than of existence — the
check being repaired would have passed both of these.

## `fn a_commit_id_that_is_not_a_full_object_id_refuses() {`

`git commit-tree` printing something that is not a full object id is
refused before any ref primitive sees it.

Reached directly because the real command cannot produce it: an id that
`update-ref` would read as a *name to resolve* rather than as an error
is the shape `workspace_manager::Refusal::MalformedObjectId` exists for,
and this is the same guard one step earlier, where the value enters.

## `fn a_pin_left_by_an_interrupted_promotion_is_pruned_by_the_closure_procedure() {`

The window O31 opens: `task_candidate_created` is durable and the pin it
should have pruned is not yet pruned.

`cleanup` says pins are "pruned right after promotion (**or as
orphans**)", so this state has to be recoverable — and it is the one
state a classifier that only looked at the *open* generation would miss,
because the append that reached it also closed the generation.

Reached by an error return rather than a kill: the claim is about what
the next process finds, and both leave the same durable prefix, so the
cheaper one is the honest choice. `Ref.DeleteCandidatePin`'s `Before` is
a reachability phase in the frozen registry rather than a declared fault
coordinate; the module-local double injects there to reach the prefix,
which is scaffolding, not a claim about the registry.

## `fn a_pin_left_by_an_interrupted_promotion_is_pruned_by_the_closure_procedure() {` › `assert_eq!(journal.count("task_candidate_created"), 1);`

The prefix: the queue position is durable, the generation is closed,
and the pin is still there.

## `fn a_pin_left_by_an_interrupted_promotion_is_pruned_by_the_closure_procedure() {` › `fixture`

…and the namespace does not refuse it, which is what lets the next
process act at all.

## `fn a_pin_left_by_an_interrupted_promotion_is_pruned_by_the_closure_procedure() {` › `let recovery = recovery_for(&fixture.manager, RUN_ID, journal.fold(), ALPHA).expect("classify");`

The closure procedure finishes it, appending nothing.

## `fn a_symbolic_pin_refuses_on_both_the_write_and_the_read() {`

`T-CAND-OBJ`'s other `refusal_condition`: "**pin symbolic** or an
unexpected ref under the run namespace".

Both the writer and the reader refuse it. That matters separately: a
symbolic pin that only the writer refused would still be followed by the
resume that reads it, and `INV-17`'s "every engine ref is direct" would
hold on the way in and not on the way out.

## `fn an_unexpected_ref_under_the_run_namespace_refuses() {`

A ref under the run namespace that no durable record accounts for is
refused — which is the half of `T-CAND-OBJ`'s refusal condition that
needs [`expected_refs`] to have derived something.

Two shapes, because the derivation has two rules and one of them is
tighter. A candidates ref for a generation that **exists and has
prepared nothing** is the shape a derivation that expected both names
for every generation would wave through; a candidates ref for a
generation that does not exist at all is the shape any derivation
catches. Only the first measures the rule.

## `fn an_unexpected_ref_under_the_run_namespace_refuses()` › `assert_eq!(`

(1) The generation exists and has prepared no candidate, so it is
entitled to a pin and to nothing else.

## `fn an_unexpected_ref_under_the_run_namespace_refuses()` › `run_to_queued(&fixture, &mut hooks, &mut journal);`

(2) After the promotion the same name is accounted for, and the
namespace holds exactly what the fold says it may.

## `fn an_unexpected_ref_under_the_run_namespace_refuses()` › `let stowaway = candidates_ref(RUN_ID, ALPHA, GenerationId(9));`

(3) …and a ref for a generation that never existed still refuses.

## `fn attempt_record() -> AttemptRecord` › `reviews: vec![ReviewRecord {`

The primary pass §11.2 requires, present and passed. Empty
`reviews` satisfies `is_successful` vacuously — the premise then
exercises none of the clause it is the positive witness for.
