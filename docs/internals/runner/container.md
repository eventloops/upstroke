# `src/runner/container.rs`

Extended notes for [`src/runner/container.rs`](../../../src/runner/container.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The **Container funnel** — `FunnelGroup::Container.module()` is this file.

`decisions.effect_site_inventory.identity`: "every effectful funnel API
takes its group's site by value, and the funnel itself calls `hook(Before,
site)` -> primitive -> `hook(After, site)`, so hooks exist for every site by
construction". `ContainerSite` in the frozen `topology::effects` inventory
— `src/topology/effects/sites.rs` since that module was split into
per-concern children — has **eight** variants and all eight are taken by
value by an API here.

#### Why this is a file and not `container/mod.rs`

`FunnelGroup::Container.module()` returns the literal
`"src/runner/container.rs"` and
`effects::tests::every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`
reads exactly that path. `container/mod.rs` would make the inventory's
`module` column false of this tree — `PR5-CONF-018` is the standing entry
for what that costs. Rust 2018 path style makes this file plus
`src/runner/container/*.rs` the ordinary layout, so both hold at once.

#### What "impossible to bypass" can and cannot mean here

Rust module privacy cannot isolate siblings under a shared ancestor: an item
private to `runner::container` is visible to `runner::container::census` and
to every other module a lane adds beside it, so no token, sealed trait or
private constructor makes a bypass a **compile** error from inside this
subtree. The project's own mechanism for exactly this is
`decisions.effect_site_inventory.mechanism` (1)-(2), and it is a **build**
error rather than a compile one:

* every effectful method of [`runtime::ContainerRuntime`] and of [`GitView`]
  is on `clippy.toml`'s disallowed list — "docker invocation helpers" is the
  packet's own phrase for them — so a module that calls one fails
  `cargo clippy -- -D warnings` unless it is in `effects/allowlist.toml`,
  which is a reviewed artifact at every gate;

  **This sentence was false between PR6 and repair round F1, and the shape
  of its falsehood is worth keeping.** The allow below is an *inner*
  attribute, and a Rust lint level is scoped by the **module tree** rather
  than by the file — so `runner::container::{census, env, exec, fake,
  intent, runtime, tests, view}` all inherited it, and a
  `ContainerRuntime::start` planted in one of them passed the exact clippy
  gate. Measured, twice: by lane A with a planted probe and by the lane-F
  review (`PR6A-CONTAINER-ALLOW-IS-INHERITED-BY-EVERY-LANE-MODULE` /
  `PR6-LANEF-004`). Each of those files now **re-denies** the three governed
  lints, and the three that need one for their *test region* carry an allow
  of their own with an `effects/allowlist.toml` entry to be read against.
  [`tests::every_child_module_of_the_container_funnel_states_its_own_lint_level`]
  refuses a new file here that states neither, which is what stops the hole
  reopening for the next lane rather than for this one;
* [`tests::every_container_effect_in_the_tree_goes_through_the_funnel`] is
  the source census beside it, in the idiom of
  `runner::tests::every_production_process_start_is_classified`: it names
  every file that may issue a container effect and fails when a new one
  appears.

`PR5D-PROCESS-FUNNEL-TAKES-NO-SITE` records what happens when a group has no
funnel that names its sites, and `PR5D-FUNNEL-RETURNS-A-COMMAND` what
happens when one hands a writable handle back. Neither is repeated here: the
site travels with every call, and no API returns a runtime handle, a
`Command`, or a `File`.

#### The orderings

`slice_contract.side_effect_vs_event_ordering` is the whole of this module's
contract: "no events; intent synced before docker create; container created
from the recorded id and verified before start; view mounted before start;
stop/rm, view removal, intent removal after completion". Each clause is an
independently droppable predicate, so [`launch`] and [`release`] perform
them in one place and [`runtime::ContainerTrace`] records the sequence,
which is what the tests assert on.

## `#![allow(`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
carries this module's review clause -- effects only inside site-taking APIs,
no writable handle returned. `decisions.effect_site_inventory.mechanism` (2).

## `pub mod census;`

-- module declarations -------------------------------------------------
APPEND ONLY. Lane A adds `exec`, `view` and `env`; lane C adds `census`;
lane B adds `resolve`.
Keep every `#[cfg(test)]` declaration at the BOTTOM of this file:
`effects::production_region` cuts a source at its FIRST `#[cfg(test)]`, so a
test-only `mod` here would remove every funnel below it from the census that
proves this group has a funnel at all (`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).

## `pub trait ContainerHooks {`

---------------------------------------------------------------------------
Hooks
---------------------------------------------------------------------------

## `pub trait ContainerHooks {`

What the funnel consults at each phase of each site.

The sibling of [`crate::rundir::RunDirHooks`] and
[`crate::workspace_manager::EffectHooks`]. The site travels with the call
because this funnel serves eight sites, which is the shape
`effect_site_inventory.identity` describes in as many words.

## `pub trait ContainerHooks` › `fn phase(&mut self, site: EffectSiteId, phase: HookPhase) -> Injection;`

The funnel reached `phase` of `site`. The answer says what to do there.

## `pub trait ContainerHooks` › `fn trace(&self) -> ContainerTrace {`

Where this observer wants the funnel's ordered record kept.

A *handle*, taken before the funnel body runs, because `funnel` holds
`&mut dyn ContainerHooks` across the body — the same reason
`EffectHooks::durability_ledger` is a handle. The default records
nothing, which is what production passes.

## `pub struct NoHooks;`

What production passes: nothing is armed and nothing is recorded.

## `pub struct HarnessHooks {`

Wires this funnel onto PR3's [`HookHarness`], the way
[`crate::rundir::HarnessHooks`], [`crate::workspace_manager::HarnessEffects`],
[`crate::events::log::HarnessEventHooks`] and [`crate::runner::HarnessHooks`]
wire the other four families onto it.

The Container group was the one family without such an adapter. Its only
observer was `fake::RecordingHooks`, which records into a
[`ContainerTrace`] — an ordered log of *runtime operations*, which is a
different thing from the *site coverage* `HookHarness` accumulates.
`check_bijection` reads `HookHarness` alone, so a coverage pass that drove
the container funnel through `RecordingHooks` would produce an evidence
table with eight sites missing and nothing to say they were missing.

## `impl HarnessHooks` › `pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {`

Observe through `harness`.

## `impl HarnessHooks` › `pub fn harness(&self) -> &Arc<Mutex<HookHarness>> {`

The harness this observer records into.

## `impl HarnessHooks` › `pub fn recording_trace(mut self, trace: ContainerTrace) -> Self {`

Also keep the funnel's ordered record of runtime operations.

The two are complementary and neither replaces the other: the harness
answers "was this site's phase executed", the trace answers "in what
order did the daemon see these calls".

## `fn apply(injection: Injection, site: EffectSiteId, phase: HookPhase) -> Result<(), UpstrokeError> {`

Turn a hook's answer into what the funnel must do at that point.

## `fn funnel<T>(`

One effect, between its two hook phases, with its site recorded in the
trace on both sides.

An `Err` from the `After` phase is returned *after* the primitive ran, which
is the whole point of the error-return mode.

## `enum Operation {`

---------------------------------------------------------------------------
The site each API takes, and the guard that keeps the parameter honest
---------------------------------------------------------------------------

## `enum Operation {`

What a site names.

Every funnel API below takes `site: ContainerSite` **by value**, which is
what `identity` requires. A free parameter can be passed a wrong value, so
each API checks it against this map: passing `ContainerSite::Start` to
[`write_intent`] refuses, before any effect, rather than writing a record
under a label that lies about what happened.

## `const fn operation_of(site: ContainerSite) -> Operation {`

The site-to-operation map, exhaustive over the frozen eight.

## `fn expect_site(site: ContainerSite, wanted: Operation) -> Result<(), UpstrokeError> {`

Refuse a site that does not name this operation.

## `pub struct GitViewRequest {`

---------------------------------------------------------------------------
The Git view (R19)
---------------------------------------------------------------------------

## `pub struct GitViewRequest {`

What a Git view needs to exist.

DESIGN.md:612: "the container overlays a disposable role-scoped Git view —
exact detached HEAD/index, no engine refs, read-only objects — so
Git-dependent tools work without exposing or mutating the coordinator's
refs."

## `pub struct GitViewRequest` › `pub path: PathBuf,`

Where the view directory is materialised, on the host.

## `pub struct GitViewRequest` › `pub workspace: PathBuf,`

The worktree the role is executing in.

## `pub struct GitViewRequest` › `pub head: Option<String>,`

The commit the view is pinned to, when the projection needs one.

## `pub trait GitView: Send + Sync {`

The R19 disposable Git view.

Its two methods are the primitives of `Container.MountGitView` and
`Container.UnmountGitView` and are on `clippy.toml`'s disallowed list, so
only this module calls them — [`mount_git_view`] and [`unmount_git_view`]
are the funnels, and they are what a caller uses.

**Lane A implements the projection.** [`DisposableDirView`] below is the
directory half — the R19 artifact whose lifecycle the resource row accounts
for ("mounted": "pruned on complete or cancel; orphan views reclaimed during
dead-owner or dead-incarnation container reclaim") — and it is what the
substrate's own tests and the reclaim path need. What it does **not** do is
the detached HEAD/index projection or the read-only object mount; those are
`src/runner/container/view.rs`.

## `pub trait GitView: Send + Sync` › `fn materialize(&self, request: &GitViewRequest) -> Result<PathBuf, UpstrokeError>;`

Bring the view into existence, returning where it is.

### Errors

[`UpstrokeError::Io`] when the view cannot be materialised.

## `pub trait GitView: Send + Sync` › `fn discard(&self, path: &Path) -> Result<(), UpstrokeError>;`

Remove it. **Idempotent**: an orphan view is reclaimed by whichever
process gets there first, and reclaim converges.

### Errors

[`UpstrokeError::Io`] when the view exists and cannot be removed.

## `pub struct DisposableDirView {`

The directory half of the view: create it, remove it.

Not a stub — this is R19's whole physical artifact, and the row's lifecycle
is about the directory. Lane A's projection fills it.

## `impl DisposableDirView` › `pub fn new(trace: ContainerTrace) -> Self {`

A view whose actions are recorded in `trace`.

## `fn discard(&self, path: &Path) -> Result<(), UpstrokeError>` › `racing_removal(path, || fs::remove_dir_all(path))?;`

R19's half of "every step idempotent and tolerant of already-gone so
two concurrent reclaimers converge". The errno is not the question —
see [`RACING_ACCESS_ATTEMPTS`], and the Windows guest measurement that
put it there.

## `pub const RACING_ACCESS_ATTEMPTS: usize = 64;`

How many times a path that another reclaimer may be removing is asked about
before a failure is believed.

**The whole reason this exists is a platform difference, measured on the
Windows guest and invisible on Linux.** "every step idempotent and tolerant
of already-gone so two concurrent reclaimers converge" is usually written as
`if error.kind() == NotFound`, and on Windows the losing reclaimer does not
get `NotFound`: a file or directory another process is deleting is
**delete-pending**, and opening it answers `ERROR_ACCESS_DENIED` — `kind() ==
PermissionDenied` — until the winner's handle closes. An errno test
therefore cannot tell "somebody else is removing it" from "I may not touch
it", and tolerating `PermissionDenied` outright would silently treat a
genuinely protected path as reclaimed.

So the question asked is the **outcome**, not the errno: retry, and believe
the failure only once the path has stopped changing under it.

**How long the loser has to wait is a scheduling quantity, not a filesystem
one**, and that is what the pause schedule below answers. Measured on the
Windows guest (Server 2025 build 26100, NTFS, Rust 1.97.1) against std's own
`remove_dir_all` sequence: the name becomes delete-pending the moment the
winner's `SetFileInformationByHandle(FileDispositionInfoEx, POSIX)` returns,
and it is unlinked only when the winner **closes** that handle. Between those
two calls every `CreateFileW` of the name — the first thing every
`remove_dir_all` attempt does — answers `ERROR_ACCESS_DENIED`, and the window
is exactly as long as the winner takes to reach its close. A winner running
on another processor that is preempted there, or whose vCPU the hypervisor
has taken, holds the name delete-pending for a quantum or more.
`std::thread::yield_now` is `SwitchToThread`: it cedes **this** processor
to a ready thread, which does nothing for a winner that is not runnable
here, so sixty-four yields span about a quarter of a millisecond and the
loser refuses with the winner's handle still open. That is
`PR154-WINDOWS-CENSUS-VIEW-REMOVAL-ACCESS-DENIED`: the failing operation
was the loser's `CreateFileW`, the error was `STATUS_DELETE_PENDING`
rendered as os error 5, and the sixty-four attempts were exhausted in less
time than a scheduler tick.

The ordinary handoff is still microseconds — in 512 native two-remover
pairs, 234 losers saw `ERROR_ACCESS_DENIED`: 209 once and then `NotFound`,
21 twice and then `NotFound`, 4 once and then their own success — so the
first [`RACING_YIELD_ATTEMPTS`] failures keep the yield, and the failures
after them sleep [`RACING_SLEEP`] apiece. A sleep gives up the processor for
a scheduler tick, which is the unit the winner's stall is measured in. The
pause sits strictly **between** attempts: after the last failure there is
nothing left to observe, so nothing sleeps, and a winner that closes after
the final attempt is met by the next census rather than by a stale answer.

Bounded rather than timed, for the reason [`TERMINATION_OBSERVATIONS`] is: a
wait with no bound turns "this path cannot be removed" into "this write
command never returns". A path that is still refusing after every attempt
is reported as the error it last gave, never as absent: the census then
keeps the intent that names the view, and the next write command's census
reclaims both once the name has gone.
`runner::container::tests::windows_a_view_held_delete_pending_past_the_budget_refuses_and_keeps_the_intent`
holds the name delete-pending through the whole budget and asserts exactly
that;
`runner::container::tests::windows_a_view_whose_remover_stalls_delete_pending_converges_once_the_stall_ends`
stalls a winner between the two calls for longer than the yields reach and
asserts the loser converges once the stall ends. On the yield-only loop the
second fails with the CI failure's own text after sixty-four failed attempts
in a few milliseconds, and the first refuses before its budget could have
been spent. Both order the winner's close against an observed attempt through
[`note_racing_attempt`], not against a clock.

## `pub const RACING_YIELD_ATTEMPTS: usize = 16;`

How many failed attempts of [`RACING_ACCESS_ATTEMPTS`] only yield before
the loop starts sleeping.

Sixteen covers every interleaving the two-remover measurement produced —
the longest run of non-`NotFound` answers on an unloaded guest was two, and
under sixteen competing threads pinned to two processors it was nine — so
the ordinary handoff still costs microseconds and nothing sleeps unless the
winner has genuinely stopped making progress.

## `pub const RACING_SLEEP: Duration = Duration::from_millis(10);`

How long each attempt after [`RACING_YIELD_ATTEMPTS`] sleeps.

Ten milliseconds is about one scheduler tick, so the loser reacts within a
tick or two of the winner's close, and the forty-seven that fit between the
sixty-four attempts bound a permanently refusing path's cost at half a second
to three quarters of one before the refusal. That is paid once, on an error path that blocks
admission, and it is what buys tolerance of a winner stalled for a Windows
Server quantum.

## `fn racing_pause(failed: usize) {`

The pause after the `failed`th failed attempt of [`racing_removal`] and
[`read_racing`], and before the next: a yield through the first
[`RACING_YIELD_ATTEMPTS`] failures, then [`RACING_SLEEP`], and nothing at
all after the [`RACING_ACCESS_ATTEMPTS`]th, because no attempt follows it
— a sleep there would be dead time in which the winner's close goes
unobserved. It records the failure through [`note_racing_attempt`] first.

Not `workspace_manager::remove_tree_once_handles_close`'s schedule, which
waits for a *dying process* to close its handles and sleeps twenty-five
milliseconds forty times over. This is still a handoff between two live
reclaimers; the sleep exists only because the winner's remaining step can
be descheduled, and the budget is an order of magnitude smaller.

## `fn note_racing_attempt(_failed: usize) {}`

The `#[cfg(not(test))]` half of a seam that exists for one reason: nothing
outside the loop can see an *attempt*, and the native Windows tests have to
order a winner's close against one rather than against a clock (§10: a sleep
may supplement a concurrency test but cannot be its only oracle). The
`#[cfg(test)]` twin forwards to `tests::note_racing_attempt`, which runs an
observer the test installed on the calling thread after the failed attempt
has already returned. The shape is `workspace_manager::note_removal_attempt`'s.

## `pub fn write_intent(`

---------------------------------------------------------------------------
The eight site-taking APIs
---------------------------------------------------------------------------

## `pub fn write_intent(`

`Container.WriteIntent` (R26) — the synced global intent record.

"every container invocation writes a **synced** intent in the global
namespace `<R>/containers/<container-name>.intent`". Written the way every
other durable record in this engine is written: staged, fsynced, renamed,
and the directory fsynced — `run_creation`'s own four steps — each recorded
in the trace beside the primitive that performs it, so a deleted step is a
missing trace entry rather than an invisible loss of durability.

Returns an [`IntentWritten`] — the **capability** the create and start
funnels require, not a handle and not merely a path.

The value is minted by [`IntentWritten::certify`], which reads the published
record back and parses it, so it is evidence about the filesystem rather
than about this function having been called. That read is also the one
observation this slice makes of "intent **synced** before docker create": a
rename that did not land is a refusal here rather than a container the
census cannot account for.

### Errors

[`UpstrokeError::Refused`] when `site` does not name this operation,
[`UpstrokeError::Io`] on any filesystem failure, [`UpstrokeError::Git`] when the
record will not serialize.

## `pub fn create_container(`

`Container.Create` (R26) — create the container **from an image id**.

INV-23: "every container of every epoch is created from the recorded image
id". [`CreateSpec`] carries no reference at all, so creating from one is not
expressible. The returned [`CreatedContainer::reported_image_id`] is the
runtime's own answer; [`launch`] verifies it against the record **before
start**.

It takes an [`IntentWritten`], which is the "intent synced **before** docker
create" clause made structural: the argument cannot be produced without the
record being on disk. The proof must be **this** container's — a proof for
another name is refused before any effect, so a caller cannot write one
intent and create a different container under it.

#### Every named volume is re-inspected here, **before** the create

`PR6-ACCT-001` / `PR6-CORRECTNESS-014`. R20 is `operator_owned` and
`persistent_output` — "**never created** or pruned by a run" — in all five
`at_run_end` outcomes, and `docker create` does not honour that: measured
against `docker` 29.7.2, `--mount type=volume,source=<absent>,target=/creds`
**succeeds and creates an empty named volume**, which the container then
starts from. A run would have created an operator-owned resource, and the
vendor CLI inside it would find no token where its credentials should be and
re-authenticate into a volume nothing provisioned.

Resolution inspects the volumes once, long before this
([`resolve::rebuild_by_inspection`]'s `inspect_volumes`); a volume can be
removed between that inspection and this call, and only a check *here* sees
it. The check is [`ContainerRuntime::volume_present`] — a read-only
inspection, so it is not a site and performs no effect — and it runs before
[`funnel`] is entered at all, so a refusal happens before
`Container.Create`'s `Before` phase rather than between the phases.

**Fail-closed in both directions**: a volume that is absent refuses, and a
runtime that will not answer *whether* it is present refuses too. "The
runtime did not say" is not "the volume is there".

### Errors

[`UpstrokeError::Refused`] when `site` does not name this operation, when
`intent` does not name `spec.name`, when a named volume the spec mounts is
absent or cannot be inspected, or when the runtime refuses.

## `fn expect_mounted_volumes_present(`

Refuse a create whose named volumes are not already there.

See [`create_container`] for why this is here and not only at resolution.

## `pub fn start_container(`

`Container.Start` (R26).

**The container to start is named by the proof and by nothing else.**
`expected_failures_refusals[6]` is "container start without an intent is
impossible by construction"; with a `&ContainerName` parameter that
sentence was true only of the sequences somebody had happened to write, and
a `start_existing(name)` added later compiled. With [`IntentWritten`] there
is no argument to pass that is not evidence.

### Errors

[`UpstrokeError::Refused`] when `site` does not name this operation or the
runtime refuses.

## `fn expect_intent_for(intent: &IntentWritten, name: &str, verb: &str) -> Result<(), UpstrokeError> {`

Refuse a proof that owns some other container.

The proof carries the name it was certified for, so this is the check that
stops "an intent was written" from standing in for "**this** container's
intent was written". Fail-closed: a mismatch refuses before the effect
rather than proceeding on evidence about something else.

## `pub fn mount_git_view(`

`Container.MountGitView` (**R19**, not R26 — the view is its own row).

### Errors

[`UpstrokeError::Refused`] when `site` does not name this operation, or
whatever the projection returns.

## `pub fn stop_container(`

`Container.Stop` (R26) — completion's `docker stop` and reclaim's `docker
kill`, which the frozen inventory accounts to one site.

### Errors

[`UpstrokeError::Refused`] when `site` does not name this operation or the
runtime refuses.

## `pub fn remove_container(`

`Container.Remove` (R26) — `docker rm`, idempotent.

### Errors

[`UpstrokeError::Refused`] when `site` does not name this operation or the
runtime refuses.

## `pub fn unmount_git_view(`

`Container.UnmountGitView` (R19), idempotent.

### Errors

[`UpstrokeError::Refused`] when `site` does not name this operation, or
whatever the projection returns.

## `pub fn remove_intent(`

`Container.RemoveIntent` (R26), idempotent.

### Errors

[`UpstrokeError::Refused`] when `site` does not name this operation,
[`UpstrokeError::Io`] when the record exists and cannot be removed.

## `let staged = staged_path(&path);`

The staged half too: a crash between the stage and the rename leaves
`<name>.intent.tmp`, and a reclaim that removed only the published
name would leave writer-owned residue in a directory the census
enumerates.

## `pub struct LaunchPlan {`

---------------------------------------------------------------------------
The sequences the contract states
---------------------------------------------------------------------------

## `pub struct LaunchPlan {`

Everything one container invocation needs.

## `pub struct LaunchPlan` › `pub private_root: PathBuf,`

`<R>` — the run's **recorded** private root.

## `pub struct LaunchPlan` › `pub invocation: InvocationId,`

Which invocation this container is, as the tuple rather than as the
rendered string the intent carries.

INV-23 gives an image-id mismatch **two** outcomes and the phase is what
chooses between them, so the phase has to be readable at the point the
mismatch is observed: see [`exec::ImageIdMismatch`].

## `pub struct LaunchPlan` › `pub spec: CreateSpec,`

The create arguments. `spec.image_id` is the record's image id, and it
is what the reported id is verified against.

## `pub struct Launched {`

A container that is running, and what it took to get there.

## `pub struct Launched` › `pub reported_image_id: String,`

The id the runtime reported, already verified equal to the record.

## `pub fn launch(`

The ordering `side_effect_vs_event_ordering` states, in one place.

> intent synced before docker create; container created from the recorded id
> and verified before start; view mounted before start

Four sites, and the verification between `Create` and everything after it.
**This is also what makes "container start without an intent is impossible
by construction"** (`expected_failures_refusals[6]`) true of the shape a
caller uses: the only sequence that reaches `Container.Start` begins by
writing the intent.

#### Why `MountGitView` precedes `Create` and not merely `Start`

The clause says "view mounted before start", which two orders satisfy. Only
one of them runs: **the view is a bind-mount *source* of the `docker create`
call**, and a bind source must exist when the container is created.
Measured against `docker` 29.7.2 with `WriteIntent -> Create ->
MountGitView -> Start` —

```text
the container runtime refused `create`: Error response from daemon: invalid
mount config for type "bind": bind source path does not exist: …/views/upstroke-…
```

— so that order cannot produce a working container for any `Implement`,
`Gate` or `Review` invocation at all. `PR6A-LAUNCH-MOUNTS-THE-VIEW-AFTER-CREATE`
is the finding; it was invisible to the fake, whose `create` does not care
whether a source path exists, and to a real-runtime test whose `LaunchPlan`
carried no mounts. `real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently`
now carries the view as a real mount, which is what makes it able to see it.

`WriteIntent -> MountGitView -> Create(+verify) -> Start` holds every clause
the contract states — intent synced before create, created and verified
before start, view mounted before start — and is the only order a container
runtime accepts.

#### The cancel path, at every point it can fail

On a reported image id that differs from the record the invocation is
**refused before start** and everything it created is released — R26's
"released on complete …, **cancel**, or shutdown" and R19's "pruned on
complete or **cancel**" — so both ledgers balance and no census finds
residue of a refusal. The view is now mounted before the create, so the
cancel has an R19 residue to prune as well as an R26 one.

**The cancel is failure-atomic in both directions** (`PR6-LANEF-006`): every
step is attempted even after an earlier one fails, and the error returned is
always the **image-integrity refusal**. Returning a cleanup failure instead
would leave the operator holding "docker stop said no" while the fact that
the runtime executed a substituted image went unsaid, and stopping at the
first failure would leave a container *and* an intent where attempting both
leaves neither — `docker rm --force` removes a running container, so
`Remove` after a failed `Stop` is not a wasted call. What could not be
released is named in the refusal rather than swallowed.

### Errors

[`UpstrokeError::Refused`] when the reported image id differs from the record,
or whatever a step returns.

## `fn cancel_created(`

Release everything a refused launch created, **attempting every step even
after one fails**, and answer what could not be released.

The four sites of [`release`], in the same order, with the `?` taken off.
`PR6-LANEF-006`: with `?` in place a failing `Container.Stop` returned the
stop error and left the container, the view and the intent behind — three
residues and a masked integrity refusal, from one failure.

The answer is a list of descriptions rather than a `Result`, because the
caller must return the refusal it already has: this function's failure is
never the thing to report *instead of* an integrity violation.

## `fn cancel_reached(`

The one exhaustive cleanup in this tree: stop, remove, unmount, remove the
intent — **attempting every step even after one fails**, and never removing
the R26 record while it is the only thing that can find the R19 residue.

`ContainerRunner::cancel` and `ContainerRunner::release` delegate here, and
so does [`cancel_created`]. One definition, deliberately: the view-path
derivation was two copies of one `join` until `PR6E-005` measured them
apart, and a cleanup rule split across two files is the same shape.

#### Why `RemoveIntent` is conditional (`PR6-ACCT-005`)

**The intent is the R19 view's only recovery anchor.** The startup census
discovers candidates from `<R>/containers` and from `docker ps` by label
(`census::run_startup_census` steps 1 and 3), and derives a view path only
*after* it has such a candidate — it never enumerates `<R>/views`. So a
cleanup that fails to prune the view and then removes the intent anyway has
deleted the only evidence that names the residue: the container is gone, the
record is gone, and the directory is permanently undiscoverable. R19's
`NoRunFinished` cell is "pruned at the next write-command start **after the
owning container is observed terminated**" and ST-16's closing clause is
"ledgers R19/R26 balance"; neither can hold for a view nothing can find.

The rule is therefore: **the anchor outlives what it anchors.** If the view
removal was attempted and failed, the intent stays, the failure is named in
the residue, and the next census finds an intent-only candidate, derives the
same view path and retries. The retained record is itself reported as
residue, so "the ledgers do not balance" is still said out loud rather than
traded for a tidy directory.

This is the fail-**closed** direction. Removing the record is the fail-open
one, and it reads as the tidier cleanup right up until an operator has to
find the directory by hand.

## `fn render_residue(residue: &[String]) -> String {`

What a cancel could not release, appended to the refusal that caused it.

Empty when the cancel balanced, which is the ordinary case and leaves the
refusal's own wording untouched.

## `pub fn release(`

The completion half: "stop/rm, view removal, intent removal after
completion".

**Exhaustive, like the cancel** (`PR6-ACCT-004`). This chained its four
sites with `?` until repair round R3b, so a `Container.Stop` that failed on
a completed invocation skipped the *still viable* forced remove, the view
prune and the intent removal — three residues from one failure, on the
ordinary completion path rather than on a refusal. `docker rm --force`
removes a running container, so `Remove` after a failed `Stop` is not a
wasted call, and R26 is "released **on complete** (stop/rm, view removed,
intent removed), cancel, or shutdown".

What could not be released is named in the error rather than swallowed, and
[`cancel_reached`]'s anchor rule applies here too: a view that could not be
pruned keeps its intent, because the intent is the only thing a later census
can find it through.

### Errors

[`UpstrokeError::Refused`] naming every step that failed.

## `pub const TERMINATION_OBSERVATIONS: usize = 8;`

How many times reclaim asks whether a container has terminated.

`determinism` forbids sleeps, so this is a bounded number of round trips and
not a timed wait: `docker kill` returns after the signal has been delivered,
and each observation is a fresh inspection. A container still running after
all of them "cannot be observed terminated", which
`crash_reconstruction` says "blocks admission".

## `pub fn reclaim(`

One container reclaimed, in the packet's own order.

> reclaim = docker kill -> wait until observed exited/removed -> docker rm
> -> remove Git view -> remove intent, every step idempotent and tolerant of
> already-gone so two concurrent reclaimers converge

The Git view path is the caller's, because a census reads it from the
intent's run directory rather than from a live [`Launched`].

### Errors

[`UpstrokeError::Refused`] when the container cannot be observed terminated
within [`TERMINATION_OBSERVATIONS`] observations, or whatever a step
returns.

## `pub fn observe_terminated(`

"wait until observed exited/removed" — a read-only observation, and so not
a site.

### Errors

[`UpstrokeError::Refused`] when the container is still running after
[`TERMINATION_OBSERVATIONS`] observations, or when the runtime refuses.

## `pub enum OrphanWindow {`

---------------------------------------------------------------------------
The orphan window
---------------------------------------------------------------------------

## `pub enum OrphanWindow {`

Who closes the window between a coordinator's death and its containers being
reclaimed.

`decisions.admission_and_leases.permits.os_matrix`, in full:

> Linux and macOS (cfg(unix)): the cleanup reaper survives coordinator
> death, settles the dead coordinator's process groups while holding R28,
> and additionally kills the dead coordinator's labeled containers, closing
> the **orphan window**; Windows: **no reaper**; the ambient
> coordinator-joined Job Object ends every ordinary host descendant incl.
> suspended stubs at any spawn sub-step, private per-invocation jobs scope
> timeouts, and containers are reclaimed at the **next write-command start**
> (orphan window until then; documented; **a portable watchdog is
> deferred**).

A value rather than a comment, so the Windows guest — which has no container
runtime at all — still asserts something about containers, and so lane C's
census can report the window it is closing rather than infer it.

## `pub enum OrphanWindow` › `ClosedByTheUnixReaper,`

`cfg(unix)`: the per-invocation cleanup reaper outlives the coordinator
and kills its labeled containers.

## `pub enum OrphanWindow` › `UntilNextWriteCommandStart,`

Windows: nothing runs between the death and the next `upstroke` write
command, so the containers survive until its startup census.

## `impl OrphanWindow` › `pub const ALL: &'static [Self] = &[`

Both answers. Written out so the platform is an axis and not a constant.

## `impl OrphanWindow` › `pub const fn closed_by_a_reaper(self) -> bool {`

Whether a reaper closes it.

## `pub const fn orphan_window() -> OrphanWindow {`

This platform's orphan window.

## `pub struct FoundIntent {`

---------------------------------------------------------------------------
Read-only observations of the global namespace
---------------------------------------------------------------------------

## `pub struct FoundIntent {`

One intent record found in `<R>/containers`.

## `pub fn read_intent(path: &Path) -> Result<ContainerIntent, UpstrokeError> {`

Read one record back.

### Errors

[`UpstrokeError::Io`] when the file cannot be read, [`UpstrokeError::Refused`]
when it is not a `ContainerIntent`.

## `fn read_racing(path: &Path) -> Result<Option<ContainerIntent>, UpstrokeError> {`

Read one record back, answering `None` when it went away under the read.

The read half of [`RACING_ACCESS_ATTEMPTS`]: on Windows a file another
process is deleting answers `PermissionDenied` until that delete completes,
so "is it gone?" is a question about the outcome and not about the first
errno. The delete completes when the deleting handle closes, which is the
scheduling event [`racing_pause`] waits out, so the two loops share its
schedule. A record that is present and unreadable is still an error, after the
bound — silently skipping one would let a census admit over a container whose
ownership evidence it could not read.

### Errors

[`UpstrokeError::Refused`] when the file is not a `ContainerIntent`,
[`UpstrokeError::Io`] when it is still there and still unreadable after
[`RACING_ACCESS_ATTEMPTS`] attempts.

## `fn read_racing(path: &Path) -> Result<Option<ContainerIntent>, UpstrokeError> {` › `Err(other) => return Err(other),`

Not an IO answer at all: the bytes were read and are not a
record. Retrying cannot change that.

## `pub fn list_intents(private_root: &Path) -> Result<Vec<FoundIntent>, UpstrokeError> {`

Every intent record under `<R>/containers`, sorted by name.

"discovery at every write-command start **scans the whole namespace
`<R>/containers`** of the command's authorized private root **and** docker
ps by `upstroke.private_root`" — this is the first half; the second is
[`runtime::ContainerRuntime::containers_with_label`]. A missing directory is
an empty namespace, not an error: a run that has never launched a container
has none.

The staged `<name>.intent.tmp` half is skipped: it is writer-owned residue
that no reader may adopt, exactly as `Answer.StageWrite`'s `.partial` is.

### Errors

[`UpstrokeError::Io`] when the directory cannot be read, or whatever
[`read_intent`] returns.

## `pub fn list_intents(private_root: &Path) -> Result<Vec<FoundIntent>, UpstrokeError> {` › `let Some(record) = read_racing(&path)? else {`

A record that vanished between the directory read and this one is a
record another reclaimer removed, and that is not an error: "every
step idempotent and tolerant of already-gone so **two concurrent
reclaimers converge**".

Measured by lane C, not reasoned. With a bare `?` here,
`census::tests::concurrent_reclaimers_converge` refused with
`Io { NotFound }` on Linux in 2 of 20 runs, and with
`Io { PermissionDenied }` on the Windows guest — a whole write
command failing because another write command was tidying at the same
moment. A **malformed** record is still an error: "the record could
not be parsed" and "the record is gone" are different answers, and
only one of them licenses proceeding.

## `pub struct StagedIntent {`

One `<name>.intent.tmp` in `<R>/containers` whose published half is absent.

`PR6-ACCT-007`. [`write_synced`] durably creates the staged file before it
renames, so a crash — or a failing write, fsync or rename — leaves one
behind **before any container exists**: [`create_container`] takes an
[`IntentWritten`], which [`IntentWritten::certify`] mints by reading the
*published* record back, so nothing can have been created under a name whose
rename never landed.

[`list_intents`] skips it, exactly as `Answer.StageWrite`'s `.partial` is
skipped — a reader may not adopt writer-owned residue. That is right for
*discovery* and leaves the file with no reclaim path at all: no intent
candidate, no labeled container, so nothing ever calls [`remove_intent`] for
it. `census::run_startup_census` now enumerates them separately and gives
each one a disposition, so the staged half is **R26 residue with an
accounting class** rather than a file the tree has no row for.

## `pub struct StagedIntent` › `pub record: Option<ContainerIntent>,`

The record, when the staged bytes are a complete one.

`Some` is a write that finished and a rename that did not: the record is
as authoritative as a published one, and the census classifies it under
the ordinary owner-liveness rule because it carries the owner's run
directory. `None` is genuinely torn — the ownership evidence is the
**name**, which carries the run id and the incarnation but not the run
directory, so arm (ii)'s lock probe has nothing to ask about.

## `pub fn list_staged_intents(private_root: &Path) -> Result<Vec<StagedIntent>, UpstrokeError> {`

Every staged intent under `<R>/containers` whose published half is absent,
sorted by name.

A name that also has a published `<name>.intent` is **not** here: that name
is an ordinary candidate, and [`remove_intent`] removes both halves when it
is reclaimed.

### Errors

[`UpstrokeError::Io`] when the directory or a file cannot be read,
[`UpstrokeError::Refused`] when a staged file's stem is not a well-formed
container name — the same refusal [`list_intents`] gives a malformed
published one, for the same reason: an unreadable name in this namespace is
evidence the census could not classify, not evidence it may ignore.

## `pub fn list_staged_intents(private_root: &Path) -> Result<Vec<StagedIntent>, UpstrokeError> {` › `let record = match fs::read(&path) {`

Tolerant of already-gone, like every other read in this namespace: a
staged file that vanished under the scan is a writer that finished
its rename, or another reclaimer, and neither is an error.

## `pub fn remove_staged_intent(`

Remove one staged intent file.

Not a site of its own: the frozen [`ContainerSite`] has eight variants and
`RemoveIntent` is the one that accounts for the R26 record, both halves of
it — [`remove_intent`] already removes the staged file beside the published
one. This is that same site, reached for a name whose published half never
existed.

### Errors

As [`remove_intent`].

## `fn refused(error: RuntimeError) -> UpstrokeError {`

---------------------------------------------------------------------------
Primitives
---------------------------------------------------------------------------

## `fn refused(error: RuntimeError) -> UpstrokeError {`

A runtime failure, as the engine's error type.

## `fn staged_path(path: &Path) -> PathBuf {`

`<name>.intent` -> `<name>.intent.tmp`.

## `fn write_synced(path: &Path, bytes: &[u8], trace: &ContainerTrace) -> Result<(), UpstrokeError> {`

Write `bytes` durably: stage, fsync, rename, fsync the directory.

`run_creation`'s own four steps, each recorded in `trace` beside the
primitive that performs it. The file and directory barriers are
[`util::fsync_file`] and [`util::fsync_dir`] — the one call each that
`effects::tests::every_file_durability_barrier_in_a_funnel_module_goes_through_one_call`
censuses — rather than a `sync_all` of this module's own.

## `fn remove_if_present(path: &Path, trace: &ContainerTrace) -> Result<(), UpstrokeError> {`

Remove a file that may not be there, or may be going away under another
reclaimer.

## `fn racing_removal(`

Perform `remove` until the path is gone, however it went.

Answers whether **this** caller was the one that removed it, so a trace
records the removal once rather than once per reclaimer.

See [`RACING_ACCESS_ATTEMPTS`] for why this is not `if kind() == NotFound`.

### Errors

[`UpstrokeError::Io`] when the path is still there, and still refusing, after
[`RACING_ACCESS_ATTEMPTS`] attempts.

## `pub const DOCKER_PROGRAM: &str = "docker";`

---------------------------------------------------------------------------
The real runtime: the `docker` CLI
---------------------------------------------------------------------------

## `pub const DOCKER_PROGRAM: &str = "docker";`

The program name. `non_goals[3]` is "remote runners", so this is the local
CLI and nothing configures a socket.

## `pub struct DockerCli {`

The `docker` CLI.

Every process it starts is a **coordinator-side control-plane** call, and
deliberately does **not** go through the Runner: DESIGN.md:612 is "Workers,
repository-controlled gates, and reviewers all cross the boundary;
authoritative Git and the event log never do", and asking the container
runtime what it holds is the same kind of thing as authoritative Git — it is
how the boundary is *built*, so it cannot execute inside it.
`runner::tests::every_production_process_start_is_classified` carries the
row that says so.

## `impl DockerCli` › `pub fn new(trace: ContainerTrace) -> Self {`

A CLI whose operations are recorded in `trace`.

## `impl DockerCli` › `pub fn available() -> bool {`

Whether `docker` is on this machine and its daemon answers.

Two questions and not one, because `docker` exits **non-zero** with a
daemon-unreachable message when the binary is present and the daemon is
not — the same shape as `codex login status`, whose exit code and output
disagree.

## `impl DockerCli` › `fn exec(&self, op: RuntimeOp, target: &str, args: &[&str]) -> Result<String, RuntimeError> {`

Run one `docker` subcommand and capture it.

`target` is what the call log names — the container, image or volume the
operation is about, rather than the subcommand, so the trace of a real
run and the trace of a fake one are the same shape.

## `impl DockerCli` › `fn exec_streams(`

The same call, keeping **both** streams.

`PR6A-DOCKERCLI-MERGES-STDERR-INTO-STDOUT`: [`Self::exec`] returns only
stdout and drops the child's stderr on success, so every container
invocation's stderr was lost. A gate's failure output is its stderr and
becomes retry feedback (DESIGN.md:576); both shipped adapters read
stderr; `host::run_shell_probe`'s refusal quotes `output.stderr` and
would have quoted nothing. Measured on docker 29.7.2, `docker logs`
**separates** the two streams — `docker logs <c> 2>/dev/null` gives the
container's stdout and `2>&1 >/dev/null` gives its stderr — so there was
never anything to merge and the previous comment's premise was wrong as
well as its consequence.

## `impl DockerCli` › `fn inspect(`

`docker inspect` a thing, tolerating "no such object" as absence.

## `impl DockerCli` › `fn image(`

An image inspection from `docker image inspect`'s Go-template output.

## `struct ImageInspectionRaw {`

What `docker image inspect` reported, before it becomes an
[`runtime::ImageInspection`].

## `fn into_inspection(self) -> runtime::ImageInspection` › `digest: self`

"the manifest digest **when reported**". An image built locally
and never pushed has no repo digest, and `None` is the record
INV-23 asks for in that case.

## `fn split_list(raw: &str) -> Vec<String> {`

A comma-separated Go-template list, with the empty case as an empty vector.

## `const PS_FIELD_SEPARATOR: char = '\u{1f}';`

The field separator of [`PS_FORMAT`]'s output.

`U+001F` INFORMATION SEPARATOR ONE, the byte its name is for. It is not a
character any of this engine's own label values carry — a path label is
percent-encoded ASCII, a run id and an incarnation are ULIDs, an invocation
is `[0-9A-Za-z._]` — but a **foreign** container carrying this private
root's label may carry anything at all, so [`parse_ps_output`] treats a line
with the wrong field count as unreadable evidence and refuses rather than
trusting a mis-split.

## `const PS_FORMAT: &str = "{{.Names}}\u{1f}{{.Label \"upstroke.private_root\"}}\`

`docker ps --format` for the census: **one placeholder per label**.

`{{.Labels}}` renders every label of a container as an **unescaped
comma-joined `key=value` string**, and a label value may itself contain
commas and `=`. Measured on this box, `docker` 29.7.2, a container labelled
`upstroke.run=RUN1` and `upstroke.run_dir=/a,b=c`:

```text
--format '{{.Names}}|{{.Labels}}'      -> r2probe|upstroke.run=RUN1,upstroke.run_dir=/a,b=c
--format '{{.Label "upstroke.run_dir"}}' -> /a,b=c
```

The first is ambiguous *in the daemon's own output* — `upstroke.run_dir=/a`
and a label `b=c` is an equally good reading of those bytes, and it is the
reading the shipped `split(',')` took. For `upstroke.run_dir` that truncation
sends arm (ii)'s probe to a shorter, different directory, finds no
`run.lock`, and reclaims a **live** owner's container (`PR6-RECOV-002`). So
this asks the daemon for each field on its own, which is the one rendering
that cannot be ambiguous, rather than parsing a format that permits its own
delimiter inside a field.

All five labels, not the three the census reads today: the discovered
container's map is the same shape it always was, so a later reader is not
silently handed a map that lost the two nobody happened to need.

## `const PS_LABELS: &[&str] = &[`

The labels [`PS_FORMAT`] asks for, in the order it asks for them.

Written out beside the format string and checked against it by
`tests::the_ps_format_asks_for_exactly_the_labels_the_parser_names`, so a
placeholder added to one and not the other is a failing test rather than a
field read under the wrong name.

## `fn parse_ps_output(text: &str) -> Result<Vec<DiscoveredContainer>, RuntimeError> {`

`docker ps`'s answer, one container per line.

**A line whose field count is not `1 + PS_LABELS.len()` is refused**, and
that is the fail-closed half of [`PS_FIELD_SEPARATOR`]: a label value
carrying the separator splits into too many fields and one carrying a
newline splits its container across two lines with too few, and either way
the values no longer line up with the names. Guessing which field is which
at that point is how a census probes the wrong lock.

An **empty** rendered value is recorded as an absent label rather than as a
present empty one, because `{{.Label "x"}}` renders both as the empty string
and this side cannot tell them apart. Both are refused downstream —
`census::from_labels_alone` refuses a missing ownership label and
`intent::owner_run_dir` refuses an empty run directory — so the collapse
costs a diagnostic's precision and never an admission.

### Errors

[`RuntimeError::Failed`] naming the line, when a line does not carry exactly
the fields [`PS_FORMAT`] asks for. `Failed` and not `Unreachable`: the
daemon answered.

## `const UNREACHABLE_DIAGNOSTICS: &[&str] = &[`

The diagnostics that mean **the daemon was never reached**, lower-cased.

`crash_reconstruction` hangs a whole branch on this distinction: "the
container runtime is required only when an intent exists or a labeled
container is discoverable … with **no intent and no reachable runtime it
proceeds**". A machine with `docker` installed and the daemon not reachable
by *this* process is the ordinary configuration, not a fault, and every
diagnostic below is one this engine must proceed past when it holds no
container evidence. Getting it wrong the other way — classifying "cannot
reach" as "reached and refused" — makes `upstroke run` refuse to start at all
on a machine that has no container work to do (`PR6-RECOV-005`).

**Measured, not recalled.** Every entry was produced on this project's build
box against `docker` 29.7.2 and is transcribed from that run's stderr; the
command that produced each is beside it, and
`tests::the_transcribed_unreachable_diagnostics_are_what_the_live_cli_prints`
replays two of them through the real CLI so the table's oracle is the daemon
rather than this list.

| # | command | stderr |
|---|---|---|
| 1 | `sudo -u nobody docker ps` | `permission denied while trying to connect to the docker API at unix:///var/run/docker.sock` |
| 2 | `DOCKER_HOST=unix:///nonexistent/docker.sock docker ps` | `failed to connect to the docker API at unix:///nonexistent/docker.sock; check if the path is correct and if the daemon is running: dial unix …: connect: no such file or directory` |
| 3 | `DOCKER_HOST=tcp://127.0.0.1:1 docker ps` | `Cannot connect to the Docker daemon at tcp://127.0.0.1:1. Is the docker daemon running?` |

Rows 1 **and** 2 were both classified `Failed` by the three-string test this
replaced: row 1 is the socket-permission case the review reproduced, and row
2 — `docker` 29's wording for an absent socket — is the shape a machine with
the daemon stopped produces, so the misclassification was not the rare case
it looked like. Row 2 does contain the words "if the daemon is running", but
the shipped predicate looked for the older `Is the docker daemon running`,
case-sensitively, and matched neither.

**Windows** keeps `error during connect` and the named-pipe wording it
wraps (`open //./pipe/docker_engine: The system cannot find the file
specified`), which is why that entry stays even though no row above produced
it on Linux.

Nothing here may overlap [`stop_already_settled`]'s `is not running`, which
is a **reached** daemon reporting a container's state; entries are whole
connection phrases for that reason, and
`tests::the_two_docker_diagnostic_tables_never_claim_one_message` holds the
two tables apart.

## `pub fn is_unreachable_diagnostic(detail: &str) -> bool {`

Whether a `docker` failure means the daemon was never reached.

Case-insensitive: `docker` 29 lower-cased "docker API" where earlier
versions wrote "Docker daemon", and a classification that turns on the case
of a vendor's prose is a classification that breaks on the next release.

See [`UNREACHABLE_DIAGNOSTICS`] for the measured table and for why this is a
named function with its own tests rather than a `matches!` at the call site:
every fake in this slice injects [`RuntimeError::Unreachable`] *directly*,
so nothing but a test of this function tests the thing that decides it.

## `pub fn classify_docker_failure(operation: RuntimeOp, detail: String) -> RuntimeError {`

One failed `docker` invocation, as the seam's error.

A free function taking the raw stderr rather than a branch inside
[`DockerCli::exec_streams`], for the reason [`settle_stop`] is one: the
classification is reachable — and testable — **without a daemon**, and the
census tests can hand a fake runtime the error a verbatim diagnostic really
produces instead of asserting on an `Unreachable` they minted themselves.

## `fn is_absent(detail: &str) -> bool {`

Whether a `docker` failure means "the object is not there".

## `pub const REMOVAL_IN_PROGRESS: &str = "is already in progress";`

The verbatim shape the daemon answers the **loser** of two overlapping
removals with.

Measured on `docker` 29.7.2 by starting a container that writes 800 MiB and
issuing eight concurrent `docker rm --force --volumes`; one succeeded and
printed the name, and every other one printed exactly:

```text
Error response from daemon: removal of container <name> is already in progress
```

This is **not** absence, and it is not "already stopped" either — it is the
third state a racing reclaimer sees, and `T-CONTAINER.resume_action`'s
"(idempotent; **concurrent reclaimers converge**)" is false without it: the
losing reclaimer would return an error before `rm`, the view prune and the
intent removal, and the write command driving it would refuse rather than
converge. `PR6-CONV-002` is the entry; every fake race converged because
`FakeRuntime::remove` cannot produce this answer at all.

## `fn remove_already_settled(detail: &str) -> bool {`

Whether a `docker rm` failure means the container is gone, or is going away
under somebody else.

## `fn settle_remove(outcome: Result<String, RuntimeError>) -> Result<(), RuntimeError> {`

`docker rm`'s raw answer, as the seam's.

A free function for the reason [`settle_stop`] is one: the branch that
matters is one a real daemon produces **only** in a race, so a gated test is
the wrong and only place it could otherwise be observed. The transcribed
diagnostic is checked against the live daemon by
`tests::real_docker_prints_the_transcribed_removal_in_progress_diagnostic`,
so the table's oracle is `docker` and not this file.

## `fn image_by_id(&self, id: &str) -> Result<Option<runtime::ImageInspection>, RuntimeError> {` › `if found.id != id {`

`docker image inspect` resolves a *prefix* of an id and a tag alike,
so an answer whose id is not the value asked for is not an answer to
this question. The rebuild path's refusal is "the recorded image id
is absent from the runtime", and a different id present is exactly
that.

## `fn collect(&self, name: &str) -> Result<ContainerExecution, RuntimeError> {` › `let (stdout, stderr) = self.exec_streams(RuntimeOp::Collect, name, &["logs", name])?;`

Both streams, separately. `docker logs` writes the container's stdout
to its own stdout and the container's stderr to its own stderr
(measured, docker 29.7.2) — see [`Self::exec_streams`].

## `fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {` › `args.push("--read-only".to_owned());`

`expected_failures_refusals[5]`. Measured against `docker`
29.7.2: without it `sh -c 'printf owned >/outside-role-mount'`
exits **0** and the byte lands in the container's writable layer,
so a gate's write outside every declared mount succeeds and only
the weaker "the host is unharmed" holds. With it the same command
answers `Read-only file system` and exits non-zero.

## `fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {` › `args.push(spec.image_id.clone());`

The **image id**, never a reference (INV-23).

## `fn create(&self, spec: &CreateSpec) -> Result<CreatedContainer, RuntimeError> {` › `let reported = self`

The id the runtime says it used, read back from the created
container. Never `spec.image_id`: the whole point of the check the
caller then performs is that these two can differ.

## `fn remove(&self, name: &str) -> Result<(), RuntimeError>` › `settle_remove(self.exec(`

`--volumes` (`PR6A-ANONYMOUS-VOLUMES-LEAK`). An image declaring
`VOLUME` gets an **anonymous** volume per container, and `docker rm`
without it leaves one behind per invocation: measured, **29** leaked
from one run of this suite. Those volumes are not R20 — R20 is the
operator's *named*, per-agent credential volume, "operator_owned" and
`persistent_output` in all five `at_run_end` outcomes. An anonymous
volume is created by `docker create` as part of the container and
nothing else can ever refer to it, so it belongs to **R26** ("container
+ labels + global intent"), and `Container.Remove` is where R26
balances. `--volumes` removes only anonymous volumes attached to this
container and **never a named one**, which is what makes reclaiming it
here a discharge of R26 rather than a violation of R20.

## `fn stop_already_settled(detail: &str) -> bool {`

Whether a `docker stop` / `docker kill` failure means the container has
already reached the state the caller asked for.

The adjacent source comment retains the daemon's concurrent-reclaimer
protocol under standards §10 and §13. A stopped or removing container lets a
losing stop request continue cleanup, while other failures retain the intent
for retry. A removal in progress does not prove absence.

"every step idempotent and tolerant of already-gone so **two concurrent
reclaimers converge**" has two shapes on a real daemon, and only one of them
is "gone". Measured on `docker` 29.7.2, verbatim:

```text
docker kill <exited> -> Error response from daemon: cannot kill container: <n>: container <id> is not running
docker kill <absent> -> Error response from daemon: cannot kill container: <n>: No such container: <n>
docker stop <absent> -> Error response from daemon: No such container: <n>
docker stop <exited> -> succeeds, and prints the name
```

The **first** row is the load-bearing one and it is what a *racing*
reclaimer sees: A kills the container, B reaches `docker kill` after the
state has become `Exited`, and without this tolerance B returns an error
before observe / rm / view / intent cleanup — the opposite of the sentence.
`PR6-LANEF-003` records that deleting the tolerance passed the tests at that
historical head because the fixtures serialized the reclaimers. That is not
a claim about the current suite, which directly checks the stop-settlement
predicate, including the daemon's removal-in-progress response.

## `fn stop_already_settled(detail: &str) -> bool` › `remove_already_settled(detail) || detail.contains("is not running")`

`remove_already_settled` and not `is_absent`: a `docker kill` issued
against a container another reclaimer is already removing answers with
`REMOVAL_IN_PROGRESS` too, and that container is on its way out either
way.

## `fn settle_stop(outcome: Result<String, RuntimeError>) -> Result<(), RuntimeError> {`

`docker stop` / `docker kill`'s raw answer, as the seam's.

A free function taking the raw outcome rather than a `match` inside
[`DockerCli::stop`], so the tolerance is reachable **without a daemon**: the
branch that matters is one a real runtime only produces in a race, and a
gated test is the wrong and only place it could otherwise be observed.

## `fn mount_argument(mount: &runtime::Mount) -> String {`

One `--mount` argument.

## `fn mount_argument(mount: &runtime::Mount) -> String` › `parts.push("type=tmpfs".to_owned());`

No source and no name: the surface exists only inside this
container and dies with it.

## `mod fake;`

-- test-only declarations ----------------------------------------------
At the BOTTOM, deliberately: `effects::production_region` cuts a source at
its first `#[cfg(test)]`, so a test module declared above would remove every
funnel in this file from the census that proves the Container group has one
(`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).

## `impl DockerCli {`

One `docker` subcommand, raw, for the Docker-gated tests only.

Two of them need what the seam deliberately does not expose: the daemon's
**verbatim** stderr for an already-stopped container, so the transcribed
table in `tests` can be checked against the live daemon rather than against
itself; and a container carrying an **anonymous** volume, which `CreateSpec`
cannot express because `Mount::Volume` requires a name.

Below the `#[cfg(test)]` cut deliberately. `effects::production_region` stops
at the first `#[cfg(test)]` in a file, so this is invisible to every source
census — including `exec::tests::the_container_subtree_can_only_inspect_a_volume`,
which is the census that keeps production able to *inspect* a volume and
nothing else. A test fixture is not a production capability, and this is
where the tree draws that line.
