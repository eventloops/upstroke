# `src/runner/container/runtime.rs`

Extended notes for [`src/runner/container/runtime.rs`](../../../../src/runner/container/runtime.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The container-runtime seam, and the ordered trace every obligation in this
slice is asserted against.

#### Why a seam at all

`decisions.tests_acceptance.determinism` requires "a fake container runtime
with owner labels, incarnations, liveness simulation, an image table keyed
by immutable id with references and digests, a mutable tag table …, an
availability toggle for ST-16 and ST-20 **plus Docker-gated real runs**".
Two implementations of one contract is what that sentence asks for, so the
contract is a trait and neither implementation is the definition.

#### The operation list is derived from obligations, not from Docker

Every method here exists because some live passage cannot be held without
it. The mapping is written out in [`RuntimeOp`], one arm per method, so a
method added later has to say which obligation it serves.

#### Reachability is not one boolean, and that is a decision

`crash_reconstruction`: "the container runtime is required only when an
intent exists or a labeled container is discoverable: if any intent exists
and the runtime cannot be reached the write command refuses (it cannot
prove those containers terminated), and with no intent and no reachable
runtime it proceeds".

A runtime that answers `docker ps` and fails `docker inspect` is a real
state — a daemon under load, a partially broken socket, a `ps` served from
cache. If reachability were one boolean taken once, such a runtime would
classify as *reachable*, the write command would proceed past the refusal
point, and the failure would arrive later — after "before any recovery
event", which is precisely the predicate the refusal exists to hold.

So reachability is **per operation**: every fallible method returns
[`RuntimeError`], which distinguishes [`RuntimeError::Unreachable`] from
[`RuntimeError::Failed`] and names the [`RuntimeOp`] that could not be
reached. [`ContainerRuntime::probe`] exists as the cheap up-front question,
but no caller may treat its answer as a promise about a later operation —
and the fake can make `ListByLabel` reachable while `InspectImageById` is
not, so the mixed state is constructible rather than merely conceded.

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

## `pub enum RuntimeOp {`

---------------------------------------------------------------------------
The operations, and why each exists
---------------------------------------------------------------------------

## `pub enum RuntimeOp {`

One operation of the runtime seam.

The discriminant is not decorative: [`RuntimeError`] names it, so
"unreachable" is always a statement about a specific question, and the fake
arms unreachability per operation.

## `pub enum RuntimeOp` › `Probe,`

The cheap up-front reachability question. `crash_reconstruction`: "the
container runtime is required only when an intent exists or a labeled
container is discoverable".

## `pub enum RuntimeOp` › `InspectImageByReference,`

Resolve a **reference** to its immutable id and its manifest digest.
`pr_sequence[7].scope`: "image reference already present in the runtime
— no implicit pull — its immutable image id and manifest digest when
reported".

## `pub enum RuntimeOp` › `InspectImageById,`

Resolve an **id**. A different question from the reference, and the
rebuild path asks only this one: "refuse … when … the **recorded image
id** is absent from the runtime". A seam that could only ask about the
reference could not express that refusal.

## `pub enum RuntimeOp` › `InspectVolume,`

`pr_sequence[7].scope`: "per-agent credential volume names present by
**volume inspection**".

## `pub enum RuntimeOp` › `ListByLabel,`

`crash_reconstruction`: "docker ps by `upstroke.private_root`". Discovery
returns names **and labels**, because a labeled container without an
intent is classified from its labels alone.

## `pub enum RuntimeOp` › `Observe,`

The reclaim sequence's middle step: "wait until observed
exited/removed".

## `pub enum RuntimeOp` › `Collect,`

The invocation's result — exit status, stdout, stderr — for a
[`crate::agent::ProcessOutput`].

## `pub enum RuntimeOp` › `Create,`

`INV-23`: "every container of every epoch is **created from the
recorded image id**". The result reports the id the runtime actually
used; see [`CreatedContainer::reported_image_id`].

## `pub enum RuntimeOp` › `Start,`

`docker start`, after the reported id has been verified.

## `pub enum RuntimeOp` › `Stop,`

Completion's `docker stop` and reclaim's `docker kill`, distinguished
by [`StopMode`] and accounted to the one site the frozen inventory has
for both, `ContainerSite::Stop`.

## `pub enum RuntimeOp` › `Remove,`

`docker rm`. Idempotent and tolerant of already-gone.

## `impl RuntimeOp` › `pub const ALL: &'static [Self] = &[`

Every operation, in the order the seam declares them.

Written out rather than derived so an operation added later has to be
added here, and so a grid over operations is a grid over all of them.

## `impl RuntimeOp` › `pub const fn is_effect(self) -> bool {`

Whether the operation changes runtime state.

The four effectful ones are the four the Container funnel wraps; the
seven read-only ones are inspections and carry no site, because
`ContainerSite` has no inspection variant and this slice may not add
one.

## `impl RuntimeOp` › `pub const fn name(self) -> &'static str {`

The operation as it is written in a trace.

## `pub enum RuntimeError {`

What went wrong, and whether the runtime could be reached at all.

The distinction is the whole point: `crash_reconstruction` refuses a write
command when "any intent exists and the runtime **cannot be reached**",
which is not the same as an operation that reached the runtime and failed.

## `pub enum RuntimeError` › `Unreachable {`

The runtime could not be reached for this operation.

## `pub enum RuntimeError` › `Failed {`

The runtime answered, and the answer was a failure.

## `impl RuntimeError` › `pub const fn operation(&self) -> RuntimeOp {`

The operation that produced this error.

## `impl RuntimeError` › `pub const fn is_unreachable(&self) -> bool {`

Whether the runtime could not be reached.

## `pub struct ImageInspection {`

---------------------------------------------------------------------------
Values the seam exchanges
---------------------------------------------------------------------------

## `pub struct ImageInspection {`

What an image inspection reports.

**Not** [`crate::topology::events::ImageIdentity`], deliberately. The
recorded identity pairs the reference the *operator wrote* with the id the
runtime resolved; an inspection reports what the runtime holds, and its
`references` may be empty (an id with no tag), may be several, or may name
a tag the operator never wrote. Returning the recorded shape here would let
a resolver take its `reference` field from the runtime's answer instead of
from the config — which is the record's own oracle, and would make "the
recorded reference now names another image" unconstructible.

## `pub struct ImageInspection` › `pub id: String,`

The runtime's immutable image id.

## `pub struct ImageInspection` › `pub digest: Option<String>,`

The manifest digest, `None` when the runtime reports none. INV-23:
"digest (the manifest digest **when reported**)".

## `pub struct ImageInspection` › `pub references: Vec<String>,`

Every reference the runtime says resolves to this id.

## `pub enum Mount {`

One mount the container receives.

DESIGN.md:400: "A container receives only its role's one worktree mount; it
never receives the public log, sibling worktrees, or private artifacts", and
DESIGN.md:612 adds the read-only reviewer mount and the per-agent credential
volume ("persistent volumes, not ephemeral copies").

**Every writable surface a container has is one of these**, which is what
makes `expected_failures_refusals[5]` — "gate write outside mount fails" —
a statement with a decidable subject. [`CreateSpec::read_only_root`] closes
the container layer, so the mount list is the whole of what a container may
write; a scratch surface a shell needs is therefore a [`Mount::Tmpfs`] and
not an implicit hole. `PR6-CORRECTNESS-008` / `PR6-ENUM-005` are the entries
for what it cost when it was not.

## `pub enum Mount` › `Path {`

A host path, bound at `target`.

## `pub enum Mount` › `Volume {`

A named volume — R20, operator-owned, "never created or pruned by a
run".

## `pub enum Mount` › `Tmpfs {`

An ephemeral in-memory scratch surface, with no host source and no
name.

A container whose root filesystem is read-only still needs somewhere to
put a temporary file — `sh` here-documents, `git`'s own temporaries, an
agent CLI's cache — and the alternative to declaring it is leaving the
whole container layer writable, which is the defect this variant exists
to remove. It carries **no host path**, so nothing the container writes
here can reach the coordinator, and it dies with the container.

## `impl Mount` › `pub fn target(&self) -> &str {`

Where the container sees it.

## `impl Mount` › `pub const fn read_only(&self) -> bool {`

Whether the mount is read-only.

A tmpfs that could not be written would be a mount with no purpose, so
this variant has no flag rather than a flag that is always `false`.

## `pub struct CreateSpec {`

Everything `docker create` is given.

`image_id` and not a reference: INV-23's "every container of every epoch is
created from the recorded image id … so a moved reference cannot change what
executes". The type carries no reference at all, so a caller cannot create
from one by accident.

## `pub struct CreateSpec` › `pub name: String,`

The container's name. `upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>`.

## `pub struct CreateSpec` › `pub image_id: String,`

The **recorded immutable image id**.

## `pub struct CreateSpec` › `pub labels: BTreeMap<String, String>,`

The five `upstroke.*` labels.

## `pub struct CreateSpec` › `pub env: Vec<(String, String)>,`

The runner-owned base environment plus the adapter's overlay
(DESIGN.md:258). Composed by lane A; carried verbatim here.

## `pub struct CreateSpec` › `pub command: Vec<String>,`

The command line, from the `CommandSpec`.

## `pub struct CreateSpec` › `pub workdir: Option<String>,`

The child's working directory inside the container.

**Always `Some`.** DESIGN.md:118 gives a runner "cwd, mounts,
environment, supervision, and timeout", and a `None` here is a runner
that let the *image* pick the working directory — which is how a probe
and the attempt it certifies came to run in two different directories
(`PR6-CORRECTNESS-006`). `Option` survives because the field is the
runtime seam's and a caller outside this slice may have no opinion; the
container runner's [`super::exec::ContainerRunner::plan`] never produces
one.

## `pub struct CreateSpec` › `pub read_only_root: bool,`

Whether the container's own root filesystem is read-only.

`expected_failures_refusals[5]`: "**gate write outside mount fails**".
Without this the role bind mounts are correct and the write still
succeeds — into the container's writable layer — so the refusal the
contract states does not hold and only the weaker "the host is unharmed"
does. DESIGN.md:610 calls the container "the first mechanism in this
design that confines gate-executed repository code"; this field is that
mechanism, and [`Mount::Tmpfs`] is what a container still legitimately
needs to write.

A field rather than an unconditional argv flag so the obligation is
assertable from a [`CreateSpec`] on a machine with no container runtime
— including the Windows guest, which has none.

## `pub struct CreatedContainer {`

What `docker create` gives back.

[`Self::reported_image_id`] is the runtime's answer and **must never be
filled in from the request**. INV-23: "its reported image id is verified
equal to the record before it starts". A `create` that echoed its argument
would make `substituted_image_id_refused_before_start` unconstructible and
the suite green because the test could not be written.

## `pub struct CreatedContainer` › `pub reported_image_id: String,`

The image id the **runtime** says the container was created from.

## `pub struct DiscoveredContainer {`

A container discovered by label.

## `impl DiscoveredContainer` › `pub fn label(&self, key: &str) -> Option<&str> {`

One label's value.

## `pub enum Liveness {`

Whether a container is still running.

Three answers and not two: reclaim must "wait until observed
exited/**removed**", and a container that is gone is as terminated as one
that exited. Collapsing them would make a reclaimer that raced another
reclaimer report "cannot be observed terminated" and block admission, which
is the opposite of "two concurrent reclaimers converge".

## `impl Liveness` › `pub const fn is_terminated(self) -> bool {`

Whether this answer proves the container is no longer running.

## `pub enum StopMode {`

How a container is stopped.

Two dispositions, one site. `ContainerSite` is frozen with eight variants
and has exactly one for stopping, so both the completion path's `docker
stop` (`at_run_end`: "released on complete (**stop**/rm, …)") and reclaim's
`docker kill` ("reclaim = docker **kill** -> observe …") are accounted to
`ContainerSite::Stop`. The disposition travels as a value so a trace still
distinguishes them.

## `pub enum StopMode` › `Graceful,`

Completion or cancellation: ask it to stop.

## `pub enum StopMode` › `Kill,`

Reclaim: kill it.

## `impl StopMode` › `pub const fn name(self) -> &'static str {`

The disposition as it is written in a trace.

## `pub struct ContainerExecution {`

What a finished container did.

## `pub struct ContainerExecution` › `pub exit_code: Option<i32>,`

`None` when the process was signalled rather than exiting.

## `pub trait ContainerRuntime: Send + Sync {`

---------------------------------------------------------------------------
The seam
---------------------------------------------------------------------------

## `pub trait ContainerRuntime: Send + Sync {`

The container runtime, as this slice needs it.

`Send + Sync` for the same reason [`crate::runner::Runner`] is: PR11 holds
one of these across await points behind a `&dyn`.

**The four effectful methods are denied in `clippy.toml`.** Only
`src/runner/container.rs` — the module the frozen inventory names as the
Container funnel — is allowed to call them, so a lane cannot perform a
container effect without going through a funnel API that takes its site by
value. `runner::container::tests::every_container_effect_in_the_tree_goes_through_the_funnel`
is the source census that says so in the tree's own idiom.

## `pub trait ContainerRuntime: Send + Sync` › `fn probe(&self) -> Result<(), RuntimeError>;`

Can the runtime be reached at all?

### Errors

[`RuntimeError::Unreachable`] when it cannot. A caller may **not** treat
`Ok(())` as a promise about any later operation; see the module docs.

## `pub trait ContainerRuntime: Send + Sync` › `fn image_by_reference(&self, reference: &str) -> Result<Option<ImageInspection>, RuntimeError>;`

Resolve a reference. `Ok(None)` means the reference is not present —
which is a refusal, not a pull. `non_goals[1]` is "implicit image pull".

### Errors

[`RuntimeError`] when the runtime cannot be reached or the inspection
fails.

## `pub trait ContainerRuntime: Send + Sync` › `fn image_by_id(&self, id: &str) -> Result<Option<ImageInspection>, RuntimeError>;`

Resolve an immutable id. `Ok(None)` means "the recorded image id is
absent from the runtime", which refuses a rebuild before any spawn.

### Errors

[`RuntimeError`] when the runtime cannot be reached or the inspection
fails.

## `pub trait ContainerRuntime: Send + Sync` › `fn volume_present(&self, name: &str) -> Result<bool, RuntimeError>;`

Whether a named volume exists. R20 volumes are operator-owned and are
never created here.

### Errors

[`RuntimeError`] when the runtime cannot be reached or the inspection
fails.

## `pub trait ContainerRuntime: Send + Sync` › `fn containers_with_label(`

Every container carrying `key=value`, with its labels.

### Errors

[`RuntimeError`] when the runtime cannot be reached or the listing
fails.

## `pub trait ContainerRuntime: Send + Sync` › `fn observe(&self, name: &str) -> Result<Liveness, RuntimeError>;`

Whether a container is running, exited, or gone.

### Errors

[`RuntimeError`] when the runtime cannot be reached or the inspection
fails.

## `pub trait ContainerRuntime: Send + Sync` › `fn collect(&self, name: &str) -> Result<ContainerExecution, RuntimeError>;`

The container's exit status and captured output.

### Errors

[`RuntimeError`] when the runtime cannot be reached or the collection
fails.

## `pub trait ContainerRuntime: Send + Sync` › `fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError>;`

Create a container **from an image id**, and report the id the runtime
used.

### Errors

[`RuntimeError`] when the runtime cannot be reached or creation fails.

## `pub trait ContainerRuntime: Send + Sync` › `fn start(&self, name: &str) -> Result<(), RuntimeError>;`

Start it.

### Errors

[`RuntimeError`] when the runtime cannot be reached or the start fails.

## `pub trait ContainerRuntime: Send + Sync` › `fn stop(&self, name: &str, mode: StopMode) -> Result<(), RuntimeError>;`

Stop or kill it. **Idempotent and tolerant of already-gone.**

### Errors

[`RuntimeError`] when the runtime cannot be reached, or the stop fails
for a reason other than the container being absent.

## `pub trait ContainerRuntime: Send + Sync` › `fn remove(&self, name: &str) -> Result<(), RuntimeError>;`

Remove it. **Idempotent and tolerant of already-gone**, because "two
concurrent reclaimers converge".

### Errors

[`RuntimeError`] when the runtime cannot be reached, or the removal
fails for a reason other than the container being absent.

## `pub trait OwnerLiveness: Send + Sync {`

---------------------------------------------------------------------------
Owner liveness — a separate seam, and that is the point
---------------------------------------------------------------------------

## `pub trait OwnerLiveness: Send + Sync {`

Is another run's coordinator alive?

`crash_reconstruction`: "owner run != this run: **probe that run's run.lock
non-blocking** (is_running semantics: src/rundir.rs:619-652)".

**This trait returns a `bool` and takes a public run directory, and both
halves of that signature are load-bearing.** The same passage says "the
coordinator incarnation id is a per-process ULID recorded in
run_started(4)/run_resumed(4) and is **never read from lock-file contents**
(run.lock content is never read: src/rundir.rs:886; a Windows exclusive lock
makes it unreadable to non-holders)". Deriving an incarnation from the lock
is a plausible implementation and a real defect; a seam whose answer is one
bit makes it **structurally impossible** — there is no incarnation in the
return type to read.

Kept out of [`ContainerRuntime`] for the same reason: liveness is a question
about a lock file on this host, not about the container runtime, and a
runtime that could answer it would be a runtime that had opened a run lock.

## `pub trait OwnerLiveness: Send + Sync` › `fn is_running(&self, public_run_dir: &Path) -> bool;`

Whether a coordinator holds the run lock of the run whose **public**
directory this is.

## `pub struct LockProbe;`

The production answer: `rundir::is_running`.

## `pub enum TracePhase {`

---------------------------------------------------------------------------
The trace
---------------------------------------------------------------------------

## `pub enum TracePhase {`

Which half of a funnel call a trace entry records.

## `impl TracePhase` › `pub const fn name(self) -> &'static str {`

As it is written in a trace.

## `pub enum DurableStep {`

A durable step of a record write, recorded where it happens.

The sibling of [`crate::util::DurableStep`], and a separate enum for one
reason: this trace interleaves durability with runtime calls and funnel
phases in **one order**, and `util`'s ledger is a separate list. "Intent
**synced** before docker create" is a statement about one sequence
containing both, so both have to be in the same sequence.

## `pub enum DurableStep` › `Synced,`

The staged file's bytes are written and fsynced.

## `pub enum DurableStep` › `Renamed,`

The atomic rename onto the published name.

## `pub enum DurableStep` › `DirSynced,`

The containing directory is fsynced, which is what makes the rename
durable.

## `pub enum DurableStep` › `Removed,`

A file was removed.

## `impl DurableStep` › `pub const fn name(self) -> &'static str {`

As it is written in a trace.

## `pub enum TraceEntry {`

One thing that happened, in order.

## `pub enum TraceEntry` › `Site {`

A funnel reached a hook phase of a site.

## `pub enum TraceEntry` › `Runtime {`

A runtime operation was issued.

## `pub enum TraceEntry` › `Durable {`

A durability step of a record write.

## `pub enum TraceEntry` › `View {`

A Git view was materialised or discarded (R19).

## `pub enum ViewAction {`

What happened to a Git view.

## `impl ViewAction` › `pub const fn name(self) -> &'static str {`

As it is written in a trace.

## `impl TraceEntry` › `pub fn render(&self) -> String {`

A short, stable rendering, so a test can assert on a `Vec<&str>`.

Orderings are most of this slice's contract and "a suite that proves the
set of operations happened without pinning their order holds none of
them". A sequence of strings is the cheapest thing to write an ordering
assertion against, which is the point: an assertion nobody writes holds
nothing either.

## `pub struct ContainerTrace(Option<Arc<Mutex<Vec<TraceEntry>>>>);`

An ordered record of everything the funnel and the runtime did.

A cloneable handle over a shared log, like [`crate::util::DurabilityLedger`]
and for the same reason: the funnel holds `&mut dyn ContainerHooks` across
its body, so the body cannot borrow the observer again. Both the funnel and
the runtime take a clone of one handle, which is what puts their entries in
one order.

The source retains the mutex protocol beside `ContainerTrace` under the
concurrency exception in §§10 and 13. It states the shared log's lifetime,
operation ordering, critical-section limits, and poison/guard cleanup behavior.

## `impl ContainerTrace` › `pub fn off() -> Self {`

A trace that records nothing. What production passes.

## `impl ContainerTrace` › `pub fn recording() -> Self {`

A trace that records. What a test passes.

## `impl ContainerTrace` › `pub fn is_recording(&self) -> bool {`

Whether this trace records at all.

## `impl ContainerTrace` › `pub fn push(&self, entry: TraceEntry) {`

Append one entry.

## `impl ContainerTrace` › `pub fn site(&self, site: ContainerSite, phase: TracePhase) {`

Record a funnel phase.

## `impl ContainerTrace` › `pub fn runtime(&self, op: RuntimeOp, target: &str) {`

Record a runtime operation.

## `impl ContainerTrace` › `pub fn durable(&self, step: DurableStep, path: &Path) {`

Record a durability step.

## `impl ContainerTrace` › `pub fn view(&self, action: ViewAction, path: &Path) {`

Record a Git view action.

## `impl ContainerTrace` › `pub fn entries(&self) -> Vec<TraceEntry> {`

Everything recorded so far, in order.

## `impl ContainerTrace` › `pub fn rendered(&self) -> Vec<String> {`

Everything recorded so far, rendered, in order.

## `impl ContainerTrace` › `pub fn sites(&self) -> Vec<(ContainerSite, TracePhase)> {`

Only the site phases, in order — the sequence the eight-site contract is
written in.

## `impl ContainerTrace` › `pub fn ops(&self) -> Vec<RuntimeOp> {`

Only the runtime operations, in order.

## `impl ContainerTrace` › `pub fn position(&self, needle: &str) -> Option<usize> {`

The index of the first entry whose rendering is exactly `needle`.

The ordering assertions in this slice are all of the form "x happened
before y", and comparing two positions is how that is said.

## `impl ContainerTrace` › `pub fn position_starting(&self, prefix: &str) -> Option<usize> {`

The index of the first entry whose rendering starts with `prefix`.

## `impl ContainerTrace` › `pub fn clear(&self) {`

Forget everything recorded so far, keeping the handle.

<!--
PR163-ASTRA-RUSTDOC-LINKS: the rustdoc shortcut syntax above (`` [`Name`] ``)
has no Markdown reference definition, so a CommonMark renderer emits plain
text instead of a link. These definitions give each shortcut a real target
so the references above resolve.
-->
[`ContainerRuntime::probe`]: ../../../../src/runner/container/runtime.rs
[`ContainerRuntime`]: ../../../../src/runner/container/runtime.rs
[`CreateSpec::read_only_root`]: ../../../../src/runner/container/runtime.rs
[`CreateSpec`]: ../../../../src/runner/container/runtime.rs
[`CreatedContainer::reported_image_id`]: ../../../../src/runner/container/runtime.rs
[`Mount::Tmpfs`]: ../../../../src/runner/container/runtime.rs
[`RuntimeError::Failed`]: ../../../../src/runner/container/runtime.rs
[`RuntimeError::Unreachable`]: ../../../../src/runner/container/runtime.rs
[`RuntimeError`]: ../../../../src/runner/container/runtime.rs
[`RuntimeOp`]: ../../../../src/runner/container/runtime.rs
[`Self::reported_image_id`]: ../../../../src/runner/container/runtime.rs
[`StopMode`]: ../../../../src/runner/container/runtime.rs
[`crate::agent::ProcessOutput`]: ../../../../src/agent/proc.rs
[`crate::runner::Runner`]: ../../../../src/runner/mod.rs
[`crate::topology::events::ImageIdentity`]: ../../../../src/topology/events.rs
[`crate::util::DurabilityLedger`]: ../../../../src/util.rs
[`crate::util::DurableStep`]: ../../../../src/util.rs
[`super::exec::ContainerRunner::plan`]: ../../../../src/runner/container/exec.rs
