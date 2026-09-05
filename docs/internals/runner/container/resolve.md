# `src/runner/container/resolve.rs`

Extended notes for [`src/runner/container/resolve.rs`](../../../../src/runner/container/resolve.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The container [`RunnerPolicy`]: resolution by read-only inspection, and the
rebuild-from-record path (INV-23's container portion).

#### Where this sits

[`crate::runner::policy`] owns the *shape* — PR3's one `RunnerPolicy` wire
type, its canonical encoding and its digest — and PR4's `resolve_host()`
beside it. Its doc comment states the boundary this module is on the other
side of: *"the inspection that can fail is the container runner's, and that
is PR6"*. So there is still exactly one record type, one canonical encoding
and one digest; what is here is the two questions a container runner has to
ask a runtime and nobody else does.

#### Read-only, and structurally so

INV-23: the policy *"is resolved once by read-only inspection **before the
worktree lock** (before the public directory, the marker, and any probe; the
runtime must already hold the image and the volumes must exist)"*.

Every function here takes a [`ContainerRuntime`] and values, and nothing
else: no run directory, no lock, no [`crate::runner::Runner`], no path it
could write. `ContainerSite` — the frozen effect inventory's eight container
sites — has **no inspection variant** and this slice may not add one, so an
inspection is not a funnel call and a funnel call is not an inspection. That
is why `probe`, `image_by_reference`, `image_by_id` and `volume_present` are
called directly here while `create`/`start`/`stop`/`remove` cannot be.

#### The refusal split, which is the contract's own phrase

`expected_failures_refusals[1]` is **two sets of three**, at two phases,
with two different ordering predicates:

| phase | refusals | predicate |
|---|---|---|
| resolution | unreachable runtime · image **reference** absent (no implicit pull) · credential volume absent | before any lock or effect |
| rebuild | unavailable runtime · recorded image **id** absent · credential volume absent | before any spawn |

The two sets are **not** the same three questions. Resolution asks the
runtime to turn a reference an operator wrote into an immutable id; a rebuild
already has the id and asks whether the runtime still holds it. A seam that
could only ask about references could not express the second, which is why
[`RuntimeOp::InspectImageById`] exists.

And the fourth rebuild behaviour is on the other side of that split
entirely: *"a recorded shell or agent CLI that fails inside the recorded
image is observed by the RunnerPreflight probes and refuses before any
recovery event or work spawn"*. `non_goals[2]` is "non-spawn shell/CLI
presence inspection" — so this module cannot answer that question at all,
and does not try. [`RunnerPreflight`] is the seam it consumes;
`crate::runner::container::exec` and `crate::runner::host::run_shell_probe`
are what implement it.

## `pub enum InspectionRefusal {`

---------------------------------------------------------------------------
Refusals
---------------------------------------------------------------------------

## `pub enum InspectionRefusal {`

Why a container runner could not be established by read-only inspection.

A typed enum and not a formatted string, because the six refusals
`expected_failures_refusals[1]` enumerates have to be told apart by a test
that is not reading prose: a suite asserting `is_err()` holds none of the
orderings and is green when the fixture path is a typo.

## `pub enum InspectionRefusal` › `RuntimeUnavailable {`

The runtime could not be reached for one of the inspections.

## `pub enum InspectionRefusal` › `RuntimeFailed {`

The runtime was reached and the inspection failed. A different answer
from unreachable, and kept apart for the reason
[`crate::runner::container::runtime`] gives: a daemon that answers `ps`
and fails `inspect` classifies as reachable under one global boolean and
carries the failure past the point the refusal exists to hold.

## `pub enum InspectionRefusal` › `ImageReferenceAbsent { reference: String },`

`non_goals[1]`: no implicit image pull. A reference the runtime does not
already hold is a refusal, never a fetch.

## `pub enum InspectionRefusal` › `ImageIdAbsent { id: String },`

The rebuild's question, and a different one: the recorded **id**.

## `pub enum InspectionRefusal` › `ImageNotIdentified { reference: String },`

The runtime answered, and its answer identifies nothing.

## `pub enum InspectionRefusal` › `CredentialVolumeAbsent { agent: String, volume: String },`

R20 is operator-owned and "never created or pruned by a run", so an
absent volume is a refusal rather than something to create.

## `pub enum InspectionRefusal` › `NotAContainerSelection { kind: RunnerKind },`

A guard, not a contract refusal: this module was handed a selection that
does not ask for a container.

## `pub enum InspectionRefusal` › `SelectionWithoutImage,`

A guard: a container selection with no image reference to look up.

## `pub enum InspectionRefusal` › `RecordIncomplete(RunnerRecordDefect),`

The record this resolution produced is one PR3's fold would refuse.
Checked here for the reason `resolve_host` checks it: a run must not
start with a record its own resume would reject.

## `impl InspectionRefusal` › `pub const fn is_runtime_unavailable(&self) -> bool {`

Whether the runtime itself was the thing that could not answer.

## `pub enum RebuildRefusal {`

Why a recorded container runner could not be re-established.

**The variant is the refusal split.** `Inspection` is the three refusals that
happen with no spawn at all; `Preflight` is the one that requires a spawn
because "this is the only observable boundary for shell/CLI availability: no
non-spawn inspection is claimed". [`Self::before_any_spawn`] is that
distinction as a value, so a test can assert the predicate rather than the
prose.

## `pub enum RebuildRefusal` › `Inspection(#[from] InspectionRefusal),`

Refused by read-only inspection, **before any spawn**.

## `pub enum RebuildRefusal` › `Preflight(#[source] UpstrokeError),`

Refused by the RunnerPreflight probe spawns — the only observation of
shell and CLI availability inside the boundary.

## `impl RebuildRefusal` › `pub const fn before_any_spawn(&self) -> bool {`

Whether this refusal was reached without spawning anything.

## `pub trait RunnerPreflight {`

---------------------------------------------------------------------------
The RunnerPreflight seam
---------------------------------------------------------------------------

## `pub trait RunnerPreflight {`

The one observation of shell and agent-CLI availability inside the boundary.

INV-23: the RunnerPreflight is *"one non-slotted shell probe (the recorded
shell executing `exit 0`) and one slotted probe per recorded agent, each a
registered invocation through the run's Runner"*, and it *"is the only
observation of shell and CLI availability inside the boundary"*.

This is a **seam and not an implementation**, and deliberately so: the probes
are `crate::runner::host::run_shell_probe` over a
`crate::runner::container::exec::ContainerRunner`, and building that runner
needs a run identity, a private root and a slot pair that `TopologyRun` owns
at PR7. The rebuild path's job is to *consume* the observation and refuse
correctly, which it can do against a seam.

## `pub trait RunnerPreflight` › `fn certify(&self, policy: &RunnerPolicy) -> Result<(), UpstrokeError>;`

Certify the recorded shell and every recorded agent CLI inside the
recorded image, by spawning them through the rebuilt runner.

### Errors

[`UpstrokeError::Refused`] when a probe does not come back clean. The
caller classifies it; `rebuild_from_record` turns it into
[`RebuildRefusal::Preflight`], which is the arm that did spawn.

## `pub fn resolve_container(`

There is deliberately no `SkipPreflight` implementation here. A caller that
has not wired the probes calls [`rebuild_by_inspection`], whose name says
which three of the four rebuild behaviours it holds; a no-op `RunnerPreflight`
would let the same caller call [`rebuild_from_record`] and appear to have all
four.

## `pub fn resolve_container(`

---------------------------------------------------------------------------
Resolution
---------------------------------------------------------------------------

## `pub fn resolve_container(`

Resolve the container runner's policy by read-only inspection.

`decisions.pr_sequence[7].scope`, which is five obligations in one sentence:

> Container variant resolved by read-only inspection (runtime reachable;
> image reference already present in the runtime — no implicit pull — its
> immutable image id and manifest digest **when reported**, per-agent
> credential volume names present by volume inspection, policy
> `container-v1`)

The inspections happen in that order and the trace records them, so "the
runtime must already hold the image and the volumes must exist" is a
sequence and not a set. The first failure refuses: the answer to "may this
run start" does not get better by asking the remaining questions, and asking
them would put runtime calls after a refusal that is supposed to be the end
of the command.

**The recorded reference is the operator's, not the runtime's.** An
[`ImageInspection`] carries every reference the runtime says resolves to the
id — possibly none, possibly several, possibly one nobody wrote — and taking
the record's `reference` from there would make the record its own oracle and
"the recorded reference now names another image" unconstructible.

### Errors

The first [`InspectionRefusal`] the inspection produces.

## `runtime.probe().map_err(InspectionRefusal::from_runtime)?;`

(1) runtime reachable.

## `let inspection = runtime`

(2) the reference is already present — no implicit pull.

## `inspect_volumes(runtime, &selection.credential_volumes)?;`

(3) the credential volumes exist.

## `let policy = RunnerPolicy {`

(4) the record: policy `container-v1`, the operator's reference, the
runtime's id, and its digest when it reported one.

## `fn reported_digest(inspection: &ImageInspection) -> Option<String> {`

The manifest digest **when reported**.

INV-23 records `digest: Option<...>` and says "the manifest digest when
reported", and [`ImageInspection::digest`] says `None` when the runtime
reports none. A runtime that answers with an empty string has not reported a
digest — it has answered the question badly — and recording `Some("")` would
put a value in the record that names no manifest while still being a
*present* digest to every reader of the record.

So absent and present-but-empty collapse **here, at the inspection seam**,
and nowhere else. The record's own encoding still separates them
(`crate::runner::policy::canonical_bytes` writes `1:0;` for `None` and
`1:1;0:;` for `Some("")`), because a record that acquired an empty digest by
some other route — a hand-edited `owner.json`, a future runtime — must not
silently equal one that has none. Both halves are asserted; see
`tests::a_runtime_that_reports_no_digest_and_one_that_reports_an_empty_string_both_resolve_to_none`.

## `fn inspect_volumes(`

Every credential volume, in the map's own sorted order.

R20 is `operator_owned` and `persistent_output` in all five `at_run_end`
outcomes — "never created or pruned by a run" — so the only operation this
module performs on a volume is the read-only presence question.

## `pub fn rebuild_by_inspection(`

---------------------------------------------------------------------------
The rebuild-from-record path
---------------------------------------------------------------------------

## `pub fn rebuild_by_inspection(`

Re-establish the recorded runner by inspection alone — three of the four
rebuild behaviours.

From `decisions.sequential_substrate.runner` and INV-23:

| behaviour | what happens |
|---|---|
| today's `[runner]` config **differs** | warn **naming the difference**, and ignore it — the record wins |
| the recorded reference **now names another image** | warn, and use the recorded id |
| the record cannot be re-established (runtime / recorded id / volume) | **refuse, before any spawn** |

The fourth — a recorded shell or CLI that fails inside the image — is not
here and cannot be: it is observed only by spawns. See
[`rebuild_from_record`].

**The record wins, exactly.** The returned policy is the recorded one, field
for field: a run's boundary and image are fixed for its life, so nothing
today's config says may reach the value the next `run_resumed(4).runner` has
to equal.

#### Why refusals come before warnings

Both warnings describe a run that is about to continue. A rebuild that
refuses is not one, and a warning that the operator's config differs from a
record whose run just stopped is noise pointing at the wrong thing. So every
inspection that can refuse runs first, and a refused rebuild emits **no**
warnings at all.

### Errors

The first [`InspectionRefusal`] — an unavailable runtime, a recorded image id
the runtime no longer holds, or an absent credential volume.

## `runtime.probe().map_err(InspectionRefusal::from_runtime)?;`

-- the three refusals, before anything is spawned or warned about ------

## `if runtime`

The **recorded id**, which is a different question from the reference.

## `match runtime`

-- the two warnings, on a rebuild that is going to succeed -------------
A reference that now resolves elsewhere, or no longer resolves at all, is
not a refusal: `expected_failures_refusals[1]` names the *id*, and INV-23
says "a reference that now names another image warns while the recorded
id is used ... so a moved reference cannot change what executes".

## `pub fn rebuild_from_record(`

Re-establish the recorded runner, and certify it with the RunnerPreflight.

This is the whole of INV-23's rebuild: [`rebuild_by_inspection`] for the
three refusals that need no spawn, then the [`RunnerPreflight`] for the one
that does. `expected_failures_refusals[2]`: "a recorded shell or agent CLI
that fails inside the recorded image is observed by the RunnerPreflight probe
spawns (registered container invocations, reclaimed like every probe) and
refuses before any recovery event or work spawn".

**The split is in the control flow, not in a comment.** No probe can run
until every inspection has passed, because the probe call is after the `?`
that carries the inspection refusal out. [`RebuildRefusal::before_any_spawn`]
reports which side of it a refusal came from.

### Errors

[`RebuildRefusal::Inspection`] before any spawn, or
[`RebuildRefusal::Preflight`] from the probe spawns.

## `static EMPTY: BTreeMap<String, String> = BTreeMap::new();`

An empty volume map, for a record whose `credential_volumes` is `None`.

`completeness()` refuses that record before this is reached; the binding
exists so the refusal is the one that reports, rather than a panic.

## `fn configured_difference(record: &RunnerPolicy, today: &RunnerSelection) -> Option<Runner…`

Which field of the recorded runner today's `[runner]` config disagrees with,
if any.

ST-20 names exactly three: *"a resume under a `[runner]` config that differs
(**kind, image reference, or credential volumes**) warns naming the
difference"*. Those are the three a config can move; the immutable image id
and the manifest digest are the runtime's answers and are not written in
`upstroke.toml` at all.

So the comparison is the record with the config's three fields overlaid, and
[`RunnerPolicy::difference`] — PR3's, which *"names **which** field moved
rather than"* merely reporting inequality — is what reads it. Taking the id
and digest from the record rather than leaving them empty is what keeps
[`RunnerField::ImageId`] and [`RunnerField::ImageDigest`] unreachable here:
a warning that said "image id" would be naming a field the operator cannot
edit and cannot fix.

#### An absent `[runner]` section is a selection, not an exemption

`PR6-CORRECTNESS-015`. This began with `if !today.from_config { return None
}`, on the reading that "a resume whose repository never configured a runner
must not be told its runner kind moved". That sentence is true, and the
guard is not what makes it true: [`RunnerSelection::host_default`] — what an
absent section means — renders through [`today_in_record_shape`] as exactly
`{Host, HostV1, None, None}`, which is exactly what a host run records, so
[`RunnerPolicy::difference`] already answers `None` for it. The guard bought
nothing there and cost the case it was hiding: a run that recorded a
**container** runner and whose `[runner]` section was then **deleted**.
Today's effective selection is the host default, which is as real an edit as
changing the kind in place, and the operator got no warning at all — the one
clause `decisions.sequential_substrate.runner` states about a differing
config ("warns naming the difference and is ignored") silently did not apply
to the largest possible difference.

So the comparison is unconditional and the *rendering* carries the rule:
absent means host-default, and a host record with no section still produces
no warning because there is genuinely no difference.

## `fn today_in_record_shape(record: &RunnerPolicy, today: &RunnerSelection) -> RunnerPolicy {`

Today's `[runner]` selection, expressed in the recorded record's shape.

## `fn today_in_record_shape(record: &RunnerPolicy, today: &Run…` › `id: record`

From the record: see `configured_difference`.
