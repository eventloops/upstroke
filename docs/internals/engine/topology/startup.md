# `src/engine/topology/startup.rs`

Extended notes for [`src/engine/topology/startup.rs`](../../../../src/engine/topology/startup.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The startup census — **both** halves, in the packet's order, returning the
witness that says which one ran.

`decisions.sequential_substrate.startup_census`: "the census then performs:
(a) global container reclaim over `<R>/containers` and docker ps by the
private-root label under the incarnation-aware liveness rule …; (b)
run-directory census over `<repo>/.upstroke/runs`".

Half (a) is [`crate::runner::container::census::run_startup_census`], landed
by PR6 and documented in the tree as "**Step (a)** of the startup census".
This module calls it and does not reimplement it.

Half (b) is written here. Before this module there was no run-directory
census anywhere in the tree: [`crate::rundir`] has `classify_run_dir`,
`husk_report` and `prove_private_half_ownership`, and none of them iterates a
runs directory or reclaims, repairs or retains anything.

### One classifier, two callers

Half (b) does **not** contain a second classifier. The packet requires a
husk to be "retained and reported with its locator and reason by every census
**and by status**", and [`rundir::husk_report`] already computes exactly that
trichotomy — `Unstarted(BothHalves | PublicOnly(shape))` / `Retained(reason)`
— and is already what `src/status.rs` drives. This module is
`run_dir_names` → `husk_report` → act. A classifier of its own would drift
from the one an operator reads.

The one thing `husk_report` cannot hand over is the deletion token: it is
read-only and drops the [`PrivateHalfProof`] unspent. So the proof is
recomputed at the deletion site, and a second answer that is not `Proven`
wins — it is the one adjacent to the effect.

### One run-directory census, both write commands

[`census_run_dirs`] is half (b) and there is exactly one of it.
[`startup_census`] is `upstroke run`'s caller and
[`super::recover::ResumeCensused::census`] is `upstroke resume`'s; the
difference between them is the `own_run` argument and nothing else. INV-15
requires pre-run husks reclaimed "at write-command start under the worktree
lock", and a resume is a write command — a second, read-only run-directory
pass on the resume path would classify and report husks that no command ever
reclaimed.

### The census returns the witness

Wrapping a census result in a witness afterwards proves *possession*, not
*order*: the holder had a report and a lock, in either order. So the
predecessor is consumed **by value** by the census call itself and the
ordering *is* the call: [`FreshCensused::establish`] takes a
[`WorktreeLocked`], performs both halves itself, and returns the witness. It
is not constructible from a container [`CensusComplete`], which proves half
(a) alone, and there is no constructor anywhere that accepts a
[`StartupCensus`] a caller made some other way.

The recovery chain's own witnesses — including the one `BarrierHeld` this
crate has — live in [`super::recover`], because the ordering they encode is
that chain's. This module defines only the two the creation path needs.

### The two rules that decide correctness

Both are INV-15's, and both are about *not* deleting:

* **Nothing private that carries a commit record is ever deleted**, by any
  census. That is not a check in this module — it is conjunct 12 of
  [`rundir::prove_private_half_ownership`], and it is why the only value that
  reaches [`rundir::remove_private_husk`] is a token this module cannot
  construct.
* **Nothing private is deleted on shape, marker parse, basename or
  reparse-point checks alone.** Every one of those answers is a
  [`HuskDisposition::Retained`], and the retain arm of [`apply`] performs no
  effect at all.

### A failed reclaim is an outcome, not a refusal

[`apply`] is infallible. Every way a `RunDir` funnel can refuse becomes a
[`RunDirOutcome::Unreclaimable`] naming the [`FailedStep`], and the census
carries on to the next directory.

That is the same answer INV-15 and `startup_census` give everywhere else:
"cannot be reclaimed" is *retained and reported*, never a command-fatal
error. It matters most on the resume path, where this function is the whole
of the run-directory half. A dead run that left a provable husk whose private
half cannot be removed — `EACCES`, `EPERM`, `EBUSY`, or on Windows any
still-open handle — used to make `upstroke resume <id>` fail for **every**
run in the repository, on every attempt, because of a different run's
residue. `run_dir_names` sorts ascending, so a husk id sorting before the
resuming run's also took the own-run stale-marker repair with it.

The census's own run is not an exception, and that is deliberate. The only
effect the `own_run` licence reaches is [`rundir::remove_marker`] on a
Committed directory — the husk arms are gated on the run lock, which a resume
holds for its own directory — and a marker that outlives its repair is
residue, not state: nothing on the resume path reads `.creating`, the removal
is documented idempotent, and the next write command repairs it. A
`RunDir.RemoveMarker` failure is also a poor predictor of anything wider: an
unwritable public directory fails the log append too, with a message naming
the step that actually stopped the run, and a Windows handle held on the
marker file says nothing about the log. Refusing here would replace a precise
later error with an imprecise earlier one, and would put a second policy in
the one function whose two callers are supposed to differ in `own_run` and
nothing else.

The one error [`census_run_dirs`] keeps is the opposite question: not "this
census could not finish one directory", which is an outcome, but "**no
census happened**", which no per-directory report can express. See
[`enumerate`]. Both halves of that policy have to be read together — a
reclaim that refuses is reported, and an enumeration that refuses is not
reportable at all.

A caller that reports what the census did therefore reads
[`RunDirCensusReport::unreclaimable`] as well as
[`RunDirCensusReport::retained`]: the two are siblings, and a report that
printed only the second would hide exactly the directories an operator has to
act on by hand.

## `pub struct CensusInputs<'a> {`

---------------------------------------------------------------------------
Inputs
---------------------------------------------------------------------------

## `pub struct CensusInputs<'a> {`

Everything both halves need, as one value.

**There is one `authorized_root` and both halves census it.** `R` is computed
read-only by recovery step (a0) before any lock is taken, and half (a)'s
`private_root` and half (b)'s ownership root are the same field here rather
than two parameters that a caller could disagree with itself about. A
container census under one root followed by an ownership proof under another
would admit over containers nobody censused; making the two one field is that
refusal expressed as a shape rather than as a check.

## `pub struct CensusInputs<'a>` › `pub repo_root: &'a Path,`

The repository whose `<repo>/.upstroke/runs` is censused.

## `pub struct CensusInputs<'a>` › `pub repo_key: &'a RepoKey,`

This repository's key. The marker and the owner record must both carry
it, or the husk is a directory copied from another repository.

## `pub struct CensusInputs<'a>` › `pub authorized_root: &'a Path,`

The authorized private root `R`, computed read-only before any lock.

## `pub struct CensusInputs<'a>` › `pub incarnation: &'a str,`

This process's per-process ULID.

## `pub struct CensusInputs<'a>` › `pub runtime: &'a dyn ContainerRuntime,`

The container runtime seam. Required only when an intent exists or a
labeled container is discoverable.

## `pub struct CensusInputs<'a>` › `pub liveness: &'a dyn OwnerLiveness,`

Whether another run's coordinator is alive.

## `pub struct CensusInputs<'a>` › `pub view: &'a dyn GitView,`

The disposable Git view seam.

## `pub enum FailedStep {`

---------------------------------------------------------------------------
The report
---------------------------------------------------------------------------

## `pub enum FailedStep {`

Which of the census's four effects returned an error.

Carried by [`RunDirOutcome::Unreclaimable`], and four values rather than one
because the residue each leaves is different and two of them are opposite.
[`Self::PublicHalfAfterPrivate`] is the only failure on which a private half
**is** gone, and [`Self::PrivateHalf`] is the only one on which nothing about
the private half is known.

## `pub enum FailedStep` › `PublicHalf,`

`RunDir.RemovePublicHusk` on a directory with nothing private bound.
Nothing private existed by ordering, so nothing private is at risk.

## `pub enum FailedStep` › `PrivateHalf,`

`RunDir.RemovePrivateHusk` under the proof token. **The public half is
deliberately left where it is**, marker and all: `.creating` is the
private half's only locator, and the census does not know whether that
half is still there — `remove_dir_all` is not atomic and its error is the
same value whether it removed nothing, every child, or the whole tree and
then failed on the way out.

## `pub enum FailedStep` › `PublicHalfAfterPrivate,`

`RunDir.RemovePublicHusk` **after** the private half went through the
proof-token funnel. The private half is gone; the public husk survives
carrying a marker whose target is absent, which the next census reclaims
public-only.

## `pub enum FailedStep` › `StaleMarker,`

`RunDir.RemoveMarker` on a committed run's stale `.creating`. The run
itself is untouched; the marker is residue the next census with the lock
free, or the owner's next resume, removes.

## `impl FailedStep` › `pub const ALL: &'static [Self] = &[`

Every step, as a closed set.

## `impl FailedStep` › `pub const fn name(self) -> &'static str {`

This step's name, for a report and for a test's table.

## `impl FailedStep` › `pub const fn what_failed(self) -> &'static str {`

The operator-facing clause: what could not be done, and what that leaves.

## `pub enum RunDirOutcome {`

What the census did with one run directory, and why.

The set is closed and mirrors `startup_census`'s own enumeration: arms (i)
and the target-absent half of (ii) reclaim the public half alone, arm (ii)
reclaims both halves under the proof, arm (iii) retains, the stale-marker
sentence repairs, and the held-`run.lock` sentence skips.

## `pub enum RunDirOutcome` › `ReclaimedPublicOnly(UnboundShape),`

Arm (i), and the target-absent half of arm (ii): nothing private is
bound, so the public half alone was reclaimed. "A bare directory or one
holding only a staged `.creating.tmp` … is reclaimed (no private half
exists by ordering)"; "if the marker's private target does not exist the
public husk alone is reclaimed".

## `pub enum RunDirOutcome` › `ReclaimedBothHalves,`

Arm (ii): the bidirectional ownership proof held and `committed.json`
was absent, so "the private half is deleted … the proof yields a
`PrivateHalfProof` token that `RunDir.RemovePrivateHusk` alone accepts,
then the public directory is removed with the marker last".

## `pub enum RunDirOutcome` › `Retained(RetainReason),`

Arm (iii): "retained and reported with its locator and reason by every
census and by status". **Nothing private was deleted.**

## `pub enum RunDirOutcome` › `RepairedStaleMarker,`

"A Committed directory still carrying `.creating` or `.creating.tmp` …
has the stale marker removed when its `run.lock` is free". The run
itself is untouched.

## `pub enum RunDirOutcome` › `Committed,`

A Committed directory with no stale marker: a run, and nothing for a
census to do. Reported so the census's answer is total over the runs
directory rather than over the husks in it.

## `pub enum RunDirOutcome` › `Skipped,`

"A Husk with a held `run.lock` is skipped (defense in depth …)", and the
same sentence's other half for a Committed directory whose live owner
"removes it in recovery step (a)".

## `pub enum RunDirOutcome` › `Unreclaimable {`

The reclaim or the repair this census planned returned an error, so the
directory is **retained with the error recorded** and the census carries
on to the next one.

Not a refusal, and that is the whole point of the arm. `startup_census`
and INV-15 answer "cannot be reclaimed" with *retain and report*; the
census "never establishes authority", so its failure to reclaim one
directory may not withhold one from the command. Before this arm the
error propagated, and one husk whose private half could not be removed —
`EACCES`, `EPERM`, `EBUSY`, or on Windows any still-open handle — made
`upstroke resume <id>` fail for **every** run in the repository, on every
attempt, because of a different run's residue.

[`super::create::Disposition::PrivateHalfRemovalFailed`] is the creator's
side of the same answer, and states the same policy: "it is not a second
error to report over the one that stopped the run".

## `pub enum RunDirOutcome` › `step: FailedStep,`

Which effect refused, and therefore what is left on disk.

## `pub enum RunDirOutcome` › `detail: String,`

The error, as the operator sees it.

## `impl RunDirOutcome` › `pub const KINDS: &'static [&'static str] = &[`

Every outcome, as a closed set.

The list a suite is measured against, so that an arm added later and
exercised by nobody fails a count rather than passing quietly — the same
device as [`RetainReason::KINDS`], and for the same reason: Rust has no
reflection over variants, so [`Self::kind`]'s exhaustive match is what
makes adding one here and not there impossible.

## `impl RunDirOutcome` › `pub const fn kind(&self) -> &'static str {`

This outcome's kind. Exhaustive by construction.

## `impl RunDirOutcome` › `pub const fn reclaimed_anything(&self) -> bool {`

Whether a deletion **completed** for this outcome.

Alethic, and [`FailedStep`] is what makes that word load-bearing. A
`RunDir.RemovePublicHusk` that returned an error may have removed every
entry of the directory, some of them, or none — `remove_dir_all` and this
funnel's entry loop are not atomic and the error is the same value in all
three cases — so [`FailedStep::PublicHalf`] answers `false` here rather
than claim a reclaim that may not have happened.
[`FailedStep::PublicHalfAfterPrivate`] answers `true`, because on that
one the private half went through the proof-token funnel and the funnel
returned `Ok`.

## `impl RunDirOutcome` › `pub const fn deleted_a_private_half(&self) -> bool {`

Whether the **private** half is known to have been deleted.

`startup_census`'s "nothing private is ever deleted on shape, marker
parse, basename, or reparse-point checks alone" is a statement about
which arm a shape reaches, and this is the predicate a test states it
with — so it stays alethic. [`FailedStep::PrivateHalf`] answers `false`
and [`Self::may_have_deleted_a_private_half`] is where it answers `true`.

## `impl RunDirOutcome` › `pub const fn may_have_deleted_a_private_half(&self) -> bool {`

Whether the private half **may** have been deleted, in whole or in part.

The epistemic sibling of [`Self::deleted_a_private_half`], and the pair
exists so that "is the private half gone" and "is there residue nobody
observed" are two questions with two answers.
[`FailedStep::PrivateHalf`] is the arm that answers them differently:
`false` to the first, because a failed `remove_dir_all` decides nothing,
and `true` here, because it may have emptied the directory on the way to
its error.

## `pub struct RunDirEntry {`

One run directory, what became of it, and the locator it recorded.

## `pub struct RunDirEntry` › `pub run_id: String,`

The directory's basename, which is the run id a marker must agree with.

## `pub struct RunDirEntry` › `pub public: PathBuf,`

`<repo>/.upstroke/runs/<run_id>` — where the husk is, reported whether or
not a marker could be read.

## `pub struct RunDirEntry` › `pub locator: Option<PathBuf>,`

The private locator, exactly as [`rundir::husk_report`] reports it to
`status`.

`None` in three cases, and each is a decision:

* there is no marker, or the marker does not parse — an unparseable
  marker names no target this census is entitled to believe, and
  reporting a guess would name `<R>/runs/<basename>`, the very path the
  proof refused to bind;
* a Committed directory, whose private half is bound by
  `run_started.private_dir` and verified in recovery step (a), not by a
  marker on the public half;
* a directory whose `run.lock` is held, which this census does not read
  the marker of at all. A skipped directory is one a live process owns,
  and the packet's "reported with its locator and reason" is the
  retention sentence, not this one.

## `pub struct RunDirEntry` › `pub class: RunDirClass,`

What [`rundir::classify_run_dir`] answered.

## `pub struct RunDirEntry` › `pub outcome: RunDirOutcome,`

What was done, and why.

## `impl RunDirEntry` › `pub const fn retain_reason(&self) -> Option<&RetainReason> {`

The reason this directory was retained, when it was.

## `impl RunDirEntry` › `pub const fn is_possibly_committed(&self) -> bool {`

Whether this is the third of `startup_census`'s three status sentences:
"a possibly committed run whose public log has no valid committed first
line".

## `impl RunDirEntry` › `pub fn describe(&self) -> String {`

The operator-facing sentence: what was done to this directory, and why.

## `pub struct RunDirCensusReport {`

What half (b) found and did, one entry per directory, in run-id order.

The census's answer is **total** over `<repo>/.upstroke/runs`: every entry
[`rundir::run_dir_names`] returns has exactly one [`RunDirEntry`] here.
`startup_census` requires every entry to classify before the write command
proceeds, and a report that only listed the directories something happened to
could not be read as evidence of that.

## `impl RunDirCensusReport` › `pub fn entries(&self) -> &[RunDirEntry] {`

Every directory censused, in run-id order.

## `impl RunDirCensusReport` › `pub fn of(&self, run_id: &str) -> Option<&RunDirEntry> {`

The entry for one run id, if the census saw that directory.

## `impl RunDirCensusReport` › `pub fn reclaimed(&self) -> Vec<&RunDirEntry> {`

Everything reclaimed, in either shape.

## `impl RunDirCensusReport` › `pub fn repaired(&self) -> Vec<&RunDirEntry> {`

Every committed run whose stale marker was removed.

## `impl RunDirCensusReport` › `pub fn retained(&self) -> Vec<&RunDirEntry> {`

Everything retained and reported, **including** the possibly committed.

A possibly committed husk *is* retained — `startup_census` puts it in the
same arm (iii) as every other retention and gives it a `RetainReason` of
its own. [`Self::possibly_committed`] is the subset, not a sibling, and
exists because the status trichotomy names it as its own sentence.

## `impl RunDirCensusReport` › `pub fn possibly_committed(&self) -> Vec<&RunDirEntry> {`

The retained husks whose private half carries a commit record.

## `impl RunDirCensusReport` › `pub fn skipped(&self) -> Vec<&RunDirEntry> {`

Everything skipped because a live process holds its `run.lock`.

## `impl RunDirCensusReport` › `pub fn unreclaimable(&self) -> Vec<&RunDirEntry> {`

Every directory whose planned reclaim or repair returned an error.

A sibling of [`Self::retained`] rather than a subset of it: these carry
no [`RetainReason`], because nothing was *classified* as unremovable —
the plan was to remove and the funnel refused. A caller that wants
"everything still on disk after this census" asks both.

## `pub struct StartupCensus {`

Both halves' results, and nothing else.

The fields are private and this module mints the only value, so a caller
holding a [`CensusComplete`] — which proves half (a) alone — cannot present
evidence of half (b). The witnesses hold one of these; they do not hold a
`CensusComplete`.

## `impl StartupCensus` › `pub const fn containers(&self) -> &CensusComplete {`

Half (a): what the global container reclaim found and did.

## `impl StartupCensus` › `pub const fn run_dirs(&self) -> &RunDirCensusReport {`

Half (b): what the run-directory census found and did.

## `impl StartupCensus` › `pub fn into_parts(self) -> (CensusComplete, RunDirCensusReport) {`

Both halves, for a caller that owns the value.

## `impl StartupCensus` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `pub fn startup_census(`

---------------------------------------------------------------------------
The entry point
---------------------------------------------------------------------------

## `pub fn startup_census(`

`upstroke run`'s census: the worktree lock is held, no run lock is.

Consumes the [`WorktreeLocked`] witness **by value** and hands the lock back
inside the [`FreshCensused`] it returns: `run_creation` holds the physical
worktree lock "across the startup census and the whole run", so a census that
dropped it would end the exclusion it was performed under. The order is the
call — there is no way to reach a `FreshCensused` without having held the
lock first, and no way to hold one afterwards without the census having run.

A fresh run has no own-run arm: `startup_census` puts the census "before …
run-lock acquisition for a fresh run", so every directory in the runs tree
belongs to somebody else and the held-`run.lock` rule governs all of them.
`upstroke resume`'s census is [`super::recover::ResumeCensused::census`],
which passes its own run id to the same [`census_run_dirs`].

### Errors

Whatever half (a) refuses with — an unreachable runtime with intents present,
an intent naming this process's own incarnation, a labeled container whose
ownership cannot be established, a dead container that cannot be observed
terminated — and [`UpstrokeError::Io`] from half (b) when the runs directory
exists and cannot be enumerated. A reclaim or a repair that fails in half (b)
is a [`RunDirOutcome::Unreclaimable`] entry and not an error.

## `fn both_halves(`

(a) then (b). The order is the packet's and is not an implementation detail.

Half (a) reclaims a husk's probe containers, and half (b) then deletes the
husk that owned them; run the other way round and the census would remove the
intent namespace's own evidence of who a running container belonged to.

## `let containers = run_startup_census(hooks.container(), &census)?;`

(a). Its own contract is "refuse before any effect" if any container
classification refuses, so an `Err` here means nothing was reclaimed and
half (b) has not touched the disk either.

## `pub(crate) fn census_run_dirs(`

---------------------------------------------------------------------------
Half (b)
---------------------------------------------------------------------------

## `pub(crate) fn census_run_dirs(`

The run-directory census over `<repo>/.upstroke/runs`.

**Every write command calls this one function.** `upstroke run` reaches it
through [`both_halves`] with no `own_run`; `upstroke resume` reaches it from
[`super::recover::ResumeCensused::census`] with its own run id. INV-15's
"reclaims pre-run husks at write-command start under the worktree lock" is
not satisfied by a second pass that classifies and reports, so there is no
second pass: the two callers differ in `own_run` alone.

Two phases, and the split is deliberate. Every classification and every
ownership proof is read-only and completes **before** the first deletion, so
a census whose plan is wrong about one directory has not already reclaimed
another on behalf of a command that then refused — the same shape half (a)
states for itself ("step 4 completes before step 5 begins on purpose"). The
worktree lock is held across both phases, so nothing else can move a
directory between them.

**Phase 2 never stops.** A funnel that refuses on one directory is that
directory's [`RunDirOutcome::Unreclaimable`], not the command's error: see
that arm. So the one error this function has left is the one that means *no
census happened at all* — the runs directory could not be enumerated.

### Errors

[`UpstrokeError::Io`] when `<repo>/.upstroke/runs` exists and cannot be read.
[`rundir::run_dir_names`] swallows that failure into an empty vector, and a
census that reported success having scanned nothing would convert INV-15's
"reclaims pre-run husks at write-command start" from an unproven claim into
an apparently-proven one. Phase 1 is otherwise read-only and cannot fail.

## `fn enumerate(repo_root: &Path) -> Result<Vec<String>, UpstrokeError> {`

Which run ids this census walks, in run-id order.

[`rundir::run_dir_names`] with the one thing it cannot say added: **an empty
answer is two answers.** It opens the runs directory with
`let Ok(entries) = fs::read_dir(…) else { return Vec::new() }`, so "there is
nothing there" and "this process could not read it" are the same value. Only
the first is a census: the second reports success having scanned nothing,
which turns INV-15's "reclaims pre-run husks at write-command start" from an
unproven claim into an apparently-proven one, and — because the walk is now
the only way this census reaches a directory — silently skips the resuming
run's own stale-marker repair as well.

So an empty answer is checked rather than believed. The probe runs *only*
then, because an empty vector is the only shape the swallow can produce, and
a runs directory that does not exist yet is a real emptiness — the shape the
first write command in a repository sees.

Refusing rather than reporting is deliberate, and it is not the refusal
[`RunDirOutcome::Unreclaimable`] removes: that one is "this census could not
finish one directory", which retains and reports; this one is "no census
happened", which no report can express. The enumeration is not forked to
close it — `status` and `rundir::list_husks` walk the same
`rundir::run_dir_names`, and a census that enumerated differently from what
an operator reads would drift from it.

## `struct Scanned {`

One directory, classified and decided. Read-only from end to end.

## `enum Planned {`

What the census intends to do with one directory.

[`Planned::ReclaimBothHalves`] carries the proof token, which is not `Clone`
and is spent by [`rundir::remove_private_husk`]. Holding it here rather than
re-proving in phase 2 is what makes "the proof that authorized this deletion"
and "the proof this census computed" the same object.

## `fn scan(run_id: &str, inputs: &CensusInputs<'_>, own_run: Option<&str>) -> Scanned {`

Classify one directory and decide, read-only.

## `fn scan(run_id: &str, inputs: &CensusInputs<'_>, own_run: O…` › `let lock_held = rundir::is_running(&public);`

`is_running` is the read-only probe: on Unix `F_GETLK` asks who holds the
lock without taking it, and an absent lock file means the run never
started. "A Husk whose `run.lock` is **free or absent** is handled by
shape and proof."

## `fn scan(run_id: &str, inputs: &CensusInputs<'_>, own_run: O…` › `let own = own_run == Some(run_id);`

The own-run exception, stated twice by the packet: recovery step (a1)'s
census covers "this run's own stale marker, **which the owner removes
here**", and the stale-marker sentence's "otherwise its live owner
removes it in recovery step (a)" is the same removal from the other
side. It licenses the marker repair and nothing else.

## `fn scan(run_id: &str, inputs: &CensusInputs<'_>, own_run: O…` › `locator: None,`

A committed run's private half is bound by
`run_started.private_dir`, which recovery step (a) verifies. A
marker on it is stale residue, not a binding to report.

## `fn scan(run_id: &str, inputs: &CensusInputs<'_>, own_run: O…` › `if lock_held {`

Every husk arm is gated on the lock alone. A husk whose lock is held is
skipped whoever holds it, this process included: under the worktree lock
no live creator can exist in this worktree, so a held lock on a husk is
either another repository's process or this resume's own run with a
damaged log — and neither is a directory a census may delete.

## `fn scan(run_id: &str, inputs: &CensusInputs<'_>, own_run: O…` › `let report = rundir::husk_report(`

The one classifier. `status` drives the same call on the same directory
and gets the same locator and the same reason.

## `fn scan(run_id: &str, inputs: &CensusInputs<'_>, own_run: O…` › `HuskDisposition::Unstarted(Reclaimable::BothHalves) => {`

`husk_report` is read-only and drops its token unspent, so the proof
is recomputed here to mint one. A second answer that is not `Proven`
**wins**: it is the one adjacent to the deletion, and every way it can
differ — a commit record that has since appeared, an owner record that
has since been rewritten — is a reason not to delete.

## `fn apply(hooks: &mut dyn RunDirHooks, public: &Path, plan: Planned) -> RunDirOutcome {`

Perform one plan. The only place in this module that has an effect.

**Infallible, by type.** Every way a funnel can refuse is a
[`RunDirOutcome::Unreclaimable`] naming the [`FailedStep`], so one
directory's residue cannot end the command that censused it. The retain arm
keeps a second guarantee it had before: it reaches no funnel at all, which is
now visible in the signature as well as in the body.

## `fn apply(hooks: &mut dyn RunDirHooks, public: &Path, plan: …` › `if let Err(error) = rundir::remove_private_husk(proof, hooks) {`

The order is load-bearing: "the census reclaims the private half
through the proof-token funnel, **then** the public directory with
the marker last … so a kill mid-census leaves a husk the next
census completes". Reversed, a kill between the two would leave a
private half no marker names and no census can ever prove.

And **the public half goes only if the private one went**: a
private removal that returned an error returns here rather than
falling through, because `remove_public_husk` deletes `.creating`
with the directory and that marker is the private half's only
locator. The creator states the identical rule at its own
`RunDir.RemovePrivateHusk`; this is the census's half of it.

## `fn apply(hooks: &mut dyn RunDirHooks, public: &Path, plan: …` › `Planned::Retain(reason) => RunDirOutcome::Retained(reason),`

Arm (iii). No effect, by construction rather than by discipline:
there is no funnel call on this path at all.

## `fn unreclaimable(step: FailedStep, error: &UpstrokeError) -> RunDirOutcome {`

One failed effect, as the outcome that replaces it.

## `fn stale_marker_present(public: &Path) -> bool {`

Whether a committed run still carries the marker its creator publishes at P1.

Both spellings: "a Committed directory still carrying `.creating` **or**
`.creating.tmp`". `symlink_metadata` rather than `exists`, so a marker that
is a dangling link is still a marker to remove rather than a file that reads
as absent.

## `mod witness {`

---------------------------------------------------------------------------
The witnesses
---------------------------------------------------------------------------

## `mod witness {`

The two witnesses the creation path consumes and mints, **each alone in its
own module**.

A nested module per witness, and not one module holding both: an item private
to a module is visible to that module **and its descendants**, so two types
sharing one module could each build the other out of its parts, and every
function in `startup` — its own tests included — could mint either from
hand-built fields. That is a naming convention, not a type. Siblings see only
what is `pub`, which here is the constructor and the accessors. The same rule
[`super::super::recover::chain`] states for the recovery order's seven, and
for the same reason.

Neither derives `Clone`, `Copy` or `Default`: a `Clone` would let one census
authorise two, and a `Default` would mint evidence out of nothing. The same
device `rundir::ownership` uses for [`PrivateHalfProof`].

**Ownership note.** [`WorktreeLocked`] is a *predecessor* of this module's
census — the creation chain's third link — and is defined here because
[`super::startup_census`] cannot be typed without it; the lane that owns
`prelock.rs`/`create.rs` extends it with whatever else its steps carry
forward. The recovery chain's predecessors are **not** restated here.
`BarrierHeld` in particular belongs to [`super::super::recover`], where its
constructor consumes a `RecordsVerified` that consumed a `LocksHeld` that
consumed a `RootDerived`: a second one defined beside this census would be a
barrier reachable with no locks, no records and no bound run id, which is
exactly the hole it was.

## `mod witness` › `mod locked {`

The creation chain's third link.

## `mod locked` › `pub struct WorktreeLocked {`

The physical worktree lock is held.

Holds the lock **by value**, so possessing the witness *is* holding
the lock: `run_creation` takes it "across the startup census and the
whole run", and a witness that merely remembered an acquisition would
outlive the exclusion it claims.

## `pub struct WorktreeLocked` › `lock: WorktreeLock,`

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `impl WorktreeLocked` › `pub fn from(lock: WorktreeLock) -> Self {`

The only constructor. Takes the lock by value.

## `impl WorktreeLocked` › `pub const fn lock(&self) -> &WorktreeLock {`

The lock this witness is holding.

## `impl WorktreeLocked` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `mod witness` › `mod fresh {`

A fresh run's completed startup census.

## `mod fresh` › `pub struct FreshCensused {`

A fresh run's startup census completed, under the worktree lock.

**The census returns the witness; it does not wrap one.**
[`Self::establish`] is the only constructor and it runs both halves
itself, so there is no signature anywhere that accepts a
[`StartupCensus`] a caller obtained some other way — a hand-built one
whose run-directory half never ran included. Not constructible from a
container `CensusComplete` either, which proves half (a) alone.

## `impl FreshCensused` › `pub(in crate::engine::topology::startup) fn establish(`

The only constructor: both halves, under the lock it consumes.

`pub(in …startup)` rather than `pub`, because
[`super::super::startup_census`] is the entry point and a second
public one would be a second answer to "what did `upstroke run`
census".

### Errors

As [`super::super::startup_census`].

## `impl FreshCensused` › `pub const fn census(&self) -> &StartupCensus {`

Both halves' results.

## `impl FreshCensused` › `pub const fn locked(&self) -> &WorktreeLocked {`

The worktree lock, still held.

## `impl FreshCensused` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `impl FreshCensused` › `pub fn into_parts(self) -> (WorktreeLocked, StartupCensus) {`

Give the lock and the report back to a caller that owns the
witness.
