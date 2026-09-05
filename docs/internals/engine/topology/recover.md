# `src/engine/topology/recover.rs`

Repository source for these notes: [`src/engine/topology/recover.rs`](../../../../src/engine/topology/recover.rs).
[Source on GitHub](https://github.com/eventloops/upstroke/blob/master/src/engine/topology/recover.rs).
The relative link works in a checkout or on GitHub; the GitHub link also works from the published site.

The code is the authority for what it does. The explanatory prose is preserved below.
Each backticked part of a section heading is an exact source excerpt. Search for the final
excerpt within the preceding item when a heading names both an item and a line inside it.

The lock ownership and release protocols remain beside `LocksHeld`, `into_guards`,
and `RunHandle` in the source, as required by standards sections 10 and 13.

## Module

The fresh-process recovery order, (a0) through (i), as a chain of witnesses.

`decisions.sequential_substrate.recovery_order` is "one checked fresh-process
order matching current practice (recovery events precede `run_resumed`)", and
O18 states the four orderings a resume has to get right:

> on resume the private root is derived read-only **before any lock**, the
> owner and commit records are verified **before any private write**, the
> stable-prefix barrier holds **before** the census's fold-derived reclaim,
> before any promotion, cleanup, admission, or report, and **before any
> recovery event** (a failed sync, unstable reread, or replay refusal ends
> the command with nothing done), and the recorded Runner is rebuilt
> (inspection) and its `RunnerPreflight` executed (spawns) **after the
> census and before any recovery event**.

### Why the order is a type and not a function body

Of 29 classified findings across PR3–PR6, `wrong_internal_assumption` is
48.3% — three times `wrong_external_fact`. Orderings are where this
project's defects live, and a recovery order written as eight statements in
one `fn` is an ordering a later edit can reorder silently and no gate will
notice. So each step produces a **witness**, and the next step's constructor
**consumes** it by value:

```text
RootDerived        (a0)  read-only, before any lock
LocksHeld          (a)   worktree lock then run lock; a reaper hold refuses
RecordsVerified    (a)   owner.json and committed.json, before any private write
BarrierHeld        (a1)  the stable-prefix barrier
ResumeCensused     (a)   the census, after the barrier
RunnerRebuilt      (c)   inspection refusals, before any spawn
PreflightCertified (c)   the shell probe then the agent probes
```

Each lives **alone in its own module** with private fields, derives no
`Clone`, `Copy` or `Default`, and has exactly one constructor. Rust privacy
is scoped to the defining module and its descendants, so a sibling witness
cannot build another out of its parts, and neither can [`chain`] itself.
"From X only" written in a comment is not a type; this is.

Every emitter of a recovery event — steps (d) through (g) — takes
`&PreflightCertified`, and [`run_resumed`] at (h) **consumes** it by value,
so nothing after the resume can present one.

### What this slice does *not* do

**Step (b) is "terminal finalization then refuse continuation", and PR7
implements the refusal only.** `RunDir.WriteReport` carries `fault_row:
t_finalize`, which is not one of this slice's eleven rows; a lane that
finalized here would write an out-of-row effect with no fault coverage.
[`refuse_if_finished`] is the refusal, and it is the whole of PR7's (b).

Step (f) is `checkpoint_refusals` territory for the same reason: "an
intermediate build refuses, before any append, any operation whose terminals
it does not implement (PR7: integration and run end beyond refusal)". A
prefix that leaves a promotion or an integration transaction unresolved is
refused rather than completed.

### Where the P7/P8 integration-ref repair sits, and why

`transaction_fault_matrix[T-RUNSTART].resume_action` gives the resume one
step this order would otherwise not have:

> **P7/P8: create the ref zero-old at the recorded base if absent; if
> present == base continue (no spend repeats)**

[`ensure_recorded_integration_ref`] is that step. Its body is
[`super::create::ensure_integration_ref`] — **P8's own body, called, not
copied**: two implementations of "if present == base continue" would be two
places for a run killed between P6 and P8 to be treated differently from one
that was not, which is the duplication that function exists to prevent.
What this module adds is the two arguments, and it takes them from the
record `RootDerived` resolved and `RecordsVerified` authenticated —
`run_started(4).integration_ref` and `run_started(4).base_sha` — never from
today's configuration.

Its position is between step (f)'s [`refuse_unimplemented_terminals`] and
step (d)'s first append, and every bound on it is a separate clause:

* **After (a1).** It is a durable effect on a repository ref. O18 puts the
  stable-prefix barrier before the census's fold-derived reclaim, before any
  promotion, cleanup, admission or report, and before any recovery event —
  that is, before every durable thing a resume derives from the record. A
  ref creation is such a thing, so it is not exempt.
* **After (b).** [`refuse_if_finished`] refuses a Complete or Halted run,
  and publishing a finished run's integration ref is continuing it.
* **After (c).** The repository is touched only once the recorded Runner has
  been rebuilt by inspection and its probes have answered, so a resume that
  cannot run at all leaves the object store exactly as it found it.
* **After (f).** This is the bound that is not merely tidy.
  [`refuse_unimplemented_terminals`] refuses a proven prefix that leaves an
  integration transaction unresolved, and an unresolved integration
  transaction is precisely the state in which the integration ref may be
  mid-move. The ref of such a run can still be *at* the recorded base — the
  CAS has not run yet — and "present == base continue" would then silently
  adopt a ref under a transaction this build cannot resolve. That case is
  the one the step's own refusals do not catch, so the checkpoint refusal
  runs first.
* **Before (d).** The step can refuse: a ref at another SHA, a symbolic ref,
  a ref checked out in a worktree. A refusal after `attempt_interrupted`,
  `generation_closed` and `run_resumed` is a resume half-performed — the
  epoch incremented and the generations closed for a command that then
  failed — and the next resume would append the same set again before
  refusing again. [`refuse_unimplemented_terminals`] gives the identical
  reason for its own position: a refusal after two appends is not "before
  any append".

O15 is "run_started before integration ref", and on a resume it is satisfied
by construction rather than by placement: `run_started(4)` is the committed
first line (a0) read before this order began. Nothing here can put the ref
first.

It is **not** a recovery event. It appends nothing, so
[`refuse_unimplemented_terminals`] does not gate it as an operation whose
terminal is missing, and it needs no terminal of its own — the effect either
happened or did not, and the next resume decides which by looking at the ref.

### Nothing here is a production path

`MAX_READABLE_SCHEMA` is 3 and `TOPOLOGY_ACTIVATION` is `Inactive`, so
[`RootDerived::derive`] refuses every schema-4 log in a released binary.
The reader ceiling is the seam a test raises — see [`RootDerived::derive`]
and [`RootDerived::derive_with`] beside it.

(That sentence deliberately does not spell the test-configuration attribute
out. Two source censuses in this tree cut a file at its first occurrence of
that attribute to find the production half, and a *prose* occurrence in a
module comment cuts the whole file out of the scan — silently, and in the
direction that makes the census pass.)

## `pub mod chain {`

---------------------------------------------------------------------------
The chain
---------------------------------------------------------------------------

## `pub mod chain {`

The seven witnesses, each alone in its own module.

A nested module per witness, and not one module holding seven types: an item
private to a module is visible to that module **and its descendants**, so
seven types in one module could each build the others out of their parts and
the chain would be a naming convention again. Siblings see only what is
`pub`, which here is the constructor and the accessors.

## `pub mod chain` › `pub mod root {`

-- (a0) ---------------------------------------------------------------

## `pub mod chain` › `pub mod root {`

Recovery step (a0): everything a resume decides **before any lock**.

## `pub mod root` › `pub struct RootDerived {`

The run this resume is about, and the private root it is authorized
to touch — derived read-only, with no lock held.

`recovery_order` (a0): "resolve the run id among Committed
directories (readers by commitment), probe the header of the
committed first line, select the engine by schema, derive the
authorized private root R from `run_started.private_dir` (refusing a
locator of any other shape than `<root>/runs/<run_id>`), compare an
explicit `--private-root` (refusing a mismatch naming both roots) —
**every refusal here precedes `Lock.AcquireWorktree`, so no R17 hold
is taken and no R25 lock file is created**".

The R25 clause is why this is a separate step rather than the first
paragraph of the one that locks: `Lock.AcquireWorktree`'s funnel
opens the lock file with `create(true)`, so *reaching* the
acquisition creates the repository-scoped file even when the hold
then fails. A refusal that has not reached it leaves no file at all.

## `pub struct RootDerived` › `reader: ReaderSelection,`

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `pub struct RootDerived` › `first_line: Vec<u8>,`

The committed first line, **without** its newline — the bytes
`committed.json.run_started_sha256` names.

## `impl RootDerived` › `pub fn derive(`

Step (a0) against this binary's reader ceiling.

Production's ceiling is 3 (`TOPOLOGY_ACTIVATION` is `Inactive`),
so a released binary refuses every schema-4 log here and no
production path reaches the rest of this file. That is
`pr_sequence[8].production_effect: none` expressed as a
refusal rather than as a promise.

### Errors

[`UpstrokeError::Refused`] for an unresolvable run id, a log
whose header does not select the topology reader, a first line
that is not a `run_started`, a recorded locator of any shape
other than `<root>/runs/<run_id>`, or an explicit
`--private-root` naming a different root.
[`UpstrokeError::Io`] when the log cannot be read.

## `impl RootDerived` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `impl RootDerived` › `pub(crate) fn derive_with(`

[`Self::derive`] against an explicit reader ceiling.

**This is the test seam, and it is the tree's own shape**:
`schema::select_reader_with(bytes, ceiling)` exists for exactly
this reason — "every decision that depends on the ceiling
already reads it through `select_reader_with`, so nothing has to
be rewritten when it moves". `pub(crate)` rather than public
because raising the ceiling is not something a caller outside
this crate may do.

### Errors

As [`Self::derive`].

## `impl RootDerived` › `let run_id = rundir::resolve_run_id(repo_root, wanted_run_id)?;`

(1) The run id, among Committed directories. `resolve_run_id`
reads `list_runs`, which is the by-commitment view: a husk is
refused here with the sentence that names it, which is
`refusal_condition`'s "resume of a husk id".

## `impl RootDerived` › `let header = probe_header(&bytes).map_err(|refusal| UpstrokeError::Refused {`

(2) The header, and the engine selection by schema.

## `impl RootDerived` › `let end = bytes.iter().position(|byte| *byte == b'\n').unwrap_or(0);`

(3) The first line as the record, for its `private_dir`, its
`incarnation` and its `runner`. Read-only, and re-proven at
(a1): the barrier compares the reread first line's digest
against `committed.json`, so nothing here is trusted past the
barrier.

## `impl RootDerived` › `let private_dir = PathBuf::from(&started.private_dir);`

(4) The authorized private root, from the record's locator.

## `impl RootDerived` › `if let Some(explicit) = explicit_private_root {`

(5) An explicit `--private-root` that names a different root
refuses, **naming both**.

## `impl RootDerived` › `pub fn run_id(&self) -> &str {`

The resolved run id.

## `impl RootDerived` › `pub fn public_dir(&self) -> &Path {`

`<repo>/.upstroke/runs/<run_id>`.

## `impl RootDerived` › `pub fn private_root(&self) -> &Path {`

The authorized private root R — never today's default.

## `impl RootDerived` › `pub fn private_dir(&self) -> &Path {`

`<R>/runs/<run_id>`, as the record wrote it.

## `impl RootDerived` › `pub fn reader(&self) -> ReaderSelection {`

The reader the header selected. Always
[`ReaderSelection::Topology`] for a value that exists.

## `impl RootDerived` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `impl RootDerived` › `pub fn first_line(&self) -> &[u8] {`

The committed first line's bytes, without the commit marker.

## `impl RootDerived` › `pub fn started(&self) -> &RunStarted4 {`

The run record the first line carries.

## `impl RootDerived` › `pub fn log_path(&self) -> PathBuf {`

The log this run appends to.

## `pub mod root` › `fn authorized_root(private_dir: &Path, run_id: &str) -> Result<PathBuf, UpstrokeError> {`

The root R such that the recorded locator is exactly
`<R>/runs/<run_id>`.

Refuses **any other shape**, which is stricter than "ends with the
run id": a locator of `<R>/runs/<other>/../<run_id>` ends correctly
and names a directory the run does not own, and a locator of
`<R>/<run_id>` has no `runs` component at all. Both are the
`malformed_recorded_locator_refused_before_any_lock` case.

## `fn authorized_root(private_dir: &Path, run_id: &str) -> Result<PathBuf, UpstrokeError> {` › `if private_dir`

No `..`, no `.`, no prefix trickery: every component is checked,
because the whole value of this refusal is that the two trailing
components are the only thing below the root.

## `pub mod root` › `fn normalize(path: &Path) -> PathBuf {`

A comparable form of a root that need not exist yet.

`fs::canonicalize` is the right answer for a path that is on disk
and no answer at all for one that is not — and an explicit
`--private-root` naming a directory that does not exist is exactly
the mismatch this comparison has to report rather than fail on. So
the canonical form is taken when it is available and the lexical one
otherwise, and the *mismatch message prints what the operator wrote*.

## `pub mod chain` › `pub mod locks {`

-- (a) locks ----------------------------------------------------------

## `pub mod chain` › `pub mod locks {`

Recovery step (a), first half: the two locks, in order.

## `pub mod locks` › `pub struct LocksHeld {`

The worktree lease and this run's run lock, both held.

`recovery_order` (a): "take the worktree lock **then** the run lock
(refused while a surviving reaper hold R28 is observed, per existing
rules)". The order is the constructor's two statements and the
refusal is the funnels' own: `WorktreeLock` scans every committed
run directory through `Lock.ObserveCleanupHold` and refuses while
one is held, and `RunLock` takes the momentary exclusive
`Lock.ProbeCleanupExclusive` on this run.

Both holds are R17 — "released at process exit (OS-released on
death)" — so this value owning them is what makes the release
happen at the end of the command whether or not anything asks.

## `pub struct LocksHeld` › `_run: RunLock,`

Dropped in declaration order, which releases the run lock before
the worktree lease — the reverse of acquisition.

## `impl LocksHeld` › `pub fn take(`

Take the worktree lease, then this run's run lock.

### Errors

[`UpstrokeError::Refused`] when either lock is held by another
process, or while a surviving reaper's shared cleanup hold (R28)
is observed. The value is consumed either way, because a
refusal here ends the command.

## `impl LocksHeld` › `pub fn root(&self) -> &RootDerived {`

What (a0) derived.

## `impl LocksHeld` › `pub fn into_guards(self) -> (RunLock, WorktreeLock, RootDerived) {`

Consume the witness and hand out the two lock guards, still
held.

The fields are `_run` and `_worktree` because nothing reads
them — they exist to be dropped, in declaration order, so the
run lock is released before the worktree lease. **Handing them
out keeps that property and moves it**: the guards outlive this
call, drop in the same order at the end of the loop, and are
still unreadable. What changes is *when* they die, and the whole
reason a loop can exist is that it is no longer at the end of
the recovery order.

## `pub mod chain` › `pub mod records {`

-- (a) records --------------------------------------------------------

## `pub mod chain` › `pub mod records {`

Recovery step (a), second half: the two private records, verified
**before any private write**.

## `pub mod records` › `pub struct RecordsVerified {`

`owner.json` and `committed.json`, both read and both agreeing.

`recovery_order` (a): "**before any private write** verify
`<R>/runs/<run_id>/owner.json` (run_id, repo_key, canonical
public_dir, incarnation == `run_started.incarnation`, runner ==
`run_started(4).runner`) and `committed.json` (`run_started_sha256`
equals the digest of the committed first line), refusing on a
missing private half, a missing record, or any disagreement (a
private half that is not provably this run's is never written into;
**a missing schema-4 private half is not recreated** — deferred)".

Five owner fields and one commit field, each with its own refusal,
because `refusal_condition` enumerates them — "run id, repo key,
canonical public path, incarnation, runner, `run_started` digest" —
and a single "the records disagree" would be green for a fixture
that damaged the wrong one.

## `pub struct RecordsVerified` › `owner: OwnerRecord,`

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `impl RecordsVerified` › `pub fn verify(locks: LocksHeld, repo_key: &RepoKey) -> Result<Self, UpstrokeError> {`

Verify both records against the run record (a0) read.

### Errors

[`UpstrokeError::Refused`] for an absent private half, an absent
or unparseable record, or a disagreement in any of the six
fields.

## `pub fn verify(locks: LocksHeld, repo_key: &RepoKey) -> Result<Self, UpstrokeError> {` › `if let Some(field) = started.runner.difference(&owner.runner) {`

INV-23: "every later incarnation rebuilds the Runner from
`run_started(4).runner` — **verified equal to
owner.json.runner** — before its RunnerPreflight". The
comparison is `difference`, which names WHICH field moved:
"the runner disagrees" is not something an operator can act
on, and the field is.

## `impl RecordsVerified` › `pub fn locks(&self) -> &LocksHeld {`

The locks this verification ran under.

## `impl RecordsVerified` › `pub fn into_locks(self) -> LocksHeld {`

Consume this witness and hand on the one it was built from.

**Mints nothing.** §2's rule is that a witness is constructible
only by its own constructor from its own predecessor, and this
goes the other way: it takes a witness apart, it does not put
one together. Walking backwards by reference was already
possible through the accessor above; what this adds is
*ownership*, which the run loop needs and a reference cannot
give — at the bottom of the chain the parts are the append
handle and the two locks, and a borrowed lock is a lock this
process is about to drop.

## `impl RecordsVerified` › `pub fn owner(&self) -> &OwnerRecord {`

The verified owner record.

## `impl RecordsVerified` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `impl RecordsVerified` › `pub fn commit(&self) -> &CommitRecord {`

The verified commit record. Its `run_started_sha256` is what the
stable-prefix barrier proves the reread first line against.

## `pub mod records` › `fn read_record<T: serde::de::DeserializeOwned>(`

Read one JSON record, or refuse naming the file.

## `pub mod records` › `fn canonical_display(path: &Path) -> String {`

A path in the form a record writes it: canonical when the filesystem
will say so, and lexical otherwise.

## `pub mod chain` › `pub mod barrier {`

-- (a1) barrier -------------------------------------------------------

## `pub mod chain` › `pub mod barrier {`

Recovery step (a1): the stable-prefix barrier.

## `pub mod barrier` › `pub struct BarrierHeld {`

The barrier of `coordinator_integration.stable_prefix_barrier`,
established.

### Why the constructor takes `StablePrefix` **by value**

`StablePrefixBarrier::establish` takes `PrefixSync`, `PrefixReread`
and `PrefixReplay`, all of which have public fields, and
`PrefixBytes::of` is public — so `establish(PrefixSync{n},
&PrefixReread{first: b, second: b}, &PrefixReplay{replayed: b})`
returns `Ok` for **any** byte string `b`, and the whole recovery
chain below this point would be reachable from three copies of one
lie. Accepting a `StablePrefixBarrier` from a caller would inherit
that.

[`crate::events::log::StablePrefix`] has private fields, derives no
`Clone`, and has exactly one constructor —
`crate::events::log::establish_stable_prefix`, which performs the
sync, the reread, the four proofs and the checked replay. So a
`StablePrefix` is unforgeable outside `src/events/log.rs`, and this
module **derives** the census's `StablePrefixBarrier` from it rather
than being handed one.

The derivation is trivially satisfiable *because the proof already
happened*: the three measurements all come from the one proven byte
string, so the four predicates hold by construction. That is the
point — the barrier value carries the evidence, and the evidence is
`StablePrefix`'s own.

## `pub struct BarrierHeld` › `log: crate::events::log::EventLog,`

The append handle the barrier entitles this command to.

## `pub struct BarrierHeld` › `bytes: Vec<u8>,`

The exact bytes that were synced, reread, proven, and replayed.

## `pub struct BarrierHeld` › `bytes: Vec<u8>,`

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `pub struct BarrierHeld` › `events: Vec<crate::topology::events::TopologyEvent>,`

The events the barrier parsed from exactly those bytes.

Carried rather than re-derived for the same reason the fold is:
there is one production parse of a log and it is the barrier's.

## `pub struct BarrierHeld` › `fold: TopologyFold,`

The fold built from exactly those bytes, and no others.

## `impl BarrierHeld` › `pub fn from(`

Hold the barrier over `prefix`, under `records`.

Both arguments by value: `records` because §2's rule is that
every witness consumes its predecessor, and `prefix` because
§4's is that the barrier's evidence cannot be borrowed from
something a caller kept a second handle on.

### Errors

[`UpstrokeError::Refused`] from
[`StablePrefixBarrier::establish`]. Unreachable for a
`StablePrefix` — the three measurements are one value measured
three times — and returned rather than unwrapped because this
crate does not panic outside tests, and because an
`establish` that grew a fifth predicate would then refuse here
rather than silently pass.

## `impl BarrierHeld` › `let (log, bytes, events, fold) = prefix.into_log_and_fold();`

Taken apart rather than kept whole: the barrier owns the
append handle from here on, and a `StablePrefix` that both
this value and a caller could reach would be two handles onto
one log.

## `impl BarrierHeld` › `PrefixSync {`

Every byte the barrier proved is a byte
`Event.OpenLog.SyncPrefix` successfully synced.

## `impl BarrierHeld` › `&PrefixReplay { replayed: measured },`

"the replay consumed exactly those reread bytes":
`establish_stable_prefix` replays `reread` and moves the
same value into the result, so there is one byte string
here and not two that happen to agree.

## `impl BarrierHeld` › `pub fn records(&self) -> &RecordsVerified {`

The records verified before the barrier.

## `impl BarrierHeld` › `pub fn into_log_fold_and_records(`

Consume the barrier and hand out the run's own state.

**The append handle and the fold are one pair, and this is the
only way to own them.** The log is the handle the barrier
entitled this command to; the fold is built from *exactly* the
bytes the barrier synced, reread, proved and replayed. A caller
that reopened the log to get a handle would be appending to a
prefix its own barrier never proved, which is the whole of what
(a1) exists to prevent — so the pair leaves together or not at
all.

## `impl BarrierHeld` › `pub fn fold(&self) -> &TopologyFold {`

The fold built from exactly the proven bytes.

## `impl BarrierHeld` › `pub fn events(&self) -> &[crate::topology::events::TopologyEvent] {`

The events the barrier parsed from exactly those bytes.

For a recovery step that needs what the fold does not keep — the
`AttemptRecord` a durable settlement carried. Reading them here
rather than parsing the log again is what keeps the barrier the
one production parse.

## `impl BarrierHeld` › `pub fn bytes(&self) -> &[u8] {`

The proven bytes.

## `impl BarrierHeld` › `#[allow(dead_code)]` (trailing)

never called at 610106b; see `PR7-NARROWED-SURFACE-19-UNCALLED` (§2)

## `impl BarrierHeld` › `pub fn stable_prefix_barrier(&self) -> StablePrefixBarrier {`

The census's evidence value, derived here and never accepted
from a caller.

## `impl BarrierHeld` › `pub(in crate::engine::topology::recover) fn writer(`

The append handle the barrier entitles this command to, and the
fold it was built from.

Split out rather than exposed as two `&mut` accessors because a
recovery emitter needs both at once and Rust's borrow checker
would otherwise force one of them through a clone.

## `pub mod chain` › `pub mod censused {`

-- (a) census ---------------------------------------------------------

## `pub mod chain` › `pub mod censused {`

Recovery step (a), third half: the startup census, **after** the
barrier.

## `pub mod censused` › `pub struct CensusSeams<'a> {`

What the census reads from the world: the four seams and the two
identities it is censusing on behalf of.

A bundle rather than six arguments because the six do not vary
independently — they are one process's view of one repository — and
a six-argument constructor is six places for two `&Path`s of the
same type to be passed in the wrong order.

## `pub struct CensusSeams<'a>` › `pub incarnation: &'a IncarnationId,`

This coordinator process's per-process ULID.

## `pub mod censused` › `pub struct ResumeCensused {`

The census of step (a), and the barrier it was decided under.

**The census returns the witness; it does not get wrapped.** A
wrapper would prove possession — the holder had a census result and
a barrier, in either order — and the packet requires the barrier
*first*: "a resume takes its run lock first, establishes the
stable-prefix barrier of recovery step (a1), **then** censuses". The
constructor consuming [`BarrierHeld`] by value is that ordering, as
a call.

## `impl ResumeCensused` › `pub fn census(`

Census under `barrier`: containers first, then the run
directories — this run's own stale marker among them.

**A resume reclaims.** `recovery_order` (a1) is a "startup
census … run-directory census incl. this run's own stale marker,
which the owner removes here, **and husk reclamation under the
ownership proof**", and INV-15 reclaims pre-run husks "at
write-command start under the worktree lock". A resume is a
write command and holds that lock, so the run-directory half is
[`census_run_dirs`] — the same function `upstroke run` calls, not
a read-only second pass. A pass that classified and reported
would leave every husk beside the resuming run on disk for ever,
with only a fresh `upstroke run` able to reclaim it.

`own_run` is this run's id and licenses exactly one thing: the
stale-marker repair that `resource_accounting` gives to "a census
with the lock free **or** its owner on resume". It cannot reach
the husk arms, which are gated on the lock alone — and this
process holds its own run's lock, so its own directory is refused
there whatever shape its log is in.

**A husk beside this run cannot end this resume.** A reclaim that
the filesystem refuses is that directory's
[`crate::engine::topology::startup::RunDirOutcome::Unreclaimable`]
entry and the census carries on: one dead run's unremovable
residue used to fail `upstroke resume`
for every run in the repository, on every attempt, and — because
the walk is in ascending run-id order — took this run's own
stale-marker repair with it whenever the husk sorted first.

### Errors

[`UpstrokeError::Refused`] from the container census — an
unreachable runtime with intents present, an intent naming this
process's own incarnation, an unreclaimable dead owner — or
[`UpstrokeError::Io`] when `<repo>/.upstroke/runs` exists and
cannot be enumerated, which is the run-directory half reporting
that it did not happen rather than that it found nothing.

## `impl ResumeCensused` › `let inputs = CensusInputs {`

One value for both halves. `CensusInputs` carries a single
`authorized_root`, so the root half (a) scans and the root
half (b) proves ownership under cannot disagree.

## `impl ResumeCensused` › `let start = CensusStart::Resume {`

(i) Containers, including every earlier incarnation of this
run under `<R>/containers`. The start value carries the
barrier this module derived, so `CensusStart::Resume` cannot
be built here without one.

## `impl ResumeCensused` › `let run_dirs = census_run_dirs(hooks.rundir(), &inputs, Some(&run_id))?;`

(ii) Run directories: classified, then reclaimed under the
ownership proof — private half through the proof-token funnel
first, public directory with the marker last — and this run's
own stale marker repaired by its owner. `own_run` is also what
guarantees this run's directory is walked at all, whatever the
enumeration of the runs tree returned.

## `impl ResumeCensused` › `pub fn barrier(&self) -> &BarrierHeld {`

The barrier this census was decided under.

## `impl ResumeCensused` › `pub fn into_barrier(self) -> BarrierHeld {`

Consume this witness and hand on the one it was built from.

**Mints nothing.** §2's rule is that a witness is constructible
only by its own constructor from its own predecessor, and this
goes the other way: it takes a witness apart, it does not put
one together. Walking backwards by reference was already
possible through the accessor above; what this adds is
*ownership*, which the run loop needs and a reference cannot
give — at the bottom of the chain the parts are the append
handle and the two locks, and a borrowed lock is a lock this
process is about to drop.

## `impl ResumeCensused` › `pub(in crate::engine::topology::recover) fn barrier_mut(&mut self) -> &mut BarrierHeld {`

The barrier, mutably — the append handle lives inside it.

## `impl ResumeCensused` › `pub fn containers(&self) -> &CensusReport {`

What the container census reclaimed and left alone.

Returns the **report** rather than the `CensusComplete` token.
The token stays owned here on purpose: it is the value
`crash_reconstruction`'s four "before"s are gated on, and
handing it out would let a caller present census evidence
without presenting the barrier this census ran under. The
report is what a caller actually reads.

(It also keeps this accessor out of
`runner::container::census::tests::census_returns_the_only_token_that_reaches_a_consumer`,
whose needle is the text `CensusComplete` followed by a brace
and therefore matches a return type as well as a struct
literal. That is a property of the needle rather than of this
code, and the right response is not to construct one — which
this does not — but there is no reason to sit on a false
positive when the narrower return type is the better API.)

## `impl ResumeCensused` › `pub const fn run_dirs(&self) -> &RunDirCensusReport {`

What the run-directory half found and did: one entry per
directory under `<repo>/.upstroke/runs`, with its locator, its
class and its outcome.

Total over the runs directory, so "every husk retained and
reported with its locator and reason" and "every husk reclaimed
under the proof" are both read off the one report — and a
directory this census did nothing to is still an entry.

## `pub mod chain` › `pub mod rebuilt {`

-- (c) rebuild --------------------------------------------------------

## `pub mod chain` › `pub mod rebuilt {`

Recovery step (c), first half: the recorded Runner, rebuilt by
**read-only inspection**, before any spawn.

## `pub mod rebuilt` › `pub struct RunnerRebuilt {`

The Runner this incarnation established, equal to
`run_started(4).runner` field for field.

`recovery_order` (c): "rebuild the Runner from
`run_started(4).runner` (today's `[runner]` config that differs
warns naming the difference and is ignored; a reference that now
names another image warns while the recorded id is used; **refusals
by read-only inspection — container runtime unavailable, recorded
image id absent from the runtime, credential volume absent — refuse
before any spawn**)".

The split between this witness and [`super::certified`] **is** the
refusal split: a `RunnerRebuilt` exists only when every inspection
passed, and nothing has been spawned to make one.

## `impl RunnerRebuilt` › `pub fn rebuild(`

Rebuild by inspection alone.

`runtime` is consulted only for a recorded **container** runner.
A host record needs no runtime, and demanding one would refuse a
host run on a machine with no container runtime — which is every
machine the host runner exists for.

**The record wins, exactly.** The returned policy is the
recorded one field for field; today's config reaches only the
warnings.

### Errors

[`UpstrokeError::Refused`] for an unavailable runtime, a
recorded image id the runtime no longer holds, an absent
credential volume, or a recorded container runner with no
runtime seam to ask.

## `impl RunnerRebuilt` › `RunnerKind::Host => {`

A host runner has nothing to inspect: `host-v1` names no
image and no volume, and `resolve_host` is total. What
remains is the config-drift warning, which applies to
both kinds.

## `impl RunnerRebuilt` › `pub fn censused(&self) -> &ResumeCensused {`

The census this rebuild followed.

## `impl RunnerRebuilt` › `pub fn into_censused(self) -> ResumeCensused {`

Consume this witness and hand on the one it was built from.

**Mints nothing.** §2's rule is that a witness is constructible
only by its own constructor from its own predecessor, and this
goes the other way: it takes a witness apart, it does not put
one together. Walking backwards by reference was already
possible through the accessor above; what this adds is
*ownership*, which the run loop needs and a reference cannot
give — at the bottom of the chain the parts are the append
handle and the two locks, and a borrowed lock is a lock this
process is about to drop.

## `impl RunnerRebuilt` › `pub(in crate::engine::topology::recover) fn censused_mut(`

The census, mutably.

## `impl RunnerRebuilt` › `pub fn policy(&self) -> &RunnerPolicy {`

The rebuilt policy — the record, field for field.

## `impl RunnerRebuilt` › `pub fn warnings(&self) -> &[String] {`

What the operator is told about a config that moved under the
run, or a reference that now names another image.

## `pub mod chain` › `pub mod certified {`

-- (c) preflight ------------------------------------------------------

## `pub mod chain` › `pub mod certified {`

Recovery step (c), second half: the `RunnerPreflight` probes.

## `pub mod certified` › `pub struct PreflightCertified {`

The shell and every recorded agent CLI, certified **inside the
recorded image**, before any recovery event.

This is the witness every recovery emitter takes by reference and
that `run_resumed` consumes by value. Holding one means: the locks
are held, the records agree, the barrier proved the prefix, the
census ran under it, the runner was re-established by inspection,
and a shell and every recorded CLI answered inside the boundary.
There is no other way to have one.

## `impl PreflightCertified` › `pub fn certify(`

Run the pre-flight through the rebuilt runner.

### Errors

[`UpstrokeError::Refused`] naming the shell or the agent whose
CLI did not answer. `expected_failures_refusals[2]`: the refusal
lands "before any recovery event or work spawn", with the probe
invocations reclaimed like every probe.

## `impl PreflightCertified` › `let agents = rebuilt`

`run_resumed(4).probed_agents` is "what this incarnation's
pre-flight probes found", and what it probed is what the run
recorded: the pre-flight is constructed from
`run_started(4).probed_agents` and certifies all of them or
refuses. Taking the list from the record rather than from the
seam is what keeps a `RunnerPreflight` double from being able
to widen the run's agent allow-list.

## `impl PreflightCertified` › `pub fn rebuilt(&self) -> &RunnerRebuilt {`

The rebuilt runner these probes executed through.

## `impl PreflightCertified` › `pub fn into_rebuilt(self) -> RunnerRebuilt {`

Consume this witness and hand on the one it was built from.

**Mints nothing.** §2's rule is that a witness is constructible
only by its own constructor from its own predecessor, and this
goes the other way: it takes a witness apart, it does not put
one together. Walking backwards by reference was already
possible through the accessor above; what this adds is
*ownership*, which the run loop needs and a reference cannot
give — at the bottom of the chain the parts are the append
handle and the two locks, and a borrowed lock is a lock this
process is about to drop.

## `impl PreflightCertified` › `pub(in crate::engine::topology::recover) fn rebuilt_mut(`

The rebuilt runner, mutably — the append handle is under it.

## `impl PreflightCertified` › `pub fn probed_agents(&self) -> &[String] {`

The agents this pre-flight certified.

## `pub struct ResumeSeams<'a> {`

---------------------------------------------------------------------------
The order, driven
---------------------------------------------------------------------------

## `pub struct ResumeSeams<'a> {`

Everything the recovery order reads from the world after (a0).

(a0) is deliberately **not** in here: it takes a repository, a wanted run id
and an optional `--private-root`, and nothing else, because everything else
is derived from the record it has not read yet.

## `pub struct ResumeSeams<'a>` › `pub worktree_git_dir: &'a Path,`

The git dir of the worktree this run drives — where `upstroke-worktree.lock`
(R25) lives. Passed in rather than derived here: deriving it opens a
`Workspace`, which runs `git`, and the recovery order's own refusals
must not depend on a subprocess.

## `pub struct ResumeSeams<'a>` › `pub incarnation: &'a IncarnationId,`

This coordinator process's per-process ULID.

## `pub struct ResumeSeams<'a>` › `pub inputs: FrozenInputs,`

The frozen plan and its digest, which the checked replay authenticates
the recorded registry against.

## `pub struct ResumeSeams<'a>` › `pub today: &'a RunnerSelection,`

Today's `[runner]` selection. Warned about when it differs; never used.

## `pub struct ResumeSeams<'a>` › `pub refs: &'a dyn IntegrationRefs,`

P8's ref funnel, for the P7/P8 repair — the same seam the creator is
given, and [`crate::workspace_manager::WorkspaceManager`] is the
production implementation of it.

A seam and not a `WorkspaceManager` for the reason [`IntegrationRefs`]
itself is one: this file is a `TOPOLOGY_MODULE`, in which
`std::process::Command` is a build error, so a resume test that had to
stand up a real repository to reach the ref could not be written here at
all.

## `pub struct ResumeSeams<'a>` › `pub manager: &'a WorkspaceManager,`

The workspace manager step (g) rebuilds worktrees through. Every Git
effect of the recovery order that is not an append goes through it.

## `pub enum RecoveryStep {`

One step of `decisions.sequential_substrate.recovery_order`, as the packet
names it.

**This type is the reason the order can be checked for completeness.** The
packet names eleven steps in one sentence, and a step that no code performs
is invisible to every technique this project runs: a mutation catalogue
measures whether existing code is pinned, and **omission has nothing to
mutate**. Step (g) was absent from this module for the whole of PR7's
implementation and two review rounds, with 117 named tests passing and every
gate green, because no test read the packet's list.

So the list is a type. Adding a step to the packet means adding a variant,
and a variant with no arm does not compile. Removing a step's call leaves a
hole in [`Recovered::steps`] that
`the_recovery_order_performs_every_step_the_packet_names` fails on.

## `pub enum RecoveryStep` › `A0,`

Read-only root derivation, before any lock.

## `pub enum RecoveryStep` › `A,`

The two locks, the two records, the census and the residue reclaim.

## `pub enum RecoveryStep` › `A1,`

The stable-prefix barrier.

## `pub enum RecoveryStep` › `B,`

Complete or Halted: terminal finalization then refuse continuation.

## `pub enum RecoveryStep` › `C,`

Rebuild the recorded Runner, then its pre-flight probes.

## `pub enum RecoveryStep` › `D,`

Settle every in-flight identity interrupted.

## `pub enum RecoveryStep` › `E,`

Close every `RetainedIdle` generation.

## `pub enum RecoveryStep` › `F,`

Complete `Promoting` promotions and authorized publications.

## `pub enum RecoveryStep` › `G,`

Recreate `OpenNoAttempt` worktrees at their bases.

## `pub enum RecoveryStep` › `H,`

Append `run_resumed(4)`.

## `pub enum RecoveryStep` › `I,`

Admission.

## `pub enum Performer {`

Which part of the system performs a step.

## `pub enum Performer` › `ThisOrder,`

[`run_recovery_order`] itself.

## `pub enum Performer` › `CallerBefore,`

The caller, before the order is entered — `(a0)` is read-only and
precedes every lock, so it cannot be inside a function that has already
taken one.

## `pub enum Performer` › `LoopAfter,`

The loop, after the order returns.

## `impl RecoveryStep` › `pub const ALL: [Self; 11] = [`

The eleven steps, in the packet's order.

Transcribed from `decisions.sequential_substrate.recovery_order`. The
order of this array **is** the claim; a test compares the trace against
it rather than against a second list written from memory.

## `impl RecoveryStep` › `pub const fn label(self) -> &'static str {`

The packet's own label for this step.

## `impl RecoveryStep` › `pub const fn position_override(self) -> Option<&'static str> {`

The live clause that moves this step out of the packet's sequence
position, where one does.

**A deviation with a reason is not the same thing as a step in the wrong
place, and the difference has to be stated somewhere a test can read.**
Left implicit, an argued reordering and an accidental one look
identical — which is how a slice ends up with a comment claiming "steps
(a) through (h), in the packet's order" over a body that performs nine
of ten in a different one.

## `pub const fn position_override(self) -> Option<&'static str>` › `Self::F => Some("decisions.sequential_substrate.checkpoint_refusals"),`

`checkpoint_refusals`: "an intermediate build refuses, **before
any append**, any operation whose terminals it does not
implement". PR7's (f) is a refusal — it does not complete a
promotion, it declines to — and a refusal taken after (d) and
(e) is a refusal after two appends, which that sentence forbids.
So (f) runs before them, and the authorized publication it does
perform (the recorded integration ref) rides with it for the same
reason: the ref is created before the first append of the resume,
which `kill_after_run_started_creates_integration_ref` asserts by
reading the log at the funnel's entry.

## `impl RecoveryStep` › `pub const fn performer(self) -> Performer {`

Who performs it, and — for the two this order does not — why.

The two exceptions are stated here rather than left implicit, because
"this module does not do that one" is exactly the sentence that hid step
(g): a reader who accepts it without a reason cannot tell a delegated
step from a missing one.

## `pub const fn performer(self) -> Performer` › `Self::A0 => Performer::CallerBefore,`

Read-only and before `Lock.AcquireWorktree`, so no R17 hold is
taken and no R25 lock file is created by a refusal here. A
function that has already taken the locks cannot perform it.

## `pub const fn performer(self) -> Performer` › `Self::I => Performer::LoopAfter,`

`checkpoint_refusals` gives the loop's refusals to `select.rs`,
and admission is the loop's first act, not recovery's last.

## `pub struct Recovered {`

What one completed recovery did.

## `pub struct Recovered` › `pub interrupted: usize,`

(d): how many in-flight identities were settled interrupted.

## `pub struct Recovered` › `pub retained_closed: usize,`

(e): how many `RetainedIdle` generations were closed.

## `pub struct Recovered` › `pub finished: Vec<TaskKey>,`

The promotions this resume carried through to `task_candidate_created`
— `T-CAND-REF`'s continuation.

**There was a `promoted` field beside this one**, holding the
generations whose `candidate_prepared` a resume *synthesised* from the
prepared pin — erratum E6's convergence. It is gone with the path that
filled it: since `candidate_prepared` became the sole successful
settlement on 2026-08-27, a `Promoting` generation always has a recorded
candidate, so the window E6 converged cannot occur and a pin without a
candidate record is orphan residue instead.

## `pub struct Recovered` › `pub recreated: Vec<(TaskKey, GenerationId, Reuse)>,`

(g): every `OpenNoAttempt` generation rebuilt, and whether its worktree
verified or had to be recreated. A value rather than a count, because
"the step ran" and "the step ran and found nothing to do" are the two
states a test of an ordered sequence has to tell apart.

## `pub struct Recovered` › `pub steps: Vec<RecoveryStep>,`

Every step this order performed, in the order it performed them.

Pushed as each step returns `Ok`, so a step whose call is deleted
disappears from the trace. That is what makes
[`RecoveryStep`]'s list checkable against something other than itself.

## `pub struct Recovered` › `pub resumed: Resumed,`

(h): what `run_resumed` established.

## `pub struct Recovered` › `pub warnings: Vec<String>,`

(c): the config drift and moved-reference warnings, in order.

## `pub fn run_recovery_order(`

Steps (a) through (h), in the packet's order, each step consuming the last
step's witness.

The ordering claim of this function is not in its statements — it is in the
types. `RecordsVerified::verify` cannot be called without a `LocksHeld`;
`ResumeCensused::census` cannot be called without a `BarrierHeld`; and
[`run_resumed`] eats the `PreflightCertified` that every recovery emitter
needs. Reordering the body does not compile.

Step (i), admission, is the loop's and is not here: `checkpoint_refusals`
gives the loop's refusals to `select.rs`, and this file owns step (b)'s.

### Errors

The first refusal of the order: a lock held elsewhere or a surviving reaper
hold (a); a missing or disagreeing record (a); a failed sync, unstable
reread or refused replay (a1); a census refusal (a); a Complete or Halted
run (b); an inspection refusal (c, before any spawn); a probe refusal (c,
before any recovery event); a recorded integration ref that is symbolic,
checked out, or at a SHA other than the recorded base (the P7/P8 repair,
also before any recovery event); or an append error at (d)–(h), after which
the fold is poisoned and the next resume repeats from (a0).

## `let locks = LocksHeld::take(`

(a) the two locks, then the two records — before any private write.

## `let log_path = records.locks().root().log_path();`

(a1) the barrier, before every fold-derived effect of every later step.

## `let censused = ResumeCensused::census(`

(a) the census, under the barrier and never before it.

## `refuse_if_finished(&censused)?;`

(b) Complete or Halted: finalize then refuse. PR7 refuses.

## `let rebuilt = RunnerRebuilt::rebuild(censused, seams.today, Some(seams.runtime))?;`

(c) the recorded Runner by inspection, then its probes.

## `refuse_unimplemented_terminals(&certified)?;`

(f) the terminals this build does not implement, refused before any
append — which is why it precedes (d) and (e) rather than sitting in its
own numbered position: a refusal after two appends is not "before any
append".

## `{`

**`T-CAND-OBJ`'s other refusal**, and it belongs here for the same reason:
`refusal_condition` is "pin symbolic or an unexpected ref under the run
namespace", and a refusal after an append is not a refusal before one.

`refuse_unexpected_refs` and `expected_refs` were both written, correct,
and called only from their own tests — `expected_refs` derives the
entitlement from the fold precisely so a ref with no durable record behind
it is what fails, and nothing derived it. Round 3 reached this twice
(`consumer`, Sol).

**The citation matters and the obvious one is wrong.**
`expected_failures_refusals[2]` naming "unexpected refs under the run
namespace" is **`pr_sequence[6]`'s** contract, not this slice's — PR7's
`[2]` is "empty-diff and unresolved-index attempt failures". What binds
here is `transaction_fault_matrix[4].refusal_condition`, and PR7 owns that
row.

## `ensure_recorded_integration_ref(&certified, seams.refs, hooks)?;`

T-RUNSTART's P7/P8 repair, after (f) and before the first append. The
module comment argues each bound; the one that is not merely tidy is (f),
because a prefix with an unresolved integration transaction can have its
ref still sitting at the recorded base, and "present == base continue"
would adopt it under a transaction this build cannot resolve.

## `let mut reservations = Reservations::new();`

The append-error protocol's two ledgers. The recovery order takes no
provisional reservation and registers no invocation of its own — (c)'s
probes are the Runner's and are reclaimed there — so on this path both are
empty and the protocol cancels nothing. They exist here rather than inside
`emit` because "nothing was held" has to be an observation the ledgers
make, not an assumption the emitter is written around.

## `let interrupted = settle_interrupted(&mut certified, &mut context)?;`

(d), (e) — recovery events, every one of them before (h).

## `let finished = finish_promotions(&mut certified, seams.manager, &mut context)?;`

**(f)'s converging half is gone, because the window it converged cannot
occur.** It walked generations that were `Promoting` with no recorded
candidate — erratum E6's window — and rebuilt a `candidate_prepared` from
whatever the prepared pin pointed at, deriving tree, message and paths
from that commit.

That window existed only because `attempt_finished{Succeeded}` promoted a
generation on its own. Since the 2026-08-27 CONFORM ruling
`candidate_prepared` is the sole successful settlement, `Promoting` is
set at exactly one place in the fold — the same block that records the
candidate — so `Promoting` now implies a recorded candidate and the walk
could only ever return nothing.

**It was also the third P1 of the `bf927f3` review**: a pin substituted
between the settlement and the append made recovery reconstruct a
successful candidate around an object no gate ran against, and the tree
check could not catch it because recovery itself recorded that tree. With
one atomic settlement the same prefix is a pin with no candidate record —
orphan residue, which `candidate::recovery_for` prunes while settling the
attempt interrupted.

`T-CAND-REF`'s four-step sequence still runs below; what is removed is
the synthetic entry to it.

## `steps.push(RecoveryStep::F);`

(g) — after (e) and before (h), the packet's own position. A worktree
effect and not an append, so it takes no `EmitContext`; the borrow of
`hooks` that `context` holds ends here for the same reason the step must
run before (h): `run_resumed` consumes the witness this step reads.

## `let (resumed, handle) = run_resumed(certified, &mut context, seams.incarnation)?;`

(h) — and the witness is consumed here.

## `pub fn refuse_if_finished(censused: &ResumeCensused) -> Result<(), UpstrokeError> {`

---------------------------------------------------------------------------
Step (b) — the refusal, and only the refusal
---------------------------------------------------------------------------

## `pub fn refuse_if_finished(censused: &ResumeCensused) -> Result<(), UpstrokeError> {`

Step (b): "if the fold outcome is Complete or Halted: terminal finalization
then refuse continuation" — **PR7 implements the refusal**.

`RunDir.WriteReport` carries `fault_row: t_finalize`, which is not one of
this slice's eleven rows, so a lane that finalized here would write an
out-of-row effect with no fault coverage in this slice. The finalization is
therefore deferred and this is the half that is in range:
`refusal_condition`'s "continuation of Complete or Halted after
finalization".

Read from the barrier-proven fold and nowhere else — that is what O18's
"before any promotion, cleanup, admission, or report" buys, and a (b) that
consulted a fold built anywhere else would be deciding a run's outcome from
bytes nobody proved.

### Errors

[`UpstrokeError::Refused`] when the proven prefix ends in `run_finished`
with [`RunOutcome::Complete`] or [`RunOutcome::Halted`].

## `pub fn refuse_if_finished(censused: &ResumeCensused) -> Result<(), UpstrokeError> {` › `RunOutcome::Parked | RunOutcome::BudgetExceeded => Ok(()),`

Parked and BudgetExceeded are resumable outcomes: the fold's own
guard lets `run_resumed` through for exactly these two, which is what
makes "raise the ceiling and resume" the response to a budget stop.

## `pub fn refuse_unimplemented_terminals(certified: &PreflightCertified) -> Result<(), UpstrokeError> {`

Step (f)'s checkpoint refusal.

`checkpoint_refusals`: "an intermediate build refuses, **before any
append**, any operation whose terminals it does not implement (PR7:
integration and run end beyond refusal)".

**The refusal here is the *integration* transaction's, and only that.** This
sentence read "completing it means `task_candidate_created` or a CAS, and
PR7 implements neither terminal", which was true when the step was written
and false by the time the driver existed: `finish_promotions` calls
`candidate::append_candidate_created`, so PR7 implements that terminal and
step (f) hands unresolved promotions to it rather than refusing them. What
this refuses is the integration half — completing one means a
compare-and-swap on the integration ref, which is PR8's. The 2026-08-26
re-review of `c2c0294` found the stale half, finding C.

Takes `&PreflightCertified` because it is a step-(f) emitter's predicate and
every recovery emitter takes one — the refusal has to be reachable from the
same place the append would have been, or it is refusing somewhere else.

### Errors

[`UpstrokeError::Refused`] naming the unresolved integration transaction.

**It named a second thing until 2026-08-27**: a task whose generation was
`Promoting` with its candidate pin gone. That refusal guarded erratum E6's
convergence, which rebuilt a candidate identity from the pin; with
`candidate_prepared` as the sole successful settlement there is no such
reconstruction and no such window, so one refusal is left and this takes no
`WorkspaceManager` — it is now a predicate over the fold alone.

## `pub fn refuse_unimplemented_terminals(certified: &PreflightCertified) -> Result<(), UpstrokeError> {` › `Ok(())`

**A `Promoting` generation whose pin is gone used to be refused here**,
because E6's convergence rebuilt the candidate identity *from* that pin
and had nothing to rebuild from without it.

Both are gone. Since the 2026-08-27 CONFORM ruling `candidate_prepared`
is the sole successful settlement and the only thing that sets
`Promoting`, in the same block that records the candidate — so a
promoting generation carries its own identity and needs no pin to
reconstruct one. A pin already pruned is `T-CAND-REF`'s ordinary
late-crash prefix, which `finish_promotions` completes; a pin with no
candidate record is orphan residue, which `candidate::recovery_for`
prunes while settling the attempt interrupted.

What still refuses before any append is the integration transaction
above, which is PR8's terminal and one of the two `checkpoint_refusals`
authorises.

## `pub fn finish_promotions(`

**The continuation `T-CAND-REF`'s `resume_action` names, and E6 defers to.**

The row's own words are a four-step sequence, not one append: *"verify
object; create exact candidates ref zero-old if absent; append
`task_candidate_created`; prune the pin (no spend repeats); the closure
procedure performs the same steps at any run end"*.

**The entry to it is the run's own `candidate_prepared`, not a resume's.**
This described complete_promotions — a function deleted with the window it
served — making that append: erratum E6's
convergence, which rebuilt a candidate identity from the prepared pin. Both
are gone: since the 2026-08-27 CONFORM ruling `candidate_prepared` is the
sole successful settlement and the only thing that promotes a generation, so
a `Promoting` generation always carries its own recorded candidate and E6's
window — promoting with none — cannot occur. What reaches this sequence is a
promotion the run itself recorded; a pin with no record is orphan residue,
which `candidate::recovery_for` prunes while settling the attempt
interrupted.

### What this repairs, and it was a stall rather than a wrong answer

Without it, a converged generation sat at `Promoting` forever. Nothing else
finishes one: `TopologyRun`'s only promotion call is on the **settle** path,
which a resumed run never reaches, and `select` has no branch that advances a
`Promoting` generation — `eligible_integration` wants `task_candidate_created`,
`first_ready_retry` wants `RetainedIdle`, and `RunState::ready` wants
`task.open().is_none()`. `Promoting` holds a pipeline entitlement, so at
`max_parallel = 1` the stall took every other task with it.

It was also a **regression against a refusal**: before E6,
[`refuse_unimplemented_terminals`] refused any `Promoting` generation before
any append. Narrowing that refusal without implementing the continuation
traded a clean pre-append refusal for a silent permanent stall, which is the
worse of the two by the project's own ordering — a refusal is a resumable
end, and this was not an end at all.

### Both windows, one sequence

[`recovery_for`] classifies an unfinished promotion from the **durable
record**, so this covers the generation a resume's convergence once appended
for *and* the ordinary `T-CAND-REF` window — a run killed between
`candidate_prepared` and `task_candidate_created`, which needs no erratum and
reached the same dead end. Two windows, one continuation, because the row
describes one.

### Idempotent, because the row requires it

Each half already refuses to repeat itself: `create_candidates_ref` accepts a
ref already at the recorded SHA, `append_candidate_created` is skipped once
the generation has left `Promoting`, and `reclaim_after_creation` prunes the
pin only while it is there. That is what lets "the closure procedure performs
the same steps at any run end" be one function rather than two.

### Errors

Any refusal of the three halves — a missing or mismatched object, a ref at
another SHA — or whatever the append returns.

## `let mut unfinished = Vec::new();`

Classified before anything is appended, from the fold as it stands. The
borrow ends here because the sequence below needs the chain mutably.
**Both products of the classification, not one.**

`CandidateRecovery` carries a `promotion`, an `orphan_pin` and
`settles_interrupted`, and an earlier draft of this function read only the
first. That left `T-CAND-OBJ` (b)'s `resume_action` — "delete the exact
orphan pin expected-old, after which the object is again Git's" —
performed by nothing, with `prune_orphan_pin` written, correct, and called
only from its own test. Round 3 found it three times over (`consumer`,
`seams`, and Sol's independent `seams`), which is the classifier's answer
being computed and dropped by its only caller.

## `for orphan in orphans {`

The prune is `T-CAND-OBJ` (b)'s and runs before the promotions: an orphan
pin is one nothing durable names, so there is no promotion that could want
it, and leaving it until after would leave a ref the closure procedure
would then have to distinguish from its own.

## `let referenced = crate::engine::topology::candidate::create_candidates_ref(`

The three halves alternate between the hooks bundle and the journal,
exactly as they do in the driver: the journal must hold the bundle to
append, and the effect halves need it back. The typestate carries the
order — only `create_candidates_ref` makes a `ReferencedCandidate`,
and only that makes an `append_candidate_created` callable.

## `struct RecoveryJournal<'c, 'e, 'x> {`

The recovery's chain, lent to the candidate sequence as a [`CandidateJournal`].

The sequence is written once and runs from two places — the driver on the
settle path and the recovery on resume — so what differs between them is the
journal and not the steps. `TopologyRun::with_journal` is the driver's half of
the same idea; this is the recovery's.

## `pub fn ensure_recorded_integration_ref(`

---------------------------------------------------------------------------
T-RUNSTART's P7/P8 repair — a durable effect, and not an event
---------------------------------------------------------------------------

## `pub fn ensure_recorded_integration_ref(`

`transaction_fault_matrix[T-RUNSTART].resume_action`: "**P7/P8: create the
ref zero-old at the recorded base if absent; if present == base continue (no
spend repeats)**".

A run killed between P6 and P8 is committed — `run_started(4)` is durable and
`committed.json` names its digest — but has no `integration_ref`. Nothing
else in this build creates one, so without this step such a run resumes into
a namespace its own record describes and the repository does not have.

**The body is P8's, called rather than copied.**
[`super::create::ensure_integration_ref`] answers all three dispositions —
absent, present at the base, present at anything else — and its doc states
why there may be only one of it. This function contributes the two
arguments and nothing else; if it ever grows a comparison of its own,
that is the duplication the shared body exists to prevent.

**Both arguments come from the record.** `run_started(4).integration_ref` and
`run_started(4).base_sha`, reached through the witness chain from the
committed first line (a0) read and (a) authenticated against
`committed.json.run_started_sha256`. Not from today's `[runner]` selection,
not from a `Workspace`, and not from the fold's current view of the run: a
resume that recomputed either would be able to publish a ref the run was
never started against.

Takes `&PreflightCertified` for the same reason
[`refuse_unimplemented_terminals`] does — it is what makes "after (c)"
unstateable as anything else — and returns `()` rather than a witness because
nothing downstream may depend on it having run: it is a repair of a prefix,
not a link in the order.

### Errors

[`UpstrokeError::Refused`] when the recorded base is not a full hexadecimal
object id or is the null id (`workspace_manager::Refusal::MalformedObjectId`,
`NullNew`; `CommitSha` does not validate this, so a record carrying one
reaches this refusal), or when the recorded ref is symbolic, checked out in
some worktree, or already at any SHA other than the recorded base; a Git
error from the creation itself, including the zero-old failure when the ref
appeared between the read and the write.

## `pub struct EmitContext<'a> {`

---------------------------------------------------------------------------
The recovery events, (d) through (h)
---------------------------------------------------------------------------

## `pub struct EmitContext<'a> {`

What one recovery event append needs beyond the witness.

Bundled because they travel together at every emitter and a signature that
spelled them out four times is four places for one of them to go missing.

Everything below `hooks` is here because [`super::emit::emit`] needs it, and
it needs it because the append-error protocol does: the barrier it
establishes at obligation (5) is established over `inputs` and the committed
first line's digest, and obligations (2) and (3) are `cancel_any` and
`cancel_all_running` on these two ledgers. Passing them in rather than
making them here is what lets a caller that *does* hold a reservation or a
running invocation have it cancelled — "recovery holds neither today" is a
fact about today's callers, not a licence to drop the obligation.

## `pub struct EmitContext<'a>` › `pub clock: &'a dyn TimeSource,`

Where a durable event's timestamp comes from. Seamed so a byte-exact
assertion over the log is possible at all.

## `pub struct EmitContext<'a>` › `pub hooks: &'a mut dyn TopologyHooks,`

The five effect-hook families. The Event funnel's are what a
`T-APPEND` fault test arms.

## `pub struct EmitContext<'a>` › `pub inputs: FrozenInputs,`

The frozen plan and its digest. The protocol's reopened barrier is
established over exactly these — the same two inputs recovery step (a1)
used — so a protocol that took its own copy could prove a prefix against
a plan the run was never folded from.

## `pub struct EmitContext<'a>` › `pub reservations: &'a mut Reservations,`

The provisional-reservation ledger. `cancel_any` on any outcome-unknown
append.

## `pub struct EmitContext<'a>` › `pub invocations: &'a mut InvocationLedger,`

The invocation ledger. Every still-running entry is cancelled; the
Runner half of that is the caller's.

## `pub struct EmitContext<'a>` › `pub warnings: &'a mut Vec<String>,`

Where the protocol's reopen reports a torn-tail normalization.

## `pub fn settle_interrupted(`

(d) Settle every in-flight identity interrupted.

`recovery_order` (d), and `T-ATTEMPT`'s resume action: an attempt whose
coordinator died is not retried in place, it is settled `interrupted` and
its generation closed, so the next dispatch opens a fresh generation at the
task's base.

### Errors

Whatever [`emit`] refuses or fails at.

## `pub fn close_retained_idle(`

(e) Close every `RetainedIdle` generation with
`generation_closed{ResumeDiscardsRetainedSession}`.

A retained session belongs to the incarnation that retained it, and this is
not that incarnation: `T-RESUME`'s authoritative state says "retained_session
authority already invalid for the new incarnation", so the generation closes
rather than being resumed into.

### Errors

Whatever [`emit`] refuses or fails at.

## `pub fn run_resumed(`

(h) `run_resumed(4)` — and the step that **consumes** the pre-flight
witness.

O33 is "recovery events before `run_resumed`", and this signature is that
clause: `certified` is taken **by value**, so no emitter of a recovery event
can present a `PreflightCertified` after this returns. The witness is gone.

INV-23: "`run_resumed(4).runner` records what the incarnation established
and **must equal `run_started(4).runner` exactly** (a `FoldError`
otherwise)". The value written is [`RunnerRebuilt::policy`], which
`rebuild_by_inspection` returns as the record field for field — so the
equality is a property of the rebuild rather than a comparison this function
performs and could get wrong.

### Errors

Whatever [`emit`] refuses or fails at — including the fold's own
`RunnerMoved` refusal if a runner identity ever reached here that did not
equal the record's.

## `let events = certified.rebuilt().censused().barrier().events().to_vec();`

Taken before the unwind, from the barrier that proved them. `run_resumed`
was appended above and is deliberately not here: these are the *proven
prefix*, which is what a spend replay is defined over.

## `let (log, fold, records) = certified`

The witness is spent; what it was carrying is not. Unwound rather than
dropped, because everything below it is the run's own state and the loop
is the thing that needs it — see [`RunHandle`].

## `let committed_first_line_sha256 = records.commit().run_started_sha256.clone();`

Taken before the records are unwound: the digest step (a) verified is the
one the loop's own appends must be able to check themselves against.

## `pub struct RunHandle {`

The run's own state, handed from a completed start to the loop that drives
it.

**Every field of this was being dropped at the end of the recovery order**,
and that is the mechanical reason `TopologyRun` could not exist. Not a
missing function — a missing *value*. `run_resumed` consumed the last
witness and returned a two-field summary, so the append handle the barrier
had just entitled the command to, the fold built from exactly the proven
bytes, and both locks died with it.

Each of the three matters for a different reason:

- **The log** is the handle `(a1)` proved. A loop that reopened the log
  would append to a prefix its own barrier never proved, which is the whole
  of what the barrier exists to prevent.
- **The fold** is derived from exactly the bytes the barrier synced, reread
  and replayed. Rebuilding it anywhere else is a second derivation that can
  disagree with the first.
- **The locks** make this process the run's only writer. A loop that had to
  retake them would be racing itself, and `run_creation` requires the
  worktree lock held "across the startup census **and the whole run**".

The lock fields keep their `_` names and stay private: nothing reads them,
they exist to be dropped, and they drop in declaration order so the run lock
is released before the worktree lease. All this changes is *when* — the end
of the loop rather than the end of recovery.

## `pub struct RunHandle` › `pub committed_first_line_sha256: String,`

The digest recovery verified `committed.json.run_started_sha256` against.

**Carried because the loop's appends need it too.** The append-error
protocol's creator disposition is a projection of the outcome onto the
run's commitment boundary, and without this the loop's `RunIdentity`
answers `None` where recovery's own emitter answers `Some` — two
emitters of one run disagreeing about whether it is committed.

## `pub struct RunHandle` › `pub log: crate::events::log::EventLog,`

The append handle the stable-prefix barrier entitled this command to.

## `pub struct RunHandle` › `pub fold: TopologyFold,`

The fold built from exactly the barrier-proven bytes.

## `pub struct RunHandle` › `pub started: RunStarted4,`

The record the run started from, which the loop's emitter stamps from.

## `pub struct RunHandle` › `pub events: Vec<TopologyEvent>,`

The events the stable-prefix barrier parsed from the proven bytes.

**Carried because the fold is not the whole of what a resume must
rebuild.** The fold keeps the run's *state*; it does not keep the
`AttemptRecord` each settlement carried, which is why
the deleted convergence needed `StablePrefix::events` and why
`Spend::replay` — whose whole purpose is rebuilding the run's spend from
the log — had **no production caller at all** until this field existed.
A resumed run started its ceiling again at zero, every time.

These are the barrier's own parse, not a second one: the census
`the_stable_prefix_barrier_is_the_only_way_a_log_becomes_a_topology_fold`
refuses any other route, and it refused an earlier draft of the E6
convergence for exactly this.

**The proven prefix, not the prefix plus recovery's own appends.** This
justified itself by naming two mechanisms that no longer exist: the E6
convergence's rebuilt `candidate_prepared`, deleted with the window it
converged, and `Spend::replay`'s identity-keyed dedupe, deleted with the
duplicate settlement it was written to hide. Neither can price anything
now.

It is still correct, for a simpler reason. Recovery appends no event for
an attempt the prefix already settled, so there is nothing here for a
dedupe to have caught: an attempt's record reaches the log exactly once,
on `attempt_finished` if it failed and on `candidate_prepared` if it
succeeded, and the fold refuses both of the shapes that made it twice.

## `impl RunHandle` › `pub fn created(`

The handle a **freshly created** run hands to the loop.

The resumed path builds this by unwinding the recovery chain, where each
field is the product of a witness. A created run has the same fields from
P0-P8 instead, and the lock guards stay private either way — they exist
to be dropped, in declaration order, and nothing reads them.

## `impl RunHandle` › `events: Vec::new(),`

**Empty, and that is a fact rather than a placeholder.** A freshly
created run has appended one line, `run_started`, which carries no
attempt and no spend. `Spend::replay` over it is zero, which is
exactly what a fresh run's ceiling should start at — so the
created and resumed paths reach the loop through the same field
rather than through two rules.

## `impl std::fmt::Debug for RunHandle` › `fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {`

Names the run and says nothing about the locks.

A derived `Debug` would print two guards whose whole contract is that
nothing reads them, and a lock that appears in a log line is a lock
someone will eventually reason about.

## `pub struct Resumed {`

What (h) established, for the caller that reports it.

A value rather than `()` so that "the epoch's budget stop cleared" and
"state.resumes increments" are things a test asserts about the fold rather
than about the absence of an error.

## `pub struct Resumed` › `pub epoch: u32,`

The epoch this resume opened. `run_resumed` increments it.

## `pub struct Resumed` › `pub budget_stop_cleared: bool,`

Whether the previous epoch's budget stop is gone.

## `fn emit(`

`coordinator_integration.emit`, for one recovery event.

**One line of body, and that is the point.** Every recovery event — (d)'s
`attempt_interrupted`, (e)'s `generation_closed`, (h)'s `run_resumed` — is an
`Event.Append`, and `append_error_protocol` applies to `Event.Append` without
exception: poison the fold, `Reservations::cancel_any`,
`InvocationLedger::cancel_all_running`, no retry and no report from memory,
then reopen through `Event.OpenLog`, establish the stable-prefix barrier, and
end naming the run id, the event kind and **whether the proven prefix
contains the line** — present, absent, or undetermined.

[`super::emit::emit`] is those five obligations and the six steps above them.
This function is the call, and `dispatch.rs` states why it is only the call:
"a module that held the log would hold the append-error protocol with it …
and there would be two implementations of it, which is the duplication class
this crate has already paid for three times". [`super::create`] keeps one of
its own on purpose — `Event.AppendFirst` has to answer *absent first line* as
one of three creator dispositions rather than as a barrier failure — and the
recovery order has no such difference to justify a third.

The two shapes an open-coded version got wrong, recorded because they are
what a reader would otherwise reintroduce:

* a `FoldError` is a **refusal**, not [`UpstrokeError::EventLog`]. Nothing
  was written, so an error naming the log file as an I/O path says the wrong
  thing about what happened.
* a funnel `Err` **before the append was entered** — a poisoned handle, a
  legacy handle, a site that is not this line's — must not poison the fold.
  `emit` decides that from `EventLog::poisoned_at()` on both sides of the
  call rather than from the error value, so "entered" is decidable and a
  wrong-site refusal leaves the fold usable.

### Errors

[`UpstrokeError::Refused`] for a transition the checked fold rejects — before
any write — and for an append that was entered and returned an error, in
which case the protocol has already run and the message carries its report.
[`UpstrokeError::Io`] or [`UpstrokeError::EventLog`] for a refusal the funnel
raised before entry.

## `let records = certified.rebuilt().censused().barrier().records();`

Built before the mutable borrow of the chain below it, and from the
witness rather than from a caller: the run id is the one (a0) resolved and
the digest is the one (a) verified, so the protocol's barrier is the same
barrier over the same two inputs as recovery step (a1)'s.

## `super::emit::emit(&identity, &mut state, context.clock, body, context.hooks)`

Recovery holds the run's ledger, so it discharges obligation (3) here
rather than carrying it further: the recovery order is the one caller
that both emits and owns every invocation this process registered.

## `fn fold_of(certified: &PreflightCertified) -> &TopologyFold {`

---------------------------------------------------------------------------
Reading the proven fold
---------------------------------------------------------------------------

## `fn fold_of(certified: &PreflightCertified) -> &TopologyFold {`

The fold the barrier proved, from any point in the chain below it.

## `fn started_of(certified: &PreflightCertified) -> &RunStarted4 {`

The committed `run_started(4)`, from the same point in the chain.

The record (a0) resolved and (a) authenticated against
`committed.json.run_started_sha256` — reached through the witnesses rather
than re-read, so the bytes a later step publishes a ref from are the bytes
the commit record proved.

## `pub fn recreate_open_no_attempt(`

**(g)** Recreate `OpenNoAttempt` worktrees at their bases.

`decisions.sequential_substrate.recovery_order`: "(g) recreate
`OpenNoAttempt` worktrees at their bases (through `Worktree.Verify` or
forced recreate)". It is one of the order's eleven steps and it was the one
this module did not perform — the omission that `resume_open_no_attempt`,
written and tested by the dispatch lane, had no production caller for.

**The recovery this step performs is not chosen here.**
`decisions.workspace_candidates.generation` gives a failed verification two
different recoveries and says which applies is a property of the
generation's *class*: an `OpenNoAttempt` or repair worktree "is removed with
force and recreated", a `RetainedIdle` generation "is closed". This step
enumerates one class and hands each member to the single function that
implements that class's recovery. The retained class is (e)'s and reaches
`Worktree.Verify` through its own seam, so no retained worktree can arrive
here to be handed the recreate branch.

**Every field is read off the proven prefix, not invented — and the value
asks for nothing else.** `base` is the generation's recorded `base_sha`, and
the slot is [`task_slot`], which derives it from `{key, generation}` so no
two callers can disagree about which worktree a generation owns. There is no
third field to get wrong, and that is deliberate: the rebuild family takes
[`OpenGeneration`] rather than a full `Dispatched` precisely so recovery
never has to reconstruct a predicted region the fold does not hand back.
Inventing one would be a field that lies about a lease; reaching into
`src/topology/`'s lease table for the real one would be an edit to PR3's
layer for a value no path below this one reads.

### Errors

[`UpstrokeError::Refused`] for a generation whose lease is an inherited
lineage — a repair, whose resume action is to re-materialize its source
candidate, and whose source the fold does not retain. `checkpoint_refusals`
gives repair execution to PR8, so this build refuses rather than
reconstructing a materialization it cannot prove. **The arm is unreachable
in this slice and both walls are measured** by
`a_repair_generation_cannot_reach_step_g_in_this_slice`: the fold refuses an
inherited lease on an ordinary task at the barrier's checked replay, and
`TaskRegistry::originals_with_agents` gives every entry `lineage: None`, so
there is no task the lease would be legal on. That test fails the day a
slice admits repairs, which is when this arm becomes reachable. Also
refused when a generation holding its lease has no recorded region, which
is a fold that disagrees with itself rather than a state to guess at.
Otherwise the containment refusals or a Git error from
[`resume_open_no_attempt`].

## `fn open_no_attempt(fold: &TopologyFold) -> Result<Vec<OpenGeneration>, UpstrokeError> {`

Every `OpenNoAttempt` generation the proven prefix records, with what a
rebuild of it needs.

Sibling of [`retained_idle`] and [`in_flight`], and deliberately shaped like
them: one enumerator per generation class, so "which class does this step
act on" is a property of the function the step calls rather than of a
predicate the step re-derives. Two rules that can disagree is the shape this
slice has paid for repeatedly.

## `fn open_no_attempt(fold: &TopologyFold) -> Result<Vec<OpenGeneration>, UpstrokeError> {` › `let Some(open) = fold.open_no_attempt(key) else {`

The class question is the fold's, through `open_no_attempt`. The
repair refusal below is recovery's own policy and stays here.

## `fn open_no_attempt(fold: &TopologyFold) -> Result<Vec<OpenGeneration>, UpstrokeError> {` › `source: None,`

`None` is not a guess. An ordinary generation has no
materialization to reproduce, and the repair case returned
above rather than reaching here — so the field is decided by
the same match that decided the refusal, and there is no
third path that could leave it wrong.

## `fn task_keys(fold: &TopologyFold) -> Vec<TaskKey> {`

Every task key the registry holds.

## `fn in_flight(fold: &TopologyFold) -> Vec<(TaskKey, GenerationId, AttemptNumber, LeaseDisposition)> {`

Every `(key, generation, attempt)` whose attempt was running when the last
coordinator died.

## `fn in_flight(fold: &TopologyFold) -> Vec<(TaskKey, GenerationId, AttemptNumber, LeaseDisposition)> {` › `found.push((`

`survives: false` — "the generation does *not* survive an
interruption" (T-ATTEMPT: generation Closed). The disposition
is therefore the lease's own answer to that question rather
than a constant, which is what keeps a lineage member
recording `LineageHeld` where an ordinary generation records
`PredictedReleased`.

## `fn retained_idle(fold: &TopologyFold) -> Vec<(TaskKey, GenerationId, LeaseDisposition)> {`

Every `(key, generation)` settled holding a session.

## `fn outcome_name(outcome: &RunOutcome) -> &'static str {`

The outcome as `run_finished` writes it.
