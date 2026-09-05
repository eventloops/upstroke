# `src/runner/container/census.rs`

Extended notes for [`src/runner/container/census.rs`](../../../../src/runner/container/census.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The **startup container census** — discovery, the incarnation-aware
ownership rule, and the reclaim driver.

`decisions.sequential_substrate.startup_census` step (a):

> performed by every topology write command (**run, resume**) after taking
> the worktree lock and before any run-id use for creation, run-lock
> acquisition for a fresh run, slot or reservation initialization, admission,
> credential-volume use, or probe (a resume takes its run lock first,
> establishes the stable-prefix barrier of recovery step (a1), then censuses,
> so no census reclaim decided from the fold precedes durability of the
> prefix it is decided from) … (a) global container reclaim over
> `<R>/containers` and docker ps by the private-root label under the
> incarnation-aware liveness rule

#### What this module owns and what it deliberately does not

It owns the **decision** and the **sequence**. It performs no container
effect itself: every reclaim goes through [`super::reclaim`], the funnel API
whose five steps are the packet's own order, and the four effectful methods
of [`ContainerRuntime`] are on `clippy.toml`'s disallowed list so calling one
from here is a build error (`decisions.effect_site_inventory.mechanism`
(1)-(2)). What it calls directly is read-only: the namespace scan, `docker
ps` by label, and the owner-liveness probe.

#### The consumers it precedes, and why they are a token rather than a comment

`crash_reconstruction`: "the census completes **before** slot/reservation
state is initialized, **before** admission, **before** any invocation uses an
agent's credential volume, and **before** this incarnation's probes". Four
consumers, none of which exists in this slice — slots and admission are
PR11's, the credential-volume turn and the RunnerPreflight probes are PR7's.

A comment saying "call this first" holds none of that. So the census returns
a [`CensusComplete`], whose fields are private to this module and which no
other code can construct: the four consumers take one by reference when they
are built, and until they are, the token is the thing this slice can hold and
the next slice can thread. [`crate::rundir::PrivateHalfProof`] is the same
device for the same reason — a proof obligation carried in the type system
rather than in prose.

**What a later slice must connect** is stated once, here, so it is not
rediscovered: PR7's `TopologyRun` calls [`run_startup_census`] after the
worktree lock (fresh) or after the run lock and recovery step (a1) (resume),
and passes the resulting `&CensusComplete` into slot/reservation
initialization, admission, the first credential-volume use, and
`RunnerPreflight`. [`census_returns_the_only_token_that_reaches_a_consumer`]
is the source census that says no other construction exists.

## `#![deny(`

`PR6-LANEF-004`: the Container funnel's module-level allow is an INNER
attribute, and a Rust lint level is scoped by the MODULE TREE rather than by
the file, so every out-of-line child of `runner::container` inherited it --
measured, a `ContainerRuntime::start` planted in a child module passed
`cargo clippy --all-targets --all-features -- -D warnings`. Re-denying here
is what makes `decisions.effect_site_inventory.mechanism` (1)'s BUILD error
true of a lane's module, which is the leg the source census cannot supply.
Enforced for every file in this directory by `runner::container::tests::
every_child_module_of_the_container_funnel_states_its_own_lint_level`.

## `pub const VIEWS_DIR: &str = "views";`

---------------------------------------------------------------------------
R19: where an orphan view is found
---------------------------------------------------------------------------

## `pub const VIEWS_DIR: &str = "views";`

The directory the disposable Git views live under, inside `<R>`.

R19's lifecycle is "pruned on complete or cancel; **orphan views reclaimed
during dead-owner or dead-incarnation container reclaim**", and a census
reclaiming an orphan holds no [`super::Launched`] to read a path out of — the
six intent fields the packet fixes carry no view path. So the location has to
be *derivable* from what a census already knows, which is the private root
and the container name.

**Cross-lane seam, and it is one definition rather than an agreement.** The
invocation path that *materialises* the projection reaches this function
too: [`super::exec::view_dir`] is a call to [`view_path`] and nothing else.
It used to be a second copy of the same `join`, and an independent review
measured what that costs — changing only the producer to
`<R>/views-v2/<name>` passed the whole suite and orphaned every view the
census would have pruned. A convention maintained in two places is a
convention only until somebody edits one of them.

## `pub fn view_path(private_root: &Path, name: &ContainerName) -> PathBuf {`

`<R>/views/<container-name>` — the R19 view of one container invocation.

The single definition: [`super::exec::view_dir`] delegates here, and
`exec::tests::the_view_path_the_census_prunes_is_the_one_the_invocation_mounts`
holds the value against a literal rather than against either caller.

## `pub fn private_root_label(private_root: &Path) -> String {`

The value of the `upstroke.private_root` label for this root.

**One definition, in the module that owns [`LABEL_PRIVATE_ROOT`]**, rather
than the rendering re-derived here and pinned to the funnel's by a test. The
two spellings were byte-identical and still not injective — `<R>/a\b` and
`<R>/a/b` are different roots on Unix and rendered to one label — and
repairing that in one copy would have left a census filtering on a value no
container carries, which discovers nothing and reports a clean machine.
[`super::intent::private_root_label`] carries the injectivity argument.

[`the_private_root_label_this_census_filters_on_is_the_one_the_intent_writes`]
still pins the census's filter value against `ContainerIntent::labels`; it
is now true by construction, and its oracle is an independent table of
hand-computed encodings rather than the other copy.

## `pub struct PrefixBytes {`

---------------------------------------------------------------------------
Recovery step (a1): the stable-prefix barrier
---------------------------------------------------------------------------

## `pub struct PrefixBytes {`

A byte range of the event log, by length and content digest.

Two fields and not one: "**proven its bytes and boundary unchanged**" is two
claims, and a digest alone would let a boundary move under a prefix whose
bytes happen to hash the same.

## `pub struct PrefixBytes` › `pub len: u64,`

The boundary — how many bytes of the log the prefix is.

## `pub struct PrefixBytes` › `pub sha256: String,`

`sha256` of exactly those bytes, lowercase hex.

## `impl PrefixBytes` › `pub fn of(bytes: &[u8]) -> Self {`

Measure a prefix.

## `pub struct PrefixSync {`

How far the surviving event-log prefix has been **synced**.

`crash_reconstruction`: "after the stable-prefix barrier of step (a1) has
**synced** the surviving event-log prefix, proven it stable, and
checked-replayed it, so that no fold-derived reclaim decision precedes
durability".

## `pub struct PrefixSync` › `pub synced_len: u64,`

The number of bytes the recovery step has made durable.

## `pub struct PrefixReread {`

The reread: the whole file read a second time, so its bytes and boundary can
be **proven** unchanged rather than assumed.

## `pub struct PrefixReread` › `pub first: PrefixBytes,`

What the first read saw.

## `pub struct PrefixReread` › `pub second: PrefixBytes,`

What rereading the whole file saw.

## `pub struct PrefixReplay {`

The checked replay: which bytes the fold was actually computed over.

## `pub struct PrefixReplay` › `pub replayed: PrefixBytes,`

The prefix the replay consumed.

## `pub struct StablePrefixBarrier {`

Recovery step (a1), established — the evidence a resume's census is
entitled to decide reclaim from the fold.

**Four separately droppable predicates**, each with its own refusal, because
"reclaim decided from a prefix that was synced but not proven stable, or
proven stable but not replayed, is reclaim on unproven authority":

1. the reread's **boundary** equals the first read's,
2. the reread's **bytes** equal the first read's,
3. the **synced** extent covers the prefix the decision rests on,
4. the replay consumed **exactly those reread bytes**.

The type has no public constructor and no public fields, so a resume census
cannot be reached with a barrier that was not established.

**What a later slice must connect**: PR7's recovery step (a1) supplies the
three measurements — it owns the event log, its `sync_data`, and the fold.
This slice owns the four comparisons and the refusals.

## `impl StablePrefixBarrier` › `pub fn establish(`

Establish the barrier, or refuse and say which predicate failed.

### Errors

[`UpstrokeError::Refused`] when the boundary moved, the bytes changed, the
prefix is not durable to its boundary, or the replay was of other bytes.

## `impl StablePrefixBarrier` › `pub const fn boundary(&self) -> u64 {`

The boundary the fold was computed to.

## `impl StablePrefixBarrier` › `pub fn digest(&self) -> &str {`

The digest of the prefix the fold was computed over.

## `pub enum CensusStart {`

---------------------------------------------------------------------------
Who is censusing
---------------------------------------------------------------------------

## `pub enum CensusStart {`

Why this process is at a census, and what it is entitled to conclude.

The two arms are not decoration: arm (i) of the liveness rule is "owner run
**==** the run this process is driving (**this process holds its
run.lock**)", and a fresh run holds no run lock at census time —
`startup_census` puts the census "**before** … run-lock acquisition for a
fresh run". So a fresh run has no own-run arm at all and every candidate goes
to arm (ii), while a resume has one and owes recovery step (a1) first.

## `pub enum CensusStart` › `FreshRun {`

`upstroke run`: the worktree lock is held, no run lock is, and the run id
has not been used for creation yet.

## `pub enum CensusStart` › `incarnation: String,`

This process's per-process ULID.

## `pub enum CensusStart` › `Resume {`

`upstroke resume`: this process holds `run_id`'s run lock and has
established recovery step (a1).

## `pub enum CensusStart` › `incarnation: String,`

This process's per-process ULID. **Never read from lock-file
contents** — `run.lock` content is never read, and a Windows
exclusive lock makes it unreadable to non-holders. It is the value
recorded in `run_resumed(4)`, handed in.

## `pub enum CensusStart` › `barrier: StablePrefixBarrier,`

Recovery step (a1), established before this census.

## `impl CensusStart` › `pub fn own_run(&self) -> Option<&str> {`

The run this process is driving, if it holds one's lock.

## `impl CensusStart` › `pub fn incarnation(&self) -> &str {`

This process's incarnation.

## `impl CensusStart` › `pub const fn command(&self) -> WriteCommand {`

Which write command this is, for the report.

## `pub enum WriteCommand {`

The topology write commands that perform a startup census.

"performed by **every topology write command (run, resume)**" — both, not
resume only. A census guarded behind resume-only logic lets a dead run's
containers survive into a fresh run's admission, which is the failure the
sentence names two commands to prevent.

## `impl WriteCommand` › `pub const ALL: &'static [Self] = &[Self::Run, Self::Resume];`

Both of them, written out so a grid over write commands is a grid over
all of them.

## `impl WriteCommand` › `pub const fn name(self) -> &'static str {`

As the report writes it.

## `pub enum Ownership {`

---------------------------------------------------------------------------
The liveness rule
---------------------------------------------------------------------------

## `pub enum Ownership {`

How one container's owner classifies.

`crash_reconstruction`, verbatim:

> (i) owner run == the run this process is driving (this process holds its
> run.lock): incarnation != this process's incarnation -> **dead by
> construction** (the run lock is exclusive, so only one incarnation of a run
> is ever live) -> reclaim; incarnation == this process's incarnation
> **cannot exist at census time** (the census precedes every invocation incl.
> this incarnation's probes) and **is refused if observed**; (ii) owner run
> != this run: probe that run's run.lock non-blocking; **free** -> dead owner
> -> reclaim **every** container of that run **whatever its incarnation**;
> **held** -> live owner -> **never touched**

**The own-incarnation refusal belongs to arm (i), and that is a reading —
the opposite of the one that shipped** (`PR6-RECOV-003`).

The rule above is an exhaustive dichotomy on the *owner run*, and the
refusal clause is written **inside arm (i)**, after its colon. Arm (ii) then
says what it does with the incarnation, in as many words: "reclaim every
container of that run **whatever its incarnation**". "Whatever" includes
this process's own, so the two clauses do not overlap and there is nothing
to adjudicate between them; `transaction_fault_matrix.T-CONTAINER
.resume_action` states the same rule in the same order — "owner run == this
run -> incarnation != this process's incarnation -> … -> reclaim; owner run
!= this run -> probe the owner's run.lock non-blocking; held -> skip; free
-> reclaim".

The shipped code hoisted the incarnation comparison **in front of** the
split and refused on it under any run id, on the strength of
`expected_failures_refusals[7]` — "an intent naming this process's own
incarnation at census time is refused" — read as unqualified. That line is
the contract's one-sentence summary of arm (i)'s clause, and a summary that
drops a qualifier is not a second rule. Two live passages state the
classification arm-first; one summary states it arm-free; the classification
wins.

**What the hoisted check cost.** A foreign run whose recorded incarnation
equals this process's never reached arm (ii) at all, so its owner's lock was
never probed and a perfectly dead owner's container blocked every write
command under that private root, permanently, with no operator remedy. The
hoisted check was not the safer choice either way: arm (ii) reclaims only on
a **free** lock, so the live-owner container it was supposed to protect is
protected by the probe.

## `pub enum Ownership` › `OwnRunEarlierIncarnation,`

Arm (i), `incarnation != mine`: dead by construction, because the run
lock is exclusive and this process holds it.

## `pub enum Ownership` › `OwnRunThisIncarnation,`

Arm (i), `incarnation == mine`: cannot exist at census time — the census
precedes every invocation including this incarnation's probes — and is
**refused** if observed. `expected_failures_refusals[7]`.

## `pub enum Ownership` › `ForeignRunDeadOwner,`

Arm (ii), the owner's `run.lock` is **free**: reclaim, whatever the
container's incarnation.

## `pub enum Ownership` › `ForeignRunLiveOwner,`

Arm (ii), the owner's `run.lock` is **held**: never touched, whatever the
container's incarnation — "that owner reclaims its own earlier
incarnations at its own startup census, which precedes its admission".

## `impl Ownership` › `pub const ALL: &'static [Self] = &[`

Every classification, written out.

## `impl Ownership` › `pub const fn reclaims(self) -> bool {`

Whether this census reclaims the container.

## `impl Ownership` › `pub const fn refuses(self) -> bool {`

Whether observing it refuses the write command.

## `impl Ownership` › `pub const fn name(self) -> &'static str {`

As the report writes it.

## `pub fn classify_ownership(`

The two-arm rule, as a pure decision.

**Arm (ii) does not consult the incarnation at all**, and that is the whole
of the residual the packet names: "a container of a dead incarnation of a
**live** run may run until that run's own census reclaims it … classified as
concurrent live-coordinator sharing of R20 (existing operator configuration;
**out of scope**)". An implementation that reclaimed dead incarnations of
live runs would pass every test that varies only one of `{owner run}` and
`{incarnation}`, and would kill a container a live coordinator is spending
through.

**Arm (i) does not probe the lock at all**: this process holds it, so a probe
would be asking whether it is itself alive.

**The owner-run split comes first, and the incarnation is read only inside
the arm that reads it** — see [`Ownership`] for the passages and for what
hoisting the comparison in front of the split cost (`PR6-RECOV-003`).

The probe is [`OwnerLiveness::is_running`], called **once**, because
`T-CONTAINER.resume_action` says "probe the owner's run.lock
**non-blocking**; held -> skip". A retry loop around a held lock is a census
that waits on a live neighbour, which is a stall at every write-command
start; `census::tests::the_owner_lock_is_probed_exactly_once_per_candidate`
asserts the call count rather than the answer.

## `return if owner_incarnation == start.incarnation() {`

Arm (i). The run lock is exclusive and this process holds it, so
every other incarnation of this run is dead; this one cannot exist.

## `if liveness.is_running(owner_run_dir) {`

Arm (ii). The incarnation is deliberately not in scope here: "reclaim
every container of that run whatever its incarnation".

## `pub enum DiscoveredBy {`

---------------------------------------------------------------------------
Discovery
---------------------------------------------------------------------------

## `pub enum DiscoveredBy {`

Which half of discovery found a container.

"discovery at every write-command start **scans the whole namespace
`<R>/containers`** of the command's authorized private root **and** docker ps
by `upstroke.private_root`" — two halves, and a container may be in either or
both. `{intent present} × {container present}` is a 2×2 grid and every cell
is a real state: intent-only is a crash after the intent write and before
`docker create`, or a Unix reaper that already killed and removed the
container; label-only is "a labeled container without an intent … treated as
an orphan of its labeled run and incarnation under the same rule".

## `pub enum DiscoveredBy` › `IntentOnly,`

A record in `<R>/containers` with no container in the runtime.

## `pub enum DiscoveredBy` › `LabelOnly,`

A container carrying `upstroke.private_root` with no record.

## `pub enum DiscoveredBy` › `IntentAndLabel,`

Both halves agree it exists.

## `impl DiscoveredBy` › `pub const ALL: &'static [Self] = &[Self::IntentOnly, Self::LabelOnly, Self::IntentAndLabel];`

Every cell of the grid.

## `impl DiscoveredBy` › `pub const fn name(self) -> &'static str {`

As the report writes it.

## `pub enum Boundary {`

The boundary identity a reclaimed container is reported against.

`T-CONTAINER.resume_action`: "the census report names each reclaimed
container's boundary from its **`runner_policy_sha256`**". The intent carries
that field, so a container with a record has an exact boundary. A labeled
container with **no** record has none from this side; PR7's owner record
(`owner.json.runner` at P3b) is the other half, and this variant says so
rather than inventing a digest.

## `pub enum Boundary` › `FromIntent(String),`

From the intent's `runner_policy_sha256`.

## `pub enum Boundary` › `NoIntentRecord,`

No record: the boundary is the owner record's, which is PR7's half.

## `impl Boundary` › `pub fn digest(&self) -> Option<&str> {`

The digest, when this side has one.

## `pub struct Candidate {`

One container the census has to decide about.

## `pub struct Candidate` › `pub run_id: String,`

Owner run id — from the record, or from `upstroke.run`.

## `pub struct Candidate` › `pub incarnation: String,`

Owning incarnation — from the record, or from `upstroke.incarnation`.

## `pub struct Candidate` › `pub run_dir: PathBuf,`

The owner's **public** run directory, which is what the lock probe is
asked about.

## `pub struct Candidate` › `pub intent_path: Option<PathBuf>,`

Where the record is, when there is one.

## `pub enum OwnerSettlement {`

---------------------------------------------------------------------------
The report, and the token
---------------------------------------------------------------------------

## `pub enum OwnerSettlement {`

How the identity that owned a reclaimed container has to be settled.

`T-CONTAINER.resume_action` ends "… **then settle the owning identity
interrupted**", and `T-CONTAINER.authoritative_state` opens "**unknown
spend**". Those two clauses are one answer and there is only one of it:
*every* container a census reclaims belonged to an attempt or verification
(or to a probe's pre-run husk) that was cut off mid-flight, and no census can
know what the vendor charged for it.

**The value is a constant, and that is the whole point** (`PR6-RECOV-006`).
The state that tempts an implementation to say otherwise is
[`DiscoveredBy::IntentOnly`]: the container is not there, so it *looks* like
nothing ran — but that is exactly the post-Unix-reaper state, where the
reaper killed and removed a container whose invocation had been running and
spending for however long. Deriving the settlement from `discovered_by`
would record those attempts as completed, with their spend unaccounted. So
the settlement is a field of [`Reclaimed`] rather than something a consumer
infers, and [`Reclaimed::settlement`] is the same value for all three
discovery cells.

**What PR7 owns and this does not**: emitting the settlement *event*. PR6
has `durable_events: none` and the container transition is "test-only until
PR7 wires `TopologyRun`". What this slice owes is the value PR7 maps, stated
where the census produces it instead of left for a later reader to derive.

## `pub enum OwnerSettlement` › `InterruptedWithUnknownSpend,`

The owning attempt, verification or probe husk is settled
**interrupted**, with **unknown spend**.

## `impl OwnerSettlement` › `pub const ALL: &'static [Self] = &[Self::InterruptedWithUnknownSpend];`

Every settlement a reclaim can produce. One, deliberately: see the type.

## `impl OwnerSettlement` › `pub const fn name(self) -> &'static str {`

As a report writes it.

## `impl OwnerSettlement` › `pub const fn spend_is_known(self) -> bool {`

Whether the owning identity's spend is known. Never.

## `pub struct Reclaimed {`

One container this census reclaimed.

## `pub struct Reclaimed` › `pub boundary: Boundary,`

"the census report names each reclaimed container's boundary from its
`runner_policy_sha256`".

## `pub struct Reclaimed` › `pub settlement: OwnerSettlement,`

"then settle the owning identity interrupted" — with "unknown spend".
See [`OwnerSettlement`] for why this does not depend on
[`Self::discovered_by`].

## `pub struct Untouched {`

One container this census deliberately left alone.

## `pub enum RuntimeUse {`

Whether the container runtime was consulted, and what it said.

"the container runtime is required **only** when an intent exists or a
labeled container is discoverable: if any intent exists and the runtime
cannot be reached the write command **refuses** …, and with no intent and no
reachable runtime it **proceeds**."

## `pub enum RuntimeUse` › `Consulted,`

`docker ps` answered.

## `pub enum RuntimeUse` › `NotRequired,`

The runtime could not be reached and no intent existed, so the census
proceeded without it. This is the ordinary state of every machine
without a container runtime, which today is every machine.

## `pub enum StagedDisposition {`

What became of one `<name>.intent.tmp` whose published half never landed.

`PR6-ACCT-007`. The staged file is **R26** — it is the intent record, one
`rename` short of published — so it needs a disposition in every census that
sees it, not merely to be skipped by discovery.

## `pub enum StagedDisposition` › `Adopted,`

The staged bytes were a complete record, so the file was classified
under the ordinary owner-liveness rule and appears in
[`CensusReport::reclaimed`] or [`CensusReport::untouched`] like any other
candidate. Its run directory came from the record it carries.

## `pub enum StagedDisposition` › `Removed,`

Genuinely torn, and the **name** says it belongs to a dead incarnation
of the run this process is driving (arm (i), dead by construction). The
file is removed.

## `pub enum StagedDisposition` › `RetainedForeignOwner,`

Genuinely torn, and the name says it belongs to **another run**. Arm
(ii) probes that run's `run.lock`, and a torn file carries no run
directory to probe — so this census cannot establish that its owner is
dead and leaves it alone. That owner's own next write-command start
classifies it under arm (i) and removes it; until then it is reported
here rather than being silent residue.

## `impl StagedDisposition` › `pub const ALL: &'static [Self] = &[Self::Adopted, Self::Removed, Self::RetainedForeignOwner];`

Every disposition, written out.

## `impl StagedDisposition` › `pub const fn name(self) -> &'static str {`

As the report writes it.

## `pub struct StagedResidue {`

One staged intent record this census accounted for.

## `pub struct CensusReport {`

What one census did.

## `pub struct CensusReport` › `pub private_root: PathBuf,`

The root that was censused — the one it was **given**, never a default.

## `pub struct CensusReport` › `pub orphan_window: OrphanWindow,`

Who closes the window between a coordinator's death and reclaim on this
platform. Named rather than inferred, so a Windows report says which
window it is closing.

## `pub struct CensusReport` › `pub reclaimed: Vec<Reclaimed>,`

Sorted by container name.

## `pub struct CensusReport` › `pub untouched: Vec<Untouched>,`

Sorted by container name.

## `pub struct CensusReport` › `pub staged: Vec<StagedResidue>,`

Every `<name>.intent.tmp` with no published half, and what became of it.
Sorted by container name.

## `impl CensusReport` › `pub fn boundary_of(&self, name: &ContainerName) -> Option<&Boundary> {`

The boundary this census reported for a reclaimed container.

## `impl CensusReport` › `pub fn was_untouched(&self, name: &ContainerName) -> bool {`

Whether a container was left alone.

## `pub struct CensusComplete {`

**The census completed.** Nothing that must follow it can be reached without
one of these.

Constructed only by [`run_startup_census`], and only on the path that
finished every reclaim: a census that refused returns `Err` and no token, so
"a dead owner's or dead incarnation's labeled container that cannot be
observed terminated **blocks admission**" is held by the type rather than by
a caller remembering to check.

The four things it precedes, from `crash_reconstruction`: slot/reservation
initialization, admission, an invocation's first use of an agent's
credential volume, and this incarnation's own probes.

## `impl CensusComplete` › `pub const fn report(&self) -> &CensusReport {`

What the census found and did.

## `impl CensusComplete` › `pub fn private_root(&self) -> &Path {`

The root the census actually scanned.

A consumer that operates on a different root than the census scanned is
operating on an uncensused root, and this is what lets it say so.

## `pub struct Census<'a> {`

---------------------------------------------------------------------------
The census
---------------------------------------------------------------------------

## `pub struct Census<'a> {`

Everything one census needs.

`private_root` is a **parameter and never a default**: "a schema-4 resume
[censuses] the canonical root such that `run_started.private_dir`
canonicalizes to `R/runs/<run_id>`", and "a resume always censuses its
recorded root" even when the default root or `HOME` moved. Recovery step (a0)
computes it read-only before any lock; this module is handed the answer.

## `pub fn run_startup_census(`

Step (a) of the startup census: global container reclaim.

The sequence, and every step of it is separately droppable:

1. scan `<R>/containers` — no runtime needed, and an absent directory is an
   empty namespace;
2. decide whether the runtime is required, and refuse or proceed;
3. `docker ps` by `upstroke.private_root`, and merge the two halves;
4. classify **every** candidate, and refuse **before any effect** if any
   classification refuses or cannot be made;
5. reclaim every dead candidate through [`super::reclaim`], in name order;
6. return the token.

Step 4 completes before step 5 begins on purpose. Every other refusal in this
slice's contract is "before any effect", and a census that killed three
containers and then refused on the fourth would have performed effects on
behalf of a write command that never ran.

### Errors

[`UpstrokeError::Refused`] when the runtime is required and cannot be reached,
when an intent names this process's own incarnation, when a labeled
container's ownership cannot be established, or when a dead container cannot
be observed terminated. [`UpstrokeError::Io`] from the namespace scan.

## `let staged = super::list_staged_intents(private_root)?;`

`PR6-ACCT-007`: the staged half of the namespace, read in the same scan
and before any effect, because a torn one that names this process's own
incarnation refuses for the same reason a published one does and a
refusal must precede every reclaim.

## `Some(record) => {`

A finished write whose rename did not land. The record carries
the owner's run directory, so this is an ordinary candidate under
the ordinary rule — arm (ii) included.

## `None => staged_residue.push(StagedResidue {`

Genuinely torn. The ownership evidence is the name.

## `let mut decided = Vec::with_capacity(candidates.len());`

Step 4: classify everything, and refuse before any effect.

## `decided.sort_by(|left, right| left.0.name.cmp(&right.0.name));`

Step 5: reclaim, in name order.

## `settlement: OwnerSettlement::InterruptedWithUnknownSpend,`

The same value for every cell of {intent present} x {container
present}, deliberately: see `OwnerSettlement`.

## `for residue in &mut staged_residue {`

Step 5b: the torn staging files, after the reclaims that may have removed
some of them. Arm (i) only — a torn record carries no run directory, so
arm (ii)'s lock probe has nothing to ask about and its owner reclaims its
own at its next write-command start (`PR6-ACCT-007`).

## `continue;`

A foreign run's torn file, or this incarnation's own — and this
incarnation has launched nothing, so its own staged file is
residue of a *previous* process that happened to share the id,
which no census may adopt. Both are left alone and reported.

## `fn discover_by_label(`

The `docker ps` half of discovery, and the runtime-required rule.

The reachability question is asked of [`RuntimeOp::ListByLabel`] — the
operation actually needed — and not of [`ContainerRuntime::probe`], whose
`Ok` binds nothing about a later call. A runtime that answers `probe` and
fails `ps` would otherwise classify reachable, the write command would
proceed past the refusal point, and the failure would land after "before any
recovery event".

The decision table, which is the packet's sentence split into its cells:

| intents | `ListByLabel` | outcome |
|---|---|---|
| none | `Ok` | proceed, with whatever it found |
| none | `Unreachable` | **proceed** — "with no intent and no reachable runtime it proceeds" |
| none | `Failed` | **refuse** — the runtime answered and would not say; nothing proves there is no labeled orphan |
| some | `Ok` | proceed |
| some | `Unreachable` | **refuse** — "it cannot prove those containers terminated" |
| some | `Failed` | **refuse**, same reason |

## `fn merge(`

The union of the two halves of discovery, keyed by container name.

Every candidate's ownership fields come from **one** source and the other is
checked against it: a container whose record and whose labels disagree about
its owner is not something this census may pick a winner for.

The container **name** is ownership evidence too — it is
`upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>` — so its
components are checked against the record's fields. A name and a record that
disagree about the incarnation would mean classifying on one value and
killing a container named for another.

## `fn candidate_from_intent(found: FoundIntent) -> Result<Candidate, UpstrokeError> {`

One record — published or staged-but-complete — as a candidate.

Factored out of [`merge`] so the staged half of the namespace is classified
by the **same** derivation and not by a second copy of it: the record's run
directory is decoded and checked rooted here, and the name is checked against
the record's fields, whichever half of the namespace the record came from
(`PR6-ACCT-007`).

## `fn candidate_from_intent(found: FoundIntent) -> Result<Cand…` › `let run_dir = found.record.run_dir_path()?;`

Decoded and checked rooted here, not turned into a `PathBuf` by
assumption: this is the directory arm (ii) probes a `run.lock` in,
and every wrong answer to that probe is "free", which reclaims.

## `fn from_labels_alone(`

A labeled container with no record — "treated as an orphan of its **labeled**
run and incarnation under the same rule".

## `let run_dir = super::intent::owner_run_dir(&fields[2], "container's labels")?;`

`PR6-CORRECTNESS-016`: a *present* `upstroke.run_dir` still has to say
where its owner's lock is. The missing-key arm above and this one are
separate predicates — the shipped code held only the first, so a label
set that varied which key was absent passed while `upstroke.run_dir=`
reached the probe as `./run.lock`.

## `fn check_name_against_record(found: &FoundIntent) -> Result<(), UpstrokeError> {`

The record's own name must be the name its fields build.

## `fn check_name_against(`

The two ownership components the classification is made on.

## `fn check_labels_against_record(`

A container found by both halves must have both halves saying one thing.

## `const LABEL_FILTER: &str = "label=";`

---------------------------------------------------------------------------
The Unix reaper's half of the orphan window (ST-16 (d))
---------------------------------------------------------------------------

## `const LABEL_FILTER: &str = "label=";`

`docker`'s argument for "carrying this label".

## `pub struct ReaperContainerScope {`

What this process's cleanup reapers kill when the coordinator dies.

`decisions.admission_and_leases.permits.os_matrix`: "Linux and macOS
(`cfg(unix)`): the cleanup reaper survives coordinator death, settles the
dead coordinator's process groups while holding R28, and **additionally
kills the dead coordinator's labeled containers**, closing the orphan
window".

**The selector is the incarnation, not the private root.** `upstroke.private_
root` alone names every container of every run under `<R>`, including a
**live** coordinator's — and "a live incarnation's containers must not be
touched" (`T-CONTAINER.authoritative_state`). The incarnation is a
per-process ULID, so it names this coordinator and nothing else; the private
root is kept beside it because "different private roots are disjoint worlds"
should be true of the reaper as well as of the census, and because two
filters cost nothing.

This type is a **value**, deliberately: the reaper is a `fork`-only child in
a multithreaded process and may call nothing that allocates, so every string
it will ever need is rendered here, on the parent side, before the fork.
[`crate::agent::proc::set_container_reclaim_scope`] is where it is handed
over.

**What a later slice must connect.** Nothing registers a scope in this
slice: `production_effect` is "none" and no run selects a container Runner
until PR12. PR7's `TopologyRun` registers it once run identity exists — the
private root from `run_started.private_dir` and the incarnation from
`run_started(4)`/`run_resumed(4)` — and must ensure a supervisor is live
across a container invocation, or the window is closed only by the next
write command's census.

## `impl ReaperContainerScope` › `pub fn new(`

Build the scope, refusing a label value `docker` could not carry.

### Errors

[`UpstrokeError::Refused`] when the incarnation is empty or when either
label value carries a byte that would end the argument or start another
filter — a newline, a comma, or an `=`. The reaper cannot report a
malformed selector: it has no error channel and no allocator, so the
check is here.

## `impl ReaperContainerScope` › `pub fn program(&self) -> &Path {`

The `docker` binary the reaper execs.

## `impl ReaperContainerScope` › `pub fn list_argv(&self) -> Vec<String> {`

`docker ps --all --quiet --no-trunc --filter label=… --filter label=…`,
including `argv[0]`.

`--all`, because a container that exited still holds its name, its
labels and its writable layer until it is removed. `--no-trunc`, so the
ids the reaper then kills are unambiguous.

## `impl ReaperContainerScope` › `pub fn kill_argv(&self, id: &str) -> Vec<String> {`

`docker kill <id>`, including `argv[0]`.

## `impl ReaperContainerScope` › `pub fn remove_argv(&self, id: &str) -> Vec<String> {`

`docker rm --force --volumes <id>`, including `argv[0]`.

The reaper does **kill/rm** and nothing else: `T-CONTAINER.resume_action`
is "on Unix the cleanup reaper performs **kill/rm** earlier when the
coordinator dies". The Git view and the intent record are removed by the
next write command's census, which is why every step of
[`super::reclaim`] is idempotent and tolerant of already-gone — the
ordinary post-reaper state is an intent whose container is already gone,
which is [`DiscoveredBy::IntentOnly`].

**`--volumes`, the same removal `DockerCli::remove` issues**
(`PR6-ACCT-006`). The anonymous volume an image's `VOLUME` declaration
creates per container is R26 — part of the container, referable by
nothing else — and the reaper is the last thing that can name it: once
the container is gone the next census sees `DiscoveredBy::IntentOnly`
and has no handle on the volume at all. Measured on docker 29.7.2:
`rm --force --volumes` removes the container's anonymous volumes and
leaves a mounted **named** one intact, so this discharges R26 without
touching R20.

`proc::tests::the_unix_reaper_kills_labeled_containers_before_releasing_r28`
asserts the argv the forked reaper **actually executed** against this
function, so the fork-side `c"…"` literals — which nothing can read back
at runtime — cannot drift from it.

## `pub const fn proceeds_without(error: &RuntimeError) -> bool {`

Whether a runtime error is the shape that lets a census proceed.

Kept as a named function rather than a `matches!` at the call site so the
distinction between "could not be reached" and "answered and failed" is one
thing with one reason, and so the two branches of the decision table above
cannot drift apart.
