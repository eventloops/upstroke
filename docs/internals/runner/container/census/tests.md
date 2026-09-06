# `src/runner/container/census/tests.rs`

Extended notes for [`src/runner/container/census/tests.rs`](../../../../../src/runner/container/census/tests.rs).

[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/runner/container/census/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The census suite.

**Every test here names the second field it holds constant.** The dominant
defect shape on this project is two axes covered separately with their
intersection never built, and this module is unusually exposed to it: the
liveness rule is `{owner run} × {incarnation}`, discovery is `{intent
present} × {container present}`, and the write-command axis is `{run} ×
{resume}`. A suite that varies one at a time passes while an implementation
that reclaims a **live** run's dead earlier incarnation ships.

## `#![allow(clippy::disallowed_methods)]`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
attachment to `src/runner/container.rs` -- the same shape `src/events/log.rs`
and `src/events/log/tests.rs` have, which is PR5's precedent for a funnel's
own test module. This file drives the eight site-taking APIs and plants the
residue they are meant to find, so it names `fs::write`, `fs::create_dir_all`
and the seam's own effectful methods directly.

`PR6-LANEF-004`: it carries this allow **of its own** because the funnel's no
longer reaches it. The two lints it does not need are re-denied, so a
`std::process::Command` or a `println!` appearing here is still a build error.
`decisions.effect_site_inventory.mechanism` (2).

## `mod this_file_is_test_only {}`

`effects::production_region` cuts a source at its FIRST `#[cfg(test)]`, and
this file is reached only through `#[cfg(test)] mod tests;` so it has no
attribute of its own for a scan to cut on. The marker below is redundant to
the compiler and load-bearing to every reader that still consults the
TRUNCATING region — `effects::externally_reachable_fns` and the three
censuses in `src/runner/container/exec.rs` — for which it makes this file's
production region empty, so a fixture that names a primitive is not reported
as a production offender (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`, used here in
the direction it is wanted).

**It does not do that for the four whole-tree censuses.** They read
`effects::production_code`, which excises this marker as the configured item
it is and then scans the file IN FULL. What keeps this file out of those is
not the marker: it is the `#[cfg(test)] mod tests;` declaration in
`src/runner/container/census.rs`, which
`effects::census_domain::declared_whole_file_test_modules` derives into their
skip set.

## `fn scratch(tag: &str) -> PathBuf {`

---------------------------------------------------------------------------
Fixtures
---------------------------------------------------------------------------

## `fn scratch(tag: &str) -> PathBuf {`

A scratch private root. Thread id is in the name because
[`concurrent_reclaimers_converge`] runs two of these at once.

## `const REPO_KEY_A: &str = "0123456789abcdef";`

Distinct values for every independently meaningful field, so a swap between
any two is visible rather than accidentally equal.

## `struct Owner {`

One owner, fully specified. Every field varies independently in the grids
below, which is why they are arguments and not defaults.

## `impl Owner` › `fn with_run_dir(mut self, run_dir: PathBuf) -> Self {`

The owner's run directory, whatever bytes it carries.

A setter and not a second constructor so a hostile directory is a
one-line variation on an otherwise identical owner — `PR6-RECOV-001`'s
grids vary the run directory and hold everything else fixed.

## `impl Owner` › `fn record(&self, invocation: &InvocationId) -> ContainerIntent {`

Through `ContainerIntent::new`, so a fixture's record carries the same
encoding a real invocation writes.

## `enum Present {`

What a fixture puts on the machine for one container.

## `enum Present` › `Both,`

A record and a container: the ordinary running state.

## `enum Present` › `IntentOnly,`

A record, no container and **no view**: a crash between the intent write
and `docker create`. Nothing was mounted, so there is nothing to prune.

## `enum Present` › `IntentAndViewAfterReaper,`

A record, no container and a **view**: the ordinary state after the Unix
reaper has run. It performs `kill/rm` and nothing else
(`T-CONTAINER.resume_action`), so the invocation's R19 directory and its
R26 record both outlive the container.

`PR6-CONV-003`. `IntentOnly` was documented as covering **both**
situations and seeded only for the first, so `{intent present} ×
{container present}` and `{view present}` were correlated in every
fixture: a regression that skipped view cleanup for an intent-only
candidate removed the final record, returned `CensusComplete`, and
stranded a now-undiscoverable R19 directory — with the whole suite
green.

## `enum Present` › `LabelOnly,`

A container and no record: "a labeled container without an intent".

## `fn seed(`

Put one container's evidence on the machine and return its name.

## `if present != Present::IntentOnly {`

R19 exists whenever the invocation got as far as mounting it, which is
every state except "crashed before `docker create`". The view is
deliberately **not** tied to the container's presence: the post-reaper
state has one and no container.

## `struct RecordingLiveness {`

An owner-liveness probe that records what it was asked.

Not [`crate::runner::container::FakeOwnerLiveness`], which answers but keeps
no log: "arm (i) does not probe the lock at all" and "arm (ii) does not read
the incarnation" are both claims about **what was asked**, and only a log can
hold them.

## `struct WedgedRuntime {`

A runtime whose `stop` **succeeds and does not stop anything**.

The container is still running after every observation, which is the state
`refusal_condition`'s "cannot be observed terminated" is about: a wedged
supervisor, a container in `removing` that never leaves it, a daemon that
accepts a signal and delivers nothing. [`FakeRuntime`] cannot reach it —
its `stop` always moves the container to `Exited` — so a suite built only on
the fake would find that branch unconstructible and green.

It delegates only the **read-only** operations. The four effectful ones are
the funnel's primitives, and this wrapper implements rather than forwards
them, so it never becomes a second caller of one.

## `impl ContainerRuntime for WedgedRuntime` › `Ok(())`

Accepted, delivered nowhere. This is the whole fixture.

## `fn barrier() -> StablePrefixBarrier {`

A resume's recovery step (a1), established from bytes this fixture owns.

## `struct Harness {`

Everything a census run needs, held together so a test varies one field.

## `fn at(trace: &ContainerTrace, needle: &str) -> usize {`

Where `needle` first appears in the trace, or a failure naming the sequence.

## `fn the_liveness_rule_classifies_every_cell_of_owner_run_by_incarnation_by_lock() {`

---------------------------------------------------------------------------
1. The liveness rule — two arms, and the intersection nobody builds
---------------------------------------------------------------------------

## `fn the_liveness_rule_classifies_every_cell_of_owner_run_by_incarnation_by_lock() {`

Every cell of `{owner run} × {incarnation} × {owner lock}`.

The rule has two arms and each arm has two outcomes, so the grid is the
product and not a list of the cases that came to mind. **Arm (i) has no lock
axis** (this process holds the lock, so a probe would be asking whether it is
itself alive) and **arm (ii) has no incarnation axis** — "reclaim every
container of that run whatever its incarnation", which includes this
process's own. So the tuples collapse to **four** classifications, and that
collapse is asserted as a distinct-value count rather than described.

**This test was rewritten by `PR6-RECOV-003`, and the previous oracle was
wrong.** It required `ForeignRunThisIncarnation` for the two cells where a
foreign run's recorded incarnation equals this process's — a refusal that
never reached arm (ii) and so never probed the owner's lock. The
classification rule splits on the owner run **first** and puts the
own-incarnation refusal inside arm (i)'s clause; arm (ii) then says
"whatever its incarnation" in as many words, and `T-CONTAINER.resume_action`
states the same order. The cost of the hoisted check was that a **dead**
foreign owner's container could never be reclaimed and blocked every write
command under that private root permanently. See `census::Ownership`.

Second field held constant: the container name, the repo key and the run
directory are the same shape in every cell, so nothing but the ownership
triple moves.

## `fn the_liveness_rule_classifies_every_cell_of_owner_run_by_…` › `let cells: Vec<(&str, &str, &str, &Path, Ownership, bool)> = vec![`

`(what, owner run, owner incarnation, owner run dir, expected, is the
owner's lock probed)`. The last field is a separate column because "arm
(i) does not probe" and "arm (ii) always probes" are independently
droppable predicates, and a fixture that only checked the classification
would pass an implementation that probed a lock it holds itself.

## `fn the_liveness_rule_classifies_every_cell_of_owner_run_by_…` › `(`

Arm (i): the run this process drives. The lock is not probed, and the
lock state is varied anyway to prove it is not consulted.

## `fn the_liveness_rule_classifies_every_cell_of_owner_run_by_…` › `(`

Arm (ii) with the incarnation equal to this process's. **The two
cells `PR6-RECOV-003` is about**: the lock decides, exactly as it
does for any other incarnation, and it is asked.

## `fn the_liveness_rule_classifies_every_cell_of_owner_run_by_…` › `(`

Arm (ii): another run, another incarnation.

## `fn the_liveness_rule_classifies_every_cell_of_owner_run_by_…` › `assert_eq!(`

Exactly one of the four refuses, and it does not reclaim. A refusal that
also reclaimed would have performed an effect on behalf of a write
command that never ran.

## `fn the_liveness_rule_classifies_every_cell_of_owner_run_by_…` › `let brand_new = fresh(INC_1);`

{start kind} × {incarnation}. A **fresh** run has no own run at all, so
arm (i) is unreachable for it and every cell above is an arm (ii) cell —
including the ones naming the incarnation this process generated at
startup. Nothing is refused, every candidate's owner lock is probed, and
the answer is the lock's.

## `fn the_owner_lock_is_probed_exactly_once_per_candidate() {`

The probe is asked **once** per candidate, and never in a loop.

`T-CONTAINER.resume_action`: "probe the owner's run.lock **non-blocking**;
held -> skip". Catalogue entry `PR6-INTENT-031` survived the whole suite by
replacing the single non-blocking probe with a blocking retry loop, because
nothing looked at *how many times* the seam was asked — and a census that
waits on a live neighbour is a stall at every write-command start, which is
the one thing "non-blocking" is there to prevent.

The call **count** is the observable, not the elapsed time: a wall-clock
bound would be a flake on a loaded box, and a retry loop that gave up after
`n` attempts would pass one anyway.

Second field held constant: one candidate, one owner directory; what varies
is only whether that owner's lock is held.

## `fn a_live_runs_dead_earlier_incarnation_is_untouched_by_a_foreign_census() {`

**The crossed fixture.** A *live* run's *dead earlier incarnation*, seen by a
*foreign* census, is never touched.

`crash_reconstruction`: "held -> live owner -> **never touched** (that owner
reclaims its own earlier incarnations at its own startup census, which
precedes its admission)"; and the residual it names — "a container of a dead
incarnation of a live run may run until that run's own census reclaims it …
**out of scope**".

This is the cell an implementation that reclaims dead incarnations gets
wrong, and it passes every test that varies only `{owner run}` or only
`{incarnation}`. The same fixture is then run again with the owner's lock
**free** and the same two incarnations are both reclaimed, so the test cannot
pass by never reclaiming anything.

Second field held constant: the two containers, their names, their records
and their private root are byte-identical between the two halves; the **only**
thing that moves is whether the owner's lock is held.

## `fn a_live_runs_dead_earlier_incarnation_is_untouched_by_a_f…` › `let incarnations: BTreeSet<&str> = report`

"reclaim EVERY container of that run WHATEVER its incarnation".

## `fn arm_two_gives_one_answer_whatever_the_incarnation_that_reaches_it() {`

Arm (ii) does not read the incarnation, over a domain of them —
**including this process's own**.

The lane's first version of this test asserted exactly this, an independent
review refuted it on `expected_failures_refusals[7]`, and `PR6-RECOV-003`
restored it: that line is the contract's one-sentence summary of arm (i)'s
clause, while the classification rule splits on the owner run first and arm
(ii) says "reclaim every container of that run **whatever its
incarnation**". `T-CONTAINER.resume_action` states it in the same order.

Second field held constant: the owner run and its lock state; only the
incarnation moves, across four distinct values, one of them this process's
own.

## `fn arm_two_gives_one_answer_whatever_the_incarnation_that_r…` › `assert_eq!(`

And every one of the eight cells asked the owner's lock exactly once:
arm (ii) reaches the probe for this process's own incarnation too, which
is what the hoisted comparison prevented.

## `fn the_census_learns_no_incarnation_from_the_owner_liveness_seam() {`

The incarnation is never read from the lock: the seam has no incarnation in
it, and this module never names a lock file.

`crash_reconstruction`: "the coordinator incarnation id is a per-process ULID
recorded in `run_started(4)`/`run_resumed(4)` and is **never read from
lock-file contents** (`run.lock` content is never read: `src/rundir.rs:886`;
a Windows exclusive lock makes it unreadable to non-holders)". Deriving it
from the lock is a plausible implementation and a real defect.

Second field held constant: the runtime and the namespace; only what the
liveness seam is handed and returns is under test.

## `fn the_census_learns_no_incarnation_from_the_owner_liveness…` › `assert_eq!(harness.liveness.asked(), vec![dead.run_dir.clone()]);`

What the seam was handed is the PUBLIC run directory and nothing else,
and what it gave back is one bit — there is no incarnation in the return
type to read.

## `fn the_census_learns_no_incarnation_from_the_owner_liveness…` › `let source = fs::read_to_string(`

And the module does not reach around the seam: its production region
names no lock file at all.

## `fn orphan_reclaimed_before_slot_reset() {`

---------------------------------------------------------------------------
2. The T-CONTAINER names
---------------------------------------------------------------------------

## `fn orphan_reclaimed_before_slot_reset() {`

(4) An orphan is reclaimed **before slot reset, credential reuse, or
admission** — expressed as the token those consumers cannot be reached
without.

ST-16 (a): "single owner dies -> next write-command start reclaims
(inspect/kill/observe/rm/view/intent) **before slot reset, credential reuse,
or admission**". Slots and admission are PR11's and the credential-volume
turn is PR7's, so what this slice can hold is that (i) the whole five-step
reclaim is complete when the census returns, in the packet's order, and (ii)
a census that could not complete it returns **no token**, so nothing that
takes one can run. `census_returns_the_only_token_that_reaches_a_consumer`
is the structural half.

Second field held constant: the owner is dead in both halves and the fixture
is byte-identical; only whether the container can be observed terminated
moves.

## `fn orphan_reclaimed_before_slot_reset()` › `let sites: Vec<ContainerSite> = harness`

The five steps, in the packet's order, all before the token existed.

## `fn orphan_reclaimed_before_slot_reset()` › `let root = scratch("blocks-admission");`

The other half: a container that cannot be observed terminated blocks
admission, so there is no token at all.

## `fn live_owner_untouched_while_dead_orphan_reclaimed() {`

(5) A live owner's containers are untouched while a dead owner's orphan in
the **same private root** is reclaimed.

ST-16 (b): "live coordinator A running while dead coordinator B's orphan
exists in the same private root (**same or different repository**) -> reclaim
kills only B's container, A's continues, and **no invocation uses the shared
credential volume before B's is observed terminated**".

The repositories differ — two repo keys under one private root, which is the
"different repository" half of that clause — and the run directories differ,
which is what the lock probe distinguishes them by. The credential-volume
clause is the token: B's observation is complete before `run_startup_census`
returns, and nothing that takes a `&CensusComplete` exists until then.

Second field held constant: both containers are `Running`, both have records,
both are under the same private root; only the owner run and its lock state
move.

## `fn live_owner_untouched_while_dead_orphan_reclaimed()` › `let named_a: Vec<String> = harness`

Only B was touched: no runtime operation names A's container at all.

## `fn live_owner_untouched_while_dead_orphan_reclaimed()` › `assert!(`

The credential-volume clause: B's termination is observed before the
token exists, and no volume operation happens in a census at all.

## `fn labeled_orphan_without_intent_reclaimed() {`

(6) A labeled container with no intent is reclaimed under the same rule.

`crash_reconstruction`: "a labeled container **without an intent** is treated
as an orphan of its **labeled** run and incarnation under the same rule".
Its ownership therefore comes from `upstroke.run` and `upstroke.incarnation`, and
the census must reach the same verdict it would have reached from a record.

Second field held constant: the same owner, the same name and the same
liveness answer are used for a record-backed container in the same fixture,
so the two differ **only** in which half of discovery found them.

## `fn labeled_orphan_without_intent_reclaimed()` › `let verdicts: BTreeSet<Ownership> = report`

Both reached the same ownership verdict, which is what "under the same
rule" means.

## `fn same_run_resume_reclaims_earlier_incarnation_orphan() {`

(7) A resume reclaims its own earlier incarnation's orphan — including a
probe invocation with the **same deterministic `InvocationId`**.

ST-16 (f): "the resuming incarnation holds the run lock … and still reclaims
its own earlier incarnation's orphan (incl. a probe invocation with the same
deterministic `InvocationId`, whose new container name and intent path
differ) before slot init, admission, credential use, or its own probes, while
containers it starts afterwards are untouched".

Second field held constant: the invocation identity is **literally the same
value** for the dead incarnation and for this one, so the only thing that can
separate their names and intent paths is the incarnation component.

## `fn same_run_resume_reclaims_earlier_incarnation_orphan()` › `let mine = Owner::new(RUN_A, INC_2, REPO_KEY_A);`

The same deterministic identity, this incarnation.

## `fn same_run_resume_reclaims_earlier_incarnation_orphan()` › `assert!(`

"while containers it starts afterwards are untouched": this incarnation's
own container appears only after the census, and a second census of the
same root would refuse it rather than reclaim it — which is the next test.

## `fn same_run_resume_censuses_recorded_root_after_default_changed() {`

(8) The census scans **exactly the root it is given**, after the default
moved.

ST-16 (f): "censuses the recorded private root **even when the default root
or `HOME` changed**". PR7 owns deriving that root from
`run_started.private_dir` (recovery step (a0)); what this slice owns is that
the census takes it as a parameter and reads no default — so a second root
holding a reclaimable orphan is left completely alone, and "different private
roots are disjoint worlds".

Second field held constant: the two roots hold **the same owner, the same
invocation and therefore the same container name**; the only thing that
differs is which root the census was handed.

## `fn same_run_resume_censuses_recorded_root_after_default_cha…` › `let mut hooks = RecordingHooks::new(ContainerTrace::off());`

The same container name and record, under the other root. If the census
read a default it would find this one instead, or as well.

## `fn same_run_resume_censuses_recorded_root_after_default_cha…` › `let filtered: Vec<String> = recorded`

And the label filter is the root it was given, not any other.

## `fn repeated_crashes_reclaim_every_dead_incarnation() {`

(10) Three incarnations, two crashes, every dead incarnation reclaimed with
no name or intent collision.

ST-16 (g): "repeated crashes across **three** incarnations leave orphans from
**two** dead incarnations that are all reclaimed with no name or intent
collision".

#### The cardinality is the clause (`PR6-ENUM-008`)

This seeded **three** dead incarnations and resumed as a **fourth**, which
is a different sentence: the variant says three incarnations *total*, of
which two are dead and the third is the one doing the censusing. The
enumerated cell — exactly two `OwnRunEarlierIncarnation` candidates — was
therefore never built, and an implementation that mishandled precisely two
while handling one and three passed.

Both cardinalities are driven now, as a grid, with the reclaimed count
asserted per cell: `{1, 2, 3} dead incarnations` × `the resuming
incarnation is the next one`. Two is ST-16 (g)'s cell; one and three are
what make it a measurement of the count rather than of a threshold.

Second field held constant: every orphan of a cell is the **same
deterministic probe identity** under the **same run** and the **same repo
key**, so the only thing separating the names and the intent paths is the
incarnation component — which is exactly the thing the packet says it is
for. The last dead incarnation of each cell carries a *different*
invocation, so no cell is n copies of one shape.

## `fn repeated_crashes_reclaim_every_dead_incarnation()` › `const INCARNATIONS: &[&str] = &[INC_1, INC_2, INC_3, "01KZTDDDDDDDDDDDDDDDDDDDDD"];`

Four incarnations, so the resuming one is always a fresh value and
never one of the dead.

## `fn repeated_crashes_reclaim_every_dead_incarnation()` › `let invocation = if ordinal + 1 == dead_count {`

The last one carries a different invocation identity.

## `fn repeated_crashes_reclaim_every_dead_incarnation()` › `let complete = harness`

The next incarnation of the same run resumes and censuses. ST-16 (g)
is the `dead_count == 2` cell: three incarnations in total, orphans
from the two dead ones.

## `fn concurrent_reclaimers_converge() {`

(11) Two reclaimers **actually racing** on one container converge.

"every step idempotent and tolerant of already-gone so **two concurrent
reclaimers converge**". A fixture that ran two censuses one after the other
would prove idempotence, which is a different claim: idempotence is about
repeating a completed operation, convergence is about two interleaved ones.
So the two run on two threads, released together by a
[`Barrier`], over many rounds so the interleaving actually varies.

#### The pair is ST-16 (h)'s pair, and the result is asserted converged

`PR6-CONV-002`. Both reclaimers used to be `CensusStart::FreshRun`, and the
closing assertion was `total >= 4` — which **a fully serialised run
satisfies**: one census reports 4, the other reports 0, and nothing about
the second one was ever a reclaim. ST-16 (h) names the pair exactly — "two
concurrent reclaimers (**a foreign write command and the resuming
incarnation**) converge idempotently on the same dead container" — so one
side is now a resume of the orphans' own run and the other a fresh foreign
write command under the same private root. They classify the same
containers through **different arms** of the liveness rule: the resuming
incarnation through arm (i) (own run, earlier incarnation, dead by
construction) and the foreign command through arm (ii) (another run, lock
free). That is the shape the packet describes and it is a strictly harder
fixture than two copies of one arm.

The serialised outcome is refused rather than accepted: **both** reclaimers
must report at least one reclaim in at least one round, and the run counts
how many rounds actually interleaved. A machine that serialised every round
fails here instead of passing.

Second field held constant: both reclaimers are handed the **same** runtime,
the same root and the same four containers; what differs is which write
command each one is and which thread gets there first.

## `fn concurrent_reclaimers_converge()` › `let mut interleaved = 0_usize;`

How many rounds saw both sides do work. Convergence is about an
interleaving, so a run in which none did is a run that measured
idempotence and called it convergence.

## `fn concurrent_reclaimers_converge()` › `let names: Vec<ContainerName> = (0..4)`

FOUR containers, not one. The dangerous window is between the
namespace directory read and the per-record reads inside it, and one
record closes that window almost immediately: with a single orphan,
this fixture detected the `list_intents` intolerance measured below
in only 2 of 20 runs. Four records widen the scan enough for the
detection to be reliable, which is the difference between a test that
holds a claim and one that occasionally notices it.

## `fn concurrent_reclaimers_converge()` › `let starts = [`

ST-16 (h)'s two write commands. `RUN_B` owns the orphans, so the
resume reaches them through arm (i) and the foreign fresh run
reaches them through arm (ii) with `RUN_B`'s lock free.

## `fn concurrent_reclaimers_converge()` › `if let Ok((_, arms)) = outcome {`

Whichever arm did the work, it was the arm that side is for.

## `fn concurrent_reclaimers_converge()` › `for name in &names {`

The converged result, asserted: nothing of any of the four remains,
whichever order they interleaved in.

## `fn concurrent_reclaimers_converge()` › `let counts: Vec<usize> = outcomes`

Somebody did the work. The loser may legitimately find a container
already gone and report fewer; between them they must account for all
four. What must never happen is a refusal, asserted above.

## `fn concurrent_reclaimers_converge()` › `assert!(`

Two threads released at a barrier and then serialised every single time
is a fixture that proved idempotence and reported convergence. This is
the assertion that the interleaving actually happened.

## `fn a_reclaimer_suspended_mid_sequence_converges_with_one_that_finished() {`

The sharpest interleaving, made deterministic.

A racing fixture visits the dangerous window by luck. This one puts a
reclaimer to sleep at `Container.Remove`'s `Before` phase, lets a second
reclaimer run the whole sequence to completion underneath it, and then
releases the first — so the first issues `docker rm`, view removal and
intent removal against a machine where all three are already gone. Every one
must be tolerant of already-gone or the census refuses and blocks admission
forever.

Second field held constant: both reclaimers see the same root, the same
runtime and the same container; only the suspension point moves, and it is
the same point every run.

## `fn a_reclaimer_suspended_mid_sequence_converges_with_one_th…` › `struct BlockAt {`

Hooks that block once, at one phase of one site.

## `fn a_reclaimer_suspended_mid_sequence_converges_with_one_th…` › `let fast = {`

The second reclaimer finishes the whole sequence underneath it.

## `fn schema4_probe_container_owned_during_preflight_untouched_by_foreign_census() {`

(12) A foreign census leaves a schema-4 run's probe containers alone while
its `run.lock` is held during preflight.

ST-16 (i): "a schema-4 run's probe containers (shell and agent probes) carry
an owner whose `run.lock` is **held** during preflight (T-RUNSTART P4) and
whose owner record already names the `RunnerPolicy`, and a concurrent foreign
census leaves them untouched".

**PR7 completes this.** The owner record at P3b and the P0-P8 sequence that
makes the lock held at P4 are `decisions.pr_sequence[8]`'s. What PR6 holds is
the half the census owns: a foreign census leaves untouched **every**
container of a run whose lock is held, including probe containers, and
including a probe container whose owner has not yet appended `run_started`.

Second field held constant: an identical dead-owner probe container is in the
same fixture, so the test cannot pass by leaving everything alone.

## `fn schema4_probe_container_owned_during_preflight_untouched…` › `assert_eq!(`

Both probe kinds were present, so the claim is about probe containers and
not about one of them.

## `fn census_refuses_when_intents_exist_without_reachable_runtime() {`

(14) Intents present + the runtime unreachable = the write command refuses,
before any effect.

ST-16 (j) and `expected_failures_refusals[8]`: "intents present without a
reachable runtime refuse the write command". It "cannot prove those
containers terminated".

The reachability question is asked of the operation the census actually needs
— `ListByLabel` — and **not** of `probe`, whose `Ok` binds nothing: the
fixture arms `ListByLabel` unreachable while leaving `Probe` reachable, so an
implementation that gated on `probe` proceeds and fails this test.

Second field held constant: the same single intent is on disk in both halves;
only the runtime's answer moves.

## `fn census_refuses_when_intents_exist_without_reachable_runt…` › `harness.runtime.set_reachable(RuntimeOp::ListByLabel);`

And the same runtime, reachable again, reclaims it — so the refusal is
about reachability and not about the fixture being unreclaimable.

## `fn census_proceeds_without_runtime_when_no_intent_exists() {`

(15) No intent + no reachable runtime = the census **proceeds**.

This is the half a plausible suite forgets, and getting it wrong makes the
engine unusable on every machine without a container runtime — which today
is every machine, because `production_effect` is "none". The whole daemon is
armed unreachable, not one operation.

Second field held constant: the private root and the write command are the
same as in the refusing half above; only the presence of an intent moves.

## `fn a_reachable_runtime_that_refuses_to_list_refuses_the_write_command() {`

A runtime that is **reached** and refuses to list is not the same answer.

`RuntimeError` distinguishes `Unreachable` from `Failed` for exactly this:
"with no intent and no **reachable** runtime it proceeds" licenses proceeding
when the runtime is not there, and says nothing about one that is there and
will not answer. A daemon that answers and fails a `ps` cannot prove there is
no labeled orphan, so the census refuses rather than admitting over one.

Recorded as a judgement, not as a packet clause: it is the conservative
reading of a case the sentence does not enumerate, and the refusal names it.

Second field held constant: the namespace is empty in both halves — the one
state that *would* license proceeding — so the only thing under test is which
kind of runtime error it is.

## `fn census_report_names_reclaimed_probe_boundary() {`

(16) The census report names each reclaimed container's boundary from its
`runner_policy_sha256`.

ST-16 (k): "a probe container killed with its coordinator **before
`run_started`** is reclaimed by the next census, whose report names its
boundary from the intent's `runner_policy_sha256` **and the owner record**".

**PR7 completes this**: the owner-record half is `decisions.pr_sequence[8]`'s
"atomic owner record with the RunnerPolicy". PR6 holds the intent half, and
[`Boundary::NoIntentRecord`] is the honest name for the case where this side
has nothing.

Second field held constant: the two reclaimed containers are the same probe
kind under the same private root and are both dead-owner orphans; the only
thing that differs is which `RunnerPolicy` their record names, so a report
that carried one digest for both fails.

## `fn census_report_names_reclaimed_probe_boundary()` › `let recorded: BTreeSet<String> = [POLICY_A, POLICY_B]`

The values are the records' own — read back off disk rather than taken
from the fixture's variables, so the report cannot be its own oracle.

## `fn census_report_names_reclaimed_probe_boundary()` › `assert!(!harness.root.join("events.jsonl").exists());`

The probe was killed before `run_started`: nothing in this fixture wrote
an event log at all, and the boundary still has a name.

## `fn an_intent_naming_this_processs_own_incarnation_is_refused_before_any_effect() {`

---------------------------------------------------------------------------
3. Refusals, each with the ordering predicate it carries
---------------------------------------------------------------------------

## `fn an_intent_naming_this_processs_own_incarnation_is_refused_before_any_effect() {`

An intent naming this process's own incarnation refuses — **before any
effect**, including before a reclaim it would otherwise have performed.

`expected_failures_refusals[7]`, and "the one most likely to be written as a
`continue`". The fixture puts a perfectly reclaimable orphan beside it, so an
implementation that skipped the offending record and got on with its work
fails here rather than passing quietly.

**The refusal is arm (i)'s**, and this fixture was rewritten by
`PR6-RECOV-003`. The owner run stays an axis — `{own run, foreign run} ×
{this incarnation, an earlier one} × {owner lock held, free}` — because the
point of the grid is that the two arms give *different* answers to the same
incarnation, and the two foreign cells naming this process's incarnation are
now classified by the owner's lock like every other arm (ii) candidate:
**held -> never touched**, **free -> reclaimed**. The previous oracle
required a refusal in those two cells; that refusal never probed the lock,
so a dead foreign owner's container was unreclaimable and blocked every
write command under the root for good.

Second field held constant: the reclaimable orphan beside the suspect is
identical in every cell — same owner, same repo key, same state — so the only
thing that moves is the suspect's own ownership triple.

## `fn an_intent_naming_this_processs_own_incarnation_is_refuse…` › `let cells: [(&str, &str, &str, bool, bool); 6] = [`

`(tag, the suspect's owner run, its incarnation, is its lock held, does
it refuse)`. RUN_A is the run this process drives; RUN_C is not.

## `fn a_live_foreign_owners_container_naming_this_incarnation_is_refused_and_not_killed() {`

A **held** foreign owner's container is never touched, and its incarnation
does not change that — including when it is this process's own.

The mutation an independent review measured — an early branch classifying
any foreign candidate carrying the process incarnation as
`ForeignRunDeadOwner` — would **kill a held owner's container**, which arm
(ii) forbids in as many words ("held -> live owner -> never touched"). It is
still forbidden, and it is still what this fixture asserts; what
`PR6-RECOV-003` changed is that the protection comes from the **probe**
rather than from a refusal in front of it, which is the only version of the
protection that also lets a *dead* owner's container be reclaimed.

So the claim is made as a runtime state and not as a classification: the
container is still there, its record is still there, its view is still
there, and the funnel issued nothing at all.

Second field held constant: one owner, one container, one lock state (held);
only the incarnation the intent names moves, between this process's own and
an earlier one.

## `fn a_live_foreign_owners_container_naming_this_incarnation_…` › `assert!(`

The state of the machine is identical in both halves, and it is the
untouched state.

## `fn a_dead_foreign_owners_container_naming_this_incarnation_is_reclaimed() {`

A **dead** foreign owner's container carrying this process's incarnation is
reclaimed, which is the half the hoisted comparison made unreachable.

`PR6-RECOV-003`'s other cell, and the one that is not merely a different
classification of the same outcome: under the shipped rule this container
could never be reclaimed by anybody. Its owner is dead, so no census of
*that* run will ever run again; every write command under this private root
met the refusal and stopped. The grid is `{owner lock held, free}` with the
incarnation held at this process's own, and the two halves must differ in
what happens to the machine — a fixture that asserted only the free half
would pass an implementation that killed both.

Second field held constant: the same owner run, the same incarnation, the
same container and the same seeded state in both halves; only the owner's
lock moves.

## `fn a_dead_foreign_owners_container_naming_this_incarnation_…` › `assert_eq!(`

The owner's lock was asked, once, about the owner's own directory —
the step the hoisted comparison skipped.

## `fn a_labeled_container_this_census_cannot_own_blocks_admission() {`

A labeled container whose name no funnel could have written blocks
admission, and one whose labels do not say who owns it blocks admission.

`refusal_condition`: "a dead owner's or dead incarnation's labeled container
that **cannot be observed terminated** blocks admission". A container
claiming this private root that the funnel cannot name, or whose ownership
cannot be established, is one this census cannot take through
kill/observe/rm — so it refuses rather than proceeding past it.

Second field held constant: every case carries a valid `upstroke.private_root`
label under the censused root, so what is under test is only what is missing
beside it.

## `fn a_labeled_container_this_census_cannot_own_blocks_admiss…` › `let cases: [(&str, &str, Option<&str>, &str); 4] = [`

`(what, the name the runtime reports, the label to withhold, the needle)`.
Data rather than closures, so every case builds the same complete label
set and then breaks exactly one thing about it.

## `fn a_name_that_disagrees_with_its_own_record_refuses() {`

A name and its ownership evidence that disagree refuse.

The name is `upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>`, so
its components **are** ownership evidence. A record that says one incarnation
while its own file name says another would mean classifying on one value and
reclaiming a container named for the other.

Second field held constant: the container exists and is running in every
case; only which of the three components disagrees moves.

## `fn labels_and_a_record_that_disagree_about_the_owner_refuse() {`

A container whose labels and whose record disagree about its owner refuses.

The labels are derived from the record when a container is created
(`ContainerIntent::labels`), so a disagreement is not a state this engine
wrote — and picking a winner would mean deciding, from corrupted evidence,
whether to kill a container.

Second field held constant: the container name, the private root and the
record are the same in both cases; only which label was tampered with moves.

## `fn the_stable_prefix_barrier_refuses_each_of_its_four_predicates_independently() {`

---------------------------------------------------------------------------
4. Recovery step (a1) — the stable-prefix barrier
---------------------------------------------------------------------------

## `fn the_stable_prefix_barrier_refuses_each_of_its_four_predicates_independently() {`

The four predicates of the barrier are separately droppable, and each has its
own refusal.

`crash_reconstruction`: the census happens "after the stable-prefix barrier
of step (a1) has **synced** the surviving event-log prefix, **proven it
stable**, and **checked-replayed it**, so that no fold-derived reclaim
decision precedes durability". Reclaim decided from a prefix that was synced
but not proven stable, or proven stable but not replayed, is reclaim on
unproven authority.

The digests are computed **out of band** (`python3 -c 'hashlib.sha256(...)'`)
and written here as literals, so the barrier is not compared against the
function that produced it.

Second field held constant: every case starts from the same healthy triple
and breaks exactly one predicate.

## `fn the_stable_prefix_barrier_refuses_each_of_its_four_predi…` › `let measured = PrefixBytes::of(PREFIX);`

The measurement agrees with the out-of-band digests, so neither side is
the other's oracle.

## `fn the_stable_prefix_barrier_refuses_each_of_its_four_predi…` › `let mut moved = healthy();`

1. The boundary moved between the two reads.

## `fn the_stable_prefix_barrier_refuses_each_of_its_four_predi…` › `let mut rewritten = healthy();`

2. The bytes changed while the boundary stayed put.

## `fn the_stable_prefix_barrier_refuses_each_of_its_four_predi…` › `let message = refusal(`

3. Proven stable, and not durable to its boundary.

## `fn the_stable_prefix_barrier_refuses_each_of_its_four_predi…` › `let message = refusal(`

4. Synced and proven stable, and the replay consumed other bytes.

## `fn the_stable_prefix_barrier_refuses_each_of_its_four_predi…` › `assert!(`

A replay of the same length but different content is refused too: length
alone is not identity.

## `fn both_halves_of_discovery_are_scanned_and_every_cell_is_classified() {`

---------------------------------------------------------------------------
5. Discovery, both halves, every cell
---------------------------------------------------------------------------

## `fn both_halves_of_discovery_are_scanned_and_every_cell_is_classified() {`

Both halves of discovery are scanned, and every cell of `{intent present} ×
{container present}` is classified.

"discovery at every write-command start scans the whole namespace
`<R>/containers` … **and** docker ps by `upstroke.private_root`". A census that
read only the namespace misses a labeled orphan whose record was already
removed; one that read only `docker ps` misses an intent whose container the
Unix reaper already killed and removed — which is the *ordinary* state after
a Unix coordinator death, because the reaper does kill/rm and leaves the
record for the next census.

Second field held constant: one owner, one liveness answer, one private root;
only which halves hold evidence moves.

## `fn both_halves_of_discovery_are_scanned_and_every_cell_is_c…` › `Present::IntentOnly | Present::IntentAndViewAfterReaper => DiscoveredBy::IntentOnly,`

The two intent-only situations are indistinguishable to
discovery, which is the point of `PR6-CONV-003`: they differ
only in whether a view is on disk, and
`an_intent_only_candidate_after_the_reaper_still_has_its_view_pruned`
is the fixture that varies that.

## `fn both_halves_of_discovery_are_scanned_and_every_cell_is_c…` › `let empty = Harness::new("neither-half");`

The fourth cell — neither half — is the empty machine, and it is what the
census reports when nothing is there.

## `fn the_private_root_label_this_census_filters_on_is_the_one_the_intent_writes() {`

The label this census filters on is the label the funnel writes, and its
value is the one an independent table says it is.

A census that filtered on a different spelling would discover nothing and
report a clean machine — the "green because the test could not run" shape,
with the runtime standing in for the test. There is now **one** rendering
(`intent::private_root_label`) and both sides call it, so the agreement is
by construction; what this test still has to hold is the *value*, and it
holds it against encodings computed **out of band** and written as literals.
Comparing the function against itself would prove nothing, which is how the
two-copy version stayed green while the encoding was wrong.

Second field held constant: one record and one owner across every cell, so
the only thing that moves is the root's bytes.

## `fn the_private_root_label_this_census_filters_on_is_the_one…` › `const EXPECTED: &[(&str, &str)] = &[`

Computed with `python3 -c` from the rule "percent-encode every byte
outside [A-Za-z0-9/:.-_]", not by calling the function under test.

## `fn the_private_root_label_this_census_filters_on_is_the_one…` › `let backslash = private_root_label(Path::new(r"/srv/a\b"));`

The one byte whose rendering is platform-shaped, stated as the two
answers rather than as the one this platform happens to give.

## `fn the_private_root_label_is_injective_over_hostile_roots() {`

**Different private roots are disjoint worlds** — proved with a collision
pair, not with a round-trip.

`crash_reconstruction` says it in those words, and the container **name**
was designed for it: its components are `[0-9A-Za-z_]` only, so the parse on
`-` is unambiguous. The **label** is the other half — it is what `docker ps
--filter label=upstroke.private_root=…` selects on — and the rendering that
shipped, `to_string_lossy().replace('\\', "/")`, was not injective. On Unix a
backslash is an ordinary filename byte, so `<base>/a\b` and `<base>/a/b` are
**different directories** that rendered to one label, and a census
authorized for either queried and reclaimed the other's containers.

A round-trip test would not have caught it: the encoding round-trips
perfectly and still maps two inputs to one output. So this asserts a
**distinct-value count** over roots that differ only in the bytes an
encoding is tempted to fold, and it names the pair the review found.

Second field held constant: every root shares the same `<base>` prefix and
differs in exactly one interior byte, so nothing but that byte can be
producing the distinctness.

## `fn the_private_root_label_is_injective_over_hostile_roots()` › `let universal = [`

Distinct on **every** platform: none of these pairs differ only by a
path separator.

## `fn the_private_root_label_is_injective_over_hostile_roots()` › `{`

The collision pair the review measured. On Unix these are two
directories; on Windows they are one, and folding them is
canonicalization rather than a collision — so the claim is made on the
platform where it is a claim.

## `fn the_private_root_label_is_injective_over_hostile_roots()` › `{`

The second collision the shipped rendering had, and the one a
`\u{fffd}` in the table above only gestures at: `to_string_lossy` maps
**every** ill-formed byte sequence to `U+FFFD`, so two distinct non-UTF-8
roots — and a root that literally contains `U+FFFD` — were one label.
Constructible only on Unix, where an `OsStr` is bytes.

## `fn the_private_root_label_is_injective_over_hostile_roots()` › `for root in &roots {`

A root may not carry a byte that ends the `--filter` argument or starts
another filter, whatever the operator called their directory. This is
what lets `ReaperContainerScope` stop worrying about the root half.

## `fn every_topology_write_command_performs_the_census() {`

Every topology write command performs the census — `run` **and** `resume`.

`startup_census`: "performed by **every topology write command (run,
resume)**". Guarding it behind resume-only logic lets dead containers survive
into a fresh run's admission.

Second field held constant: the orphan, its owner, its liveness and the
private root are identical between the two halves; only the write command
moves.

## `fn census_returns_the_only_token_that_reaches_a_consumer() {`

---------------------------------------------------------------------------
6. The token, and what it precedes
---------------------------------------------------------------------------

## `fn census_returns_the_only_token_that_reaches_a_consumer() {`

[`CensusComplete`] is constructed in exactly one place.

`crash_reconstruction`'s four "before"s — slot/reservation initialization,
admission, credential-volume use, and this incarnation's probes — are
consumers PR7 and PR11 build. This slice cannot test against a consumer that
does not exist, so what it holds instead is that the token those consumers
will take can be made in exactly one way: by a census that completed.

The source census is the tree's own idiom
(`runner::container::tests::every_container_effect_in_the_tree_goes_through_the_funnel`),
and it has a positive control so a scan that stopped finding anything fails
rather than reporting silence.

Second field held constant: **none, and that is the answer rather than an
omission.** This is a census over the whole tree, so the axis it varies is
*which file* and there is no other field to pin. What replaces a second axis
here is the positive control — a scan whose needle stopped matching would
otherwise report an empty offender set and pass.

**The region is the whole file, and the floor is over bytes.** Two repairs,
both measured on this tree:

* The region was `effects::production_region`, which truncates a file at its
  first `#[cfg(test)]`. Ten files stop at something that is not a module —
  `src/engine/coordinator.rs` at a `#[cfg(test)] use` on line 36 of 1599 —
  so a `struct CensusComplete { forged: () }` at the **bottom** of that file
  passed while the identical five lines above the cut failed.
* The floor was `scanned > 20` with `scanned += 1` unconditional, so it
  counted files walked rather than regions read: it would have passed with
  every region empty, which is exactly the state the previous point put ten
  of them in. It is a byte floor now, and every region is asserted non-empty
  by name.

## `fn census_returns_the_only_token_that_reaches_a_consumer()` › `assert!(`

Per file, because the byte floor below is a sum: it stands at
1,000,000 against an actual over 1,500,000, so one file's region can
empty itself and the sum still clears the bar.

**Necessary, not sufficient**, and this is the assertion most likely
to be mistaken for more than it is. It sees a region that COLLAPSES.
It does not see one that is REPLACED: `PR7-R2C-CHAR-LITERAL-DESYNC`'s
refined form removes exactly the forged lines and adds a probe of the
same size, and was measured at 8525 non-whitespace bytes both with the
attack and without it. No floor, per-file or aggregate, can see a
zero-byte delta. What closes that is `effects::char_literal_end` and
`effects::configured_item_end` returning `start` instead of the file's
length when it cannot find an item's end.

## `fn census_returns_the_only_token_that_reaches_a_consumer()` › `assert_eq!(`

The positive control. `CensusComplete {` appears three times here — the
declaration, the `impl` header and the one construction — so the control
needle is the construction shape alone, and the scan above would find it
if it moved into another file.

## `fn census_returns_the_only_token_that_reaches_a_consumer()` › `let harness = Harness::new("token-shape");`

And the type really is closed: its field is private, so no other module
can build one even with a struct literal.

## `fn constructs_the_token(production: &str) -> bool {`

Whether `production` contains a **construction** of the token, as opposed to
a return type whose function body brace follows it.

`CensusComplete {` is also the last sixteen characters of
`-> &CensusComplete {`, and the first legitimate consumer of the token
necessarily has an accessor of that shape — PR7's startup census holds the
token and hands it out. A bare `contains` therefore reports the first real
caller as a forger, which is a scan that has stopped measuring what it
names.

**The exclusion is the arrow, not the ampersand.** Excluding every
`CensusComplete {` preceded by `&` would also excuse `&CensusComplete { .. }`
— a reference to a struct literal, which *is* a construction and is the
shape a forged token takes when it is passed to something that borrows one.
Only a return position can be followed by a body brace, so only a return
position is excused. Every construction shape — `CensusComplete { .. }`,
`&CensusComplete { .. }`, `Ok(CensusComplete { .. }`,
`Self::CensusComplete { .. }` — still matches, and the positive control
above is what keeps that true.

## `fn the_token_scan_excuses_a_return_position_and_nothing_else() {`

The scan's needle classifies a return position and a construction apart,
including the construction that hides behind an ampersand.

A unit test over strings rather than a planted file, because a planted
forgery **does not compile**: `CensusComplete`'s fields are private, so the
type system already refuses one. That makes this scan defence-in-depth for
the day those fields widen — and defence-in-depth that is never exercised is
the thing this project keeps paying for. So the classifier is exercised
directly.

## `fn the_token_scan_excuses_a_return_position_and_nothing_els…` › `"consume(&CensusComplete { report });",`

The one lane C's first needle would have missed: a reference to a
struct literal is a construction, and it is the shape a forged token
takes when it is handed to something that borrows one.

## `fn walk(dir: &Path) -> Vec<PathBuf> {`

Every `src/**/*.rs`, sorted.

## `fn r20_is_persistent_output_in_every_at_run_end_outcome_and_no_census_path_touches_it() {`

---------------------------------------------------------------------------
7. The resource rows this census is accountable for
---------------------------------------------------------------------------

## `fn r20_is_persistent_output_in_every_at_run_end_outcome_and_no_census_path_touches_it() {`

R20 is `operator_owned` and `persistent_output` in **all five**
`at_run_end` outcomes, and no census path touches it.

`resource_accounting[R20]`: "per-agent credential volume … `persistent_output`
(**never created or pruned by a run**)" for `Complete`, `Parked`, `Halted`,
`BudgetExceeded` and `NoRunFinished`. A run that tidied a volume it mounted
would destroy operator credentials, and the CLIs **rotate refresh tokens on
use**, so a discarded rotation forces a re-login.

Two halves, because either alone is weak: the five outcomes are transcribed
from the packet as an independent table, and the census is measured to issue
no volume operation at all on a fixture that reclaims two containers.

Second field held constant: a volume **is present** throughout, and two
containers really are reclaimed around it. Varying only the outcome column
would leave a table nothing executes; varying only the census would leave a
run that never had a volume to spare. The pair is what makes "never created
or pruned by a run" a measurement.

## `fn r20_is_persistent_output_in_every_at_run_end_outcome_and…` › `const AT_RUN_END: &[(&str, &str)] = &[`

Transcribed from `decisions.resource_accounting.rows[R20].at_run_end`,
not read back from any code.

## `fn r20_is_persistent_output_in_every_at_run_end_outcome_and…` › `let production =`

And the module names no volume operation at all.

## `fn r26_is_released_in_four_outcomes_and_the_census_is_the_mechanism_for_no_run_finished() {`

R26 is `released` in `Complete`, `Parked`, `Halted` and `BudgetExceeded`, and
the census is the mechanism for the fifth cell.

`resource_accounting[R26].at_run_end`: four outcomes release the container
(`release`, which is the funnel's completion sequence), and `NoRunFinished`
is reclaimed at the next write-command start — which is this module. A
container surviving a **budget stop** or a **park** would keep spending while
the run is supposed to be quiescent, which is why the first four are
`released` rather than "left for the census".

The four `released` cells belong to `release` and are held by
`runner::container::tests`; what is executed here is the fifth, and that a
container left by a run that never finished is gone, record and view with it.

Second field held constant: the owner is dead and the container is running
in the executed half, so the only thing distinguishing `NoRunFinished` from
the four `released` outcomes is **which mechanism disposes of it** — the
census here, `release` there. All three of R26's container, R19's view and
R26's record are asserted gone, because a fifth cell that pruned two of
three would leave the ledgers unbalanced in a way a single assertion misses.

## `fn r26_is_released_in_four_outcomes_and_the_census_is_the_m…` › `const AT_RUN_END: &[(&str, &str)] = &[`

Transcribed from `decisions.resource_accounting.rows[R26].at_run_end`.

## `fn a_container_that_never_terminates_exhausts_the_bounded_observation_and_refuses() {`

The observation wait is a step, not an implementation detail, and it is
**bounded**.

"reclaim = docker kill -> **wait until observed exited/removed** -> docker rm
…". Dropping the wait is the classic mutation: `kill` then `rm` still leaves
the container gone at the end, so a test that only checks the final state
passes. Here the container never terminates, the bound is exhausted, and the
refusal names the clause — and `docker rm` is never issued, which is what
says the wait sits **between** kill and rm rather than after both.

Second field held constant: the same container, owner and dead-owner verdict
as [`orphan_reclaimed_before_slot_reset`]; only whether `stop` actually stops
it moves.

## `fn the_reapers_container_selector_names_the_incarnation_and_not_the_root_alone() {`

---------------------------------------------------------------------------
8. The Unix reaper's selector — ST-16 (d)'s half that is pure
---------------------------------------------------------------------------

## `fn the_reapers_container_selector_names_the_incarnation_and_not_the_root_alone() {`

The reaper's selector names **both** labels, and every component varies the
rendering independently.

`os_matrix`: the reaper "kills the **dead coordinator's** labeled
containers". `upstroke.private_root` alone names every container of every run
under `<R>`, including a **live** coordinator's — which
`T-CONTAINER.authoritative_state` forbids in as many words ("a live
incarnation's containers must not be touched"). The incarnation is a
per-process ULID and is what makes the selector name one coordinator.

Second field held constant: the program is the same in every cell, so the
only thing that moves is the pair of label values.

## `fn the_reapers_container_selector_names_the_incarnation_and…` › `let scope = super::ReaperContainerScope::new("docker", roots[0], INC_1).expect("a scope");`

kill and rm carry the id and nothing else, and the reaper does only those
two: the view and the record are the next census's.

## `fn the_reapers_container_selector_names_the_incarnation_and…` › `assert_eq!(`

`--volumes` is in the table because the reaper is the last thing that can
name a container's **anonymous** volumes: after it removes the container
the following intent-only census has no handle on them
(`PR6-ACCT-006`). The expected vector is written out here rather than
read back from the function.

## `fn a_reaper_scope_whose_label_value_could_widen_the_filter_cannot_reach_the_reaper() {`

A label value that could change what the filter selects cannot reach the
reaper — refused for the incarnation, impossible for the root.

The reaper has no error channel and no allocator: it cannot report a
malformed selector, and a filter that matched more than it should would kill
a live coordinator's containers. The two halves of the selector are now
protected differently, and the difference is the point:

* the **incarnation** is used verbatim, so a hostile value is **refused**
  here, on the parent side;
* the **root** is rendered by [`private_root_label`], which percent-encodes
  every byte that could end the argument, so a hostile root is *accepted*
  and cannot widen anything. This is strictly stronger than refusing it: an
  operator whose private root contains a comma gets a working reaper rather
  than a refusal. The scope's own check is kept as the post-condition on
  that encoding — it inspects the **rendered** value, so an encoding that
  regressed would still fail closed here.

Second field held constant: the private root is well-formed in the
incarnation cases and vice versa, so each case names one hostile value.

## `fn a_reaper_scope_whose_label_value_could_widen_the_filter_…` › `assert!(`

The root half. An empty root still refuses — it renders to an empty
label, and a filter that matches everything would kill a live
coordinator's containers. Every other hostile root is accepted, and the
selector it produces carries exactly two filters.

## `fn a_reaper_scope_whose_label_value_could_widen_the_filter_…` › `assert!(super::ReaperContainerScope::new("docker", good_root, INC_1).is_ok());`

And the well-formed pair is accepted, so this is not a function that
refuses everything.

## `fn real_docker_census_owners() -> (Owner, Owner) {`

---------------------------------------------------------------------------
9. Docker-gated: a census against the real runtime
---------------------------------------------------------------------------

## `fn real_docker_census_owners() -> (Owner, Owner) {`

The two owners `real_docker_census_reclaims_a_dead_owner_and_spares_a_live_one`
creates, built here so a test that **always** runs can assert the property
the gated one depends on.

Owner constants that test alone uses. Container names are deterministic and
the daemon is one namespace shared with every other Docker-gated test in this
tree, which run concurrently: reusing the fixture constants above made
`docker create` fail with a name conflict against
`runner::container::tests`'s own gated test. Measured, and the reason these
run ids exist.

**The repo key is the build slot's, not a constant.** It was
`"cccccccccccccccc"`, and `fake::preclean_names` kills by name with no
liveness check: two suites running in two slots built the same three
components, so each one's pre-clean killed the other's **live** container.
`PR7-R3-CONTRACT-001`, which `b44040a` repaired at `exec.rs`'s caller and not
at this one.

The run ids stay fixed and must: a pre-clean matches a **previous run's**
residue exactly when the name recurs, and a component that is unique per
process would make it a no-op that looks like protection.

## `fn the_gated_censuss_names_are_scoped_to_this_build_slot() {`

**The gated census's own names are scoped to this build slot.**

The instance of `PR7-R3-CONTRACT-001` on this path, asserted by a test that
runs on every platform and needs no runtime — which is the point. The
pre-clean it guards is inside a Docker-gated test, so on a machine with no
usable image the hostile name was never even constructed, and four commits
of green suites said nothing about it either way.

The class boundary is
`runner::container::tests::a_pre_clean_refuses_every_name_a_concurrent_run_could_also_ask_for`
and its sibling, which assert the rule and that the helper enforces it. This
asserts that *these* names satisfy it.

## `fn real_docker_census_reclaims_a_dead_owner_and_spares_a_live_one() {`

A census over **real Docker** reclaims a dead owner's labeled orphan and
leaves a live owner's container alone.

The fake proves the decision; this proves the decision survives contact with
the runtime the decision is about — `docker ps --filter label=…` really does
return the containers this census expects, `docker kill`/`rm` really are
idempotent, and `observe` really does report a removed container as gone.

**Never pulls** (`non_goals[1]`): the image is discovered among what the
machine already holds, and a machine holding none reports absence through the
same loud, counted gate.

Second field held constant: both containers are created from the same image
with the same command under the same private root; the only thing that
differs is whether their owner's run directory is reported live.

## `fn real_docker_census_reclaims_a_dead_owner_and_spares_a_li…` › `crate::runner::container::fake::preclean_names(`

Pre-clean before the first `docker create`, not after the last teardown.

`reviews/FINDINGS.md` §16: this test's own cleanup is correct and cannot
help, because no in-process cleanup runs when the process is SIGKILLed.
These two names come from [`real_docker_census_owners`]: two fixed run
ids and this slot's own repo key, all stable across runs *in* this slot,
so the name a previous SIGKILLed run left is exactly the name this run is
about to ask for — and no name another slot's suite asks for. That
recurrence is what makes a pre-clean meaningful rather than an
unconditional retry.

## `fn real_docker_census_reclaims_a_dead_owner_and_spares_a_li…` › `let cleanup = |name: &ContainerName| {`

Whatever happened, do not leave real containers behind.

## `fn a_record_that_vanishes_between_the_scan_and_the_read_is_skipped() {`

A record that disappears between the namespace scan and the read of it is
**skipped**, deterministically.

This is the discovery half of "every step idempotent and tolerant of
already-gone so **two concurrent reclaimers converge**", and the racing
fixture above reaches it only by luck. The state it reaches — a directory
entry whose file is not there — is constructible on demand as a **dangling
symlink**: `read_dir` lists it and `fs::read` answers `NotFound`, which is
byte-for-byte the answer the losing reclaimer gets.

Measured, not assumed: before the repair in `list_intents`, a whole write
command refused with `Io { NotFound }` because another write command was
tidying at the same moment.

Second field held constant: a real, readable record sits beside the vanished
one in the same namespace, so the test cannot pass by skipping everything.

Unix-only because a dangling symlink needs a privilege the Windows guest's
test user does not have; the racing fixture above covers the same property
on every platform, less sharply, and this comment is the record of which
half runs where.

## `fn a_record_that_vanishes_between_the_scan_and_the_read_is_…` › `let malformed = Harness::new("malformed-record");`

And a record that is present but unreadable is still an error: "the
record could not be read" and "the record is gone" are different answers,
and only one of them licenses proceeding. Two shapes, because the
tolerance has two ways to be too wide.

## `fn a_record_that_vanishes_between_the_scan_and_the_read_is_…` › `let protected = Harness::new("unreadable-record");`

The one that matters for the Windows repair: a record whose read fails
with **`PermissionDenied`** and keeps failing. The repair tolerates that
errno while a delete is pending, and a repair that tolerated it outright
would let a census admit over a container whose ownership evidence it
could not read. The bound is what separates the two, and this is the
fixture that holds the separation.

## `fn colliding_run_dir_pairs() -> Vec<(&'static str, PathBuf, PathBuf)> {`

---------------------------------------------------------------------------
12. `PR6-RECOV-001` — the owner's run directory is recorded injectively
---------------------------------------------------------------------------

## `fn colliding_run_dir_pairs() -> Vec<(&'static str, PathBuf, PathBuf)> {`

Run directories that a lossy rendering maps onto **each other**.

The oracle of every test in this section, and it is a table of *pairs*: an
encoding is proved wrong by a collision, and a round trip cannot see one.
Each entry is `(what, left, right)`, and both sides are directories a
filesystem can name.

The rendering this replaced — `to_string_lossy().replace('\\', "/")` —
collides on the platform-specific pairs below, and the mutation an
independent review measured (extending the rewrite to another valid byte
such as `:`) collides on the first universal one. The universal pairs run
**everywhere**, so the property is not one a Windows build stops checking.

## `fn colliding_run_dir_pairs() -> Vec<(&'static str, PathBuf,…` › `{`

Unix only, and each for a stated reason. A backslash is an **ordinary
filename byte** there — `/repo\a/...` is a directory whose first
component is literally `repo\a` — while on Windows `\` and `/` are both
separators and folding them is canonicalization rather than a collision.
An ill-formed byte sequence is not constructible as a Windows path at
all.

## `fn the_recorded_run_directory_distinguishes_directories_a_lossy_rendering_merged() {`

The recorded run directory is **injective**, proved on colliding pairs.

`crash_reconstruction` records "run directory (**public path**)" and arm (ii)
probes "that run's run.lock". `PR6-RECOV-001`: with the shipped rendering,
live run B under `/repo\a/...` recorded `/repo/a/...`, a **different, real**
directory; a foreign census probed there, found no lock, called B dead and
killed B's running container.

Asserted as a distinct-value count over the pairs and then again as a
pairwise inequality, so a rendering that collided *one* pair could not hide
inside a set that happened to stay the right size.

Second field held constant: one owner run id, one incarnation, one repo key,
one invocation — only the run directory moves.

## `fn the_recorded_run_directory_distinguishes_directories_a_l…` › `assert_eq!(`

And the record still names the directory it was built from: an
injective encoding nobody can undo would send the probe nowhere.

## `fn the_recorded_run_directory_distinguishes_directories_a_l…` › `let by_path: BTreeMap<&PathBuf, &String> = recorded`

Across the whole table at once, and keyed by path because a directory may
appear in more than one pair: `n` distinct directories must record `n`
distinct values, so an encoding that merged two paths from *different*
pairs is caught as well as one that merged a pair.

## `fn the_recorded_run_directory_distinguishes_directories_a_l…` › `for (encoded, path) in &recorded {`

The label the container carries is the same string, so the two halves of
discovery cannot disagree about where the owner's lock is.

## `fn a_live_owner_under_a_hostile_run_directory_is_probed_where_it_actually_is() {`

The census probes the directory the owner really used — a **live** owner
under a hostile path is not killed.

The end of `PR6-RECOV-001`'s failure sequence, as a runtime state rather than
as a string comparison. Live run B holds the lock of `/repo\a/.upstroke/runs/B`
and a foreign census runs; the neighbouring directory `/repo/a/.upstroke/runs/B`
is deliberately **free**, so a census that probes the lossy rendering
classifies B dead and kills its container.

Second field held constant: an ordinary dead owner is seeded beside B in
both halves and must be reclaimed either way, so "nothing happened" cannot
pass this.

## `fn a_label_only_container_under_a_hostile_run_directory_reaches_the_same_lock() {`

A label-only container carries the same encoding, so the label half of
discovery reaches the same lock.

`{intent present} × {container present}` is a real grid and the label-only
cell has its own path into `Candidate.run_dir`
(`census::from_labels_alone`). An encoding applied on one side only would
pass every intent-carrying fixture.

Second field held constant: the same owner, the same hostile directory and
the same lock state as the intent-carrying case above; only which half of
discovery found it moves.

## `fn a_path_label_decodes_exactly_or_refuses() {`

The encoding is undone **exactly**, and a value no funnel could have written
is refused rather than guessed at.

The fail-closed half. `decode_path_label` is what turns evidence into the
path a lock is probed in, so a malformed value must not become *some* path:
every wrong probe answers "free", and "free" reclaims.

Second field held constant: one decoder, one call shape; the table varies
only the value handed to it, across well-formed and malformed.

## `fn a_path_label_decodes_exactly_or_refuses()` › `let exact: &[(&str, &str)] = &[`

Well-formed: `(the value, the path it names)`. The oracle is written out
by hand rather than taken from `path_label`, which is the function under
test's own inverse.

## `fn a_path_label_decodes_exactly_or_refuses()` › `assert_eq!(`

Decoding is the same on both platforms: it is a function of the
value's own bytes and knows nothing about separators.

## `fn a_path_label_decodes_exactly_or_refuses()` › `let backslash = expected.contains('\\');`

The encode direction is a fixed point too — **except** for the one
byte that is platform-shaped. On Windows `\` and `/` are both
separators, so `<x>\a` and `<x>/a` name one directory and rendering
the backslash as `/` maps *equal* paths to one label, which is the
canonicalization injectivity over paths asks for. On Unix `\` is an
ordinary filename byte and is escaped like any other.

## `fn a_path_label_decodes_exactly_or_refuses()` › `for value in ["%", "%5", "%zz", "/repo/%g0/x", "/repo/x%", "/repo/%5c/x"] {`

Malformed. Each is a shape `path_label` cannot emit.

## `fn a_path_label_decodes_exactly_or_refuses()` › `assert!(decode_path_label("/repo/%5c/x").is_err());`

Lower-case hex is deliberately refused: `path_label` emits upper case, so
accepting both would give one path two labels and lose injectivity in the
other direction.

## `fn a_run_directory_that_names_no_lock_blocks_admission_from_either_source() {`

`PR6-CORRECTNESS-016` — a run directory that does not say where its owner's
lock is blocks admission, from **either** evidence source.

`expected_failures_refusals[8]`: "an unreclaimable labeled container blocks
admission". The shipped code refused a *missing* `upstroke.run_dir` and
accepted `upstroke.run_dir=`, which joined to `run.lock` — a path relative to
this process's working directory, where there is no lock — so a live foreign
owner was classified dead and its container killed.

The grid is `{empty, relative, malformed} × {from the record, from the
labels}`, because the two sources reach `Candidate.run_dir` down different
code paths and the shipped check was on neither.

Second field held constant: the container's name, run and incarnation labels
are valid and identical in every cell, so nothing but the run-directory value
moves — a cell that refused for the wrong reason would say so.

## `fn a_run_directory_that_names_no_lock_blocks_admission_from…` › `let harness = Harness::new(&format!("unownable-run-dir-label-{tag}"));`

(a) From the labels, with no record: `from_labels_alone`.

## `fn a_run_directory_that_names_no_lock_blocks_admission_from…` › `let harness = Harness::new(&format!("unownable-run-dir-record-{tag}"));`

(b) From the record: the same predicate, the other path in.

## `fn a_run_directory_that_names_no_lock_blocks_admission_from…` › `for good in ["/repo/.upstroke/runs/B", "/repo/a%5Cb/runs/B"] {`

The rooted values the same function must **accept**, so the check is not
simply "refuse everything". `has_root` and not `is_absolute`: on Windows
`is_absolute` additionally wants a prefix, and `/repo/...` is the shape
every fixture and every Unix-written record carries.

## `fn a_census_with_no_intents_proceeds_past_every_diagnostic_that_means_unreachable() {`

---------------------------------------------------------------------------
13. `PR6-RECOV-005` — the census's runtime-required rule, over the
    diagnostics a real `docker` prints
---------------------------------------------------------------------------

## `fn a_census_with_no_intents_proceeds_past_every_diagnostic_that_means_unreachable() {`

`{intent present} × {verbatim docker diagnostic}`, through the production
classifier.

`crash_reconstruction`: "the container runtime is required **only** when an
intent exists or a labeled container is discoverable: if any intent exists
and the runtime cannot be reached the write command refuses …, and with no
intent and no reachable runtime it **proceeds**."

The finding's own note is why this test exists in this shape: every other
census fixture arms `RuntimeError::Unreachable` **directly**, so nothing
exercised the function that decides whether a real diagnostic *is*
unreachability — and the shipped one classified `permission denied while
trying to connect to the docker API` as an answered failure, which made a
census with **no container evidence at all** refuse. Here the fake is armed
with the verbatim stderr and `super::super::classify_docker_failure` picks
the variant.

Second field held constant: the same private root, the same absent
container, the same write command in every cell — only the diagnostic and
whether an intent is on disk move.

## `fn a_census_with_no_intents_proceeds_past_every_diagnostic_…` › `let diagnostics: [(&str, &str, bool); 4] = [`

`(what, verbatim stderr, does it mean the daemon was never reached)`.
Measured on docker 29.7.2; see `container::tests::UNREACHABLE_STDERR`.

## `fn a_census_with_no_intents_proceeds_past_every_diagnostic_…` › `let harness = Harness::new("no-intents-diagnostic");`

(a) No intents, no labeled container. "with no intent and no
reachable runtime it proceeds".

## `fn a_census_with_no_intents_proceeds_past_every_diagnostic_…` › `let harness = Harness::new("one-intent-diagnostic");`

(b) The same diagnostic with **one intent** on disk: refused either
way. This is the axis a fixture that only varied the diagnostic
would miss — a classifier repaired into "always unreachable" passes
half (a) and admits over a container it cannot prove terminated.

## `fn a_resume_converges_on_a_container_a_foreign_fresh_census_already_removed() {`

---------------------------------------------------------------------------
14. `PR6-ENUM-009` — ST-16 (h)'s racers are a **foreign write command and a
    resuming incarnation**, which is a role intersection, not one role twice
---------------------------------------------------------------------------

## `fn a_resume_converges_on_a_container_a_foreign_fresh_census_already_removed() {`

A **resuming** incarnation converges on a container a foreign **fresh**
census already removed.

ST-16 (h) is "(h) two concurrent reclaimers (**a foreign write command and
the resuming incarnation**) converge idempotently on the same dead
container", and the seam test's `slice` field says "PR11 (under
concurrency)". `concurrent_reclaimers_converge` races two `FreshRun`
censuses, so `{racer role} × {racer role}` has one cell filled and the named
one empty: the reviewer's mutation was to break **only** the Resume path
when it finds a container already gone, and every Fresh/Fresh fixture stays
green under it (`PR6-ENUM-009`).

**What PR6 owns and what PR11 owns**, stated here rather than in a table
somewhere else: PR6 owns that each *role* converges on already-gone state —
deterministically here, and interleaved in
[`a_fresh_and_a_resuming_census_race_one_container_and_converge`]. PR11 owns
the clause "under concurrency" in the sense ST-16 means it, which is **two
coordinator processes**: this slice has no `TopologyRun` to start a second
one with, and a resume's own precondition — holding its run lock — is PR7's
to establish.

The deterministic half first, because an interleaving test that passes for
the wrong reason is hard to see: the fresh census reclaims, then the
resuming one runs over the same root and must return a clean report rather
than an error.

Second field held constant: one root, one container, one owner; only which
role's census is second moves — and both orders are run.

## `fn a_resume_converges_on_a_container_a_foreign_fresh_census…` › `for (tag, first, second) in [`

`(tag, first, second)`. Both orders, because "converge" is symmetric and
an implementation that broke one direction is not converging.

## `fn a_resume_converges_on_a_container_a_foreign_fresh_census…` › `let two = harness.census(&second).unwrap_or_else(|error| {`

The second reclaimer sees the container gone, the record gone and the
view gone — the ordinary post-reaper state — and must converge.

## `fn a_resume_converges_on_a_container_a_foreign_fresh_census…` › `assert_ne!(one.report().command, two.report().command, "[{tag}]");`

And the two roles really were different, which is the axis the
Fresh/Fresh fixtures hold constant.

## `fn a_fresh_and_a_resuming_census_race_one_container_and_converge() {`

A **fresh** census and a **resuming** one race one container and converge.

ST-16 (h)'s racers, interleaved rather than sequenced — the same instrument
[`concurrent_reclaimers_converge`] uses, with the second axis filled in. Two
threads, released together by a [`Barrier`], over many rounds; one starts as
`FreshRun` and the other as `Resume`, and neither may refuse.

This is still *in-process* concurrency: two real coordinator processes are
PR11's, and the run-lock precondition a resume carries is PR7's. What it
holds is that the reclaim steps converge when a resume and a foreign write
command interleave, which is the part expressible in this slice.

Second field held constant: both racers get the same runtime, the same root
and the same containers; only the `CensusStart` differs between them.

## `fn a_fresh_and_a_resuming_census_race_one_container_and_con…` › `for start in [`

The two roles, distinct: a foreign write command that holds no run
lock, and an incarnation resuming a run of its own. RUN_A is neither
container's owner, so both racers reach arm (ii) and the containers
are reclaimable by either.

## `fn a_fresh_and_a_resuming_census_race_one_container_and_con…` › `assert!(`

Somebody did the work, in each role. The loser of a step may
legitimately find a container already gone and report fewer; between
them they must account for all four, and neither may refuse — which
is asserted above. Both reporting all four is the ordinary outcome
of two interleaved idempotent reclaimers and is not a defect.

## `fn st16_j_refuses_before_any_effect_and_before_the_token_that_precedes_recovery() {`

---------------------------------------------------------------------------
15. `PR6-ENUM-010` — ST-16 (j)'s "before any recovery event", split
---------------------------------------------------------------------------

## `fn st16_j_refuses_before_any_effect_and_before_the_token_that_precedes_recovery() {`

The refusal of ST-16 (j) happens before the census has done **anything**,
and before it can hand anybody the token that reaches a recovery event.

ST-16 (j): "with container intents present and the runtime unreachable the
write command refuses **before any recovery event**". That clause is an
ordering between the refusal and an *event log* — and this slice has no
event log and no production caller (`production_effect` is "none"; PR7 wires
`TopologyRun`). `PR6-ENUM-010` is that the reconciliation assigned the whole
clause to PR6 with no deferral recorded, so the surviving mutation is a
future caller that appends a recovery event **before** invoking the census.

**The split, stated so it is not rediscovered.** PR6 owns two predicates,
both asserted below:

1. the refusal precedes every effect this census could perform — no funnel
   site, no runtime operation beyond the reachability question itself, no
   record or view touched;
2. the refusal precedes the **`CensusComplete`** token, which by
   construction is the only value that reaches the four consumers
   (`census_returns_the_only_token_that_reaches_a_consumer`).

PR7 owns the third: that its `TopologyRun` calls the census **before** it
appends any recovery event. Nothing in this slice can hold that, and saying
so is the deferral.

Second field held constant: the same single intent and the same root in both
halves; only the runtime's answer moves.

## `fn st16_j_refuses_before_any_effect_and_before_the_token_th…` › `assert!(`

(1) No effect at all, and the runtime was asked exactly one question.

## `fn st16_j_refuses_before_any_effect_and_before_the_token_th…` › `assert!(`

(2) No token. The token is the only value that reaches a consumer, so a
refusal that produced one would have licensed the four things the census
precedes — of which "any recovery event" is the one ST-16 (j) names.

## `fn an_intent_only_candidate_after_the_reaper_still_has_its_view_pruned() {`

---------------------------------------------------------------------------
R3b: the post-reaper state, the recovery anchor, the settlement, the staged
half, and what a removal that never succeeds does to admission
---------------------------------------------------------------------------

## `fn an_intent_only_candidate_after_the_reaper_still_has_its_view_pruned() {`

The ordinary post-Unix-reaper state: no container, **a view**, an intent.

`PR6-CONV-003`. `DiscoveredBy::IntentOnly` covers two situations — a crash
between the intent write and `docker create`, and the state the Unix reaper
leaves, since it performs `kill/rm` and nothing else
(`T-CONTAINER.resume_action`). Every fixture seeded only the first, so
"intent-only" and "no view" were perfectly correlated and a reclaim that
skipped view cleanup for intent-only candidates removed the final record,
returned `CensusComplete`, and stranded an R19 directory nothing can ever
find again — with the suite green.

The grid is **{crash before create, after the reaper} × {view on disk}** and
its diagonal is the cell that was missing. The reclaimed report is
`IntentOnly` in both cells, which is what makes the two indistinguishable to
a consumer and is exactly why the *view* has to be handled unconditionally.

Second field held constant: the same owner, the same invocation and the same
resuming incarnation in both cells; only whether the view was mounted moves.

## `fn an_intent_only_candidate_after_the_reaper_still_has_its_…` › `assert_eq!(`

The premise of each cell, asserted rather than assumed.

## `fn an_intent_only_candidate_after_the_reaper_still_has_its_…` › `assert_eq!(`

And the whole namespace is empty, so nothing is left under `<R>` for
a later census to find — which is what "the ledgers balance" means.

## `fn an_unpruned_view_is_reclaimed_because_its_intent_survived() {`

A view that could not be pruned keeps its intent, and the **next** census
reclaims it.

`PR6-ACCT-005`, end to end. Discovery is `<R>/containers` plus `docker ps`
by label, and the view path is derived only after a candidate exists —
`<R>/views` is never enumerated. So an intent removed after a failed view
prune is an R19 directory with no discoverable owner, permanently. The cure
is that the record outlives what it anchors; the proof is that a second
census, with the obstruction gone, finds it and prunes it.

The intersection: **{view removal fails, succeeds} × {census runs again}**.
Cell (fails, no second census) is the residue state; cell (fails, second
census) is the recovery; the success cells are the control that says the
obstruction is what did it.

## `fn an_unpruned_view_is_reclaimed_because_its_intent_survive…` › `let mut hooks = RecordingHooks::new(harness.trace.clone());`

First census: the view removal is made to fail.

## `fn an_unpruned_view_is_reclaimed_because_its_intent_survive…` › `assert!(!harness.holds(&name), "the container was removed");`

The residue state: the container is gone, the view is not, and the
record that names it is **still there**.

## `fn an_unpruned_view_is_reclaimed_because_its_intent_survive…` › `let complete = harness`

Second census, obstruction gone: the anchor is what makes this possible.

## `fn an_unpruned_view_is_reclaimed_because_its_intent_survive…` › `let clean = Harness::new("anchor-control");`

The control: with nothing armed, one census does the whole thing.

## `fn every_reclaimed_container_settles_its_owner_interrupted_with_unknown_spend() {`

Every reclaimed container settles its owning identity **interrupted, with
unknown spend** — whichever half of discovery found it.

`PR6-RECOV-006`. `T-CONTAINER.authoritative_state` opens "**unknown
spend**" and `resume_action` ends "then settle the owning identity
**interrupted**". The container tests asserted cleanup and record deletion
and nothing about the outcome, so a `Reclaimed` that derived a *success*
from `discovered_by == IntentOnly` compiled and passed — and that is the
tempting derivation, because an intent-only candidate has no container and
so looks like an attempt that never ran. It is the ordinary post-Unix-reaper
state: the container was killed *because* it was running, and whatever it
spent is unaccounted.

The grid is **{IntentOnly, LabelOnly, IntentAndLabel} × {the settlement}**,
which is every cell of `DiscoveredBy::ALL`, plus the post-reaper cell that
has a view — so the answer is asserted to be a constant over the whole
discovery axis rather than over the two cells that happened to be seeded.

## `fn every_reclaimed_container_settles_its_owner_interrupted_…` › `assert_eq!(`

One settlement, over the whole grid: the value is not a function of
anything the census observed.

## `fn a_staged_intent_with_no_published_half_is_accounted_for() {`

A staged `<name>.intent.tmp` with no published half is accounted for, and
what happens to it depends on whose it is.

`PR6-ACCT-007`. `write_synced` durably creates the staged file before it
renames, so a crash between the two leaves one behind **before any container
exists** — `create_container` takes an `IntentWritten`, which is minted by
reading the *published* record back. `list_intents` skips the staged half
(writer-owned residue no reader may adopt), which is right for discovery and
left the file with no reclaim path at all: no candidate, no labeled
container, so nothing ever called `remove_intent` for it.

The grid is **{the staged bytes parse, are torn} × {whose name it carries}**:

| staged bytes | owner | disposition |
|---|---|---|
| a complete record | anyone | `Adopted` — an ordinary candidate under the ordinary rule |
| torn | this run, earlier incarnation | `Removed` — arm (i), dead by construction |
| torn | another run | `RetainedForeignOwner` — arm (ii) needs a run directory a torn file has none of |

The `Adopted` row is what makes the reclaim path exist at all; the third row
is the fail-closed one, and it is *reported* rather than silent so INV-22
has a class for it.

## `fn a_staged_intent_with_no_published_half_is_accounted_for()` › `let adopted_owner = Owner::new(RUN_A, INC_1, REPO_KEY_A);`

(a) A complete record, staged but never renamed, owned by a dead
    incarnation of the run this process is resuming.

## `fn a_staged_intent_with_no_published_half_is_accounted_for()` › `fs::create_dir_all(view_path(&harness.root, &adopted)).expect("a view");`

Its view exists: the crash was after the mount and before the rename.

## `fn a_staged_intent_with_no_published_half_is_accounted_for()` › `let mine = Owner::new(RUN_A, INC_2, REPO_KEY_A).name(&agent_probe());`

(b) Torn bytes under this run's earlier incarnation.

## `fn a_staged_intent_with_no_published_half_is_accounted_for()` › `let foreign = Owner::new(RUN_C, INC_1, REPO_KEY_B).name(&probe);`

(c) Torn bytes under **another** run.

## `fn a_staged_intent_with_no_published_half_is_accounted_for()` › `assert!(`

The premise: `list_intents` sees none of them, which is why nothing
reclaimed them.

## `fn a_staged_intent_with_no_published_half_is_accounted_for()` › `assert!(!staged_path(&adopted).exists());`

The adopted one was reclaimed like any other candidate: both halves of
its record and its view are gone.

## `fn a_staged_intent_with_no_published_half_is_accounted_for()` › `assert!(!staged_path(&mine).exists(), "this run's own torn residue");`

The torn ones went where the table says.

## `fn a_staged_intent_with_no_published_half_is_accounted_for()` › `assert_eq!(`

And nothing else in the namespace: exactly the foreign torn file.

## `fn a_view_removal_that_never_succeeds_blocks_admission() {`

A removal that keeps failing **blocks admission** rather than admitting over
residue.

`PR6-CONV-004`. `racing_removal` retries `RACING_ACCESS_ATTEMPTS` times and
then returns `Io`, and that final refusal is the fail-closed half of the
Windows delete-pending repair: a delete-pending name disappears within a few
attempts and a genuinely protected one still refuses after all of them.
Nothing kept a *view or intent* removal failing through the bound, so
turning that `Err` into `Ok(false)` — "treat it as gone" — passed: every
removal fixture reached `Ok` or `NotFound` first.

What that mutation costs is not a wrong return value, it is **admission**:
the census would return `CensusComplete`, the token that
`crash_reconstruction` requires before "slot/reservation initialization,
admission, an invocation's first use of an agent's credential volume, and
this incarnation's own probes" — over a view it could not remove and whose
intent it had just deleted.

The obstruction is a parent directory with its write bit cleared, which is
deterministic and is **not** delete-pending: the two are different states
and only one is transient. Skipped under a uid that ignores the bit.

The intersection: **{removal succeeds, removal never succeeds} × {is there a
census token}**. The success cell is the control, without which a test in
which nothing could be reclaimed would pass.

## `fn a_view_removal_that_never_succeeds_blocks_admission()` › `let _ = fs::set_permissions(&views, fs::Permissions::from_mode(0o755));`

Running as root, or on a filesystem that ignores the mode.

## `fn a_view_removal_that_never_succeeds_blocks_admission()` › `let _ = fs::set_permissions(&views, fs::Permissions::from_mode(0o755));`

The control: with the obstruction gone, the same census completes and
hands out the token.
