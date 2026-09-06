# `src/runner/container/tests.rs`

Extended notes for [`src/runner/container/tests.rs`](../../../../src/runner/container/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The container substrate's own suite.

Four things this file is organised around, each learned expensively on this
project:

* **Orderings are most of the contract.** "intent synced before docker
  create", "verified before start", "view mounted before start", "stop/rm,
  view removal, intent removal after completion", and reclaim's own five
  steps are each an independently droppable predicate. Every one is asserted
  as a **sequence** taken from [`ContainerTrace`], never as membership.
* **A function may not be its own oracle.** Every expected digest and every
  expected name in this file is a literal, computed out of band with
  `python3 -c 'hashlib.sha256(...)'` against the packet's own template, and
  the tuple that produces it is written beside it.
* **Fixtures vary every independently meaningful field independently**, and
  hostility is asserted as **distinct-value counts**.
* **The dominant defect is two axes covered separately with the intersection
  never built.** Each test below names the second field it holds constant.

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

## `fn scratch(tag: &str) -> PathBuf {`

---------------------------------------------------------------------------
Fixtures
---------------------------------------------------------------------------

## `fn scratch(tag: &str) -> PathBuf {`

A scratch private root, in the idiom of `effects::tests::scratch_dir`.

## `const REPO_KEY: &str = "0123456789abcdef";`

The four name components used across this file, each a distinct value so a
swap between two of them is visible.

## `const IMAGE_ID: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";`

The recorded image id, and a different one, and a third.

## `fn shell_probe() -> InvocationId {`

A shell probe identity. Deterministic across incarnations **by
construction** — `InvocationId::Probe`'s own doc says so — which is why the
container name carries the incarnation.

## `fn agent_probe() -> InvocationId {`

An agent probe identity.

## `fn intent_for(run: &str, incarnation: &str, invocation: &InvocationId) -> ContainerIntent {`

The intent record for `run`/`incarnation`, with every field a distinct
value.

## `fn name_for(run: &str, incarnation: &str, invocation: &InvocationId) -> ContainerName {`

The name for `run`/`incarnation`.

## `fn labels_for(root: &Path, record: &ContainerIntent) -> BTreeMap<String, String> {`

The five labels a container of this run carries.

## `fn spec_for(`

A create spec that asks for `image_id`.

## `struct Fixture {`

A whole plan, plus a fake runtime already holding the recorded image.

## `fn skipped(reason: &str) {`

What a Docker-gated test does when there is no runtime.

It **reads** the reason rather than returning silently, so a skip that had
stopped saying why would not compile. Combined with
[`super::fake::REQUIRE_DOCKER`] — which turns a skip into a failure on a
machine that has Docker — and with
[`every_docker_gated_test_is_named_and_present`], which counts the gated
tests by name, this is the whole of "loud and counted, never silent".

## `fn no_image(reason: &str) {`

What a Docker-gated test does when the runtime holds no usable image.

The second absence, and it is a different one: Docker answers, and there is
nothing to inspect. It is loud under the same variable, because a machine
that has a runtime and no image would otherwise pass three tests that never
touched it.

## `fn at(trace: &ContainerTrace, needle: &str) -> usize {`

Where `needle` first appears in the trace, or a failure naming the whole
sequence — because "x before y" is unreadable when the report is `None`.

## `fn a_pre_clean_refuses_every_name_a_concurrent_run_could_also_ask_for() {`

**Every name a pre-clean touches is scoped to this build slot.**

The class boundary, and it is the *caller* half of `PR7-R3-CONTRACT-001`.
The instance is that `fake::preclean_names` kills by name with no liveness
check, so a name built from a **fixed** repo key is a name a concurrent
suite in another slot asks for too, and the kill lands on that suite's live
container. `exec.rs`'s caller was scoped by `b44040a`;
`census/tests.rs`'s was not, and stayed hostile for four commits.

`a_container_name_is_scoped_to_its_build_slot` asserts the **key** is
per-slot. This asserts the property the pre-clean actually depends on:
that a name it is handed carries that key. The two are different claims —
the first was true the whole time the second was false — and a rule that
callers are told to follow is a rule a caller can be missing, which is why
`preclean_names` now consults [`super::fake::unscoped_names`] rather than
documenting the precondition.

**A liveness check would not have fixed it**, and that is why the boundary
is here: the state the helper exists for is a SIGKILLed run whose container
is still *running*, so "do not kill running ones" defeats the helper
outright.

## `fn a_pre_clean_refuses_every_name_a_concurrent_run_could_also_ask_for() {` › `const STRANGERS: &str = "cccccccccccccccc";`

Not this slot's, whatever slot this is: sixteen hex characters that no
`CARGO_TARGET_DIR` digest and no empty-scope default can equal.

## `fn a_pre_clean_of_a_strangers_name_refuses_before_it_reclaims_anything() {`

**And the helper consults the rule**, rather than stating it in a doc.

The other half of the pair above, and the half that decides whether the
class is closed. `unscoped_names` being correct closes nothing on its own —
`preclean_names`'s doc *already* said "callers must build them from their
own fixed constants", and one of the two callers read that and built a fixed
constant, which is precisely the hostile case. A precondition a caller is
asked to satisfy is one a caller can fail to satisfy.

The refusal is asserted to land **before any reclaim**, through the trace,
because a guard that fires after the `docker kill` has already killed the
stranger's container.

## `fn a_pre_clean_of_a_strangers_name_refuses_before_it_reclaims_anything() {` › `let hook = std::panic::take_hook();`

The panic is expected and its message is the assertion, so the hook is
silenced: an expected panic printing a backtrace into a green run is how
a real one stops being noticed.

## `fn the_image_table_is_keyed_by_id_and_references_resolve_through_it() {`

---------------------------------------------------------------------------
1. The fake's six required capabilities
---------------------------------------------------------------------------

## `fn the_image_table_is_keyed_by_id_and_references_resolve_through_it() {`

(1) an image table keyed by **immutable id**, with references and digests.

Second field held constant: the runtime is reachable throughout, so what
varies is only which key the table is read by. Without an id-keyed table,
`image_by_id` could not answer at all and the rebuild path's refusal — "the
**recorded image id** is absent from the runtime" — would be unwritable.

## `fn the_image_table_is_keyed_by_id_and_references_resolve_through_it() {` › `let without = runtime`

"the manifest digest **when reported**" — absent is a real state and a
separately encodable one, not a missing fixture.

## `fn the_image_table_is_keyed_by_id_and_references_resolve_through_it() {` › `assert_eq!(runtime.image_by_reference("ghcr.io/nobody:v9"), Ok(None));`

The two questions are independent: an id present under no reference is
findable by id and by no reference.

## `fn a_reference_can_be_moved_to_another_id_and_the_old_id_stays() {`

(2) a **mutable tag table** — a reference can be moved to another id while
the id stays.

ST-20: "a resume after the recorded reference was moved to another image
warns and creates every container from the recorded id". Without a mutable
tag table that sentence has no fixture at all.

Second field held constant: the image table itself. Both ids are present
before and after; only the tag moves — which is the whole point, because a
fixture that also deleted the old id would prove the wrong thing.

## `fn a_reference_can_be_moved_to_another_id_and_the_old_id_stays() {` › `let answers: BTreeSet<String> = [`

Two distinct answers to two distinct questions about one reference: the
intersection {image id recorded} x {reference moved} rather than either
alone.

## `fn the_fake_can_report_an_image_id_that_differs_from_the_one_create_asked_for() {`

(3) per-container **reported image ids with substitution injection**.

The correlated-fixture trap this slice was warned about: if the reported id
were set from the requested id there would be no way to build a
substitution, and `substituted_image_id_refused_before_start` would be green
because it could not be written. This is the test that proves the two are
separate inputs.

## `fn the_fake_can_report_an_image_id_that_differs_from_the_one_create_asked_for() {` › `let honest = runtime.create(&spec).expect("created");`

Healthy: the runtime reports what it was asked for.

## `fn the_fake_can_report_an_image_id_that_differs_from_the_one_create_asked_for() {` › `runtime.substitute_reported_image_id(&spec.name, OTHER_IMAGE_ID);`

Injected: it does not.

## `fn the_fake_can_report_an_image_id_that_differs_from_the_one_create_asked_for() {` › `let held = runtime.container(&spec.name).expect("held");`

And the container the fake holds records both, separately.

## `fn volume_presence_is_a_toggle_and_absence_refuses_a_create() {`

(4) **volume presence toggles**.

R20 is operator-owned and `persistent_output` in all five `at_run_end`
outcomes — "never created or pruned by a run" — so the only thing a run does
with a volume is *observe* it, and absence is a refusal. Second field held
constant: the image table, so a refusal here cannot be an image problem
wearing a volume's name.

## `fn the_availability_toggle_is_per_operation_so_ps_can_answer_while_inspect_cannot() {`

(5) an **availability toggle**, and it is per operation.

The reachability decision this lane made, stated as a test: a runtime that
answers `docker ps` and fails `docker inspect` is a real state, and a seam
with one global boolean could not express it. The intersection here is
{operation} x {reachable?}, which one boolean collapses.

## `fn the_availability_toggle_is_per_operation_so_ps_can_answer_while_inspect_cannot() {` › `assert_eq!(`

`ps` answers.

## `fn the_availability_toggle_is_per_operation_so_ps_can_answer_while_inspect_cannot() {` › `let error = runtime`

`inspect` does not, and says which operation could not be reached.

## `fn the_availability_toggle_is_per_operation_so_ps_can_answer_while_inspect_cannot() {` › `runtime.set_all_unreachable();`

The whole daemon down is the other end of the same toggle, and every
operation reports it.

## `fn a_seeded_container_carries_owner_labels_and_an_incarnation() {`

(6) owner **labels**, **incarnations**, and the two image ids as separate
inputs.

Second field held constant: the label *keys* are the packet's five for both
containers; what varies is the run and the incarnation, which is the axis
the census classifies on.

## `fn a_seeded_container_carries_owner_labels_and_an_incarnation() {` › `IMAGE_ID,`

Separate argument, always.

## `fn a_seeded_container_carries_owner_labels_and_an_incarnation() {` › `assert_eq!(runs.len(), 2, "{runs:?}");`

Distinct-value counts, not prose: two runs, two incarnations, two
invocations, and the pairs are not the same partition — which is what
makes {owner run} x {incarnation} a real grid rather than one axis twice.

## `fn owner_liveness_answers_one_bit_and_carries_no_incarnation() {`

(6b) **liveness simulation**, and the shape that makes an incarnation
unreadable from a lock.

`crash_reconstruction`: the incarnation id "is **never read from lock-file
contents**". [`OwnerLiveness`] answers one bit about a public run directory,
so there is no incarnation in the return type to read — the defect is not
refused, it is unexpressible.

## `fn owner_liveness_answers_one_bit_and_carries_no_incarnation() {` › `let probe = super::runtime::LockProbe;`

The production probe is `rundir::is_running`, and it answers the same
shape for a directory that never held a run.

## `fn the_call_log_is_ordered_and_holds_every_operation() {`

The call log is ordered and holds every operation.

The instrument the rest of this file rests on. Second field held constant:
one runtime, one trace; what varies is only how many operations have run.

## `fn every_container_sites_row_adjacency_fault_row_and_scope_is_the_packets() {`

---------------------------------------------------------------------------
2. The eight sites and the funnel's shape
---------------------------------------------------------------------------

## `fn every_container_sites_row_adjacency_fault_row_and_scope_is_the_packets() {`

The row, adjacency, fault row and scope of each of the eight sites,
transcribed from the packet rather than read back from the enum that
produces them.

`effect_site_inventory.identity`: "Container.* (R19/R26; Container.Create
verifies the created container's image id against the record before
Container.Start)", and `slice_contract.owned_resources` splits them:
"R26 container + labels + global intent incl. runner digest", "R19
disposable Git view per request".

## `fn every_container_sites_row_adjacency_fault_row_and_scope_is_the_packets() {` › `let r19 = EXPECTED.iter().filter(|e| e.1 == ResourceRow::R19).count();`

Two of the eight are R19 and six are R26, which is the split
`owned_resources` states. A count, so a site moved between rows fails
here as well as in its own row above.

## `fn every_container_sites_row_adjacency_fault_row_and_scope_is_the_packets() {` › `for site in ContainerSite::ALL {`

No Container site exposes a parent-side sub-effect point or registers a
command-internal residue class, and both absences are **stated** rather
than left unmentioned. `command_internal_sub_effects` registers
`ObjectResidue::Internal` for the Object sites because a Git child writes
objects before publishing their reference; a `docker create` publishes
nothing the parent can observe halfway, and the intent record is a
stage/rename whose torn half is writer-owned residue the scan skips.
`effect_site_inventory.scope` makes every Topology site owe evidence for
"every parent-side sub-effect point"; an empty list is that debt being
zero, and this is where a variant that grew one would be noticed.

## `fn windows_orphan_window_documented() {`

**T-CONTAINER (19)** `windows_orphan_window_documented`.

`decisions.admission_and_leases.permits.os_matrix`:

> Linux and macOS (cfg(unix)): the cleanup reaper survives coordinator
> death, settles the dead coordinator's process groups while holding R28,
> and **additionally kills the dead coordinator's labeled containers,
> closing the orphan window**; Windows: no reaper; … and **containers are
> reclaimed at the next upstroke write-command start (orphan window until
> then; documented; a portable watchdog is deferred)**.

The window is a **value** and not only a sentence, so the two platforms give
different answers and the Windows guest — which has no container runtime at
all — still asserts something about containers. The intersection here is
{platform} x {who closes the window}, and a constant would collapse it.

## `fn windows_orphan_window_documented()` › `let answers: BTreeSet<OrphanWindow> = OrphanWindow::ALL.iter().copied().collect();`

Both answers exist and differ, so the value is a platform axis rather
than a constant this platform happens to agree with.

## `fn windows_orphan_window_documented()` › `let raw = fs::read_to_string(`

And the sentence is in the tree, next to the reclaim path it governs, so
"documented" is a fact about this file rather than about the packet.

## `fn windows_orphan_window_documented()` › `let region = {`

Only the region the documentation lives in, so what follows is a claim
about *that* documentation and not about whatever else the file says.

## `fn windows_orphan_window_documented()` › `let source: String = region`

Doc-comment markers, block quoting and emphasis removed and whitespace
collapsed, because a quoted sentence is wrapped by `rustfmt` at whatever
column it lands on and a phrase search over the raw bytes would be
asserting the wrap rather than the sentence.

## `fn windows_orphan_window_documented()` › `const UNIX: &str = "cfg(unix)";`

The four phrases above are a **set**, and a set survives having its
platform names swapped: documenting a Windows reaper and no Unix reaper
leaves every one of them present. So the documentation is read as a
*mapping* — each platform marker owns the prose up to the next marker —
and the mapping is then checked against the code's own `cfg`.

## `fn windows_orphan_window_documented()` › `let unix_has_a_reaper = unix_said.contains("cleanup reaper");`

What each platform is documented as having, read out of its own prose.

## `fn windows_orphan_window_documented()` › `assert_eq!(`

**The tie.** The platform this test is running on is a `cfg`, and
`orphan_window()` answers for that same `cfg`. A documentation block
whose platform names are reversed disagrees with it here — on both
platforms, in opposite directions — where the phrase set could not tell.

## `fn windows_orphan_window_documented()` › `assert!(!unix_said.contains("no reaper"), "{unix_said}");`

The named consequences, each against the platform that has them.

## `fn windows_orphan_window_documented()` › `let unarmable = crate::runner::container::census::ReaperContainerScope::new(`

And a second tie, to code that is not `orphan_window` itself: arming the
reaper is a **no-op on Windows**, so a scope naming a program that cannot
be executed is refused on the platform that has a reaper and accepted on
the platform that has nothing to arm. Nothing is installed on either
path, so no other test in this process inherits a scope.

## `fn every_container_site_is_taken_by_value_by_a_funnel_that_hooks_both_phases() {`

Every one of the eight sites is taken **by value** by a funnel API, and the
funnel records both hook phases around the primitive.

`identity`: "every effectful funnel API takes its group's site by value, and
the funnel itself calls hook(Before, site) -> primitive -> hook(After,
site), so hooks exist for every site by construction". This is the runtime
evidence for that sentence — `effects::tests::every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`
is the source-level half.

## `fn every_container_site_is_taken_by_value_by_a_funnel_that_hooks_both_phases() {` › `let expected: Vec<(ContainerSite, TracePhase)> = [`

Both phases, once each, for all eight, in the order they were called.

## `fn a_funnel_api_refuses_a_site_that_does_not_name_its_operation() {`

A funnel API refuses a site that does not name its operation, and **no
primitive effect occurs**.

The site is a by-value parameter, which is what `identity` asks for; a free
parameter can be passed a wrong value, so the guard is what keeps the
parameter load-bearing rather than decorative. The grid is all eight sites
against all eight APIs: eight accept and fifty-six refuse.

**`PR6-LANEF-002` is why every cell asserts more than `is_err()`.** Seven of
the eight APIs used to count any `Err`, over a fixture holding nothing —
and an empty fixture makes the *runtime* supply the error. Deleting only
`expect_site(site, Operation::Start)` from [`start_container`] passed the
whole suite, because there was no container to start and the test counted
the runtime's incidental refusal. A refusal test that passes for the wrong
reason is exactly the class this project keeps paying for.

So each cell is prepared in a state where its primitive **would succeed if
it were reached** — a container to start, a container to stop, a view to
remove, a record to delete, a free name to create — and each asserts three
things:

1. the call refused, with [`UpstrokeError::Refused`];
2. **the trace is empty**: no site phase, no runtime operation, no view
   action and no durability step, which is the whole observable surface this
   module has and is what "before any effect" means;
3. the API's own state is byte-for-byte what it was.

And every API is then driven with its **own** site as a positive control, so
a cell whose primitive could not have succeeded anyway fails here rather
than passing vacuously.

Second field held constant: the runtime is reachable and holds the recorded
image throughout, so no cell can refuse because the runtime was armed.

## `fn a_funnel_api_refuses_a_site_that_does_not_name_its_operation() {` › `let proof = matches!(own_site, ContainerSite::Create | ContainerSite::Start).then(|| {`

`create_container` and `start_container` take an `IntentWritten`, and
there is no way to call them without one — that is
`expected_failures_refusals[6]`, "container start without an intent
is impossible by construction". The proof is minted from a record
written **directly** rather than through `write_intent`, so this
cell's trace still starts empty and assertion (2) keeps meaning what
it says.

## `fn a_funnel_api_refuses_a_site_that_does_not_name_its_operation() {` › `match own_site {`

The state in which THIS API's primitive succeeds.

## `fn a_funnel_api_refuses_a_site_that_does_not_name_its_operation() {` › `ContainerSite::WriteIntent => {}`

Nothing on disk, so a write would land.

## `fn a_funnel_api_refuses_a_site_that_does_not_name_its_operation() {` › `ContainerSite::Create => {}`

The name is free and the image is present, so a create would work.

## `fn a_funnel_api_refuses_a_site_that_does_not_name_its_operation() {` › `ContainerSite::MountGitView => {}`

No directory, so a materialize would create one.

## `fn a_funnel_api_refuses_a_site_that_does_not_name_its_operation() {` › `assert_eq!(`

(2) Nothing happened at all. The guard runs before `funnel`, so a
correct refusal records neither hook phase — and a broken one
records the phases AND the primitive's own entry.

## `fn a_funnel_api_refuses_a_site_that_does_not_name_its_operation() {` › `let held = runtime.container(name.as_str());`

(3) And the state the primitive would have changed is untouched.

## `fn a_funnel_api_refuses_a_site_that_does_not_name_its_operation() {` › `trace.clear();`

The positive control: the same API, its own site, and the primitive
really does run. Without this a cell whose primitive could not have
succeeded would satisfy every assertion above by doing nothing.

## `fn a_hook_armed_at_a_phase_fails_the_funnel_at_that_phase() {`

A hook armed at a phase makes the funnel return `Err` there, and an `After`
error arrives **after** the primitive ran.

## `fn a_hook_armed_at_a_phase_fails_the_funnel_at_that_phase()` › `let mut hooks = fixture.hooks();`

Before: nothing is written.

## `fn a_hook_armed_at_a_phase_fails_the_funnel_at_that_phase()` › `let mut hooks = fixture.hooks();`

After: the record is on disk and the call still fails.

## `fn the_intent_record_carries_the_six_fields_and_each_is_read_back() {`

---------------------------------------------------------------------------
3. The intent record — six fields, each read back
---------------------------------------------------------------------------

## `fn the_intent_record_carries_the_six_fields_and_each_is_read_back() {`

The six fields `crash_reconstruction` and R26 enumerate, each written and
each read back.

"A field written and never read is invisible to mutation witnessing", so
every field is given a value distinct from every other field's and the
round trip is asserted field by field. The distinct-value count is the
hostility assertion: six fields, six distinct values, so a record that
copied one field into another fails.

## `fn the_intent_record_carries_the_six_fields_and_each_is_read_back() {` › `assert_eq!(written.record(), &read, "the proof and the file disagree");`

The proof `write_intent` mints carries the record it read back, so the
capability and the file are the same six fields rather than two.

## `fn the_intent_record_carries_the_six_fields_and_each_is_read_back() {` › `let document: serde_json::Value =`

The serialized document has exactly six keys, in the packet's order, and
the key names are pinned as literals rather than taken from the struct.

## `fn an_intent_record_with_an_unknown_field_is_refused() {`

A record with a seventh field is not this engine's record.

## `fn the_five_labels_are_the_packets_five_and_each_carries_its_own_field() {`

The five labels, each carrying its own field.

`crash_reconstruction`: "labels upstroke.private_root, upstroke.run,
upstroke.run_dir, upstroke.incarnation, upstroke.invocation". Written out as
literals, and each value asserted against the field it comes from — a label
map with five keys and one value repeated would pass a count and fails here.

## `fn the_five_labels_are_the_packets_five_and_each_carries_its_own_field() {` › `assert!(`

Discovery is by `upstroke.private_root` and the record's own location is
inside that root, so the one label with no field of its own is the one
the census already knows.

## `fn the_container_name_is_the_packets_template_and_its_hash_is_pinned() {`

---------------------------------------------------------------------------
4. The name
---------------------------------------------------------------------------

## `fn the_container_name_is_the_packets_template_and_its_hash_is_pinned() {`

The name is the packet's template, and the expected value is a literal.

> the container name is `upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>`

The invocation hash is pinned against a value computed **out of band**:

```text
python3 -c 'import hashlib; print(hashlib.sha256(
    b"upstroke.container-invocation.v1" + b"\x00" + b"p.shell.o0").hexdigest()[:16])'
1a8e276b273887c0
```

A digest compared only against the code that produced it proves nothing.

## `fn the_name_is_injective_over_every_component_varied_independently() {`

The parse is injective over a hostile component grid.

Every component is varied independently and the counts are asserted:
2 repo keys x 2 run ids x 2 incarnations x 2 hashes = 16 tuples, 16 distinct
names, and 16 distinct parses that each round-trip. A name produced two ways
by two different tuples is an ownership record that lies.

## `fn the_name_is_injective_over_every_component_varied_independently() {` › `assert!(ContainerName::from_parts("a-b", "c", INCARNATION_1, "d").is_err());`

The adversarial pair: components chosen so that a template joining them
without a refusal on the separator would collide. `a-b` + `c` and `a` +
`b-c` render the same string under a naive join; here both are refused.

## `fn a_hostile_name_component_is_refused_and_the_refusal_says_why() {`

A component carrying a separator, a `.`, or a path separator is refused.

The name goes into a **file name** — `<name>.intent` — so a component with a
path separator names a different file than the record says, which is the
same class `workspace_manager::remove_intent` validates its slot names
against.

## `fn a_hostile_name_component_is_refused_and_the_refusal_says_why() {` › `assert!(`

Seven hostile values in four positions, and the message names the
position, so the refusals are not one message repeated.

## `fn a_hostile_name_component_is_refused_and_the_refusal_says_why() {` › `let at_limit = "a".repeat(intent::MAX_COMPONENT_LEN);`

Over-long is refused too, and the boundary is exact.

## `fn probe_name_reuse_across_incarnations_never_collides() {`

**T-CONTAINER (9)** `probe_name_reuse_across_incarnations_never_collides`.

`crash_reconstruction`: "the container name is
`upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>`, so
**deterministic InvocationIds never collide across incarnations and no
earlier ownership evidence is overwritten**". ST-16 (f) is the same claim
from the other side: "a probe invocation with the same deterministic
InvocationId, whose **new container name and intent path differ**".

The intersection: {probe kind} x {incarnation}. Both probe targets, both
incarnations, one run — so a name that dropped the incarnation collides in
**two** places, and one that dropped the invocation collides in two others.

## `fn probe_name_reuse_across_incarnations_never_collides()` › `assert_eq!(`

The identity really is the same across incarnations: that is the
premise the incarnation component exists for, and asserting it
here stops the test passing because the ids happened to differ.

## `fn probe_name_reuse_across_incarnations_never_collides()` › `let mut hooks = RecordingHooks::new(ContainerTrace::recording());`

And no earlier ownership evidence is overwritten: writing all four leaves
four records on disk.

## `fn container_intent_written_before_run() {`

---------------------------------------------------------------------------
5. The orderings
---------------------------------------------------------------------------

## `fn container_intent_written_before_run() {`

**T-CONTAINER (1)** `container_intent_written_before_run`.

`side_effect_vs_event_ordering`: "**intent synced before docker create**".
Both halves: the record's *sync* (not merely its write) precedes the create,
and the create precedes the start.

Second field held constant: the runtime is reachable and reports the
recorded id, so nothing here can pass because the launch failed early.

## `fn container_intent_written_before_run()` › `assert!(launched.intent_path.exists());`

The record really is on disk, with the run that owns it.

## `fn container_created_from_recorded_image_id_and_verified() {`

**T-CONTAINER (2)** `container_created_from_recorded_image_id_and_verified`.

INV-23: "every container of every epoch is created from the **recorded image
id** and its reported image id is verified equal to the record **before it
starts**".

The intersection: {image id recorded} x {reference moved}. The reference is
moved to another image *before* the launch, and the container is still
created from the recorded id — which is the sentence "so a moved reference
cannot change what executes" and is not provable by a fixture whose
reference never moved.

## `fn container_created_from_recorded_image_id_and_verified()` › `fixture.runtime.move_tag(IMAGE_REFERENCE, OTHER_IMAGE_ID);`

The reference now names another image. The record still names the id.

## `fn container_created_from_recorded_image_id_and_verified()` › `let created = at(&fixture.trace, &format!("rt:create:{}", fixture.plan.name));`

Verified *before* start: the verification is between create and start in
the sequence, and the start happened, so it passed there.

## `fn substituted_image_id_refused_before_start() {`

**T-CONTAINER (3)** `substituted_image_id_refused_before_start`.

INV-23: "a mismatch refuses during pre-flight or rebuild". The refusal is
**before start**, and the assertion is that `Container.Start` is absent from
the sequence — not that an error was returned, which a refusal after the
start would also produce.

The intersection: {reported id} x {start reached}. R26 balances afterwards,
because a refusal is a cancel and a cancel releases.

## `fn substituted_image_id_refused_before_start()` › `let mounted = at(&fixture.trace, "site:MountGitView:after");`

The view IS mounted — it precedes `Create`, because it is a bind-mount
source of it (`PR6A-LAUNCH-MOUNTS-THE-VIEW-AFTER-CREATE`) — and the cancel
therefore has an R19 residue to prune. This assertion used to read "no
view is mounted for a container that will not start", which the corrected
order makes false; the claim it was standing for is R19 balancing, and
that is asserted here directly and more strongly.

## `fn substituted_image_id_refused_before_start()` › `assert_eq!(fixture.runtime.container_names(), Vec::<String>::new());`

R19 and R26 both balance: the container it created is released, the view
is gone and the intent is gone, so no census finds residue of a refusal.

## `fn the_git_view_is_mounted_before_create_and_before_start() {`

"view mounted before start" — and before **create**, because it is a
bind-mount source of that call.

The contract clause is satisfied by two orders and only one of them runs:
Docker requires a bind source to exist at `docker create`, so
`WriteIntent -> Create -> MountGitView -> Start` refuses with
`invalid mount config for type "bind": bind source path does not exist`.
`PR6A-LAUNCH-MOUNTS-THE-VIEW-AFTER-CREATE` — measured against docker 29.7.2
by lane A, invisible to the fake (whose `create` does not look at a mount
source) and invisible to this file's own gated test until it started
carrying the view as a real mount.

Second field held constant: the runtime reports the recorded id and the
launch succeeds, so nothing here passes because a step failed early. The
intersection is {which pair of steps} × {the sequence between them}, and the
directory's existence at the moment of the create is asserted as well as the
order, because "the site ran earlier" and "the directory was there" are two
claims.

## `fn the_git_view_is_mounted_before_create_and_before_start()` › `let dir_synced = fixture`

The intent is still first, so moving the mount up did not move it past
"intent synced before docker create".

## `fn release_stops_removes_unmounts_and_removes_the_intent_in_that_order() {`

"stop/rm, view removal, intent removal after completion" — the four sites in
the contract's own order.

## `fn release_stops_removes_unmounts_and_removes_the_intent_in_that_order() {` › `assert!(!launched.view_path.exists(), "the view is pruned");`

R19 and R26 both balance.

## `fn reclaim_kills_observes_removes_the_view_and_then_the_intent() {`

Reclaim, in the packet's order:

> reclaim = docker kill -> wait until observed exited/removed -> docker rm
> -> remove Git view -> remove intent

Five steps, and the **observation between the kill and the rm** is the one a
set-membership assertion would lose.

## `fn reclaim_converges_from_every_combination_of_intent_and_container() {`

Reclaim is idempotent and tolerant of already-gone, so two reclaimers
converge.

The intersection: {intent present} x {container present}. All four cells are
driven, and each must converge on the same terminal state.

## `fn the_intents_durability_barriers_are_entered_and_not_merely_traced() {`

The intent's durability barriers are **entered**, not merely traced.

`PR6-LANEF-001`. `crash_reconstruction` requires every container invocation
to write a **synced** global intent, and [`super::write_synced`] records
`DurableStep::Synced` / `DirSynced` in the trace beside each barrier. That
record is written by the same function that performs the barrier, so it
certifies itself: **deleting `util::fsync_file` and `util::fsync_dir` while
leaving the two trace calls in place passed the entire suite** — every
ordering assertion in this file reads the record, and the record was still
there. Write and rename the intent, create the container, lose power before
either the file or its directory reaches stable storage, and Docker keeps a
container whose ownership record crash reconstruction cannot find.

So this reads the **syscall** instead. [`crate::util::barriers_on_this_thread`]
counts entries into the two barrier functions per thread and per half; a
funnel performs its barriers on the thread that called it, so the delta is
exact rather than the lower bound a process-wide counter can support while
the suite is threaded. The two halves are counted separately because
`fsync_file` and `fsync_dir` are two independently droppable predicates.

The two axes: {which barrier} × {which call}. The trace is read only to show
the two agree — never as the evidence that a barrier happened.

## `fn the_intents_durability_barriers_are_entered_and_not_merely_traced() {` › `let file = fixture.plan.name.intent_file_name();`

The other axis, and it is a different claim: the trace says the same
thing. If these two ever disagree the trace is the one that is wrong.

## `fn the_intents_durability_barriers_are_entered_and_not_merely_traced() {` › `let fixture = Fixture::new("barriers-launch", RUN_A, INCARNATION_1, &agent_probe());`

Second cell: the whole launch. Exactly one of each, still — the view, the
create and the start perform no barriers — so a barrier that quietly
appeared or disappeared elsewhere in the sequence is visible here too.

## `fn a_cancel_whose_cleanup_fails_still_refuses_with_the_integrity_error() {`

A cancel whose own cleanup fails still refuses with the **integrity** error,
and still attempts every remaining step.

`PR6-LANEF-006`. [`launch`]'s image-id refusal used to `?`-chain its cleanup,
so a failing `Container.Stop` returned the *stop* error before `rm`, the view
removal or the intent removal ran: one failure, three residues, and the fact
that the runtime had created a container from a substituted image went
unsaid. "Is that true at every point it can fail?" — it was not.

The grid is {which of the four cancel steps fails} × {what survives}, and
each of the four cells has a **distinct** observable, so a fix that merely
stopped returning the first error would still fail three of them:

| armed | container | view | intent |
|---|---|---|---|
| `Stop` | removed | pruned | removed |
| `Remove` | **left** | pruned | removed |
| `UnmountGitView` | removed | **left** | **left, deliberately** |
| `RemoveIntent` | removed | pruned | **left** |

#### The third row changed in repair round R3b, and it is the finding

`PR6-ACCT-005`. It read "`UnmountGitView` → removed / left / **removed**",
and that is the state a startup census cannot recover from: discovery is
`<R>/containers` plus `docker ps` by label, and the view path is derived
only *after* a candidate is found — `<R>/views` is never enumerated. So a
cancel that failed to prune the view and then removed the intent anyway had
deleted the only thing that could ever find the directory again. The test
pinned that state as correct.

The intent is the R19 view's **recovery anchor** and now outlives what it
anchors: the retained record is itself reported in the residue, so the
ledgers are still said not to balance, and
`census::tests::an_unpruned_view_is_reclaimed_because_its_intent_survived`
drives the census that closes it.

Second field held constant: the substitution, the image ids and the plan are
identical in all four cells, so what varies is only which step was armed.

## `fn a_cancel_whose_cleanup_fails_still_refuses_with_the_integrity_error() {` › `assert!(message.contains(OTHER_IMAGE_ID), "{armed:?}: {message}");`

The integrity refusal survives the cleanup failure. This is the whole
finding: the operator needs to know the runtime executed something
other than the record, not that `docker stop` said no.

## `fn a_cancel_whose_cleanup_fails_still_refuses_with_the_integrity_error() {` › `assert!(`

And the residue is reported rather than swallowed: fail-closed means
the refusal names what it could not release.

## `fn a_cancel_whose_cleanup_fails_still_refuses_with_the_integrity_error() {` › `assert!(`

Never started, whatever else happened.

## `fn a_cancel_whose_cleanup_fails_still_refuses_with_the_integrity_error() {` › `let container_left = !fixture.runtime.container_names().is_empty();`

The three steps that were NOT armed all ran.

## `fn a_cancel_whose_cleanup_fails_still_refuses_with_the_integrity_error() {` › `ContainerSite::UnmountGitView => (false, true, true),`

The anchor rule: an unpruned view keeps its record.

## `fn a_cancel_whose_cleanup_fails_still_refuses_with_the_integrity_error() {` › `assert!(`

Retained on purpose, and said so — an operator reading "the view
could not be pruned" and finding the record gone would have no
way to know the directory exists.

## `fn step_phrase(site: ContainerSite) -> &'static str {`

The phrase [`super::cancel_created`] uses for each step, so the assertion
above reads the message rather than the enum that wrote it.

## `const DAEMON_ALREADY_STOPPED: &str = "Error response from daemon: cannot kill container: \`

---------------------------------------------------------------------------
5b. Two reclaimers that actually race
---------------------------------------------------------------------------

## `const DAEMON_ALREADY_STOPPED: &str = "Error response from daemon: cannot kill container: \`

What `docker` 29.7.2 writes to stderr, measured on the build box.

```text
$ docker kill <exited>
Error response from daemon: cannot kill container: f1probe-a: container
0079320fdf5654fbf3aa45a154e4d49328c1cc1de3b1af4a6cc24540519ecede is not running
$ docker kill <absent>
Error response from daemon: cannot kill container: f1probe-nope: No such container: f1probe-nope
$ docker stop <absent>
Error response from daemon: No such container: f1probe-nope
$ docker stop <exited>
f1probe-a                                            (exit 0)
```

Transcribed, not invented — and
`real_docker_kill_on_an_already_exited_container_is_tolerated` asks the live
daemon the same question, so the table cannot drift into being its own
oracle.

## `fn a_stop_answer_meaning_already_settled_is_tolerated_and_a_real_failure_is_not() {`

A `docker stop` answer meaning "already settled" is tolerated; a real
failure is not.

`PR6-LANEF-003`. `DockerCli::stop`'s tolerance is load-bearing — it is what
makes a reclaimer that arrives second converge instead of aborting — and
**removing it passed every test**, because every fixture serialized the
reclaimers and a serialized second reclaimer never sees the answer.
[`super::settle_stop`] is a free function taking the raw outcome for exactly
this reason: the branch is reachable without a daemon.

The intersection: {what the daemon said} × {is it tolerable}. Three
tolerable answers and three that are not, counted rather than described —
and the third intolerable one is `Unreachable` carrying tolerable *text*,
because a runtime that could not be reached did not tell us anything about
the container.

## `fn a_stop_answer_meaning_already_settled_is_tolerated_and_a_real_failure_is_not() {` › `for detail in [`

Real failures stay failures. `--force` removal and a kill the daemon could
not deliver are things a reclaimer must NOT report as convergence.

## `fn a_stop_answer_meaning_already_settled_is_tolerated_and_a_real_failure_is_not() {` › `let unreachable = super::settle_stop(Err(RuntimeError::Unreachable {`

And unreachable is a different answer even when its text would be
tolerable: `crash_reconstruction` refuses a write command when the runtime
"cannot be reached", and swallowing that here would turn a refusal into a
convergence.

## `fn a_stop_answer_meaning_already_settled_is_tolerated_and_a_real_failure_is_not() {` › `assert_eq!(super::settle_stop(Ok("upstroke-c\n".to_owned())), Ok(()));`

The control: a stop that simply worked.

## `struct DockerLikeStop<'a> {`

A runtime whose `stop` answers the way the daemon does, settled through the
production tolerance.

The point is that the **raw** answer is the daemon's and the settling is
[`super::settle_stop`], the production function — so a fixture built on this
is exercising the tolerance rather than a test-local copy of it. Every raw
answer is recorded, so a test can assert the already-stopped branch actually
fired instead of hoping it did.

## `fn stop(&self, name: &str, mode: StopMode) -> Result<(), RuntimeError> {` › `let outcome = match self.inner.observe(name)? {`

The daemon's own three answers, chosen by the state the container is
actually in — which is what makes a second reclaimer see the
already-stopped one rather than a flag a test had to set.

## `fn a_reclaimer_arriving_after_another_killed_the_container_converges() {`

A reclaimer that arrives after another has already killed the container
**converges**, rather than aborting before observe / rm / view / intent.

`PR6-LANEF-003`'s scenario, made deterministic: reclaimer A kills the
container and then crashes; B reaches `docker kill` after the state has
become `Exited` and gets "is not running". Without the tolerance B returns an
error and R19/R26 both keep residue — which is the opposite of "every step
idempotent and tolerant of already-gone so **two concurrent reclaimers
converge**".

Second field held constant: the container, the intent and the view are all
present when B starts, so B has real work to do at every one of its five
steps and cannot pass by finding nothing.

## `fn a_reclaimer_arriving_after_another_killed_the_container_converges() {` › `stop_container(`

Reclaimer A: kills it, and gets no further.

## `fn a_reclaimer_arriving_after_another_killed_the_container_converges() {` › `reclaim(`

Reclaimer B: the whole sequence, over a container that is already stopped.

## `fn a_reclaimer_arriving_after_another_killed_the_container_converges() {` › `let answers = docker_like.raw_answers();`

The already-stopped branch actually fired — without this the test could
pass having never reached the tolerance at all.

## `fn a_reclaimer_arriving_after_another_killed_the_container_converges() {` › `assert!(!view_path.exists(), "B stopped before pruning the view");`

And B finished the job: nothing of R19 or R26 is left.

## `fn two_reclaimers_racing_one_container_converge() {`

**T-CONTAINER** convergence, with the two reclaimers genuinely concurrent.

The reviewer's refutation of lane F's claim was that two reclaimers that
actually **race** were not constructible in any fixture it built — running
`reclaim` twice proves idempotence, which is a different property. This is
the race: two threads, released together by a [`std::sync::Barrier`], both
inside `reclaim` on one container, one runtime, one intent and one view.

Every interleaving must converge, and the assertion is on the terminal state
rather than on who won — which is the only thing a race is allowed to
assert.

## `fn a_container_that_cannot_be_observed_terminated_refuses() {`

A container that cannot be observed terminated refuses.

`refusal_condition`: "a dead owner's or dead incarnation's labeled container
that cannot be observed terminated **blocks admission**". The fake's stop is
armed failing so the container stays `Running` and the observation never
converges — the second field held constant is that the runtime is
*reachable* throughout, so this is not the unreachable refusal wearing
another name.

## `fn a_container_that_cannot_be_observed_terminated_refuses()` › `let error = observe_terminated(&NeverTerminates(&fixture.runtime), &name)`

Stop succeeds and the container stays running: a kill that was delivered
to a process the kernel has not reaped.

## `fn a_failed_operation_and_an_unreachable_one_are_different_answers() {`

Unreachable and failed are **different answers**, and the refusal split
rests on the difference.

`crash_reconstruction` refuses a write command when "any intent exists and
the runtime **cannot be reached**"; an operation that reached the runtime
and failed is a different thing, and a seam that reported one error kind
would make lane C's refusal unwritable. The intersection here is {operation}
x {reachable? failed? fine?} — three states over one operation, not two axes
tested apart.

## `fn a_failed_operation_and_an_unreachable_one_are_different_answers() {` › `assert!(runtime.image_by_id(IMAGE_ID).expect("reachable").is_some());`

Fine.

## `fn a_failed_operation_and_an_unreachable_one_are_different_answers() {` › `runtime.set_failing(RuntimeOp::InspectImageById);`

Reached and failed.

## `fn a_failed_operation_and_an_unreachable_one_are_different_answers() {` › `runtime.set_unreachable(RuntimeOp::InspectImageById);`

Not reached at all.

## `fn a_failed_operation_and_an_unreachable_one_are_different_answers() {` › `runtime.set_reachable(RuntimeOp::InspectImageById);`

And back: the toggle is a toggle, so a fixture can restore a runtime
mid-test — which is what a census that refuses and then succeeds needs.

## `fn a_containers_exit_status_and_streams_come_back_through_the_seam() {`

A container's exit status and output come back through the seam, which is
what lane A turns into a `ProcessOutput`.

Second field held constant: one container, one runtime; what varies is only
what it exited with. Three distinct exit values and two distinct streams, so
a `collect` that returned a constant fails.

## `fn a_containers_exit_status_and_streams_come_back_through_the_seam() {` › `runtime.set_container_state("upstroke-a-b-c-d", Liveness::Exited);`

Liveness is a separate axis from the exit status: a container can be
observed running while carrying an exit value from its previous state.

## `fn a_containers_exit_status_and_streams_come_back_through_the_seam() {` › `assert_eq!(`

A container the runtime does not hold is Gone, not an error: that is what
makes reclaim tolerant of already-gone.

## `fn the_docker_gate_refuses_an_uncounted_test_and_names_what_is_absent() {`

The Docker gate refuses a test nothing counts, and its absence reason says
what is missing.

## `fn the_docker_gate_refuses_an_uncounted_test_and_names_what_is_absent() {` › `let unlisted = ["a", "test", "nobody", "listed"].join("_");`

Built rather than written, so `every_docker_gated_test_is_named_and_present`
— which reads gate call sites out of the source — does not see this
negative control as a fourth gated test.

## `struct NeverTerminates<'a>(&'a FakeRuntime);`

A runtime that never reports termination, wrapping another.

A wrapper rather than a flag on the fake, because "still running after the
kill" is a property of the *sequence of answers*, and a fake that could only
be armed to fail would make the refusal an error rather than a
never-converging observation.

## `fn the_namespace_scan_reads_every_record_and_skips_the_staged_half() {`

---------------------------------------------------------------------------
6. The namespace scan
---------------------------------------------------------------------------

## `fn the_namespace_scan_reads_every_record_and_skips_the_staged_half() {`

The scan reads every record and skips the writer-owned staged half.

"discovery at every write-command start scans the whole namespace
`<R>/containers`". A `<name>.intent.tmp` is a crash between the stage and
the rename; adopting it would be adopting a record that was never
published.

## `fn the_namespace_scan_reads_every_record_and_skips_the_staged_half() {` › `let dir = containers_dir(&fixture.root);`

Residue a reader must ignore: a staged half, and a file that is not an
intent at all.

## `fn the_namespace_scan_reads_every_record_and_skips_the_staged_half() {` › `let mut sorted = found.iter().map(|f| f.name.clone()).collect::<Vec<_>>();`

Sorted by name, so a census's report is stable across filesystems whose
directory order is not.

## `fn an_absent_containers_directory_is_an_empty_namespace() {`

A private root with no `containers` directory is an **empty namespace**, not
an error.

`crash_reconstruction`: "with no intent and no reachable runtime it
proceeds". A run that has never launched a container has no directory, and a
scan that treated that as a failure would refuse every write command on a
host runner.

## `fn every_container_effect_in_the_tree_goes_through_the_funnel() {`

---------------------------------------------------------------------------
7. Enforcement: nothing performs a container effect outside this funnel
---------------------------------------------------------------------------

## `fn every_container_effect_in_the_tree_goes_through_the_funnel() {`

Every container effect in the tree goes through the funnel.

The census beside the denylist, in the idiom of
`runner::tests::every_production_process_start_is_classified`. Module
privacy cannot make a bypass a compile error from inside this subtree — an
item private to `runner::container` is visible to every module a lane adds
beside this one — so the enforcement is the clippy denylist (a build error)
and this census (a red test), and the two fail for different reasons.

**Lanes A and C: if this test names your file, you are calling the runtime
or the view directly. Call the funnel instead.**

## `fn every_container_effect_in_the_tree_goes_through_the_funnel() {` › `const PRIMITIVES: &[&str] = &[`

The effectful primitives, and the only file that may name them.

## `fn every_container_effect_in_the_tree_goes_through_the_funnel() {` › `if relative == "src/runner/container/fake.rs"`

Test modules of this subtree drive the funnel and may construct a
fake; they are excluded by name rather than by a pattern, so a new
one is a change here.

## `fn every_container_effect_in_the_tree_goes_through_the_funnel() {` › `let funnel = fs::read_to_string(root.join(FUNNEL)).expect("the funnel");`

The control: the funnel itself names every one of them, so a census that
had stopped finding anything fails here rather than reporting silence.

## `fn walk(dir: &Path) -> Vec<PathBuf> {`

`source` with every comment blanked and every string literal left intact.

[`crate::effects::blank_comments_and_strings`] blanks both, which is right
for finding *code* and wrong for finding a name that lives inside a string —
a gated test's own name, or a `docker` program name. Blanking only the
comments is the half this census needs, and it is the half that stops a doc
comment about the scan being counted by the scan.
Every `src/**/*.rs`, sorted.

## `fn stated_lint_level(source: &str, lint: &str) -> Option<&'static str> {`

Which level `source` states for `lint` **at file-module level**, or nothing.

[`crate::effects::lint_levels::file_level_lint_state`] is the whole of it, and the
delegation is the repair rather than a tidy-up. This body used to search the
blanked file for `allow(` or `deny(` holding the lint name, anywhere — so an
**item-level** `#[deny(clippy::disallowed_types)]` written on one `fn`
satisfied a census whose entire subject is whether the *file module* states
a level of its own. A lint level is scoped by the module tree: the item
attribute governs that item, the file goes on inheriting the funnel's allow
for everything else in it, and the census reported the hole closed.
`forbid` was not recognised at all, so a file stating the strongest possible
denial read as stating nothing.

The shared instrument walks the prologue — whitespace and inner attributes
only — and stops at the first item, which is exactly the region an `#![…]`
governs the file from. Comments and string literals are blanked there too,
so a level quoted in prose is invisible (`PR4-CENSUS-COMMENT-ORACLE`).

## `fn closes_the_hole(stated: Option<&str>) -> bool {`

Whether a stated level closes `PR6-LANEF-004` for the lint it names.

`deny` and `forbid` are build errors and `allow` is a reviewed exception
carrying an `effects/allowlist.toml` row. `warn` and `expect` are neither:
they leave the module compiling and are indistinguishable from inheriting,
which is the whole failure. Hoisted out of the grid so every level can be
driven through it -- the tree states only two of the five, so the other
three would otherwise be arms nobody has watched decide.

## `fn allowlist_records(path: &str, lint: &str) -> bool {`

Whether `effects/allowlist.toml` records `path` as allowing `lint`.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {`

Every child module of a Process or Container funnel **states its own lint
level**.

`PR6-LANEF-004`, and it is the one finding of that slice whose repair is
about the *next* lane rather than its own. `src/runner/container.rs` opens
with `#![allow(clippy::disallowed_methods, disallowed_types,
disallowed_macros)]` — an **inner** attribute — and a Rust lint level is
scoped by the **module tree**, not by the file. So every out-of-line child of
`runner::container` inherited it, and the build-error leg of
`effect_site_inventory.mechanism` (1) was not holding for exactly the modules
it exists for: a `ContainerRuntime::start` planted in a child passed
`cargo clippy --all-targets --all-features -- -D warnings`, measured twice.

**The Process funnel is in the domain too, and was not when this was
written.** `src/agent/proc.rs` carries the identical inner allow and had no
out-of-line child at all, so the census that closed the hole for one funnel
left the other covered by nothing but the absence of a directory. It has one
now — `src/agent/proc/test_support/readiness.rs` — and a census scoped to
`src/runner/container/` would have watched it inherit all three allows in
silence. The domain below is derived from the funnel list rather than
written out, so the next funnel to grow a child is covered by the same line.

**The RunDir funnel is in the domain too, and W2 is when that stopped being
optional.** `src/rundir.rs` carries the same inner allow of all three and has
had out-of-line children since W1 — `src/rundir/tests.rs` and
`src/rundir/scratch_tree.rs` — which this census never visited, because
deriving the domain from a funnel *list* only covers the funnels somebody put
on it. `src/rundir/scratch_tree.rs`'s own `effects/allowlist.toml` row says so
in as many words: its level "is written here because the hole is the same one,
not because a gate caught it". The `m3-rundir` split then added five
**production** children under that allowance, and the escape it opened is
concrete rather than theoretical: delete one child's `#![deny(…)]`, put a
`std::fs::write` in an existing function, and the child inherits the parent's
allowance, so Clippy accepts it; the allow-placement scan sees no child
allowance to object to; this census never walks the directory; and wrapper
classification matches the same bare `fn` name without reading the body. An
effect lands with no site while every control stays green. The
`src/rundir.rs` entry below is what closes it.
**The schema-4 workspace funnel is in the domain too, and W2 is when that
stopped being optional.** `src/workspace_manager.rs` carries the same inner
allow of all three and has had out-of-line children since W1 —
`src/workspace_manager/tests.rs` and `src/workspace_manager/fixture.rs` —
which this census never visited, because deriving the domain from a funnel
*list* only covers the funnels somebody put on it. The `fixture.rs` row in
`effects/allowlist.toml` said so in as many words: its level was "written
here because the hole is the same one, not because a gate caught it". The
`m4-workspace` split then added eight **production** children under that
allowance, and the escape it opened is concrete rather than theoretical:
delete one child's `#![deny(…)]`, put a `std::fs::write` in an existing
function, and the child inherits the parent's allowance, so Clippy accepts
it; the allow-placement scan sees no child allowance to object to; this
census never walks the directory; and wrapper classification matches the
same bare `fn` name without reading the body. An effect lands with no site
while every control stays green. The `src/workspace_manager.rs` entry below
is what closes it.

Every file under a funnel's directory either **denies** a governed lint or
**allows** it with an `effects/allowlist.toml` entry a reviewer reads. The
grid is {file} × {which of the three governed lints}, every cell asserted:
a file that states nothing about a lint is inheriting, and inheriting is the
defect.

The negative controls at the end are what stop this being a census that
cannot refuse: the predicate is driven over sources that state nothing, that
state a level only inside a doc comment, that state a level inside a string
literal, that state each level plainly, and that allow a lint the allowlist
records for a *different* file.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `const FUNNELS: [&str; 5] = [`

Every funnel module that allows a governed lint at file scope, and
therefore every module tree an out-of-line child can inherit one through.
`src/runner/host.rs` was named here before it had a directory, so that
the day it grew one the walk would find it rather than the reviewer
having to. **That day arrived in W1**: `src/runner/host/tests.rs` is its
extracted test module, and this walk finds it and grades it against all
three governed lints like any other child.

`src/rundir.rs` was added in W2. It allowed all three at file scope and
had children this census did not visit, so the list -- not the walk --
was the whole of the gap. Measured when it was added: the arm grades
seven files and every cell already passes, because
`src/rundir/tests.rs` allows all three against a row recording all three,
`src/rundir/scratch_tree.rs` allows two against a row recording those two
and denies the third, and the five production children of the `m3-rundir`
split deny all three. Nothing was red; the entry exists so that the next
one would be.

Every funnel named here has a directory today, so no arm of the domain is
inert.
`src/workspace_manager.rs` was added in W2. It allowed all three at file
scope and had two children this census did not visit, so the list -- not
the walk -- was the whole of the gap. Measured when it was added: the arm
grades ten files and every cell already passes, because
`src/workspace_manager/tests.rs` allows all three against a row recording
all three, `src/workspace_manager/fixture.rs` allows two against a row
recording those two and denies the third, and the eight production
children of the `m4-workspace` split deny all three. Nothing was red; the
entry exists so that the next one would be.

Every funnel named here has a directory today, and the assertions below
require that rather than tolerating it, so no arm of the domain is inert.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `let arm = walk(&directory);`

**Per arm, not in aggregate.** A union floor cannot see one arm go
missing once the other arms are large enough to cover for it: the arms
are of very unequal size, so a union floor set for the small ones
survives losing the largest entirely. The floor is therefore stated
where the loss happens -- once per arm, over the class rather than
over the arms that happen to have a named assertion below -- so the
next funnel root inherits the guard instead of needing its own.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `assert_eq!(`

Every funnel in the list, not a floor under it. The comment above says
no arm is inert; this is that sentence asserted rather than believed, and
a funnel that loses its directory is now the finding it always should
have been rather than a silently skipped arm.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `assert!(`

The union backstop stays, and it is a backstop: with the per-arm floor
above it can no longer be the thing that catches a lost arm, and it is
deliberately not raised to the true population, because every remaining
W2 split adds files to one of these arms and a tightened union count
would conflict at each merge while binding nothing the per-arm assertion
does not already bind.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `assert!(`

`readiness.rs` is in the domain **by name**. A count alone would stay
green if the walk lost the `src/agent/proc/` arm entirely, and that arm is
the one this census was widened for. The Process funnel has had siblings
there since the `m6-proc` split, so the pin is on this path rather than on
the arm holding one file: a named path keeps saying the same thing however
many children the arm grows.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `for child in [`

And the RunDir funnel's five production children, by name, for the same
reason: a count would stay green if the walk lost the `src/rundir/` arm
entirely, and those five are what this census was widened for. Named
rather than counted, because *which* file stopped being graded is the
finding -- a count survives a swap.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `for child in [`

And the schema-4 workspace funnel's ten children, by name, for the same
reason: a count would stay green if the walk lost the
`src/workspace_manager/` arm entirely, and that arm is what this census
was widened for. Named rather than counted, because *which* file stopped
being graded is the finding -- a count survives a swap. The two W1
children are named beside the eight the `m4-workspace` split added,
because they are the two that were inheriting in silence until this entry
existed.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `let stated = stated_lint_level(&source, lint);`

**A denial or a recorded allowance, and nothing else counts.** A
`warn` or an `expect` is not a build error, so a module that
states one has not closed the hole this census is about; it is
reported as stating nothing rather than quietly accepted.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `for funnel in FUNNELS {`

The funnels themselves are the files that legitimately carry the allow,
and each is in the allowlist. Asserted here so "everything denies" cannot
become true by a funnel quietly denying itself out of existence.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `let readiness = fs::read_to_string(root.join("src/agent/proc/test_support/readiness.rs"))`

And `readiness.rs` **denies all three at file scope**. It
allowed one of them until `standards/02_standards_automated_baseline.md`,
and the allowance is six per-site `#[expect]` attributes now: narrower
than the file-scope allow it replaces, and counted by the compiler in both
directions under `-D warnings` — a seventh denied call is an error, and an
expectation that stops being met is `unfulfilled_lint_expectations`.
`effects::tests::the_readiness_expectations_are_per_site_and_both_records_say_so`
is the census that keeps the file, the row and the prose agreeing.

The row still records **exactly** the one lint those expectations name and
neither of the two that are only denied. An entry that is merely *present*
is the widening this file's own history is about — `allows` is compared
for equality by `effects::tests::every_allow_of_a_governed_lint_is_module_\
level_and_in_the_allowlist`, and this is the same claim read from the
other end, so a row that grew a second lint fails here too.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `assert!(closes_the_hole(Some("deny")));`

The accept/reject decision, over every level the reader can return --
including the two this tree does not use, whose arms would otherwise be
written and never executed.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `assert_eq!(`

Negative controls: the predicate refuses what it is for.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `for item_level in [`

**Item level is not file level**, and this is the one the scan used to
accept. A lint level is scoped by the module tree, so an attribute on a
single `fn` governs that `fn` and leaves the rest of the file inheriting
whatever its ancestors allow -- which is `PR6-LANEF-004` still open,
reported closed.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `assert_eq!(`

An inner attribute AFTER the prologue governs nothing above it and is not
the file module's own statement either.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `assert_eq!(`

`forbid` is a denial and must read as one; `warn` is not and must not
read as a level the census accepts.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `assert_eq!(`

Both spellings of one lint are one lint, and a prologue of several
attributes is walked through rather than stopped at.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `for (fixture, lint, expected) in [`

**CRLF.** The prologue walk is over bytes, and the guest checks this tree
out with `\r\n`. Every answer above is taken again over the same
fixtures converted, so a walk that treated `\r` as the first token of an
item -- and so ended the prologue at the first line break -- fails here
rather than on the Windows leg.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `for path in [`

And over the real files the census reads, both spellings.

## `fn every_child_module_of_the_container_funnel_states_its_own_lint_level() {` › `assert!(allowlist_records(`

A path that is in the allowlist for one lint does not read as recorded
for another, and a path that is in it at all does not vouch for a
different path: both are how an "is it listed?" check goes vacuous.

## `fn collapsed_prose(text: &str) -> String {`

Prose with its line endings and its wrapping taken out.

Two records state the same two numbers, and both wrap: the TOML with a
trailing `\` and the Rust with a fresh `//`. A search for a phrase that
crosses either wrap is a search for a spelling of a newline, and the guest
checks this tree out with **CRLF** -- so `\n` and `\r\n` are two spellings
again, and `find("… six\nsites")` matches on one platform only.

So nothing here reads a line ending. `str::lines` splits on `\n` and drops
a trailing `\r` itself, `trim` would take any that survived, and what is
rebuilt is the words: comment markers and TOML continuations removed, every
whitespace run collapsed to one space. An explicit `replace('\r', "")` was
written here first and deleted -- measured, it changes no answer, and a line
that cannot fail is a line that says the normalisation is happening
somewhere it is not.

## `fn denied_call_needles() -> Vec<(String, Vec<String>)> {`

The denied-method paths `clippy.toml` names, as needles that find a call.

Derived from the denylist rather than written out, which is what makes the
census below a **closed set**: a list of five needles can only ever confirm
those five, and the question an allowance answers is "which primitives does
this file reach", where a sixth is the whole risk.

Two needles per path, and the second only when the segment before the last
is a type or a trait. `std::io::Write::write_all` is called as
`.write_all(` and `std::fs::rename` as `fs::rename(`, so both forms are
needed; but a method needle for `std::fs::write` would be `.write(` on any
receiver at all, and `libc::write` the same, so those keep the path form
only. The `(` is part of every needle, which is what keeps
`File::create(` from matching `File::create_new(`.

## `fn denied_calls_in(source: &str) -> BTreeMap<String, usize> {`

Every denied method `readiness.rs` reaches, and how many times.

Over the blanked source, so comments and literals cannot count as calls.
`PR4-CENSUS-COMMENT-ORACLE`.

## `fn the_readiness_allowance_names_the_paths_it_is_written_against() {`

The readiness allowance names **exactly** the primitives it is written
against, and the record's arithmetic is the tree's.

`PR72-COUNT-002`. Both records said "five calls", which is two claims run
together and one of them wrong: there are five distinct denied **paths** and
six **sites**, because `fs::remove_file` is called on each of the two
failure paths. A reviewer checking the row against the file would have
counted six and had no way to know which number the row meant.

**Closed over the denylist, not over a list of five.** The set comes from
`clippy.toml` and is compared for equality, so a sixth primitive appearing
in this file fails here whether or not anybody edits the row -- which is the
only version of this census worth having, since an allowance is a claim
about what a file may reach and a list of five needles can only confirm the
five it already knows.

## `fn the_readiness_allowance_names_the_paths_it_is_written_against() {` › `assert_eq!(`

**CRLF.** The guest checks this tree out with `\r\n`, and every count
above has to be the same there. Converted deterministically from the
source just read rather than assumed to be line-ending-blind.

## `fn the_readiness_allowance_names_the_paths_it_is_written_against() {` › `let allowlist = fs::read_to_string(root.join("effects/allowlist.toml")).expect("the allowlist");`

Both records carry the same two numbers, read through the wrapping and
the line endings rather than around them.

## `fn the_readiness_allowance_names_the_paths_it_is_written_against() {` › `let collapsed_row = collapsed_prose(row);`

And the row names each path it is written against, so the record is a
closed set on its own side too.

## `fn every_docker_gated_test_is_named_and_present() {`

Every Docker-gated test is named in the list that counts them, and every
name in the list is a test in this tree.

The skip is loud because it is **counted**: `docker_gate` refuses a test
that is not on the list, and this test refuses a name on the list that is
not a test. A gated test that vanished would otherwise shorten the list and
nothing would say so.

## `fn every_docker_gated_test_is_named_and_present()` › `let mut called: BTreeSet<String> = BTreeSet::new();`

And every test that calls the gate is on the list: the name is readable
from the call site. Comments are blanked first, so this file's own prose
about the gate is not mistaken for a call — measured, because the first
version of this scan reported the placeholder in a doc comment as a
fourth gated test (`PR4-CENSUS-COMMENT-ORACLE`, the fifth occurrence).

## `fn every_docker_gated_test_is_named_and_present()` › `let Some(open) = rest.find('"') else { break };`

`rustfmt` may put the name on the next line, so the first quote after
the call site is what names the test rather than the byte after the
paren. Measured: with a contiguous `gate("` needle this census found
**zero** call sites and reported the whole list as missing.

## `const PREFERRED_IMAGES: &[&str] = &["alpine:3.20", "busybox:latest", "debian:stable-slim"];`

---------------------------------------------------------------------------
8. Docker-gated: the real runtime
---------------------------------------------------------------------------

## `const PREFERRED_IMAGES: &[&str] = &["alpine:3.20", "busybox:latest", "debian:stable-slim"];`

The references the gated tests prefer, in order.

**These tests never pull.** `non_goals[1]` is "implicit image pull", and a
fixture that pulled would be exercising the behaviour the slice forbids on
the very runtime it is meant to prove the refusal against. So the image is
*discovered* among what the machine already holds, and a machine holding
none reports absence through the same loud, counted gate as a machine with
no Docker at all.

## `fn gated_image(docker: &dyn ContainerRuntime) -> Result<(String, ImageInspection), String> {`

A reference the runtime holds, with its id and digest, or the reason there
is none.

## `fn real_docker_reports_an_image_id_and_a_digest_for_a_reference_it_holds() {`

The real runtime resolves a reference it holds to an id and, when it has
one, a manifest digest.

## `fn real_docker_reports_an_image_id_and_a_digest_for_a_reference_it_holds() {` › `let by_id = docker`

The same image asked for by id gives the same id back, and a prefix of it
does not answer this question.

## `fn real_docker_refuses_a_reference_it_does_not_hold_without_pulling() {`

A reference the runtime does not hold is **absent**, and nothing pulls it.

## `fn wait_until_terminated(docker: &dyn ContainerRuntime, name: &str) -> Liveness {`

Poll a real container until it is no longer running.

Bounded round trips rather than a sleep, in the idiom of
[`super::observe_terminated`] — `determinism` forbids sleeps, and each
`docker container inspect` is itself a round trip that takes tens of
milliseconds, so the bound is a real one.

## `fn real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently() {`

The whole R26 lifecycle against the real runtime: create from an id, verify
what it reports, launch through the funnel, reclaim, and reclaim again.

**The plan carries the Git view as a real bind mount**, and that is the whole
reason this test can see `PR6A-LAUNCH-MOUNTS-THE-VIEW-AFTER-CREATE`. It used
to carry `mounts: Vec::new()`, and a real-runtime test with no mounts cannot
see a mount defect: `launch` mounted the view *after* `docker create`, the
view is a bind-mount **source** of that call, and Docker requires a bind
source to exist at create time — so `launch` could not produce a working
container with a Git view at all, and this test passed anyway.

## `fn real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently() {` › `mounts: vec![Mount::Path {`

The R19 view, as a REAL bind mount whose source is the directory
`Container.MountGitView` materialises. This is the mount the
daemon refuses if the view is not there when the container is
created, and it is what makes this test able to see the ordering.

## `fn real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently() {` › `let _ = reclaim(`

Leave nothing behind even when the launch itself failed.

## `fn real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently() {` › `let discovered = docker`

Discovery finds it by `upstroke.private_root`, with its five labels.

## `fn real_docker_creates_from_an_id_reports_it_and_reclaims_idempotently() {` › `for round in 0..2 {`

Reclaim, twice: idempotent and tolerant of already-gone.

## `fn real_docker_kill_on_an_already_exited_container_is_tolerated() {`

The daemon really does answer "is not running", and `DockerCli::stop`
tolerates it.

`PR6-LANEF-003`'s other half. `a_stop_answer_meaning_already_settled_is_tolerated_and_a_real_failure_is_not`
drives [`super::settle_stop`] over a **transcribed** table, and a transcribed
table becomes its own oracle the moment the daemon's wording changes. This
asks the live daemon the same question — a `docker kill` of a container that
has already exited, which is exactly what the second of two racing reclaimers
issues — and asserts both that the phrase is still there and that the seam's
`stop` converges on it.

Second field held constant: the same container, in one state; what varies is
whether the question is asked raw or through the seam.

## `fn real_docker_kill_on_an_already_exited_container_is_tolerated() {` › `let raw = docker`

The raw answer, from the daemon, verbatim.

## `fn real_docker_kill_on_an_already_exited_container_is_tolerated() {` › `docker`

And through the seam, which is what a reclaimer calls: it converges.

## `fn real_docker_returns_both_streams_of_a_container_separately() {`

A container's **stderr** comes back, and separately from its stdout.

`PR6A-DOCKERCLI-MERGES-STDERR-INTO-STDOUT`. `collect` returned
`stderr: Vec::new()` with a comment claiming `docker logs` interleaves the
streams; measured on docker 29.7.2 it **separates** them, and the code did
not merge them — it discarded one. A gate's failure output becomes retry
feedback (DESIGN.md:576), so a gate that fails with everything on stderr
produced empty feedback, and `host::run_shell_probe`'s refusal quotes
`output.stderr` and would have quoted nothing.

The intersection: {which stream a byte was written to} × {which field it
arrives in}. Both directions of the cross are asserted — stdout must **not**
carry the stderr marker and stderr must **not** carry the stdout one — so a
`collect` that merged the two into both fields fails here as surely as one
that dropped one.

## `fn real_docker_removing_a_container_reclaims_its_anonymous_volumes() {`

Removing a container reclaims the **anonymous** volumes it was created with.

`PR6A-ANONYMOUS-VOLUMES-LEAK`. `docker rm --force` without `--volumes` leaves
one anonymous volume per container behind for any image declaring `VOLUME`
or any create carrying `--volume <path>`; **29 leaked from one run of this
suite**, counted by lane A. Those volumes are not R20 — R20 is the
operator's *named*, per-agent credential volume, `operator_owned` and
`persistent_output` in all five `at_run_end` outcomes — they belong to
**R26**, the container's own row, and `Container.Remove` is where R26
balances.

The volume is identified **by name**, read back from the container the test
created, rather than by counting `docker volume ls`: a count is polluted by
whatever else on this machine is making volumes, and a named volume is not.
The control is the assertion **before** the removal — without it, a fixture
that had stopped creating an anonymous volume at all would pass silently.

The intersection: {a volume the run created} × {a volume the operator owns}.
`--volumes` removes only the first kind, which is what makes this repair a
discharge of R26 rather than a violation of R20 — and the R20 half is
asserted here too, with a named volume that must survive.

## `fn real_docker_removing_a_container_reclaims_its_anonymous_volumes() {` › `let _ = docker.raw(`

R20's half: a NAMED volume the operator owns, mounted into the same
container. `--volumes` must not touch it.

## `fn real_docker_removing_a_container_reclaims_its_anonymous_volumes() {` › `"--volume",`

An ANONYMOUS volume: `CreateSpec::Mount::Volume` cannot express
one because it requires a name, which is why this goes through
the test-only raw accessor.

## `fn real_docker_removing_a_container_reclaims_its_anonymous_volumes() {` › `assert!(`

The control: it really is there before the removal, so this test cannot
pass by never having created one.

## `fn real_docker_removing_a_container_reclaims_its_anonymous_volumes() {` › `let _ = docker.raw(`

Clean up before asserting, so a failure does not leave the daemon dirty.

## `fn the_ps_format_asks_for_exactly_the_labels_the_parser_names() {`

---------------------------------------------------------------------------
`PR6-RECOV-002` — `docker ps` renders labels ambiguously, so this does not
ask it to
---------------------------------------------------------------------------

## `fn the_ps_format_asks_for_exactly_the_labels_the_parser_names() {`

The format string asks for exactly the labels the parser names, in order.

Two lists that must agree; the failure mode if they drift is a field read
under the wrong name, which for `upstroke.run_dir` is a probe of another run's
lock. The oracle is the format string's own text, scanned for `{{.Label
"…"}}` — an independent derivation from `PS_LABELS`, not a restatement.

## `fn the_ps_format_asks_for_exactly_the_labels_the_parser_names() {` › `assert!(PS_FORMAT.starts_with("{{.Names}}"));`

The name field, and exactly one separator per field boundary.

## `fn a_label_value_carrying_a_comma_or_an_equals_is_read_whole() {`

A label value carrying the delimiters of the *old* rendering survives whole.

The parse's oracle is a hand-written table of `(rendered line, expected
name, expected labels)`, built from what `docker ps` really prints — see
`real_docker_renders_a_comma_bearing_label_value_whole` for the same values
checked against the live daemon.

Second field held constant: every line carries the same container name and
the same four other labels; only `upstroke.run_dir`'s bytes move, across
values that are and are not hostile to a comma-joined format.

## `fn a_label_value_carrying_a_comma_or_an_equals_is_read_whole() {` › `"/repo/a,b/.upstroke/runs/B",`

Not values `path_label` emits — a foreign container may carry
anything, and the parser must still read the field it was given
rather than the prefix before the first comma.

## `fn a_ps_line_whose_fields_do_not_line_up_is_refused() {`

A line whose fields do not line up is **refused**, not mis-split.

The fail-closed half of choosing a delimiter: this engine's own label values
never carry `U+001F` or a newline, but a foreign container carrying this
private root's label may carry anything, and a census that guessed which
field was the owner's run directory would probe another run's lock. `Failed`
and not `Unreachable`, because the daemon answered.

Second field held constant: every line below is one container's worth of
output with a valid name; only the field count moves.

## `fn a_ps_line_whose_fields_do_not_line_up_is_refused()` › `let empty = format!("c{sep}/srv/private{sep}RUNB{sep}{sep}INC2{sep}p.shell.o0");`

An empty rendered value is an absent label, and both are refused
downstream. A container with no name at all is skipped rather than
refused: `docker ps` renders a blank line for nothing this filter
selected.

## `const UNREACHABLE_STDERR: &[(&str, &str)] = &[`

---------------------------------------------------------------------------
`PR6-RECOV-005` — "permission denied" is not "no runtime"
---------------------------------------------------------------------------

## `const UNREACHABLE_STDERR: &[(&str, &str)] = &[`

The verbatim stderr of a `docker` that could not be reached, and the command
that produced each.

Transcribed from runs on this project's build box against `docker` 29.7.2.
This is the table `is_unreachable_diagnostic` is measured against, and it is
**not** derived from `UNREACHABLE_DIAGNOSTICS`:
`real_docker_prints_the_transcribed_unreachable_diagnostics` replays two of
these through the live CLI so the oracle is the daemon.

## `const ANSWERED_STDERR: &[&str] = &[`

The stderr of a daemon that **answered** and refused.

The other half of the classification, and it has to be a table of its own:
a predicate that returned `true` for everything would pass the table above
and turn every real failure into "proceed without a runtime".

## `fn the_docker_diagnostic_classifier_tells_unreachable_from_answered() {`

Every measured "could not be reached" classifies as unreachable, and every
measured "answered and refused" does not.

`crash_reconstruction`: "with no intent and no reachable runtime it
**proceeds**". `PR6-RECOV-005`: the shipped three-string test classified the
socket-permission diagnostic as `Failed`, so a census with no intents at all
refused — on the single most common configuration of a machine that has
Docker installed and not configured. Measuring it turned up a second one:
`docker` 29's wording for an **absent socket** was also classified `Failed`.

Second field held constant: one operation and one call shape in every cell;
only the diagnostic text moves.

## `fn the_two_docker_diagnostic_tables_never_claim_one_message() {`

The two `docker` diagnostic tables never claim one message.

`stop_already_settled` matches "is not running", which is a **reached**
daemon reporting a container's state; if an unreachable shape ever matched
it, a racing reclaimer's tolerated error would become "the runtime cannot be
reached" and refuse the write command. Asserted over both tables at once so
a future entry in either is checked against the other.

## `fn the_two_docker_diagnostic_tables_never_claim_one_message() {` › `let racing =`

And the one message that is *most* at risk: a racing reclaimer's kill.

## `fn real_docker_renders_a_comma_bearing_label_value_whole() {`

The live daemon's `.Labels` really is ambiguous, and `.Label "…"` really is
not.

`PR6-RECOV-002`'s premise, checked against `docker` rather than against a
transcription of it: a container is created whose `upstroke.run_dir` contains
a comma, and the two renderings are compared. The comma-joined one is
**asserted to be ambiguous** — it is byte-identical to what a container with
an extra label would print — and the census's own path is asserted to give
the value back whole.

Second field held constant: one container, one label set; only the format
string handed to `docker ps` moves.

## `fn real_docker_renders_a_comma_bearing_label_value_whole()` › `read_only_root: true,`

Repair R1 added this field after R2 wrote this test. `true` matches
every other CreateSpec in the suite and is what the runner now
supplies; this container only runs `exit 0` and writes nothing.

## `fn real_docker_renders_a_comma_bearing_label_value_whole()` › `let found = docker`

(a) What the census asks for, through the production seam.

## `fn real_docker_renders_a_comma_bearing_label_value_whole()` › `let raw = docker`

(b) The rendering that shipped, from the same daemon, asserted to be
ambiguous. `{{.Labels}}` prints the comma inside the value exactly as it
prints the separator between labels, so the bytes below are also a
perfectly good rendering of a *different* label set — which is what makes
parsing them a guess rather than a read.

## `fn real_docker_prints_the_transcribed_unreachable_diagnostics() {`

The transcribed unreachable diagnostics are what the live CLI prints.

`UNREACHABLE_STDERR` is a table of strings, and a table compared only
against the classifier built from it proves nothing. This asks the real
`docker` binary for two of them — an absent socket and a socket this process
may not use — and classifies **its** stderr.

It drives `docker` directly rather than through [`DockerCli`], because
`DOCKER_HOST` is process-wide and the seam deliberately configures no
socket (`non_goals[3]`, "remote runners").

## `fn real_docker_prints_the_transcribed_unreachable_diagnostics() {` › `let denied = {`

A socket path this process may not reach into. `chmod 000` is the
deterministic way to produce the *permission* diagnostic without a second
user account; running as root would defeat it, so that case is skipped
rather than asserted falsely.

## `fn container_lock_probe_child_holds_the_run() {`

---------------------------------------------------------------------------
`PR6-RECOV-004` — the production liveness probe, against a lock a real other
process holds
---------------------------------------------------------------------------

## `fn container_lock_probe_child_holds_the_run() {`

The child of [`the_production_lock_probe_sees_a_lock_another_process_holds`].

Takes a real `RunLock` on the directory it is given, creates the readiness
file it was told to, and waits. `#[ignore]`d because it is a fixture rather
than a test: it is invoked by name, as a subprocess, in the idiom
`rundir::tests::lock_child_holds_the_run` established for exactly this.

It signals through a **file** rather than through stdout, so the fixture
needs neither `println!` nor a piped `Stdio` — `clippy::disallowed_macros`
is re-denied in this file by `PR6-LANEF-004` and this repair does not widen
that.

## `fn the_production_lock_probe_sees_a_lock_another_process_holds() {`

[`LockProbe`] answers **true** for a run another **process** is really
driving, and **false** once it lets go.

`PR6-RECOV-004`. Arm (ii) is "probe that run's run.lock non-blocking; free ->
dead owner -> reclaim …; **held -> live owner -> never touched**", and every
census fixture in this slice injects a `RecordingLiveness` or a
`FakeOwnerLiveness`. The only assertion against the production adapter used a
directory with **no lock** and expected `false` — so `is_running` returning a
constant `false` passed the whole suite, and a constant `false` classifies
every live owner as dead and kills its containers.

It has to be a real second **process**, and that is not incidental:
`fcntl` locks are per-process, and `rundir::is_running` answers from this
process's own `claims` table before it opens anything. A lock taken *here*
would exercise the claims path and bless an adapter that consulted only
that — which is precisely the "reports false while foreign run B holds it"
shape the finding names, since a foreign run is by definition another
process. `rundir::tests::a_second_process_is_refused_the_run_lock` spawns a
child for the same reason, and this borrows its shape.

The child is started through [`host::test_support::build_command`], the crate's one
producer of a `std::process::Command`, so this file never names the
disallowed type.

Second field held constant: one directory, one probe, one process asking;
only whether the owner process is alive moves.

## `fn the_production_lock_probe_sees_a_lock_another_process_holds() {` › `assert!(`

Before: nobody holds it.

## `fn the_production_lock_probe_sees_a_lock_another_process_holds() {` › `let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);`

Wait for it to say it has the lock rather than sleeping and hoping.

## `fn the_production_lock_probe_sees_a_lock_another_process_holds() {` › `let started = std::time::Instant::now();`

Held, and the probe says so **and returns**: `T-CONTAINER.resume_action`
is "probe the owner's run.lock **non-blocking**", so this call is the one
a blocking implementation would never come back from. The bound is
generous — it is here to fail a `LockProbe` that waits, not to measure
one that does not.

## `fn the_production_lock_probe_sees_a_lock_another_process_holds() {` › `let _ = child.kill();`

And the answer follows the world rather than being a constant: once the
owner is gone the same directory reads free. Without this half a probe
hard-coded to `true` would pass the assertion above.

## `fn every_view_discard_removes_through_the_one_racing_removal() {`

---------------------------------------------------------------------------
`PR6-CORRECTNESS-009` — the R19 view's removal converges, and fails closed
---------------------------------------------------------------------------

## `fn every_view_discard_removes_through_the_one_racing_removal() {`

Every `GitView::discard` in the tree removes through the **one** retrying
removal.

`crash_reconstruction`: "every step idempotent and tolerant of already-gone
so **two concurrent reclaimers converge**". On Windows the loser of that
race does not get `NotFound` — a directory whose last handle has closed is
delete-pending and `remove_dir_all` reports `PermissionDenied` until the
name goes away — so a `match` that tolerates only `NotFound` refuses one of
two converging write commands. `DisposableDirView` had been repaired;
`RoleGitView`, the projection a real run mounts, had not, and every
concurrent-census fixture used the other one (`PR6-CORRECTNESS-009`).

This is a source census because the platform behaviour it is about cannot be
produced deterministically on Linux, and because "there is one removal" is a
claim about the tree rather than about a call. It is the shape
`the_view_directory_has_one_definition_in_the_tree` already uses for the
other half of this same seam. The behavioural halves are the two tests
below.

## `fn every_view_discard_removes_through_the_one_racing_removal() {` › `const SUBSTRATE: &[&str] = &[`

The out-of-line test substrate of this subtree, excluded **by name**
rather than by a pattern, so a new one is a change here. Everything else
is cut at its first `#[cfg(test)]` by `production_region`; these files
have none, being test modules in their entirety.

## `fn every_view_discard_removes_through_the_one_racing_removal() {` › `let production =`

Strings blanked as well: this test's own doc comment and the literal
it scans for would otherwise be findings about itself. Whitespace
flattened so a rustfmt wrap is not a false finding.

## `fn discarding_a_role_view_twice_converges() {`

Discarding a view twice converges, and the second call is not an error.

The already-gone half of "two concurrent reclaimers converge", made
deterministic by serialising the two reclaimers rather than racing them —
the race itself is `census::tests::concurrent_reclaimers_converge`, and this
is the predicate underneath it, held against the **`RoleGitView`** the
concurrent fixtures do not use.

Second field held constant: the same path, the same view, the same trace in
both calls; only whether the directory is still there moves.

## `fn a_role_view_that_cannot_be_removed_refuses_and_records_nothing() {`

A view that genuinely **cannot** be removed refuses, and says nothing was
discarded.

The fail-closed half, and the test for what this repair could have got
wrong. The smaller fix — tolerating `PermissionDenied` outright — would make
a protected view report success; the census would then go on to remove the
intent, and admission would proceed over R19 residue that nothing can ever
reclaim, because the record naming it is gone. `resource_accounting` R19
requires orphan views reclaimed, and "reclaimed" is not "forgotten".

Constructed on Unix by clearing the parent directory's write bit, which is
deterministic and is not delete-pending — the two are different states and
only one of them is transient. Skipped under a uid that ignores the bit.

Second field held constant: the same view, the same path, the same content;
only the parent's permissions move.

## `fn a_role_view_that_cannot_be_removed_refuses_and_records_nothing() {` › `let _ = fs::set_permissions(&parent, fs::Permissions::from_mode(0o755));`

Running as root, or on a filesystem that ignores the mode. Restore
and say so rather than asserting something that is not true here.

## `pub(super) fn note_racing_attempt(failed: usize) {`

The `#[cfg(test)]` half of the attempt seam in `super::racing_pause`: record
the count, and run whichever observer this thread installed. `try_borrow_mut`
so an observer that itself removes something is a no-op here rather than a
panic inside production code. The shape, the thread-local counter, the
`RacingObservation` guard whose `Drop` uninstalls the observer and the
`observe_racing_attempts` installer are `workspace_manager::fixture`'s.

## `fn windows_posix_delete_pending(path: &Path) -> fs::File {`

Put `path` into the state the loser of a Windows removal race sees while the
winner is stalled: **delete-pending, name still present**.

std's `remove_dir_all` ends by reopening the root relative to its listing
handle with `DELETE`, setting `FileDispositionInfoEx` with
`FILE_DISPOSITION_FLAG_POSIX_SEMANTICS`, and closing that handle;
`DeleteFileW` does the same three things to a file. The name is unlinked by
the close, not by the disposition call — measured on the Windows guest against
a raw replay of exactly that sequence: between the two calls a `CreateFileW`
of the name answers `ERROR_ACCESS_DENIED`, and the instant the delete handle
closes it answers `ERROR_FILE_NOT_FOUND`. This helper reproduces the state,
not the call sequence: one open with `DELETE`, one disposition call, and the
returned handle *is* the winner's delete handle, so dropping it is the winner
reaching its close.

The premise is asserted with the same open `remove_dir_all` makes first, not
with `symlink_metadata`: that one falls back to `FindFirstFileExW`, which
still lists a delete-pending name and would report the premise met while the
open that matters had never been tried.

The `unsafe` is one `SetFileInformationByHandle` on a handle this function
owns, with a fully initialised `FILE_DISPOSITION_INFO_EX` and its size — the
call's documented contract, and the whole of the SAFETY obligation.

## `fn windows_stalled_removal_budget() -> std::time::Duration {`

How long `racing_removal` sleeps in total before it believes a refusal:
`RACING_SLEEP` for each failure after `RACING_YIELD_ATTEMPTS` except the last,
which no attempt follows. Module-scope rather than a number restated in each
test, for the reason `workspace_manager::ATTEMPTS` gives: the control has to
reason about the budget in the units that produce it, or a budget cut in half
passes a test that only asserted "it waited".

## `const WINDOWS_RELEASE_AFTER_FAILURES: usize = super::RACING_YIELD_ATTEMPTS + 4;`

The failed attempt after which the stalled winner is released: past the
yields, so the loser is provably in the sleeping part of its budget when the
close lands, and far enough past them that the release is not on the boundary.

## `fn windows_close_once_the_loser_sleeps(`

The stalled winner, as a thread: it closes the delete-pending handle only
after the loser has reported `WINDOWS_RELEASE_AFTER_FAILURES` failed attempts
through the seam, which orders the close against an attempt that has already
returned rather than against a clock. Then one more rule, so the yield-only
loop fails **deterministically** rather than by a coin toss: a repaired loop
sleeps `RACING_SLEEP` between attempts, so nothing arrives within half of
one and the close happens at once; a yield-only loop reports its remaining
attempts within microseconds, so it is drained to its bound first and closes
only after the refusal it was always going to give. The single residual
non-determinism is a yield-only loser preempted for longer than half a sleep
between two of its own attempts, which turns a witnessed failure into a pass
and is stated here rather than hidden.

Returns the failure number the handle was released after, so the test can
say which attempt converged.

## `fn windows_stall_then_release(`

Run `work` on this thread as the loser with the seam observed, the winner
stalled in `pending`, and the closer above waiting on the observation.
Returns the loser's failed-attempt count, the failure the release followed,
and the elapsed time — the observation is dropped before the closer is
joined, which is what lets a refused loser's closer return.

## `fn windows_assert_converged_through_the_wait(tag: &str, loser_failed: usize, released_after: usize) {`

The two facts that say the convergence came through the wait and not through
a name that was never delete-pending: the release fell inside the sleeping
part of the budget, and the loser's last failure *is* the one the release
followed, so the attempt after it is the one that found the name gone.

## `fn windows_a_view_whose_remover_stalls_delete_pending_converges_once_the_stall_ends() {`

`PR154-WINDOWS-CENSUS-VIEW-REMOVAL-ACCESS-DENIED`, forced. The census race
that failed on the Windows CI leg lost sixty-four yields in a fraction of a
millisecond to a winner that had marked the view delete-pending and not yet
closed its handle. Here the winner is [`windows_posix_delete_pending`], its
close is ordered against the loser's twentieth failed attempt, and the loser
is the real `discard` of both views through the one `racing_removal`.

On the yield-only loop this fails with the CI failure's own text after
sixty-four failed attempts: `failed to read <view>: Access is denied. (os
error 5)`. On the repaired loop the discard returns `Ok` on the attempt after
the release, which [`windows_assert_converged_through_the_wait`] checks.

Second field held constant: the same empty view the census seeds, the same
release rule; only which `GitView` discards it moves.

## `fn windows_a_view_held_delete_pending_past_the_budget_refuses_and_keeps_the_intent() {`

The fail-closed half of the same repair, at the reclaim boundary: a view held
delete-pending through the **whole** budget still refuses, and the refusal
carries the native error, comes after exactly the bound, keeps the intent,
and records no discard.

The things a longer wait could have broken. It must still be bounded, so the
refusal has to arrive, after exactly `RACING_ACCESS_ATTEMPTS` failures — the
seam counts them — and within ten times the sleep budget, generous enough for
a starved runner and small enough to catch a loop that never returns. It must
not arrive early, so the lower time bound is the sleep budget itself, which a
yield-only loop fails after a few milliseconds. And it must not be read as
absence: `reclaim` stops at `UnmountGitView`, so `RemoveIntent` never runs and
the record that names the residue survives for the next census — which the
second half demonstrates by dropping the handle and reclaiming, at which
point view and intent both go.

Second field held constant: one launched container, one intent, one view;
only whether the winner's handle ever closes moves.

## `fn windows_an_intent_whose_remover_stalls_delete_pending_is_read_and_removed_once_the_stall_ends() {`

The same forced stall on the **intent file**, which is the other path the
census race has refused on: PR #152's run 34001739777 named
`containers/<name>.intent` with the same os error 5. Both loops that meet it
are exercised: `read_racing`, through `list_intents`, is the census
discovering records while another reclaimer is mid-delete, and
`racing_removal`, through `remove_intent`, is two reclaimers on one record.
Each gets the release rule above; discovery must answer with the record
absent rather than refuse, and removal must converge. On the yield-only loop
discovery fails first, with the #152 text.

Second field held constant: one record, written the same way for both halves;
only which loop meets the stall moves.

## `fn a_create_whose_named_volume_is_absent_is_refused_before_any_effect() {`

---------------------------------------------------------------------------
R3b: R20 is never created by a run, and the runtime does not enforce that
---------------------------------------------------------------------------

## `fn a_create_whose_named_volume_is_absent_is_refused_before_any_effect() {`

A create whose named volume is absent is **refused before any effect**.

`PR6-ACCT-001` / `PR6-CORRECTNESS-014`. R20 is `operator_owned` and
`persistent_output` — "never created or pruned by a run" — in all five
`at_run_end` outcomes, and `docker create` does not honour it: measured
against `docker` 29.7.2, `--mount type=volume,source=<absent>,target=/creds`
**succeeds and creates an empty named volume**. Resolution inspects the
volumes once, before the worktree lock; a volume removed between that
inspection and the invocation is seen by nothing but a check at the create
itself.

The grid is **{which agent's volume is missing} × {how the runtime answers}**
and its second axis is the one a single-cell fixture misses: a runtime that
*will not say* whether the volume exists must refuse too, because "the
runtime did not answer" is not "the volume is there". Every cell holds the
same spec, the same intent proof and the same image; only the volume state
moves.

"Before any effect" is asserted on the **trace**, not inferred: no
`rt:create` and no `site:Create` entry at all, so the refusal precedes even
the funnel's `Before` phase.

## `fn a_create_whose_named_volume_is_absent_is_refused_before_any_effect() {` › `const CREDENTIALS: &[(&str, &str)] = &[`

Three agents, three volume names — all distinct, so a check that
inspected the wrong one is visible.

## `fn a_create_whose_named_volume_is_absent_is_refused_before_any_effect() {` › `fixture.runtime.set_unreachable(RuntimeOp::InspectVolume);`

Only the volume inspection is armed: the whole daemon being
down is a different refusal, and this cell is about a runtime
that answers everything else and will not answer this.

## `fn a_create_whose_named_volume_is_absent_is_refused_before_any_effect() {` › `assert!(`

**Before ANY effect, asserted first.** This fake happens to
refuse an absent mounted volume too, and a real daemon does the
opposite — it creates one — so an assertion on *who* refused
would be an assertion about the fixture. The ordering is the
claim that belongs to the engine: the create site was never
entered at all, which is only true of a check that runs before
the funnel.

## `fn a_create_whose_named_volume_is_absent_is_refused_before_any_effect() {` › `if reachable {`

And nothing was created on the way past: the other two volumes
are still exactly the two the fixture provisioned.

## `fn real_docker_creates_an_absent_named_volume_rather_than_refusing() {`

The daemon really does create the volume, so the guard above is not
defending against a fake.

`PR6-ACCT-001`'s premise, measured rather than asserted. The engine's own
`create` is never reached: this drives `docker create` directly through the
test-only raw accessor with a volume name the daemon does not hold, and then
asks the daemon whether it now holds one. R20's "never created by a run" is
therefore an **engine** guarantee and not a runtime one, which is exactly
why the check lives in `create_container`.

Second field held constant: the same image and the same container name in
both halves; only whether the volume was provisioned first moves.

## `fn real_docker_creates_an_absent_named_volume_rather_than_refusing() {` › `assert!(`

The premise: it is not there.

## `fn real_docker_creates_an_absent_named_volume_rather_than_refusing() {` › `let _ = docker.remove(name);`

Clean up before asserting.

## `const DAEMON_REMOVAL_IN_PROGRESS: &str =`

---------------------------------------------------------------------------
R3b: the third answer a racing reclaimer gets
---------------------------------------------------------------------------

## `const DAEMON_REMOVAL_IN_PROGRESS: &str =`

What `docker` 29.7.2 answers the **loser** of two overlapping removals,
measured on the build box.

```text
$ docker create --name c alpine sh -c 'dd if=/dev/zero of=/big bs=1M count=800; sleep 5'
$ docker start c
$ for i in 1..8; do docker rm --force --volumes c & done
c                                                          (one winner, exit 0)
Error response from daemon: removal of container c is already in progress   (x7)
```

Transcribed, not invented, and
[`real_docker_prints_the_transcribed_removal_in_progress_diagnostic`] asks
the live daemon the same question so the table cannot become its own oracle.

## `fn a_removal_answer_meaning_already_in_progress_is_tolerated_and_a_real_failure_is_not() {`

A `docker rm` answer meaning "somebody else is already removing it" is
tolerated; a real failure is not.

`PR6-CONV-002`. This is the **third** state a racing reclaimer sees, and it
is neither "gone" nor "already stopped": `T-CONTAINER.resume_action`'s
"(idempotent; **concurrent reclaimers converge**)" is false without it,
because the loser returns an error before `rm`, the view prune and the
intent removal, and the write command driving it refuses instead of
converging. Deleting the tolerance passed the whole suite: `FakeRuntime`'s
`remove` cannot produce this answer at all, and the one real-Docker reclaim
is sequential.

[`super::settle_remove`] is a free function over the raw outcome for the
reason [`super::settle_stop`] is: the branch is reachable — and testable —
**without a daemon**, which is what makes it assertable at all on CI and on
the Windows guest.

The intersection: {what the daemon said} × {is it tolerable}, with the
tolerable answers counted as **distinct** values so three cells cannot be
one string repeated. The `Unreachable` cell carries tolerable *text*,
because a runtime that could not be reached said nothing about the
container.

## `fn a_removal_answer_meaning_already_in_progress_is_tolerated_and_a_real_failure_is_not() {` › `assert!(`

The clause is its own predicate, and it is not covered by absence: the
in-progress answer contains none of the "no such …" shapes.

## `fn a_removal_answer_meaning_already_in_progress_is_tolerated_and_a_real_failure_is_not() {` › `assert!(super::remove_already_settled(`

Case-insensitively, because a vendor that recapitalises its prose must
not turn a convergence into a refusal.

## `fn a_removal_answer_meaning_already_in_progress_is_tolerated_and_a_real_failure_is_not() {` › `assert!(super::stop_already_settled(DAEMON_REMOVAL_IN_PROGRESS));`

A `docker kill` racing a removal gets the same answer, and it is on its
way out either way.

## `fn a_removal_answer_meaning_already_in_progress_is_tolerated_and_a_real_failure_is_not() {` › `assert_eq!(super::settle_remove(Ok("upstroke-c\n".to_owned())), Ok(()));`

The control: a removal that simply worked.

## `fn real_docker_prints_the_transcribed_removal_in_progress_diagnostic() {`

The live daemon really does answer overlapping removals that way.

The oracle for [`DAEMON_REMOVAL_IN_PROGRESS`]. A transcribed diagnostic
checked only against the code that reads it proves nothing, so this races
real `docker rm` calls against a container whose removal takes long enough
to overlap — 800 MiB of zeroes written into its layer — and asserts that the
loser's verbatim stderr both (a) matches the shape the table carries and
(b) settles through the production tolerance.

Second field held constant: every racer issues the **same** removal against
the **same** container; only which one the daemon serves first moves. A run
in which no racer loses is reported as a skip of the measurement rather than
as a pass, so a machine fast enough to serialise them cannot make this
vacuously green.

## `fn real_docker_prints_the_transcribed_removal_in_progress_diagnostic() {` › `for _ in 0..4 {`

A few attempts: the race is real and a machine may serve one removal
before the others are issued.

## `fn real_docker_prints_the_transcribed_removal_in_progress_diagnostic() {` › `std::thread::sleep(std::time::Duration::from_millis(1_500));`

Let it write, so the removal has something to tear down.

## `fn real_docker_prints_the_transcribed_removal_in_progress_diagnostic() {` › `return no_image(`

Not a pass: the measurement did not happen, and this says so in the
same voice a missing image does.

## `fn real_docker_prints_the_transcribed_removal_in_progress_diagnostic() {` › `assert!(`

(a) The transcribed table names the same shape the daemon printed.

## `fn real_docker_prints_the_transcribed_removal_in_progress_diagnostic() {` › `assert_eq!(`

(b) The production tolerance settles it.

## `fn a_release_whose_cleanup_fails_still_attempts_every_remaining_step() {`

---------------------------------------------------------------------------
R3b: the completion path releases exhaustively too
---------------------------------------------------------------------------

## `fn a_release_whose_cleanup_fails_still_attempts_every_remaining_step() {`

A release whose own cleanup fails still attempts every remaining step, and
says what it could not release.

`PR6-ACCT-004`. [`release`] — the "stop/rm, view removal, intent removal
**after completion**" half — `?`-chained its four sites, so a
`Container.Stop` that failed on an invocation that had **completed** skipped
the still-viable forced remove, the view prune and the intent removal: three
residues from one failure, on the ordinary path rather than on a refusal.
`docker rm --force` removes a running container, so `Remove` after a failed
`Stop` is not a wasted call.

The cleanup-fault grid already existed for the *cancel* path and the two
were separate implementations, so the exhaustive one was tested and the
fail-fast one shipped. There is now one implementation
([`super::cancel_reached`]) and this is the completion path's grid over it.

| armed | container | view | intent |
|---|---|---|---|
| `Stop` | removed | pruned | removed |
| `Remove` | **left** | pruned | removed |
| `UnmountGitView` | removed | **left** | **left** (the R19 anchor) |
| `RemoveIntent` | removed | pruned | **left** |

Second field held constant: the launch succeeds identically in all four
cells — same image ids, same plan, no substitution — so what varies is only
which release step was armed.

## `fn a_release_whose_cleanup_fails_still_attempts_every_remaining_step() {` › `assert!(!fixture.runtime.container_names().is_empty());`

The control: everything the release has to remove is really there.
