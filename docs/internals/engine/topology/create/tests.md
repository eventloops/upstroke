# `src/engine/topology/create/tests.rs`

Extended notes for [`src/engine/topology/create/tests.rs`](../../../../../src/engine/topology/create/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Lane B's suite: the pre-lock checks' witness, P0–P8, and the one deletion
boundary.

Kept out of `create.rs` for the reason `src/runner/container/resolve/tests.rs`
is kept out of `resolve.rs`: `effects::production_region` cuts a source at
its **first** `#[cfg(test)]`, so a suite inline in the module shrinks every
source census's view of that module to whatever precedes it
(`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`). Out here, `create.rs` is production
in full to every census that reads it.

This file carries **no** module-level lint allow, and needs none: it creates
directories through the run-directory funnel, spawns its kill children
through the `Runner`, and reads with `std::fs`'s read-only calls, which are
not on the denylist.

## `const SECOND_AGENT: &str = "claude-code";`

A second recorded agent, so P4's per-agent grant check has two agents to be
per-agent about.

## `struct Fixed {`

-----------------------------------------------------------------------
Fixed identities and a fixed clock
-----------------------------------------------------------------------

## `struct Fixed {`

Fixed, because `run_started_sha256` is the digest of the exact line and
a live clock would move it.

## `struct Armed {`

-----------------------------------------------------------------------
The hook bundle: records into one `HookHarness`, and can be armed
-----------------------------------------------------------------------

## `struct Armed {`

What a fault was armed at.

A module-local double is unavoidable: `HookHarness::arm` takes only a
`SubEffectPoint`, and `HookHarness::hook` answers `Proceed` to `Before`
and `After` unconditionally, so no phase of a `RunDir` or `Lock` site can
be armed through it. What is **not** local is the recording: every family
below still reports into the shared harness, or the sites this slice
drives would contribute nothing to the coverage evidence.

## `struct Armed` › `delayed: Vec<(EventSite, SubEffectPoint, InjectionMode, u32)>,`

`(site, point, mode, consultations still to skip)`.

A delay is needed for exactly one coordinate and for a real reason:
`Event.OpenLog.SyncPrefix` is consulted by **every** open, and P5's
own open is one of them. Arming it up front would fail P5 rather
than the barrier the test is about.

## `impl Faults` › `fn due(&self, site: EventSite, point: SubEffectPoint, mode: InjectionMode) -> bool {`

Whether a delayed arming is due now, consuming one of its skips.

## `impl Faults` › `fn disarm(&self) {`

Clear every phase arming, so a test can drive the repair the same
funnel would perform after the failure it injected.

## `struct ArmedRunDir {`

The run-directory and lock families, armed and recording.

## `struct ArmedEvents {`

The append funnel, armed and recording — including
`written_kill_shape`, without which `WrittenShape::Torn` is unreachable
and no torn first line can be produced at all.

## `impl EventHooks for ArmedEvents` › `fn point(&mut self, site: EventSite, point: SubEffectPoint, mode: InjectionMode) -> Injection {`

Straight to the shared harness: `EventSite`'s points are the ones
`HookHarness::arm` accepts, so the injection *and* the coverage
record come from the same place. Only the two phases of a `RunDir`
or `Lock` site need the module-local table, because `arm` takes no
phase.

## `fn point(&mut self, site: EventSite, point: SubEffectPoint, mode: InjectionMode) -> Injection {` › `harness`

Armed in the shared harness at the moment it becomes due, so
the injection and the coverage record still come from one
place rather than two that could disagree.

## `struct ArmedContainer {`

The container funnel, armed and recording.

Needed for the same reason `ArmedRunDir` is: `ContainerSite` exposes no
sub-effect point, so `Container.Start`'s two phases are the only
coordinates a fault can be placed at and `HookHarness::arm` reaches
neither.

## `struct TestHooks {`

All five families on one harness, four of them armable.

## `impl TestHooks` › `fn container_double(&self) -> ArmedContainer {`

The container family as a value, for installing on a
`ContainerRunner` — which owns its own observer.

## `impl TestHooks` › `fn first_execution_order(&self, of_interest: &[EffectSiteId]) -> Vec<EffectSiteId> {`

The sites of `of_interest` in the order their funnels **first** ran.

[`Self::observed`] is a membership test: it answers the same for a
sequence and for every permutation of it, so an ordering claim asserted
with it is not asserted at all. `HookHarness::coverage` is
first-observation order, so filtering it preserves the order, records a
site that runs more than once at its first execution, and leaves a site
that never ran simply absent — which makes one `assert_eq!` cover
presence and order together.

## `struct RecordingProbes {`

-----------------------------------------------------------------------
The two seams
-----------------------------------------------------------------------

## `struct RecordingProbes {`

Records the order the probes ran in, and can refuse either.

## `struct RecordingProbes` › `kill_shell: bool,`

`std::process::abort()` inside the probe — the same primitive
`Injection::Kill` performs, at the one prefix that has no funnel
site of its own.

## `impl Probes for RecordingProbes` › `fn shell(`

Records the call **and runs one process through the capability**.

It used to only record. That was enough while the seam's obligation was
documentary; it is not enough now that P4 refuses a probe which
registers nothing into the granted pair, and a double that could not
satisfy the check would be a double of something production is not.
The recorded ordering — what this exists for — is unchanged.

## `struct FakeRefs {`

A ref namespace with no Git behind it.

## `struct FakeRefs` › `kill_on_create: bool,`

Die inside `Ref.CreateIntegration`, which is the P7 prefix.

## `impl IntegrationRefs for FakeRefs` › `crate::workspace_manager::refuse_new(refname, new)?;`

The contract's refusal, before the kill: the real primitive refuses
a malformed or null value before its funnel runs.

## `fn the_fake_refs_refuse_a_null_new_value_as_the_real_primitive_does() {`

The contract [`IntegrationRefs::create_zero_old`] states binds this double
as it binds `WorkspaceManager` (`PR126-REVIEW2-DOUBLES-ACCEPT-NULL-NEW`):
a null value is refused and nothing is stored.

## `fn ensure_integration_ref_refuses_a_null_base_whether_the_ref_is_absent_or_at_it() {`

The reviewer's two sequences: an absent ref and a null recorded base is
refused and stores nothing, and a ref already at the null id is not adopted
as "already at the base" (`PR126-REVIEW2-DOUBLES-ACCEPT-NULL-NEW`).

## `struct Fixture {`

-----------------------------------------------------------------------
A repository, a private root, and a record the fold accepts
-----------------------------------------------------------------------

## `fn new(tag: &str) -> Self` › `let root =`

A ULID rather than the pid: two `cargo test` runs on one box can
share a pid, and a stale private half left by the previous run
would make a retention test pass without the code retaining
anything. Deletion is a funnel with a proof token here, so a
fixture cannot simply wipe its own root.

## `fn new(tag: &str) -> Self` › `create_private_dir(&repo, &mut NoRunDirHooks).expect("repo root");`

The run-directory funnel, because `std::fs::create_dir_all` is a
build error in a `TOPOLOGY_MODULE` — tests included.

## `impl Fixture` › `fn at(root: &Path) -> Self {`

The same layout at a root that already exists — how a kill child
lands in the directory its parent will inspect.

## `impl Fixture` › `fn private_root_canonical(&self) -> PathBuf {`

The private root as the marker and the intents spell it.

## `fn record(agents: &[String], runner: RunnerPolicy) -> RunStarted4 {`

A `run_started(4)` the fold accepts, authenticated against its own
registry over `agents`.

`probed_agents` is stamped by P4, so the digest has to be taken over the
agents the request will actually probe — which is what a real caller
does too.

## `struct Driver<'a> {`

Everything a run needs, assembled around whichever doubles a test wants.

## `struct Driver<'a>` › `execution: RecordingRunner,`

The run's Runner, which the caller owns because the capabilities it
hands the probes are built from it and the pair together.

## `impl<'a> Driver<'a>` › `fn leak_into_the_pair(&self, agent: &str) {`

Leave an unsettled registration in the granted pair.

The balance check is only worth something if it reads a **populated**
pair rather than an empty one it could never have failed on, and since a
probe registers only by running through a capability that settles what
it runs, the leak has to be seeded by whoever owns the pair. That is the
caller — here, this fixture.

## `fn committed_first_line(public: &Path) -> Option<TopologyEvent> {`

The first committed line of a run's log, as the event it records.

## `fn a_created_run_hands_itself_to_the_loop() {`

=======================================================================
The happy path, and the order it runs in
=======================================================================

## `fn a_created_run_hands_itself_to_the_loop() {`

P0-P8, in `run_creation`'s order, each step through its own site.

The ordering claims that are not visible from the returned value are
asserted from the harness and from the tree: every site of the sequence
was reached, the private skeleton exists *and* the owner record does
(O08 is asserted separately by the P3a tests, which is where the
ordering is observable), and the marker is gone while the log is
**A freshly created run is drivable, not just creatable.**

`decisions.sequential_substrate.engine` is one sentence about both paths —
"`TopologyRun` drives schema 4 at max_parallel = 1 synchronously; every path
exists here before Tokio" — and `pr_sequence[8]`'s scope names "serialized
run creation P0-P8" and the dispatch chain together.

Nothing consumed `Started`, so half of that sentence had no caller: only a
**resumed** run could reach the loop. `PR31-CONTRACT-005`, and the §17
omission shape — a packet-named path with no production caller, green
everywhere because nothing asked which command runs it.

## `fn a_created_run_hands_itself_to_the_loop()` › `crate::workspace_manager::fixture::git(&fixture.repo, &["init", "-q", "-b", "main"]);`

A real `.git`, because the worktree lock lives inside it. This fixture
builds run directories rather than repositories, so nothing else here has
needed one.

## `fn a_created_run_hands_itself_to_the_loop()` › `assert!(`

The digest a created run hands over is the one P6 committed. Without it
the loop's appends cannot report a creator disposition — `EMIT-002`'s
defect on the resumed path, arriving on the created one.

## `fn the_publication_prefixes_run_in_the_packets_order() {`

committed.

## `fn the_publication_prefixes_run_in_the_packets_order()` › `let calls = probes.calls();`

P4: the shell probe, then the agent probe, in that order.

## `fn the_publication_prefixes_run_in_the_packets_order()` › `assert!(fixture.public().join(PLAN).is_file(), "P5 wrote the plan");`

P5, P5b, P6, P7, P8, on disk.

## `fn the_publication_prefixes_run_in_the_packets_order()` › `let private = names_in(&fixture.private());`

The private skeleton exists, and so does the record that had to
precede it.

## `fn the_publication_prefixes_run_in_the_packets_order()` › `assert_eq!(`

The run the caller is handed: the exact bytes on disk, folded once.

## `fn the_publication_prefixes_run_in_the_packets_order()` › `const PUBLICATION_ORDER: &[EffectSiteId] = &[`

Every publication site of the sequence was driven through its funnel,
**in `run_creation`'s order** — which is what this test is named for and
what a membership test cannot say. `RunDir.CreatePrivateDir` runs six
times (P3a, then once per private skeleton directory) and is recorded at
its first; sites this order does not own are filtered out rather than
listed, so `Lock.ProbeCleanupExclusive` staying inside P2's acquisition is
P2's business and not this assertion's.

## `fn the_publication_prefixes_run_in_the_packets_order()` › `let mut started = started;`

And the run comes apart into the four things the loop drives.

## `fn run_started_records_runner_policy_resolved_before_worktree_lock() {`

INV-23: `run_started(4).runner` is the policy resolved **before the
worktree lock**, not the one the caller handed in.

The caller's record names a container boundary; the pre-lock checks
resolved the host. Four copies of one record have to agree — the
marker's digest (P1), `owner.json.runner` (P3b), the probes' boundary
(P4), and `run_started(4).runner` (P6) — and all four are asserted
against the witness rather than against each other, so a stamp that took
the *caller's* value would agree with itself and fail here.

## `fn run_started_records_runner_policy_resolved_before_worktree_lock() {` › `driver.runner = container_policy();`

What the caller claims, and what pre-flight actually resolved, differ.

## `fn run_started_records_runner_policy_resolved_before_worktree_lock() {` › `hooks.faults().arm_phase(`

The marker is read before P7 removes it, so the run is stopped at P6.

## `fn creator_error_at_p3a_retains_both_halves_and_reports_them() {`

=======================================================================
The deletion boundary
=======================================================================

## `fn creator_error_at_p3a_retains_both_halves_and_reports_them() {`

**P3a: the creator removes neither half.**

ST-19, verbatim: "the private directory exists without an owner record —
unprovable — so both halves are retained and reported (content-free by
ordering; deferred prune)". The named sibling of
`creator_error_before_commit_record_removes_both_halves`, which is scoped
to P3b-P5 precisely because this prefix answers differently.

Two windows, and both retain. The tree labels `stage_owner_record` P3a
and `publish_owner_record` P3b, while ST-19's P3a is the
private-directory-without-owner-record *prefix* — so "P3a" spans before
staging (empty, content-free) and after it (holding `owner.json.tmp`,
which is **not** content-free, and the report says so).

Removing the public half here instead would orphan the private one
permanently: the only production `read_dir` over a runs root is
`rundir::run_dir_names` over `<repo>/.upstroke/runs`, and the private half
is reachable only through the marker inside the public husk. The second
half of this test is that assertion.

## `fn creator_error_at_p3a_retains_both_halves_and_reports_them() {` › `(`

The staging file never landed: an empty private half, and P3a.

## `fn creator_error_at_p3a_retains_both_halves_and_reports_them() {` › `(`

The **same site**, failing after its primitive — which is what a
failing fsync inside `stage_json` leaves, because it creates the
`.tmp` and writes it before it syncs. The residue is `owner.json.tmp`,
so the prefix the operator is given has to be P3a (staged): a private
half holding a staging file is retained and is **not** content-free,
and the two are separate names for exactly that reason.

## `fn creator_error_at_p3a_retains_both_halves_and_reports_them() {` › `let husk = fixture.husk();`

And the census reaches the same answer, which is the convergence
property: a kill here and an error here leave one shape.

## `fn creator_error_before_commit_record_removes_both_halves() {`

P3b-P5, and the P5b case where a read-only stat shows the record absent:
the creating process, which holds both locks and knows the run never
committed, removes both halves.

## `fn creator_error_before_commit_record_removes_both_halves()` › `("plan", RunDirSite::WritePlan, HookPhase::Before),`

P3b-P5: the plan write fails after the owner record exists.

## `fn creator_error_before_commit_record_removes_both_halves()` › `("stage", RunDirSite::StageCommitRecord, HookPhase::Before),`

P5b with the record absent: the staging file never landed.

## `fn failing_preflight_probe_at_p4_removes_both_halves() {`

A failing pre-flight probe at P4 is a returned error **before P5b**, so
it removes both halves.

And the shell probe goes first: a machine whose shell does not run
`exit 0` never spends a slot on an agent.

## `fn failing_preflight_probe_at_p4_removes_both_halves()` › `let fixture = Fixture::new("probe-agent");`

(1) The agent probe fails, after the shell probe ran.

## `fn failing_preflight_probe_at_p4_removes_both_halves()` › `let fixture = Fixture::new("probe-shell");`

(2) The shell probe fails, and no agent is probed at all.

## `fn commit_record_rename_error_with_record_absent_removes_both_halves() {`

A `PublishCommitRecord` error whose rename **never landed**: the stat
says absent, so both halves go.

## `fn commit_record_rename_error_with_record_absent_removes_both_halves() {` › `hooks.faults().arm_phase(`

`Before`: the funnel returns `Err` without performing the rename.

## `fn commit_record_rename_error_with_record_present_treated_as_published_removes_nothing() {`

A `PublishCommitRecord` error whose rename **did** land: the stat says
present, the run is treated as published, and nothing is deleted.

This is the error-return mode's whole point — the funnel returns `Err`
*after* performing the primitive — and it is why the boundary is decided
by a stat rather than by the error, which is the identical value on both
sides of the rename.

## `fn creator_error_after_commit_record_present_removes_nothing_and_reports_possibly_committed() {`

The same crossing, reported: a retained, possibly committed husk that the
census classifies identically and the deferred prune is the only path to.

The sibling above asserts the **tree**; this asserts the **report** and
the census's agreement with it, which is the convergence property the
packet states ("the census later classifies the same shapes
identically").

## `fn a_failed_private_half_removal_keeps_the_public_half_that_names_it() {`

**The public half goes only if the private one went.**

`remove_public_husk` deletes `<public>` **including `.creating`**, and that
marker is the private half's only locator: the only production `read_dir`
over a runs root is `rundir::run_dir_names` over `<repo>/.upstroke/runs`, and
nothing enumerates `<R>/runs`. So a `RunDir.RemovePrivateHusk` that returns
an error must not be followed by the public removal — the private half would
survive at `<R>/runs/<run_id>` with no husk naming it, and no census, no
`status` and no `upstroke runs prune` could ever reach it again. It is the
one shape in this module that no later pass can repair.

**Three** error windows, because `remove_dir_all`'s error does not say which
one it is. `Before` is the removal that never ran; `After` is the removal
that ran and then returned `Err`; and the third — the window the arm exists
for — is the one an unwritable parent or a Windows handle on the directory
itself leaves: **every child removed and the directory not**.

The three do not converge the same way, and the report must not say they do.
The first two are finished by a later reclaim — the pair is still provable
when the private half survived, and the marker's target is absent when it did
not. The third is finished by being **reported**: the marker's target exists
with no `owner.json` in it, so the proof answers `OwnerRecordMissing` and the
census retains both halves for the deferred prune. What all three share, and
what the short-circuit is actually for, is that none of them orphans
anything.

The third row is **planted**, not injected. `Injection::Error` is all or
nothing at a phase boundary, so no arming can produce a half-removed
directory; and this is a `TOPOLOGY_MODULE`, where the only deletions a test
can reach are `RunDir.RemovePublicHusk` and the proof-token funnel, neither
of which empties a directory without removing it. So the row runs the `After`
window — the same funnel, the same error, the same `Disposition` — and then
re-creates the private directory through `RunDir.CreatePrivateDir`, which
leaves the byte-for-byte shape a partial `remove_dir_all` leaves. The claim
under test is what the **next census** does with that shape, and the census
reads the disk, not the history.

## `fn a_failed_private_half_removal_keeps_the_public_half_that_names_it() {` › `hooks`

P5: past the owner record, so the proof holds and the creator is
entitled to remove both halves, and before the deletion boundary, so
it is required to.

## `fn a_failed_private_half_removal_keeps_the_public_half_that_names_it() {` › `let Disposition::PrivateHalfRemovalFailed {`

The report says what happened, not a condition nobody observed. The
owner record is present on the surviving-half window — the proof that
minted the spent token read it — so `OwnerRecordMissing` was a false
statement about the tree and not merely an imprecise one.

## `fn a_failed_private_half_removal_keeps_the_public_half_that_names_it() {` › `assert!(`

The three questions, and the point of there being three. A failed
removal completed nothing, so `removed_anything` is `false` — the same
answer `Retained` gives — and what separates it from a retention is
the epistemic predicate: the tree may have been emptied on the way to
the error. Answering `removed_anything` `true` here made this arm
indistinguishable from `PublicHalfRemoved`, whose public half is gone
and whose private half never existed.

## `fn a_failed_private_half_removal_keeps_the_public_half_that_names_it() {` › `if plant_partial {`

The third window: every child gone, the directory not. Planted here
rather than injected, for the reason the doc comment gives.

## `fn a_failed_private_half_removal_keeps_the_public_half_that_names_it() {` › `assert_eq!(`

And all three shapes are ones a later pass finishes: the first two by
reclaiming, the third by retaining and reporting for the deferred
prune. None of them is unreachable.

## `fn a_failed_public_half_removal_is_best_effort_and_converges() {`

The public removal is best-effort — and its two windows are the two shapes
a best-effort removal can leave.

`After` is the removal that ran and then returned `Err`: both halves really
are gone and the error is swallowed, so the run is reported with the error
that stopped it rather than with a second one about the cleanup. `Before` is
the removal that never ran: the private half is gone, the public husk
survives carrying a marker whose target is absent, and the next census
reclaims it public-only. Nothing is orphaned in either — the public half
needs no proof and no locator.

## `fn a_failed_public_half_removal_is_best_effort_and_converges() {` › `assert!(`

Best-effort: the operator is given the error that stopped the run,
not the one the cleanup hit on the way out.

## `fn a_failed_public_half_removal_is_best_effort_and_converges() {` › `let enumerated = crate::rundir::run_dir_names(&fixture.repo);`

`run_dir_names`, not `list_runs`: the survivor is a husk, and
`list_runs` answers only for committed runs. `run_dir_names` is the
enumeration the census walks, so it is the one that says whether the
next census can still see this directory.

## `fn a_failed_public_half_removal_is_best_effort_and_converges() {` › `assert_eq!(`

The survivor needs no proof and no locator: its marker names a
private half that is no longer there.

## `fn create_public_dir_failing_creates_no_run_directory_and_removes_nothing() {`

`RunDir.CreatePublicDir` itself failing: no run directory came to exist, and
there is nothing for cleanup to decide.

The one prefix that returns before the stat, the proof and the lock release,
because there is no half to stat, nothing to prove about and no lock to give
back — `Prefix::Nothing` is the coordinate for exactly that.

## `fn a_commit_record_stat_that_cannot_answer_retains_both_halves_and_releases_the_lock() {`

The commit-record stat that **cannot answer**: fail-closed, retained,
nothing deleted — and the run lock still handed back through `Lock.Release`.

`commit_record_after_error` answers `Unknown` when `symlink_metadata` fails
with anything other than `NotFound`, and every caller treats `Unknown` as
`Present`: the cost of being wrong is asymmetric, because a retained husk is
reported until an operator prunes it and a deleted committed run is gone.

Driven through `stat_after_error` with a locator built to make the stat fail,
rather than through `create_run`. The portable way to fail a stat with
anything other than `NotFound` is an interior NUL, which both Unix and
Windows reject with `InvalidInput` before they touch the filesystem — and no
run can be created at such a path in the first place, so there is no
`create_run` that reaches this arm. A permission bit would be the production
shape and is neither portable nor available: `set_permissions` is on the
effect denylist, in tests too.

## `fn a_commit_record_stat_that_cannot_answer_retains_both_halves_and_releases_the_lock() {` › `assert!(`

The fixture's own precondition, asserted rather than assumed: an interior
NUL is rejected by `std` before either platform reaches the filesystem —
Unix's `CString::new` and Windows's `to_u16s` both answer `InvalidInput`,
and neither answers `NotFound`. Asserted here so a platform that ever
disagreed would say *the fixture* stopped working rather than leave the
arm below looking broken.

## `fn the_p1_staging_label_names_the_residue_the_stat_finds() {`

The P1 staging label names the residue, not the sub-step before it.

`stage_json` creates `.creating.tmp`, writes it, and **then** fsyncs, so one
`RunDir.StageMarker` error covers two trees: `Before` is the create that
never happened and leaves the directory bare, `After` is the sync that failed
and leaves the staging file. `P0` and `P1a` are separate names because the
census separates the shapes, so the report has to separate them too.

The witness is independent of the label: `UnboundShape` is computed by
`prove_private_half_ownership` from the tree, by production code that never
sees a `Prefix`, so the two agreeing is evidence and not a restatement.

## `fn a_leaked_probe_registration_is_reported_by_the_append_error() {`

=======================================================================
The append-error protocol
=======================================================================

## `fn a_leaked_probe_registration_is_reported_by_the_append_error() {`

**A leaked registration is reported, which is what proves the check reads the
ledger the probe registered into.**

"The probes' own ledger" is what this said, and probes have not had one since
`ledger()` and `slots()` left the trait on 2026-08-27: the `Request` owns the
single pair and hands it to each probe as an argument. The property is the
same and its name was a round out of date.

Its sibling above drives a balanced run, and a balanced run cannot tell two
ledgers apart: an empty one balances too. This is the discriminating half —
the pair the request holds carries an unsettled registration, so a check
reading any other would report the run clean and this assertion would fail.

## `fn a_leaked_probe_registration_is_reported_by_the_append_error() {` › `let probes = RecordingProbes::new(&host_digest());`

A probe that uses what it is handed, because P4 now refuses one that does
not: the leak this test needs is seeded by the owner of the pair below.

## `fn a_leaked_probe_registration_is_reported_by_the_append_error() {` › `driver.leak_into_the_pair(AGENT);`

Seeded by the owner of the pair, because a probe can no longer leak:
what it is handed settles what it registers. The property under test is
unchanged — the balance reads the granted pair — and the seam that
populates it moved to the only place that can.

## `fn the_append_error_balance_reads_the_ledger_the_probes_used() {`

**Creation's closing balance assertion reads the ledger P4 actually used.**

The append-error protocol reports whether the probes left anything unsettled,
and it reports it into a refusal an operator reads. That claim is only worth
something if the ledger it consults is the one the processes ran through.

It was not necessarily. `Request` carried its own ledger and slots beside a
`&dyn Probes` that carried another pair, and nothing required them to match —
so a caller could hand P4 locks A and the balance check empty locks B, and the
refusal would report a balanced run whatever A held. The round-4 review of
`09f9a99` set that construction out as a P1.

**The repair for that was wrong, and this comment claimed it worked.** Moving
the pair onto the `Probes` trait was said here to make a second pair
"unrepresentable" — a compile-time property. It is not: a trait exposing
`agent()` beside `ledger()` cannot force `agent()` to use what the accessor
returns, so an implementation could run through one pair and report another.
The `b1f54a5` review disproved it, and the claim is **retracted** rather than
restated a fourth time.

What holds now is narrower and true: `Request` owns the single pair and hands
it to each probe, and the trait has **no accessor at all** — so a probe has
nothing of its own to report. That is a property of the signature, and what
this test adds is the other half: the check reads a **populated** ledger
rather than an empty one it could never have failed on.

The previous witness went through `RunnerProbes::agent` directly and inspected
that wrapper's own locks; it never built a `Request` and never called
`create_run`, so it proved the wrapper and not the accounting. This drives
`create_run` to the forced first-append error.

## `fn the_append_error_balance_reads_the_ledger_the_probes_used() {` › `let source = OneSource::default();`

**The production probes, not a recording double.** A double that registers
nothing leaves an empty ledger, and a balance check over an empty ledger is
true for the wrong reason — which the premise assertion below refuses. This
is `RunnerProbes` over a runner that answers, so P4's registrations are
real and the balance is earned.

## `fn the_append_error_balance_reads_the_ledger_the_probes_used() {` › `let probes = RunnerProbes {`

No pair and no runner are built here: the `Request` grants the one pair,
owns the run's Runner, and builds each probe's boundary from the two — so
the witness reads `driver.ledger` below and `RunnerProbes` has nothing of
its own to read instead.

## `fn the_append_error_balance_reads_the_ledger_the_probes_used() {` › `let executed = driver.execution.requests().len();`

The premise: P4 ran, and **every process it ran** is accounted in the
granted pair. A count greater than zero is not enough — the shell probe
alone satisfies it, so a run whose *agent* probe was bound to some other
pair would pass. The claim is therefore the equality: the run's Runner
executed exactly the processes the pair settled.

## `fn the_append_error_balance_reads_the_ledger_the_probes_used() {` › `let text = format!("{}", refused.error);`

And that is what the refusal says. A balanced run adds **nothing** — only an
unbalanced one appends "(and this process still holds a registered
invocation…)" — so the assertion is the absence of that clause, which is
only meaningful because the premise above proved the ledger was populated
and did balance. An empty ledger would satisfy this line and fail that one.

## `fn append_first_error_after_partial_write_reopens_truncates_and_reports_not_committed_without_deletion()`

A partial write: the reopen truncates the torn first line, the proven
prefix has no committed line, and nothing is deleted.

## `fn append_first_flush_error_after_full_line_reports_by_replay_without_retry() {`

A full line then a flush error: the barrier's replay shows the line, the
run is reported committed, and the append is **never retried** — one
line, not two.

## `fn append_first_sync_error_reports_by_replay_and_never_deletes() {`

A sync error after the data reached the disk: reported by replay, and
nothing is ever deleted.

## `fn append_first_error_with_failed_prefix_sync_reports_undetermined_and_never_deletes() {`

The barrier's own sync fails: the outcome is **undetermined**, the step
is named, and nothing is deleted.

## `fn append_first_error_with_failed_prefix_sync_reports_undetermined_and_never_deletes() {` › `hooks.faults().arm_point_after(`

Skip P5's own open, which consults the same coordinate.

## `fn foreign_integration_ref_refused() {`

=======================================================================
P7 and P8
=======================================================================

## `fn foreign_integration_ref_refused() {`

A ref that is symbolic, checked out, or at another SHA refuses — and the
refusal deletes nothing, because the run already exists.

## `fn foreign_integration_ref_refused()` › `let fixture = Fixture::new("ref-foreign");`

(1) At another SHA.

## `fn foreign_integration_ref_refused()` › `let fixture = Fixture::new("ref-checkedout");`

(2) Checked out in a worktree.

## `fn p7_error_leaves_run_started_durable_with_no_integration_ref() {`

The creator's half of the P7/P8 claim: a P7 failure leaves `run_started`
durable, the marker still on disk, and **no** integration ref.

This is the prefix, produced by the creator itself rather than assembled —
and it is only half of
`transaction_fault_matrix[T-RUNSTART].resume_action`'s "P7/P8: create the
ref zero-old at the recorded base if absent; if present == base continue
(**no spend repeats**)". The other half is what a *resume* does about it, and
it is
`recover::tests::kill_after_run_started_creates_integration_ref`, which
drives [`super::super::recover::run_recovery_order`] over exactly this shape.

It used to be one test, and the second half of it called
[`ensure_integration_ref`] directly — which proved that the *function*
creates and adopts, and could not prove that any resume ever calls it, with
what arguments, or at what point in the order. The resume-side test proves
all three, so this one keeps the claim it can actually make.

## `fn p7_error_leaves_run_started_durable_with_no_integration_ref() {` › `hooks.faults().arm_phase(`

Stop at P6: `run_started` is durable, the marker is present, and no
ref exists — exactly what a kill between P6 and P8 leaves.

## `fn the_p8_report_promises_exactly_the_resume_action_the_resume_performs() {`

The `Committed { stale_marker: false }` sentence promises exactly the resume
action the resume performs — no more, and no less.

This arm has now had three sentences. The first promised a stale-marker
repair for a marker P7 had already removed. The second promised that "the
resume creates the integration ref zero-old at the recorded base" while no
code did that. The third — this one — says the same words, and now
[`super::super::recover::ensure_recorded_integration_ref`] is the step that
performs them.

So the check is not on the words alone, and it is not one-directional
either. It reads the resume module's own **production code** — comments and
string literals blanked, `#[cfg(test)]` items removed, so a mention in prose
or in a test cannot satisfy it — and asserts the **biconditional**: the
sentence promises the action if and only if the resume calls P8's body.
Deleting the caller fails it in one direction; deleting the promise fails it
in the other; and the pair can only be made green together, which is the
property that was actually wanted both times it was got wrong.

## `fn the_p8_report_promises_exactly_the_resume_action_the_resume_performs() {` › `let from = resume`

Read out of the recovery **driver's own body**, not out of the module.
"The module defines a function that would create the ref" is exactly the
state this tree was in when the sentence was wrong the second time: the
step existed as a body and the order never called it. So the window is
`run_recovery_order`'s, and it ends at the next item.

## `fn the_p8_report_promises_exactly_the_resume_action_the_resume_performs() {` › `assert_eq!(`

And the step, wherever it is called from, may not carry a second copy of
"if present == base continue": it has to be P8's own body.

## `fn committed_run_with_stale_marker_listed_and_repaired_by_resume() {`

A committed run whose marker is still there is **listed** by the readers
and repaired by the resume's step (a).

## `fn committed_run_with_stale_marker_listed_and_repaired_by_resume() {` › `hooks.faults().disarm();`

The repair: recovery step (a) removes the stale marker through the
same funnel P7 uses, and the run is unchanged otherwise.

## `fn torn_first_line_without_commit_record_reclaimed_and_with_commit_record_retained() {`

A torn first line is reclaimed without a commit record and retained with
one — the same bytes, two answers, decided by the boundary and nothing
else.

Both husks are built through the funnels rather than by `create_run`,
because the creator publishes `committed.json` **before** it appends: the
no-record half of this shape is a husk some other writer left, and the
census has to classify it either way.

## `fn torn_first_line_without_commit_record_reclaimed_and_with_commit_record_retained() {` › `let fixture = Fixture::new("torn-norecord");`

(1) No commit record: provable, and reclaimed.

## `fn torn_first_line_without_commit_record_reclaimed_and_with_commit_record_retained() {` › `let fixture = Fixture::new("torn-record");`

(2) With one: retained, possibly committed, and no token exists.

## `fn torn_first_line_without_commit_record_reclaimed_and_with_commit_record_retained() {` › `let fixture = Fixture::new("torn-kill");`

(3) And the same shape left by a real process death inside the
append, rather than by an error return: `WrittenShape::Torn` is
reachable only through the observer's `written_kill_shape`, so
without that override this prefix has no kill at all.

## `fn plant_husk_with_a_torn_first_line(`

P0-P3b through the funnels, optionally P5b, then an append that returns
an error after a partial write.

## `const KILL_PREFIXES: &[(&str, Prefix)] = &[`

=======================================================================
Kills — a real process death, at every prefix
=======================================================================

## `const KILL_PREFIXES: &[(&str, Prefix)] = &[`

Which prefix a kill child stops at, and what a census must then say.

The table **is** the convergence claim: every entry names a prefix of
`run_creation`'s sequence and the answer ST-19's `resume_action` gives
for it, and the child reaches the prefix by dying inside a funnel rather
than by returning early — an early return unwinds, and what is under test
is what a coordinator that runs **no** cleanup leaves on disk.

## `fn drive_into_the_kill(which: &str, fixture: &Fixture) -> ! {`

Arm `which` on a fresh bundle and drive `create_run` into it.

Shared by the child of every kill test so a prefix is reached by exactly
one description.

## `fn drive_into_the_kill(which: &str, fixture: &Fixture) -> !` › `hooks.faults().arm_phase(`

`After`: the rename happened and nothing has been appended,
which is exactly the P5b prefix.

## `fn drive_into_the_kill(which: &str, fixture: &Fixture) -> !` › `hooks.faults().tear_the_first_line(EventSite::AppendFirst);`

The other durable shape of a kill inside the first append:
a torn first line, past the commit record. `WrittenShape` is
an observer's answer and nothing else can produce one, which
is why the bundle overrides `written_kill_shape` at all.

## `fn drive_into_the_kill(which: &str, fixture: &Fixture) -> !` › `assert!(outcome.is_ok(), "P8 must have been reached");`

Everything is done; the kill is what a coordinator dying with the
run complete leaves, which is the whole of P8's durable shape.

## `fn spawn_and_wait(child: &str, root: &Path, site: &str, ordinal: u32) -> Option<i32> {`

Spawn `child` with the two env vars and wait for it to die.

Through the `Runner`, because `std::process::Command` is a build error in
a `TOPOLOGY_MODULE` — the process funnel is `Process.Spawn` and every
process start goes through it, tests included.

## `fn kill_at_each_prefix_p0_to_p8_converges() {`

A kill at every prefix of P0-P8 leaves a shape the census classifies as
ST-19's `resume_action` says, and an error return at the same prefix
leaves the same one.

`Injection::Kill` is `std::process::abort()` — a real process death,
chosen so the claim is what a coordinator that runs **no** cleanup leaves
behind. An early `return` would unwind and prove something weaker.

## `fn kill_at_each_prefix_p0_to_p8_converges()` › `assert!(`

Every row of this table is P0 or later, so the public directory
exists — and saying so is what stops the `p0` row passing with
nothing on disk at all. `classify_run_dir` calls a **missing**
directory a `Husk` and `husk_report` calls one
`NothingBound(Bare)`, which is exactly the pair `p0` expects, so a
child that panicked before `create_public_dir` ever ran would be
indistinguishable from one that died after it.

## `fn expected_disposition(prefix: Prefix) -> &'static str {`

ST-19's `resume_action`, as a function of the prefix.

## `fn expected_disposition(prefix: Prefix) -> &'static str` › `Prefix::P0 => "public-only:bare",`

"P0-P1: the next write command's census reclaims the bare or
staged-only public directory (no private half exists by
ordering)".

## `fn expected_disposition(prefix: Prefix) -> &'static str` › `Prefix::P1 | Prefix::P2 => "public-only:target-absent",`

"P1-published and P2: the marker's private target does not exist,
so the public husk alone is reclaimed".

## `fn expected_disposition(prefix: Prefix) -> &'static str` › `Prefix::P3a | Prefix::P3aStaged => "retained:owner-record-missing",`

"P3a: the private directory exists without an owner record —
unprovable — so both halves are retained and reported".

## `fn expected_disposition(prefix: Prefix) -> &'static str` › `Prefix::P3b | Prefix::P4 | Prefix::P5 => "both-halves",`

"P3b-P5: the ownership proof passes ... and the census reclaims
the private half through the proof-token funnel, then the public
directory with the marker last".

## `fn expected_disposition(prefix: Prefix) -> &'static str` › `Prefix::P5b => "retained:possibly-committed",`

"P5b ...: committed.json exists, so both halves are retained and
reported as possibly committed with nothing deleted".

## `fn kill_between_commit_record_and_run_started_retained_as_possibly_committed() {`

The one window `run_creation` says holds no other step: `committed.json`
published, `run_started` not yet durable.

A kill here is indistinguishable from a kill *inside* the append, and
both are retained as possibly committed with nothing deleted — which is
why no separate never-entered proof exists.

## `fn worktree_lock_child() {`

=======================================================================
The worktree lock
=======================================================================

## `fn worktree_lock_child()` › `let outcome = match crate::rundir::WorktreeLock::acquire_in(&repo, &git_dir) {`

The evidence is a directory rather than a printed line: `println!` is
a build error in a `TOPOLOGY_MODULE`, and a directory created through
the run-directory funnel is a durable answer the parent can stat.

## `fn creator_versus_census_serialized_by_worktree_lock() {`

Two write commands in one worktree are serialized by the worktree lock:
the second is refused rather than racing the first's creation.

A second **process**, because the process-local claim table would refuse
a second acquisition in this process whatever the OS lock did, and it is
the OS lock that serializes a creator against a census.

## `fn creator_versus_census_serialized_by_worktree_lock()` › `let code = spawn_and_wait(`

The control half: with nothing holding it, the child acquires.

## `fn creator_versus_census_serialized_by_worktree_lock()` › `let held = crate::rundir::WorktreeLock::acquire_in(&fixture.repo, &git_dir)`

Held by this process — the creator — the child is refused.

## `fn creator_versus_census_serialized_by_worktree_lock()` › `let child_root = fixture.root.join("second");`

The same git dir: the lock is repository-scoped, not run-scoped.

## `fn image_absent_refused_before_any_lock_no_lock_file_created() {`

=======================================================================
Refused before any lock
=======================================================================

## `fn image_absent_refused_before_any_lock_no_lock_file_created() {`

An image reference absent from the runtime refuses **before any lock**,
and the assertion is that no R25 lock file was created — not merely that
the command refused.

`Lock.CreateWorktreeLockFile` is a site of its own precisely because the
file "spans runs; never removed by a run": once it exists, the refusal
has left a durable artifact behind, and "before any lock or **other
effect**" is false.

## `fn image_absent_refused_before_any_lock_no_lock_file_created() {` › `let refused = write_command_start(&fixture, &selection, &git_dir, &Inventory::reachable())`

The write command's start, in order: the read-only checks, then the
lock. A refusal in the first statement never reaches the second.

## `fn image_absent_refused_before_any_lock_no_lock_file_created() {` › `let present =`

The control half. Without it this test passes against a
`write_command_start` that never takes a lock at all.

## `fn write_command_start(`

The two statements every schema-4 write command opens with: O01's
read-only checks, then O02's worktree lock.

## `type ContainerState = BTreeMap<String, (Liveness, BTreeMap<String, String>)>;`

=======================================================================
A container runtime with no daemon behind it
=======================================================================

## `type ContainerState = BTreeMap<String, (Liveness, BTreeMap<String, String>)>;`

A `ContainerRuntime` that answers from an in-memory inventory.

Written here rather than reached for: `runner::container::fake` is
`#[cfg(test)] mod fake` and is private to `runner::container`, so a
sibling subtree cannot name it. Implementing the trait is not calling
one of its methods, which is what the effect denylist forbids — every
body below is pure bookkeeping.

**No daemon, deliberately.** The container-shaped assertions here are
about the intent record and the census's boundary report, both of which
are files and values rather than anything Docker does.
Every container the fake runtime holds: its liveness and its labels.

## `impl Inventory` › `fn seed_running(&self, name: &str, labels: BTreeMap<String, String>) {`

Put a running container into the runtime, the way one survives the
process that started it.

## `const IMAGE_REFERENCE: &str = "ghcr.io/upstroke/sandbox:1";`

=======================================================================
Containerized probes
=======================================================================

## `fn container_execution(`

The probes of a containerized run: the real [`ContainerRunner`], over a
runtime with no daemon behind it.

[`ContainerRunner`]: crate::runner::container::exec::ContainerRunner
The container runner a containerized creation executes through.

**Built by the caller, because the caller owns the run's Runner.** It used
to live on [`ContainerProbes`], which then ran its processes through it and
ignored the capability P4 handed in — the in-tree instance of the
substitution the `6c6cb3d` review named, and the reason its closing balance
read a pair nothing had used. `hooks` is installed **on the runner**:
`Runner::run` takes no observer, so the container funnel's hooks are the
ones the runner was built with, and a bundle that only reached the funnel
through `TopologyHooks::container` would arm nothing a probe executes.

## `struct ContainerProbes {`

The containerized `Probes`: the shell probe, through the capability P4
hands it, over the container runner the caller owns.

## `impl Probes for ContainerProbes` › `crate::runner::host::run_shell_probe(`

**Through the capability, over the caller's container runner.** This
double drives a real containerized process to its death, and it does
so on the boundary production uses — which is what makes the residue
it leaves behind the residue a real run would leave.

## `fn container_probe_kill_child()` › `hooks.faults().arm_phase(`

A real process death **inside** the containerized probe, after
`Container.WriteIntent` and `Container.Create`: the durable residue is
an intent record and a container nobody owns.

## `fn intents(`

Every intent under `<R>/containers`, with the name it was filed as.

## `fn probe_intent_carries_runner_policy_digest_matching_owner_record() {`

INV-23: "carried as a digest by every container intent", and the digest
is the run's — the one the owner record spells out in full.

Asserted against `sha256(owner.json.runner)` rather than against the
value the run was constructed with, so a probe executing under some
other boundary is caught rather than agreeing with itself. The other
half of the claim — that P4 refuses when the two disagree — is the
second block.

## `fn probe_intent_carries_runner_policy_digest_matching_owner_record() {` › `let fixture = Fixture::new("intent-digest-refusal");`

And P4 refuses before the first probe when the probes' boundary is
not the one P1 and P3b published.

## `fn kill_during_containerized_probe_before_run_started_reclaims_container_and_husk_and_reports_boundary()`

A kill during a containerized probe, before `run_started`: the next
census reclaims the container **and** the husk, and reports the
container's boundary from the intent's `runner_policy_sha256`.

## `assert_eq!(`

The durable residue of the kill: the husk, and an owned intent.

## `let runtime = container_runtime();`

Step (a): the container outlives the process that started it, so a
fresh runtime is seeded with it exactly as `docker ps` would report.

## `let husk = fixture.husk();`

Step (b): the husk. The proof passes — the creator published both
records and never crossed P5b — so both halves are reclaimed, private
first and the public directory with the marker last.

## `struct DiscardOnly;`

A `GitView` that materialises nothing and discards anything.

The census only ever discards, and the R19 directory the child left is
not what this test is about.

## `struct RecordingRunner {`

===========================================================================
The production `Probes`
===========================================================================

## `struct RecordingRunner {`

A `Runner` that records the request and answers exit 0.

## `struct FailsTheSecondRequest {`

A runner that serves the first request and refuses the second.

The shape a real probe fails in: a version probe answers, and the help probe
after it does not. What the ledger says about *which* process failed is the
whole of what `the_creation_ledger_accounts_every_probe_process` asserts.

## `struct TwoRequestAdapter;`

An adapter whose probe runs **two** processes, as every real one does.

## `fn the_creation_ledger_accounts_every_probe_process() {`

**Every Runner process a probe builds is its own registered invocation.**

`permits.protocol` asks for "registered/completed/cancelled **exactly
once**" per invocation, and `R3`'s subject is a *process*. Fresh creation
registered one `probe(agent, 0)` identity around the whole adapter call and
handed the adapter the raw Runner — so an adapter that runs ten processes,
which a current Codex probe does, put one of them in the ledger.

The consequence is not a missing row, it is a **wrong** one. With the version
probe at ordinal 0 succeeding and the help probe at ordinal 1 failing, the
creation ledger read `completed: 0, cancelled: 1` — it cancelled the identity
of the process that *succeeded*, and held no record at all of the one that
failed. The `bf927f3` review's third P1.

This asserts the counts, because the counts are what tell the two accounts
apart: one process accounted, or two.

## `fn the_creation_ledger_accounts_every_probe_process()` › `let slots = slots.lock().unwrap_or_else(PoisonError::into_inner);`

And the slot the failing process took was released, so a second agent
could still be probed after this one refused.

## `struct OneAdapter {`

One adapter, whose `probe` is the only method this seam reaches.

## `fn the_production_probes_run_both_halves_through_the_runs_runner() {`

The production [`Probes`]: both probes go through the run's own `Runner`,
and the shell probe carries the identity this module minted.

The seam has a test double everywhere else in this file, which is what makes
the *ordering* observable; this is the other half — that the implementation
production passes is the two existing functions and not a third one.

## `fn the_production_probes_run_both_halves_through_the_runs_runner() {` › `let refusal = probes`

An agent this machine has no adapter for refuses rather than being
silently skipped: a run whose pre-flight certified nothing would still
record it in `probed_agents`.

## `struct RunsThroughWhatItIsHanded {`

===========================================================================
The granted pair
===========================================================================

`PR7-G2-W1-PROBE-PAIR-NOT-OBLIGED` (§2, §22e). The claim was asserted three
times as a property of a signature and refuted three times; what is asserted
now is a property of the *types a probe receives*, and these are the
measurements of it.

## `struct RunsThroughWhatItIsHanded {`

A probes double that runs one request of each shape through whatever
boundary it is handed, and records what came back.

## `struct RunsThroughWhatItIsHanded` › `cross_the_paths: bool,`

When set, the agent probe runs a **shell** (non-slotted) identity on the
slotted path, and the shell probe runs an **agent** (slotted) identity
on the non-slotted one.

## `fn an_agent_probe_registers_only_into_the_pair_its_caller_bound() {`

**An agent probe registers into the pair its caller bound, and there is no
other pair for it to reach.**

The measurement the row asks for, and it needs two pairs to be worth
anything: a witness over one pair is true however many pairs exist. So a
**second** pair is granted here, over its own ledger and its own slots, and
handed to nobody. The probe registers; the caller's pair accounts it; the
second stays empty.

What this does **not** claim: that no `impl Probes` can hold a pair of its
own. One can — this file's `ContainerProbes` runs a real process through a
runner of its own — and the consequence is that it registers *nothing*,
which the caller's pair shows as an empty ledger rather than as a balance
read off the wrong locks. That distinction is the whole of what the
structural change buys, and it is asserted below rather than argued.

## `fn an_agent_probe_registers_only_into_the_pair_its_caller_bound() {` › `let other_ledger = Mutex::new(InvocationLedger::new());`

The pair nobody is handed.

## `fn the_two_probe_paths_refuse_each_others_identities() {`

**The shell probe's boundary holds no slots, and the agent probe's refuses
anything that takes none.**

INV-23's asymmetry — "one non-slotted shell probe … and one slotted probe
per recorded agent" — as two types rather than as one type and a comment.
The crossing double runs an agent identity on the shell path and a shell
identity on the agent path; both are refused, and neither leaves anything
behind.

## `fn the_shell_probe_registers_without_taking_a_slot() {`

**A shell probe that registers through its boundary takes no slot.**

The positive half of the pair above: the non-slotted probe really does run,
really is accounted, and R4 is never consulted for it — which is why its
capability can hold no slots at all.

## `fn a_probe_that_registers_nothing_does_not_move_the_grant() {`

**A probe that registers nothing cannot reach the closing balance at all.**

This used to assert the residue: an implementation was free to run nothing,
the granted pair stayed empty, and the balance passed *vacuously* — which is
the shape the `6c6cb3d` review refused to accept as closed. It is closed
now, and the assertion is the closure rather than the gap.

Driven at the seam rather than through `create_run`, because the point is
the arithmetic P4's check is made of: the grant count does not move, so
"returned `Ok`" and "executed through the run's boundary" are distinguishable
facts. `a_probe_that_substitutes_its_own_authority_is_refused_at_p4` is the
same claim end to end.

## `struct IgnoresTheCapability {`

A probes double that returns `Ok` having run nothing at all.

The shape the `6c6cb3d` review named: `Probes::shell`/`agent` could ignore
the capability they were handed and substitute an authority of their own —
or none — and creation would go on to publish a run whose closing balance
was clean because the pair it read had never been used. A balance over an
empty pair is true for the wrong reason.

## `struct IgnoresTheCapability` › `ignore_shell: bool,`

Which of the two seams ignores what it is handed.

## `struct IgnoresTheCapability` › `ignore_agent: Option<Option<String>>,`

The agent whose probe substitutes, or `None` for all of them.

Naming **one** agent is what makes the per-agent check load-bearing: a
grant check hoisted out of P4's loop is satisfied by the first agent
that ran and would certify every substituted probe behind it.

## `struct IgnoresTheCapability` › `elsewhere: RecordingRunner,`

Somewhere else to run, standing in for a probe that substitutes its own
boundary.

## `impl IgnoresTheCapability` › `fn only_the_agent(agent: &str) -> Self {`

Every agent probe runs through the capability except `agent`.

## `impl Probes for IgnoresTheCapability` › `self.elsewhere.run(&request).map(|_| ())`

Substituted: a real process, run through an authority the caller
never granted.

## `fn a_probe_that_substitutes_its_own_authority_is_refused_at_p4() {`

**A probe that substitutes its own authority is refused at P4, on either
seam.**

The obligation stops being documentary here. Passing the capability in
cannot make an implementation *call* it — the review's point, and the fourth
time a signature-level claim about this seam has been wrong — so the caller
checks what it granted: a probe that returns `Ok` having registered nothing
into the granted pair did not execute through the run's boundary, and P4
refuses rather than certifying it.

Both halves are driven, because a check that only watched the shell probe
would pass a run whose *agent* probe was the substituted one. Both leave the
run removed rather than published: an uncertified pre-flight has no run.

## `fn a_probe_that_substitutes_its_own_authority_is_refused_at_p4() {` › `assert_eq!(`

The substituted authority really did run a process, so the refusal is
about *where* it ran and not about a probe that did nothing.

## `fn a_probe_that_substitutes_its_own_authority_is_refused_at_p4() {` › `let fixture = Fixture::new("substituted-second-agent");`

**And the check is per agent, not per run.** The first agent's probe uses
the capability, so a check hoisted out of P4's loop would see the grant
move and certify the second one behind it. Two agents are the smallest
fixture that can tell the two placements apart.

## `struct RefusesEveryRequest {`

A `Runner` that refuses every request it is given.

## `fn the_grant_counts_a_probe_process_that_started_and_failed() {`

**The grant counts what was registered, not what succeeded.**

P4 asks "did this probe execute through the boundary this run granted it",
and a process that started and failed answers yes. Counting completions
instead would call a probe whose CLI refused a probe that never ran, and the
refusal it produced would say "registered nothing" about a probe that
registered — a false statement in the one place an operator reads to find
out what happened.

The two halves of the pair answer different questions and both are asserted:
the grant moved, and it still balances, because `Registering` cancels what
it registered on the failure path too.

## `fn the_granted_pairs_balance_answers_for_the_ledger_and_the_slots() {`

**The closing balance consumes both halves of the binding.**

`permits.protocol` asks for "registered/completed/cancelled exactly once"
and R4 for a pair given back; `ProbePair::balances` is the conjunction, and
the slot half had no witness at all — the check was two field reads at the
call site and every fixture that reached it had an empty `SlotAssertion`, so
deleting the R4 conjunct changed nothing anybody could see.

Each half is driven on its own, so a balance that answered on one of them
fails here rather than passing on the other's evidence.

## `fn the_granted_pairs_balance_answers_for_the_ledger_and_the_slots() {` › `let id = PreflightIdentities::agent(AGENT, 0).expect("a probe identity");`

The R3 half: a registration that never settled.

## `fn the_granted_pairs_balance_answers_for_the_ledger_and_the_slots() {` › `slots`

The R4 half: a pair taken and not given back. The ledger is clean
throughout, so only the slot conjunct can answer.

## `fn the_deletion_boundary_falls_between_p5_and_p5b() {`

The vocabulary the report and the fault registry share.

`Prefix::ALL` is the closed set a suite is measured against, and
`is_past_the_deletion_boundary` is the **ordering** claim beside the stat —
never in place of it, because a `PublishCommitRecord` error is the same value
on both sides of the rename.

## `fn the_deletion_boundary_falls_between_p5_and_p5b()` › `let removed = Disposition::BothHalvesRemoved {`

And the **three** questions a report answers about what is on disk. Two
alethic — did a reclaim complete, is the private half known gone — and one
epistemic, which is the only one a failed removal can answer `true` to.

## `fn the_deletion_boundary_falls_between_p5_and_p5b()` › `let failed = Disposition::PrivateHalfRemovalFailed {`

A removal that returned an error decided nothing: `remove_dir_all` is not
atomic, so the arm may claim neither that the tree is untouched nor that
the private half is gone.

## `fn the_deletion_boundary_falls_between_p5_and_p5b()` › `let answers = |disposition: &Disposition| {`

The pair the third predicate exists for. These two trees are opposite —
the public half gone with nothing private ever bound, against a public
half deliberately on disk with a private half in an unobserved state — so
no two of the predicates may answer them the same way.

## `fn the_deletion_boundary_falls_between_p5_and_p5b()` › `assert_eq!(`

And it is the *new* question that separates them: without it the two arms
are one answer.

## `fn the_deletion_boundary_falls_between_p5_and_p5b()` › `let stale = Disposition::Committed { stale_marker: true };`

Finding 5's pair: the two committed shapes are one variant and two
sentences, and only one of them promises a marker repair.
