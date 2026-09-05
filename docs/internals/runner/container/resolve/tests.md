# `src/runner/container/resolve/tests.rs`

Extended notes for [`src/runner/container/resolve/tests.rs`](../../../../../src/runner/container/resolve/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Lane B's suite: container `RunnerPolicy` resolution, the rebuild-from-record
path, and the schema-1..3 container refusal.

Kept out of `resolve.rs` so `effects::production_region` — which cuts a
source at its **first** `#[cfg(test)]` — sees that module whole
(`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`).

## `#![allow(clippy::disallowed_methods, clippy::disallowed_types)]`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, by
attachment to `src/runner/container.rs` -- the same shape
`src/runner/container/tests.rs` and `src/runner/container/census/tests.rs`
have.

`PR6-LANEF-004`: this file states its level **of its own** rather than
inheriting the Container funnel's inner `#![allow(...)]` through the module
tree. `resolve.rs`, the production half, carries `#![deny(...)]` for all
three and reaches no denied primitive at all.

WHAT IT NEEDS THE ALLOW FOR, and the residual is stated rather than implied:
it builds real temporary Git repositories (`std::process::Command` running
`git`, `fs::write`, `fs::create_dir_all`, `fs::remove_dir_all`) and wraps a
`ContainerRuntime` whose four effectful methods it delegates. It is the one
child of this directory that allows `clippy::disallowed_types` as well as
`clippy::disallowed_methods`, so a `std::process::Command` here is NOT a
build error the way it is in the two sibling test modules -- a real
difference, recorded here and in `effects/allowlist.toml` instead of being
left to a reviewer to discover. `src/events/log/tests.rs` and
`src/engine/tests.rs` are the precedent for a test module needing both.
`clippy::disallowed_macros` is re-denied, so a `println!` is still an error.

## `const REFERENCE: &str = "upstroke/ci:3.2";`

---------------------------------------------------------------------------
Fixtures
---------------------------------------------------------------------------

## `fn volumes() -> BTreeMap<String, String> {`

The two credential volumes, as an independent table.

Written out rather than read back from a `RunnerSelection`, so a test that
compares a record against this is comparing it against a value nothing under
test produced.

## `fn selection() -> RunnerSelection {`

The `[runner]` selection every fixture starts from.

## `fn ready_runtime() -> (FakeRuntime, ContainerTrace) {`

A runtime holding the image at [`IMAGE_ID`] under [`REFERENCE`], with both
credential volumes present. Every refusal fixture below is this, minus one
thing.

## `fn recorded() -> RunnerPolicy {`

The record a first incarnation would have written, built by hand from
INV-23's field list rather than by calling `resolve_container`.

## `struct RecordingPreflight {`

A [`RunnerPreflight`] that records what it was asked and can be armed to
refuse.

It also snapshots the runtime trace **at the moment it is called**, which is
what makes "before any spawn" a statement about a sequence rather than about
a boolean: the snapshot is the prefix of runtime operations that had already
happened when the first spawn was about to occur.

## `fn the_resolved_container_policy_is_the_record_inv23_describes() {`

---------------------------------------------------------------------------
1. Resolution by read-only inspection
---------------------------------------------------------------------------

## `fn the_resolved_container_policy_is_the_record_inv23_describes() {`

The five obligations `pr_sequence[7].scope` packs into one sentence, each
read off the resolved record and compared against an independent value.

Second field held constant: the runtime's whole state — one image, one tag,
both volumes — is identical for every assertion. What varies is which field
of the record is being asked about.

## `fn the_resolved_container_policy_is_the_record_inv23_descri…` › `assert_eq!(policy.policy, RunnerContract::ContainerV1);`

"policy container-v1" — the mount, environment, Git-view and supervision
contract this binary implements for that kind.

## `fn the_resolved_container_policy_is_the_record_inv23_descri…` › `policy`

A run must not start with a record its own resume would reject.

## `fn the_recorded_reference_is_the_operators_and_never_the_runtimes() {`

The recorded reference is the one an operator wrote, never one the runtime
volunteered.

`ImageInspection` carries **every** reference the runtime says resolves to
the id. A resolver that took the record's `reference` from there would make
the record its own oracle and "the recorded reference now names another
image" unconstructible — `runtime.rs` says so in as many words, and this is
the assertion behind it.

Second field held constant: the id and digest, which are the runtime's in
both cells.

## `fn the_recorded_reference_is_the_operators_and_never_the_ru…` › `runtime.tag("mirror.example/upstroke:latest", IMAGE_ID);`

The same image, additionally tagged twice under names nobody configured.

## `fn resolution_issues_only_read_only_operations_in_the_scopes_order() {`

Resolution issues read-only operations only, in the order the scope names.

"Before any lock or effect" has two halves and this is the effect half:
every operation the resolution performs is one [`RuntimeOp::is_effect`]
calls false. Asserted as the **sequence**, not as a set, because
`probe → reference → volumes` is what "the runtime must already hold the
image and the volumes must exist" means in order.

Second field held constant: the runtime is ready in every cell, so the trace
is the full happy-path sequence rather than a truncated one.

## `fn resolution_issues_only_read_only_operations_in_the_scope…` › `RuntimeOp::InspectVolume,`

One per credential volume, in the map's sorted order.

## `fn resolution_issues_only_read_only_operations_in_the_scope…` › `let asked: Vec<String> = trace`

The volumes are asked about by name, and both of them are.

## `fn a_runtime_reporting_no_digest_and_one_reporting_an_empty_string_both_resolve_to_none() {`

A digest the runtime does not report, and one it reports as an empty string,
both resolve to `None` — and the *record* still separates them.

Two halves, because collapsing them at the inspection seam is only safe if
the encoding underneath has not collapsed them too. INV-23 compares four
copies of this record exactly; a canonicalisation in which `None` and
`Some("")` agree would let a marker attest a record the fold calls different.

Second field held constant: the image id and the reference, identical in all
three cells, so what varies is only what the runtime said about the manifest.

## `fn a_runtime_reporting_no_digest_and_one_reporting_an_empty…` › `let mut absent = recorded();`

And the encoding underneath has not collapsed them, so a record that
acquired an empty digest by some other route is still a different record.

## `fn resolution_refuses_each_of_its_faults_before_any_lock_or_effect() {`

The three resolution refusals, each with a control that differs in exactly
one thing, and each proved to have reached no lock and no effect.

The grid is **{fault} × {phase = resolve}** with the selection held constant;
`the_rebuild_refuses_each_of_its_faults_before_any_spawn` is the same faults
at the other phase, and
`resolution_and_rebuild_ask_different_questions_of_the_runtime` is the cross.

Each cell asserts the **typed** refusal rather than `is_err()`: a test that
only proves an error came back is green when the fixture is misspelt, which
is the failure this grid is built against. The `none` cell is the control
that says the fixture would otherwise resolve.

## `fn resolution_refuses_each_of_its_faults_before_any_lock_or…` › `const FAULTS: &[Fault] = &[`

Written out so a fault that stops being driven is a compile-time hole
rather than a silently shorter grid.

## `fn resolution_refuses_each_of_its_faults_before_any_lock_or…` › `Fault::ReferenceAbsent => runtime.tag(REFERENCE, "sha256:not-a-real-id"),`

The image is still in the table; only the tag is gone, so this
cell is about the *reference* and nothing else.

## `fn resolution_refuses_each_of_its_faults_before_any_lock_or…` › `assert!(`

No effect, ever, on any path.

## `fn resolution_refuses_each_of_its_faults_before_any_lock_or…` › `assert_eq!(trace.ops(), vec![RuntimeOp::Probe]);`

Nothing was asked after the runtime failed to answer.

## `fn resolution_refuses_each_of_its_faults_before_any_lock_or…` › `assert!(!trace.ops().contains(&RuntimeOp::InspectVolume));`

The volumes were never asked about: the refusal is the end of
the command, not a step in it.

## `fn resolution_refuses_each_of_its_faults_before_any_lock_or…` › `assert_eq!(`

The *other* volume was asked about and answered yes, so this
cell is about one absent volume and not about volumes at all.

## `fn the_pre_lock_sequence_reaches_no_lock_no_marker_and_no_probe_when_resolution_refuses() {`

"Before any lock or effect", as one ordered sequence.

The effect half is asserted above. This is the **lock** half, and it needs a
caller: `resolve_container` cannot take a lock — it is handed a runtime and
values and has no path, no run directory and no runner — but that is an
argument from a signature, and INV-23's clause is about an order.

So the documented pre-lock sequence is driven against one log that both the
runtime and the driver write into: resolution's operations and the caller's
worktree lock, public directory, marker and first probe, interleaved in the
order they actually happened. A refusal must leave the last four absent.

Second field held constant: the driver is identical in both cells: only the
runtime's readiness varies.

## `fn the_pre_lock_sequence_reaches_no_lock_no_marker_and_no_p…` › `let resolved = resolve_container(&runtime, &selection());`

INV-23's pre-lock order, as a caller performs it: "resolved once by
read-only inspection before the worktree lock (before the public
directory, the marker, and any probe)".

## `fn the_pre_lock_sequence_reaches_no_lock_no_marker_and_no_p…` › `let first_after = entries`

And every one of them is after every inspection.

## `fn resolution_and_rebuild_ask_different_questions_of_the_runtime() {`

Resolution and rebuild ask **different** questions, and the four cells of
{reference present} × {recorded id present} prove it.

`expected_failures_refusals[1]`'s two sets of three are not the same three:
resolution looks up the **reference**, the rebuild looks up the **recorded
id**. A seam with one image question would make the two indistinguishable,
and a suite that never crossed them would not notice.

Second field held constant: the credential volumes, present in every cell,
so no cell can pass or fail for the volume reason.

## `fn resolution_and_rebuild_ask_different_questions_of_the_ru…` › `let target = if id_present { IMAGE_ID } else { OTHER_ID };`

A reference that resolves — to *another* image when the
recorded id is gone, which is the only way to have one
without the other.

## `fn the_resolved_records_digest_moves_with_the_id_the_runtime_reported() {`

Two runtimes holding the same reference at different ids resolve to two
execution identities.

The digest is what the marker, the owner record and every container intent
carry, so "the runtime's immutable image id" being in the record is only
worth something if moving it moves the digest. Pinned against
`canonical_bytes` written by hand in `crate::runner::policy`, not round-
tripped here.

Second field held constant: the reference and the volume set, identical in
both cells.

## `fn the_only_volume_operation_the_seam_has_is_a_read_only_presence_question() {`

R20 is operator-owned, and the seam makes that structural rather than
merely observed.

The row says `persistent_output` in all five `at_run_end` outcomes and
"never created or pruned by a run" — and a run that tidied a volume it
mounted would destroy operator credentials, which CLIs rotate on use, so a
discarded rotation forces re-login. `ContainerRuntime` has exactly **one**
volume operation, it is read-only, and there is therefore no create or prune
for this module to reach: the enumeration is derived from `RuntimeOp::ALL`
rather than from a list somebody remembered to write.

The runtime half of the same claim is lane C's
`r20_is_persistent_output_in_every_at_run_end_outcome_and_no_census_path_touches_it`;
this is the resolution half.

## `fn the_only_volume_operation_the_seam_has_is_a_read_only_pr…` › `let (runtime, trace) = ready_runtime();`

And this module reaches nothing else: resolution and rebuild both.

## `fn the_only_volume_operation_the_seam_has_is_a_read_only_pr…` › `assert!(runtime.volume_present(CLAUDE_VOLUME).expect("inspects"));`

The volume is still there afterwards, which is the thing the row is
actually about.

## `fn a_configured_mount_does_not_reach_the_recorded_execution_identity() {`

A `[runner] mounts` entry changes the boundary and **does not** move the
recorded execution identity.

Stated as an assertion rather than left implicit, because it is a real gap
and a reviewer should find it named rather than derive it. INV-23's
`RunnerPolicy` has four fields — kind, policy, image, credential volumes —
and none of them is a mount list, so two runs whose `[runner]` sections
differ only in `mounts` record the same runner and carry the same
`runner_policy_sha256`. Filed as `PR6B-MOUNTS-ARE-NOT-EXECUTION-IDENTITY`.

Second field held constant: everything but `mounts`, so the equality below
is about that field alone.

## `fn a_configured_mount_does_not_reach_the_recorded_execution…` › `let mut warnings = Vec::new();`

And a rebuild therefore cannot warn about a mount that changed: the
comparison has no field for it.

## `fn resolution_refuses_a_selection_that_does_not_ask_for_a_container() {`

A selection this module is not for.

## `fn the_rebuild_returns_the_recorded_runner_exactly_however_the_config_differs() {`

---------------------------------------------------------------------------
2. The rebuild-from-record path
---------------------------------------------------------------------------

## `fn the_rebuild_returns_the_recorded_runner_exactly_however_the_config_differs() {`

However today's config differs, the rebuilt runner is the recorded one,
field for field.

"warns naming the difference and **is ignored**" — the second half is the
one a plausible implementation drops, by merging the config in "where it does
not conflict". `run_resumed(4).runner` must equal `run_started(4).runner`
exactly, so anything today's config reaches is a `FoldError` later.

Second field held constant: the runtime, ready in every cell, so no cell can
succeed or fail for an inspection reason.

## `fn a_config_that_differs_warns_naming_the_field_that_moved() {`

A config that differs warns **naming the field that moved**, and the fields
it can name are ST-20's three.

"warns naming the difference" is a real assertion and "config differs" fails
it: PR3 built `RunnerPolicy::difference()` to name *which* field moved
precisely so a warning could. The grid drives one config edit per field and
asserts the named field, and then asserts the **set** of reachable fields —
so a comparison that started reporting `image id` (which no operator can
edit and no operator can fix) fails here.

Second field held constant: the record, identical in every cell.

## `fn a_config_that_differs_warns_naming_the_field_that_moved()` › `let mut reference_and_volumes = selection();`

**Two fields at once.** Every other cell moves exactly one, and an
implementation that answered `None` whenever more than one had moved
would pass all of them while a two-field edit warned about nothing
(`PR6-CORRECTNESS-015`). `RunnerPolicy::difference` reports the first
field in its own order, which for these two is the reference.

## `fn a_config_that_differs_warns_naming_the_field_that_moved()` › `let mut only_reference = selection();`

The control: each half of that edit is independently a difference, so the
cell below is genuinely the *intersection* and not one edit with a
no-op beside it.

## `fn a_config_that_differs_warns_naming_the_field_that_moved()` › `assert_eq!(`

ST-20: "a `[runner]` config that differs (kind, image reference, or
credential volumes)". Three, and only three, are reachable.

## `fn an_absent_runner_section_warns_only_when_the_record_is_not_the_default() {`

An absent `[runner]` section is a **selection**, and whether it differs
depends on what the run recorded.

The intersection **{section present or absent} × {recorded kind}**, which is
the cell `PR6-CORRECTNESS-015` found missing. This test previously asserted
only the first axis — "absent never warns" — against a **container** record,
and so pinned the defect: a run that recorded a container runner and whose
`[runner]` section was subsequently **deleted** is running under an
effective selection of host/default, which is as real an edit as changing
`kind` in place, and it warned about nothing.

The claim the original test was protecting is still here and is still true,
and it is the **host-record** row: a repository that never configured a
runner is not told its runner kind moved. It holds because
`RunnerSelection::host_default()` renders to exactly what a host run
records, not because a flag suppresses the comparison — which is the
difference between a guarantee and a silence.

Second field held constant: the runtime, ready in every cell, so no cell can
warn or not warn for an inspection reason.

## `fn an_absent_runner_section_warns_only_when_the_record_is_n…` › `let cells: Vec<(&str, RunnerPolicy, RunnerSelection, Option<RunnerField>)> = vec![`

{section} x {record}: four cells, and only the three that are a real
difference warn.

## `fn a_moved_or_vanished_reference_warns_and_the_rebuild_keeps_the_recorded_id() {`

A reference that now names another image warns and the recorded id is used;
a reference that no longer resolves at all warns too, and neither refuses.

`expected_failures_refusals[1]` names the **id** and not the reference, and
INV-23 says "so a moved reference cannot change what executes". The grid is
{reference resolves to the recorded id, to another id, to nothing} with the
recorded id present in all three — that is the second field, held constant,
and it is what makes every cell a rebuild that succeeds.

## `fn a_moved_or_vanished_reference_warns_and_the_rebuild_keep…` › `None => runtime.move_tag(REFERENCE, "sha256:nothing-here"),`

Point the tag at an id the table does not hold, which is how a
reference stops resolving while the recorded id stays present.

## `fn the_rebuild_refuses_each_of_its_faults_before_any_spawn() {`

The three rebuild refusals, each **before any spawn**, each with a control.

Two independent witnesses of the same ordering predicate, because one of them
could be a lie: [`RebuildRefusal::before_any_spawn`] is what the code says,
and the preflight's own call count is what actually happened. A refusal that
classified itself correctly while having already spawned fails the second.

The grid is **{fault} × {phase = rebuild}**, the record held constant across
every cell.

**Today's config differs in every cell, deliberately.** A refused rebuild
must emit no warnings — the refusals come first and a warning about a config
difference describes a run that is about to continue — and with an identical
config there is nothing for the warning block to say, so the assertion holds
vacuously and a mutation that hoisted the warnings above the refusals
survives. Measured: it did (M15). The control at the end of the test proves
the same `today` *does* warn when the rebuild succeeds, so the emptiness
above is about the ordering rather than about the fixture.

## `fn the_rebuild_refuses_each_of_its_faults_before_any_spawn()` › `let today = RunnerSelection {`

Differs from the record in its image reference, so `configured_difference`
has something to name in every cell.

## `fn the_rebuild_refuses_each_of_its_faults_before_any_spawn()` › `Fault::RecordedIdAbsent => {`

The reference still resolves; only the recorded id is gone, so
this cell is about the id.

## `fn the_rebuild_refuses_each_of_its_faults_before_any_spawn()` › `let preflight = RecordingPreflight::accepting(ContainerTrace::off());`

Drive the replacement rather than the ready fixture: it holds
the reference and not the recorded id.

## `fn the_rebuild_refuses_each_of_its_faults_before_any_spawn()` › `assert!(`

The control for every `warnings.is_empty()` below: this same
`today` warns when the rebuild gets that far.

## `fn a_failing_preflight_probe_refuses_after_every_inspection_and_only_a_spawn_observes_it() {`

The fourth behaviour: a shell or CLI that fails inside the recorded image is
observed **only** by a spawn, and refuses on the other side of the split.

The two arms of [`RebuildRefusal`] are the contract's own refusal split, and
this is the arm that is not `before_any_spawn`. The preflight's snapshot of
the trace at call time is the ordering evidence: every inspection had already
happened when the first spawn was about to.

Second field held constant: the runtime, which is ready in both cells — so
what varies is only what the *process* did, exactly as
`non_goals[2]` ("non-spawn shell/CLI presence inspection") requires.

## `fn a_failing_preflight_probe_refuses_after_every_inspection…` › `let calls = preflight.calls();`

The observation is a spawn, and it came after every inspection.

## `fn a_failing_preflight_probe_refuses_after_every_inspection…` › `assert!(trace.ops().iter().all(|op| !op.is_effect()));`

Nothing was created, started, stopped or removed on the way.

## `fn the_rebuild_refuses_an_incomplete_record_before_asking_the_runtime_anything() {`

A record the fold would refuse never reaches an inspection.

## `fn legacy_container_selection_refused_before_effects() {`

---------------------------------------------------------------------------
3. `[runner]` config and the schema-1..3 refusal
---------------------------------------------------------------------------

## `fn legacy_container_selection_refused_before_effects() {`

**T-CONTAINER (13).** `[runner] kind = "container"` under a schema-1..3
fresh run **or** resume is a config error before any effect.

Both write commands, because `expected_failures_refusals[0]` names both and a
suite that covers one covers half. The grid is
**{command = run, resume} × {`[runner]` = host, container}**, in two phases
with two different witnesses, because "before any effect" is an ordering and
an ordering needs something on the far side of it to be visible.

**Phase A — a competing worktree lease is held for the whole grid.**
`WorktreeLock::acquire_in` is the first effect either command performs
(`coordinator.rs`: "every read-only refusal precedes every lock";
`resume.rs` marks the line after `validate_inputs` "the first effect of the
command"). So with the lease held by this test:

* `kind = "host"` fails with the **lease** refusal — it reached the lock;
* `kind = "container"` fails with the **config** error — it did not.

One fixture, one held lease, two configs, two *different* failures. A
refusal moved after the lock fails this by turning the container cell into a
lease refusal, and a test that only asserted "an error came back" would not
notice.

**Phase B — the lease is released, and the tree is inspected.** No run
directory under either half of the §15 split, no `run.lock`, no branch, no
container intent namespace, and — for the fresh run — no adapter ever
resolved, so no pre-flight probe could have been spawned. The `host` control
of phase B is what proves the run command reaches pre-flight at all.

The whole-tree half of ST-16 (i)'s second clause is
[`no_module_outside_the_container_runner_writes_a_container_intent`].

## `fn legacy_container_selection_refused_before_effects()` › `let seeded: Vec<(u32, String)> = [1_u32, 2, 3]`

One seeded run per legacy schema: `EngineLimits::for_resume` reads the
header's schema, so a suite driving only one of the three has driven
only one reading.

## `fn legacy_container_selection_refused_before_effects()` › `{`

-- phase A: the lease is the far side of the ordering ---------------

## `fn legacy_container_selection_refused_before_effects()` › `let adapters = RecordingAdapters::default();`

-- phase B: nothing was created ------------------------------------

## `fn every_engine_limits_reading_refuses_a_container_selection() {`

Every reading of `[engine]`'s limits refuses a container selection.

`EngineLimits` is what distinguishes "a run being created now" from "a
sequential run's resume", and `expected_failures_refusals[0]` names both. The
grid is written out with an exhaustive `match` beside it, so an additional variant
is a compile error here rather than a reading that quietly escapes.

## `fn every_engine_limits_reading_refuses_a_container_selection()` › `EngineLimits::SequentialResumeWithRecordedGates => {}`

Exhaustive on purpose: a new variant must be classified here.

## `fn every_engine_limits_reading_refuses_a_container_selectio…` › `fs::write(dir.join("upstroke.toml"), "[runner]\nkind = \"host\"\n").expect("config");`

The control, byte-identical apart from the kind: the same reading
accepts a host selection, so the refusal is about the value.

## `fn no_module_outside_the_container_runner_writes_a_container_intent() {`

ST-16 (i)'s second clause: **no legacy process ever writes a container
intent** — a claim about the whole tree, not about the parser.

A census rather than a behavioural test, because the clause is a universal:
it is not satisfied by showing that one legacy path does not write one. The
set of files whose production region names the intent record or the funnel
that writes it is written out here; a legacy module that acquired one fails
this by name.

The control at the bottom is what stops this from becoming
`PR6F-DOCKER-CENSUS-CANNOT-FAIL`: the census must still be finding the files
it is supposed to find, or "no offenders" means "the needle is unfindable".

## `fn no_module_outside_the_container_runner_writes_a_containe…` › `const WRITERS: &[&str] = &[`

Everything that could put a record in `<R>/containers`.

Container-specific by construction: `write_intent` alone would also match
`crate::workspace_manager`'s **worktree** intent (DESIGN.md:234, a
different R-row and a different namespace), and a census that reported
that would be a census nobody could keep green. Measured — it did.

## `fn no_module_outside_the_container_runner_writes_a_containe…` › `const ALLOWED: &[(&str, &str)] = &[`

The files allowed to name one, each with the reason.

## `fn no_module_outside_the_container_runner_writes_a_containe…` › `const EXCLUDED: &[&str] = &[`

The files left out of the scan, each an **exact** repo-relative path.

Every one of them is asserted to exist below: an exclusion that names no
file excludes nothing today and silently excludes whatever is created at
that path tomorrow.

## `fn no_module_outside_the_container_runner_writes_a_containe…` › `if EXCLUDED.contains(&relative.as_str()) {`

Test modules of the container subtree drive the funnel and name its
types; they are excluded by name, so a new one is a change here.

**Every exclusion is an exact path.** Three of them were
`starts_with` — `src/runner/container/tests`,
`.../census/tests`, `.../resolve/tests` — under a comment claiming
the opposite, and a prefix widens to every sibling whose name begins
the same way. Measured: a `pub fn write_one(intent: &ContainerIntent)`
module failed this census as `src/runner/container/rogue.rs` and
passed it as `src/runner/container/tests_of_the_funnel.rs`. The list
is `EXCLUDED` above, every entry of it is asserted to name a file that
exists, and the match is `==`.

`src/effects/tests.rs` is the fourth, added by PR6 lane E. It is the
`#[cfg(test)] mod tests;` of `src/effects.rs` — a test module, never
reachable from production — and it names `ContainerName` for one
reason: `the_view_directory_has_one_definition_in_the_tree` calls
`exec::view_dir` and `census::view_path` with the same name and
asserts they answer the same path (`PR6E-005`, a divergence that
survived all 1324 tests). It writes no intent and constructs no
container. The exclusion is by exact path rather than by prefix, so
it cannot widen to a sibling.

`src/engine/topology/recover/tests.rs` is the fifth, added by PR7
lane E. It is the `mod tests;` of `src/engine/topology/recover.rs`,
declared under a test configuration and never reachable from
production, and it names these types for one reason: recovery step
(a)'s row is "containers **incl. every earlier incarnation of this
run** under `<R>/containers`", so
`resume_of_nondefault_root_run_reclaims_earlier_incarnation_intents_in_recorded_root`
has to *plant* a dead incarnation's intent for the census to find,
and it plants it through this very funnel rather than with `fs`. A
fixture that writes an intent for the census to reclaim is the same
category as `src/runner/container/census/tests.rs` above. The
exclusion is by exact path, so it cannot widen to a sibling.
`src/engine/topology/create/tests.rs` is the seventh, added by PR7
lane B, on the same terms. It is the `#[cfg(test)] mod tests;` of the
schema-4 creator. It names `ContainerIntent`, `ContainerName` and
`containers_dir` to **read back** the intent a containerized probe
left after a kill —
`probe_intent_carries_runner_policy_digest_matching_owner_record` and
`kill_during_containerized_probe_...` — and writes none: the one that
exists was written by `ContainerRunner` through the funnel. Exact, so
it cannot widen to `src/engine/topology/create.rs`, which is
production and is scanned.

## `fn no_module_outside_the_container_runner_writes_a_containe…` › `let scanned_text = crate::effects::blank_comments_and_strings(&source);`

The WHOLE file, comments and strings blanked — deliberately **not**
`effects::production_region`. That helper cuts a source at its first
`#[cfg(test)]`, and `src/engine/coordinator.rs` has a `#[cfg(test)]
use` on **line 36 of 1599**: 97% of the schema-1..3 coordinator, and
96% of `attempt.rs` and `resume.rs`, are outside it. A prohibition
about the legacy engine that could not see the legacy engine would be
the vacuous census this project has already paid for twice
(`PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN`, `PR6F-DOCKER-CENSUS-CANNOT-FAIL`).
Measured: with `production_region` a planted `ContainerIntent` in
`run_harness_inner_on` SURVIVED. Filed as
`PR6B-PRODUCTION-REGION-CUT-AT-A-CFG-TEST-USE`.

Scanning the whole file is strictly stronger for a prohibition, and
its only cost is that a *test* naming these types is an offender —
which is why the container subtree's test files are excluded above,
by name.

## `fn no_module_outside_the_container_runner_writes_a_containe…` › `if relative == "src/engine/coordinator.rs" {`

The domain control, and the reason this census is not the one above:
the body of the legacy coordinator must be inside what was scanned.

## `fn no_module_outside_the_container_runner_writes_a_containe…` › `assert_eq!(`

The needle control, in two halves, because the file half alone does not
hold. Without either, a census whose needles stopped matching would be
silently green — `PR6F-DOCKER-CENSUS-CANNOT-FAIL`, measured this slice, in
this repository, on this clause.

(a) Every allowed file was reached.

## `fn no_module_outside_the_container_runner_writes_a_containe…` › `assert_eq!(`

(b) Every needle matched something. This half was missing, and (a) does
not imply it: `ContainerName` alone appears in all four allowed files, so
the other four `WRITERS` could each have stopped matching anywhere in the
tree and (a) would still have counted four. Measured: rewriting the two
`crate::runner::container::write_intent(` call sites as
`super::super::write_intent(` — a legal, meaning-preserving refactor —
left the needle `container::write_intent` matching nothing in the scanned
set, and this census stayed green.

## `fn the_resolution_module_names_no_lock_no_write_and_no_spawn() {`

This module reaches no lock, no filesystem and no spawn.

The structural half of "before any lock or effect": the argument that
`resolve_container` cannot take a worktree lock is an argument about what it
is given, and this is that argument executed. A call planted in the
production region fails it.

## `fn the_resolution_module_names_no_lock_no_write_and_no_spaw…` › `assert!(`

The control: the census is reading the module and not an empty string.

## `struct SharedLog(std::sync::Arc<Mutex<Vec<String>>>);`

---------------------------------------------------------------------------
Test-only substrate
---------------------------------------------------------------------------

## `struct SharedLog(std::sync::Arc<Mutex<Vec<String>>>);`

One ordered log both a runtime and its caller write into.

## `struct LoggingRuntime {`

A [`ContainerRuntime`] that records every call into a log the caller also
writes into, so "before the worktree lock" is one sequence.

## `struct RecordingAdapters {`

An [`AdapterSource`] that records every id it was asked for and hands back
nothing.

The recording is the point: an empty log proves the command refused before
pre-flight ever tried to resolve an agent, which is what "before any effect"
buys and what a refusal returning the right message would not.

## `fn temp_repo(tag: &str) -> PathBuf {`

A clean repository with a two-task plan, seeded and committed.

## `fn empty_pools(dir: &Path) -> PathBuf {`

An explicit pools file with no pools in it — never the operator's real one.

## `fn seed_legacy_run(repo: &Path, run_id: &str, private: &Path, schema: u32) {`

A legacy run directory with a `run_started` header and nothing else.

Enough for `resume` to reach `validate_inputs`, which is the statement under
test: `resume.rs` marks the line after it "the first effect of the command".
`schema` is 1, 2 or 3 — `expected_failures_refusals[0]` says "a schema-1..3
fresh run **or** resume", and the reading a resume gets is chosen by
`EngineLimits::for_resume(header_schema)`, so all three are driven.

## `fn seed_legacy_run(repo: &Path, run_id: &str, private: &Pat…` › `normalized_plan_digest: Some(format!("sha256:{}", "0".repeat(64))),`

Both required by `ensure_supported_schema` for a schema-3 header,
which runs *before* `validate_inputs` — so without them a schema-3
resume never reaches the refusal under test. Schemas 1 and 2 accept
them and do not require them, so they are set unconditionally.

## `fn rust_sources(dir: &Path) -> Vec<PathBuf> {`

Every `src/**/*.rs`, sorted.
