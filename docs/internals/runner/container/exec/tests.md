# `src/runner/container/exec/tests.rs`

Extended notes for [`src/runner/container/exec/tests.rs`](../../../../../src/runner/container/exec/tests.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## `#![allow(clippy::disallowed_methods)]`

Allowlist placement: the **funnel section** of `effects/allowlist.toml`, which
carries this file's own review clause. This is the extracted test module of
the `ContainerRunner` funnel -- the region that lived inline in
`src/runner/container/exec.rs`, under an outer allow on its declaration
there, moved out unchanged. That outer attribute is GONE, deleted by the same
change: with the level stated here it allowed one lint twice for one module,
which is what `clippy::duplicated_attributes` is, and `exec.rs`'s own
allowlist row records the empty `allows` that leaves. The set of calls
permitted here is unchanged by the move: the fixtures build worktrees and
scratch trees and drive the runtime seam directly, exactly as they did
inline. What moved is where the permission is stated, not what it permits.

`PR6-LANEF-004`: it states that level **of its own** rather than inheriting
one. A lint level is scoped by the MODULE TREE and not by the file, so an
out-of-line child of a funnel is covered by the parent's allow unless it says
otherwise, and the funnel's child-module census requires each child to say so.
The two lints this file does not need are re-denied, so either one appearing
here is still a build error.
`decisions.effect_site_inventory.mechanism` (2).

## `fn repo_key() -> &'static str {`

This module's repository key: **the build slot's**, through the one
place that derives it.

The derivation and the reason it is not a constant moved to
[`crate::runner::container::fake::slot_repo_key`], beside the pre-clean it is a
precondition of. It lived here, and `census/tests.rs` — the other caller
of `fake::preclean_names` — went on using a fixed `"cccccccccccccccc"`,
which is `PR7-R3-CONTRACT-001` still live on that path. A rule with one
implementation per caller is a rule each caller can be missing.

## `fn a_container_name_is_scoped_to_its_build_slot() {`

**A pre-clean reclaims its own slot's residue and nobody else's.**

The class boundary, not the instance. `PR7-R3-CONTRACT-001` is that
`fake::preclean_names` kills by name with no liveness check, and a
*fixed* name means the container it kills may be a live stranger's.

**A liveness check would not have fixed it.** The state this helper
exists for is a SIGKILLed run whose container is still *running*, so
"don't kill running ones" defeats the helper. The fix is that two
concurrent runs never ask for the same name, and this asserts that
property rather than the instance that prompted it.

## `const EVENT_LOG_MARKER: &str = "COORDINATOR-EVENT-LOG-a5f2";`

Written into the run's public log, so a container that could read it
would be caught by content rather than by the absence of a file.

## `const IMAGE_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";`

The `PATH` the fake fixtures' image environment carries.

Absolute-only, which is what `ContainerEnvironment::certify_path` now
requires: a runner whose composed environment names no `PATH`, or names
one with a working-directory-relative component, refuses every
invocation (`PR6-CORRECTNESS-006`). The value is the one every image
this suite discovers actually carries, read off `docker image inspect`
for `upstroke-test/git:v1`, `alpine:3.20` and `busybox:latest`.

## `const CWD_RELATIVE_PATH: &str = "/usr/local/bin:.:/usr/bin";`

A `PATH` whose second component is the working directory.

`.` explicitly rather than an empty component, so the two shapes
`cwd_dependent_path_components` classifies are exercised by different
fixtures rather than by one.

## `fn image_environment() -> ContainerEnvironment {`

The image environment the fake fixtures compose over.

Explicit rather than `ContainerEnvironment::inherited()`, which is now a
base that refuses: DESIGN.md:260 has the runner supply `PATH`, and an
empty base supplies nothing.

## `struct Scripted {`

-----------------------------------------------------------------------
A runtime that can finish, and that a test keeps a handle on
-----------------------------------------------------------------------

## `struct Scripted {`

The fake, wrapped so a test can hold it while the runner owns it, and so
a container can be made to **finish**.

`FakeRuntime::start` leaves a container `Running` and nothing in a
synchronous `Runner::run` could move it afterwards, so the success path
would be unreachable and only the timeout path would ever be measured. A
decorator that exits the container at `start` — and, when asked, gives
it an exit status and output — is what makes both paths constructible;
the plain fake still drives the timeout.

## `impl Runtime` › `fn scripts(&self, execution: ContainerExecution) -> Self {`

What every container of this runtime reports when it finishes.

## `struct Fixture {`

-----------------------------------------------------------------------
A realistic run layout
-----------------------------------------------------------------------

## `struct Fixture {`

One run, laid out where the engine really puts things.

Every path here comes from the type that owns it — [`RunPaths`] for the
two halves of a run directory, `workspace_manager::execution_root_of`
for the worktrees — rather than from string literals, so a layout change
moves the fixture with it. A hand-built layout is a fixture that keeps
passing after the thing it describes has moved.

## `impl Fixture` › `fn confinement(&self) -> Confinement {`

Everything this run withholds.

**The two sibling worktrees are no longer added by hand.** They were
until repair R1, and `PR6-CORRECTNESS-013` is what that cost:
`Confinement::of_run` named no worktree path at all, so the helper
production uses withheld nothing about worktrees and only the
fixtures pretended otherwise. `of_run` now derives the execution
root and its three namespaces, which is what makes this method a
plain delegation — and what makes the sibling assertions in
`the_mount_set_is_the_roles_own_and_reaches_nothing_of_the_coordinators`
statements about the production helper rather than about the
fixture.

## `impl Fixture` › `fn withheld(&self) -> Vec<(Withheld, PathBuf)> {`

The concrete host paths this run withholds, as a table a test can
iterate — derived from the same accessors the layout is built from.

## `fn requests(workspace: &Path) -> Vec<RunnerRequest> {`

A request in every role, over one workspace, with the binding each role
takes in production.

## `fn hostile_bindings(workspace: &Path) -> Vec<RunnerRequest> {`

Requests whose **role and agent binding are varied independently**.

`runner::gate_request` and `host::shell_probe_request` bind no agent, so
a grid built only from the production builders never asks the question
the role rule exists to answer: what happens to a role that takes no
credentials and names an agent anyway. `host-v1`'s own
`reserved_values` says it in as many words — "neither is told where an
agent's credentials live, **whatever agent the request happens to
name**" — and until this grid existed, deleting the role check from the
container's mount plan changed nothing any test could see (measured:
mutation `M8-credential-volume-for-every-role` survived the whole
suite). That is `PR4-CONF-002`'s class exactly: a predicate keyed on a
field no fixture varies on its own.

## `fn sources(mounts: &[Mount]) -> Vec<PathBuf> {`

Every host path a mount hands over.

## `fn sources(mounts: &[Mount]) -> Vec<PathBuf>` › `Mount::Volume { .. } | Mount::Tmpfs { .. } => None,`

Neither carries a host path, so neither can hand one over.

## `fn the_view_path_the_census_prunes_is_the_one_the_invocation_mounts() {`

-----------------------------------------------------------------------
1. Mounts, and the negative space
-----------------------------------------------------------------------

## `fn the_view_path_the_census_prunes_is_the_one_the_invocation_mounts() {`

The mount set is the role's one worktree, its view, its borrowed object
store and its credential volume — and **nothing that reaches the
coordinator**.

Both halves, because either alone passes on a wrong implementation:
a positive check ("the worktree is mounted") passes on a container that
also mounts `/`, and a negative check alone passes on a container that
mounts nothing at all. The withheld set is derived from [`RunPaths`] and
`workspace_manager::execution_root_of`, so it moves when the layout does.

Second field held constant: the role (`Implement`) and the agent
binding; what varies is which withheld path is offered.
The view path an invocation **mounts** is the view path a census
**prunes**, taken from the plan the runner actually builds.

`<R>/views/<container-name>` is a convention with a producer in one lane
and a consumer in another, and the six intent fields the packet fixes
carry no view path — so the census has to *derive* it. An independent
review measured what two copies cost: changing only the producer to
`<R>/views-v2/<name>` passed the entire suite while silently orphaning
every view the census would have pruned, which is R19 quietly ceasing to
balance after a crash. There is now one definition; this is what fails
if a second one appears.

The oracle is not either function: the expected value is the literal
`<R>` joined with `views` joined with the name, written here.

Second field held constant: one run identity and one private root; the
only thing that moves is the invocation, and with it the container name.

## `fn the_view_path_the_census_prunes_is_the_one_the_invocatio…` › `let expected = fixture.private_root.join("views").join(name.as_str());`

The literal convention, written out rather than called.

## `fn the_view_path_the_census_prunes_is_the_one_the_invocatio…` › `assert!(`

And the mount the container is actually given carries that path,
so this is the path on the machine and not only in a field.

## `fn the_mount_set_is_the_roles_own_and_reaches_nothing_of_th…` › `let mounts = plan.mounts();`

Positive: five mounts, each with its target and its disposition.

## `fn the_mount_set_is_the_roles_own_and_reaches_nothing_of_th…` › `assert_eq!(`

The scratch surface is a tmpfs and therefore carries **no host
source**: it is the one writable place that is neither the role's own
worktree nor anything of the coordinator's, which is what lets
`CreateSpec::read_only_root` close the container layer without making
`sh` unusable.

## `fn the_mount_set_is_the_roles_own_and_reaches_nothing_of_th…` › `let withheld = fixture.withheld();`

Negative: no mount source is a withheld path or an ancestor of one.

## `fn the_mount_set_is_the_roles_own_and_reaches_nothing_of_th…` › `let hostile = vec![Mount::Path {`

The control: the same check over a mount set that *does* reach the
coordinator finds every category. Without it a `violations` that
always returned an empty vector would pass the assertion above.

## `fn a_workspace_that_contains_a_withheld_path_is_refused_before_any_effect() {`

A workspace that contains a withheld path is refused, by name, before
anything is created.

This is the assertion a membership test cannot make. The repository root
contains the public log and authoritative Git; `/` contains everything.
Both are plausible values for `RunnerRequest.workspace` — the second is
what a path-joining mistake produces — and both are refused with the
paths named.

Second field held constant: the role, the agent and the image; what
varies is the workspace.

## `fn a_workspace_that_contains_a_withheld_path_is_refused_bef…` › `"the filesystem root",`

The **volume** root of the fixture's own tree, not a bare
`Component::RootDir`. `Path::starts_with` is component-wise,
and on Windows `C:\\x` begins with a `Prefix` component that
a bare `\\` does not have — so a bare root contains nothing
there and the refusal would not fire. Measured on the
Windows guest, where the first spelling of this row was the
slice's only guest failure.

## `fn a_workspace_that_contains_a_withheld_path_is_refused_bef…` › `assert!(`

And nothing was created on the way to any of those refusals: the
refusal is in `plan`, which performs no effect at all.

## `fn only_the_reviewer_receives_a_read_only_worktree() {`

Only the reviewer's worktree is read-only.

DESIGN.md:610: "a `:ro` mount makes the reviewer's read-only
**mechanically** perfect instead of flag-deep." A count, not a spot
check: exactly one of the five roles gets `:ro`, and the other four do
not — a runner that made every mount read-only would pass a test that
only looked at the reviewer.

Second field held constant: the workspace, the image and the agent
binding each role takes in production; what varies is the role.

## `fn only_the_reviewer_receives_a_read_only_worktree()` › `assert_eq!(`

The two probe roles receive no worktree at all — a probe has none.

## `fn the_credential_volume_is_mounted_exactly_when_its_location_is_supplied() {`

The credential volume is mounted **exactly** when its location is
supplied, and both follow one predicate.

The intersection that makes this worth writing: {role} × {volume
recorded}. A rule keyed only on the role mounts a volume the record does
not name; a rule keyed only on the record hands a gate an agent's
credentials. And the mount and the environment variable are asserted to
agree cell by cell — two rules that happen to agree today is the shape
this project keeps paying for.

## `fn the_credential_volume_is_mounted_exactly_when_its_locati…` › `let in_env = key.and_then(|key| {`

`filter(non-empty)`: since `PR6-CORRECTNESS-007` a location
the role is **not** given is named with nothing rather than
omitted, because `docker create --env` overlays the image's
environment and an omitted key is one the image decides. So
"supplied" is a value and not a presence.

## `fn the_credential_volume_is_mounted_exactly_when_its_locati…` › `assert_eq!(`

The one predicate, asserted rather than assumed.

## `fn the_credential_volume_is_mounted_exactly_when_its_locati…` › `let mut hostile_cells = 0_usize;`

The cell the production builders cannot reach: a role that takes no
credentials, carrying an agent whose volume the record **does** name.
Without it the role check in the mount plan is unmeasured, because
`agent.is_some()` already excludes every such role in production.

## `fn the_credential_volume_is_mounted_exactly_when_its_locati…` › `assert_eq!(`

Named with **nothing**, not omitted: an omitted key is one the
recorded image decides, and this role must be told it has no
location rather than left to inherit one
(`PR6-CORRECTNESS-007`).

## `fn every_container_is_created_from_the_recorded_id_even_after_the_reference_moves() {`

-----------------------------------------------------------------------
2. Creation from the recorded image id
-----------------------------------------------------------------------

## `fn every_container_is_created_from_the_recorded_id_even_after_the_reference_moves() {`

Every container is created from the **recorded id**, and a moved
reference does not change what executes.

The intersection: {image id recorded} × {reference moved}. A runner that
resolved the reference at each invocation passes every test that never
moves the tag, which is every test that does not build this cell.

## `fn every_container_is_created_from_the_recorded_id_even_aft…` › `fixture`

The reference now names another image, and the old id stays.

## `fn every_container_is_created_from_the_recorded_id_even_aft…` › `fixture.trace.clear();`

And it really runs from the recorded id. The trace is cleared first
so the fixture's own `image_by_reference` — which is how the moved
tag was verified above — cannot be mistaken for one the runner made.

## `fn a_substituted_reported_image_id_refuses_before_start_in_both_phases() {`

A reported image id that differs from the record never reaches
`Container.Start`, and **the two phases arrive at the caller as
different things**.

`expected_failures_refusals[3]` gives the mismatch two outcomes: "refused
before start (**pre-flight/rebuild**)" or "settled as a
**`RunnerSpawnFailure` outage** (mid-run)". The shipped code returned one
`UpstrokeError::Refused` for both, so the mid-run half was unreachable to
any caller — `PR6-CORRECTNESS-001`, whose surviving mutation was to
change the variant and keep the message, because the test checked only
`expect_err`, substrings, `Start`'s absence and cleanup.

So the grid is `{pre-flight probe, in-run worker, in-run integration
sequence} × {mismatch}` and the assertion is on the **variant**, not on
prose. What is common to every cell — never started, nothing left behind
— is asserted in every cell too, because a fix that distinguished the
phases by *starting* one of them would otherwise pass.

The settlement event itself is PR7's: `invariants_introduced` makes the
container transition "test-only until PR7 wires TopologyRun", and
`src/topology/**` is frozen. What this slice owes is the distinction,
and `ImageIdMismatch` is where PR7 reads it.

Second field held constant: the runtime is reachable throughout and the
same image id is substituted in every cell, so only the invocation's
phase moves.

## `fn a_substituted_reported_image_id_refuses_before_start_in_…` › `assert_eq!(`

The phase, as the error's own shape. A caller settles from this.

## `fn a_substituted_reported_image_id_refuses_before_start_in_…` › `assert!(`

Before start, and that is asserted as an absence rather than as
an error having come back.

## `fn a_substituted_reported_image_id_refuses_before_start_in_…` › `assert!(`

R26 and R19 balance: no container, no intent, no view.

## `fn the_image_mismatch_phase_is_read_from_the_invocation() {`

The phase is read from the invocation, over every form an invocation has.

The oracle is an independent table over `InvocationId`'s three variants —
derived from the type, not from `ImageIdMismatch::of` — and the
distinct-value count is asserted, so a classifier that collapsed to one
answer fails here whatever it collapsed to.

## `fn the_image_mismatch_phase_is_read_from_the_invocation()` › `let name =`

And the error the classification builds carries it in its variant
rather than in prose.

## `fn a_policy_that_is_not_a_container_policy_is_refused_at_construction() {`

A policy that is not a usable container policy is refused at
construction, before a runner exists to execute anything.

## `fn a_policy_that_is_not_a_container_policy_is_refused_at_co…` › `let runner = ContainerRunner::new(`

The control: the good one is accepted, and its digest is the record's.

## `fn the_shell_probe_runs_through_this_runner_as_a_registered_container_invocation() {`

-----------------------------------------------------------------------
3. Probes through the runner
-----------------------------------------------------------------------

## `fn the_shell_probe_runs_through_this_runner_as_a_registered_container_invocation() {`

The `RunnerPreflight` shell probe executes through **this** runner, as a
registered container invocation created from the recorded image id.

`decisions.sequential_substrate.runner`: "both implement the
RunnerPreflight shell probe (the recorded shell executing `exit 0`
through the Runner: on the host as an ordinary supervised process, **in
a container from the recorded image id**)". The probe is not
re-implemented here — `host::run_shell_probe` is a free function over
`&dyn Runner`, and this is the same call the host makes, with the runner
varied and everything else held fixed.

## `fn the_shell_probe_runs_through_this_runner_as_a_registered…` › `let rendered = fixture.trace.rendered();`

It was a container invocation, in the contract's order, from the
recorded id — and the intent that owns it was written first.

## `fn the_shell_probe_runs_through_this_runner_as_a_registered…` › `assert!(invocation.probe_target().is_some());`

The command really was the recorded shell executing `exit 0`, and the
probe carries a probe-role identity.

## `fn the_shell_probe_runs_through_this_runner_as_a_registered…` › `assert!(`

A registered invocation: the intent named it, and the record carried
the runner digest. The intent is gone now, so the evidence is the
container the runtime saw.

## `fn failing_preflight_probe_on_resume_refuses_before_recovery_event_and_reclaims_probe_con…`

T-CONTAINER (17), PR6 half: a failing pre-flight probe refuses and its
probe containers are reclaimed.

**What PR7 completes**, and this test does not claim: the *ordering*
against a recovery event ("refuses before any recovery event") and the
resume that produces one. `decisions.pr_sequence[8].scope` puts "rebuild
of the recorded Runner … with **RunnerPreflight before any recovery
event**" in PR7, and this slice's `permitted_transitions` says the
container transition is "test-only until PR7 wires TopologyRun". What is
held here is the half the mechanism owns: the probe spawn is the only
thing that observes the failure, the refusal names the shell, and the
probe's container, view and intent are all gone afterwards so the run
stays resumable.

Both probe kinds, because `expected_failures_refusals` names both — "a
recorded **shell or agent CLI** that fails inside the recorded image".
Second field held constant: the image id, which matches the record in
every cell, so what varies is only what the process did.

#### Both cells go through the thing that turns failure into refusal

`PR6-ENUM-007`. The agent cell called `runner.run` directly and asserted
a nonzero `ProcessOutput`, which is not what the clause says: the shell
cell reaches its refusal through `host::run_shell_probe`, and the agent
half's equivalent is [`AgentAdapter::probe`], which is where a nonzero
`--version` becomes a `UpstrokeError`. Deleting that refusal left this
named test green, because "the runner returned code 1" was all it
asked. It now drives `ClaudeCodeAdapter::probe` over the container
runner and asserts the **refusal**, so the cell holds
`proof_tests[3]`'s "an agent probe **fails** when the CLI is absent".

## `fn failing_preflight_probe_on_resume_refuses_before_recover…` › `if tag == "shell" {`

The observation is a spawn, not an inspection: `non_goals[2]` is
"non-spawn shell/CLI presence inspection", and the container was
created and started before anything knew the answer.

## `fn failing_preflight_probe_on_resume_refuses_before_recover…` › `let adapter: &dyn crate::agent::AgentAdapter = &crate::agent::claude::ClaudeCodeAdapter;`

Through the adapter, which is what turns a nonzero
`--version` into a refusal. `runner.run` alone returns
`Ok(ProcessOutput { code: Some(1) })` — a spawn that
succeeded — and a run that stopped there would settle a
resume as *certified* on a CLI that is not there.

## `fn failing_preflight_probe_on_resume_refuses_before_recover…` › `assert!(`

**The `--version` refusal specifically.** `probe` runs
`--version` and then `--help`, and the second has refusals of
its own; an `expect_err` alone is satisfied by either, so
deleting the nonzero-exit check on `--version` still left
this cell green (measured). The clause is "an agent probe
**fails when the CLI is absent**", and what observes that is
the first spawn's exit status.

## `fn failing_preflight_probe_on_resume_refuses_before_recover…` › `let request = crate::agent::probe_request(`

The control: the spawn itself succeeded, so the refusal is
the adapter's reading of the result and not a spawn failure
dressed up as one.

## `fn failing_preflight_probe_on_resume_refuses_before_recover…` › `assert!(`

The probe containers are reclaimed, and the run stays resumable:
no container, no intent, no view.

## `fn failing_preflight_probe_on_resume_refuses_before_recover…` › `assert!(fixture.paths.events().exists());`

And the run's own record is untouched by any of it.

## `fn two_incarnations_of_one_probe_are_two_container_invocations() {`

One probe identity, two incarnations, two container invocations.

The intersection {probe kind} × {epoch}. `InvocationId::probe` is
deterministic **by construction**, so the same probe of a resumed run
carries the same identity; without the incarnation in the name the
second epoch's intent would overwrite the first's and the census would
lose the evidence it needs. This is that property at the *runner* level:
two runners differing in nothing but the incarnation.

## `fn two_incarnations_of_one_probe_are_two_container_invocati…` › `assert_eq!(shell_probe_id().render(), shell_probe_id().render());`

The identity repeats across incarnations — which is why the name may
not — and the fixture proves that rather than assuming it.

## `fn probe_and_execution_compose_through_one_code_path() {`

-----------------------------------------------------------------------
4. Environment composition, and parity with the host
-----------------------------------------------------------------------

## `fn probe_and_execution_compose_through_one_code_path() {`

Probe and execution compose through **one** code path, and produce the
same environment.

DESIGN.md:263. "Two call sites that happen to agree today" is the shape
this sentence is most often satisfied by, so both halves are asserted:
a source census that there is one composition site and one plan site in
this module's production region, and a runtime comparison of the pair
the sentence names.

The one difference is stated rather than hidden: a probe receives no
worktree ([`receives_a_worktree`]), so its mount set is the execution's
minus the worktree, the view and the borrowed object store. Everything
that decides what the process *is* — the image id, the credential
volume, the reserved values, the overlay — is identical.

## `fn probe_and_execution_compose_through_one_code_path()` › `let source = std::fs::read_to_string(`

(a) the source census.

## `fn probe_and_execution_compose_through_one_code_path()` › `let fixture = Fixture::new("parity-probe", true);`

(b) the pair the sentence names, composed.

## `fn probe_and_execution_compose_through_one_code_path()` › `let composed = |plan: &InvocationPlan| -> BTreeMap<String, String> {`

The overlay differs only where the *request* differs, so the
composed environments are compared as sets of (key, value).

## `fn probe_and_execution_compose_through_one_code_path()` › `let probe_targets: BTreeSet<&str> = probed.mounts().iter().map(Mount::target).collect();`

Mounts: the probe's set is the execution's minus the worktree.

## `fn host_and_container_compose_the_same_environment_for_every_role() {`

`decisions.tests_acceptance.parity`: "host and container runners produce
identical … **environment composition**".

The runner is varied and **everything else is held fixed**: one explicit
base, one name rule, one overlay, and all five `ExecutionRole` values
including both probe targets — `ExecutionRole::all()` returns five for
exactly this reason. The base is explicit rather than each runner's own,
because the two bases are *supposed* to differ (the Upstroke environment
and the image environment) and a comparison of those would be a
comparison of two fixtures rather than of two composition rules.

The one place they legitimately differ is stated as an assertion rather
than skipped: a credential *location* is a path at the boundary that
executes, so the host names a host directory and the container names its
mount target. Both are supplied for exactly the same three roles.

## `fn host_and_container_compose_the_same_environment_for_ever…` › `let withheld: BTreeMap<String, String> = container_env`

Same keys, for every role — **plus** the locations the container
withholds explicitly.

This is the one structural asymmetry between the two boundaries
and it is stated rather than skipped (`PR6-CORRECTNESS-007`): the
host runner calls `env_clear()` and installs the composed vector
as the *whole* environment, so a key it omits is genuinely
absent; the container runner's vector is `docker create --env`,
which **overlays** the image's environment, so a key it omits is
a key the image decides. Naming what is withheld is how the
container reaches the same *effective* environment the host
reaches by omission — which is what parity is about.

## `fn host_and_container_compose_the_same_environment_for_ever…` › `let location = agent.as_ref().and_then(host::credential_location);`

Same values everywhere except the credential location.

## `fn host_and_container_compose_the_same_environment_for_ever…` › `for reserved in host::reserved_keys() {`

And both refuse the same overlay keys.

## `fn host_and_container_compose_the_same_environment_for_ever…` › `assert!(base.iter().any(|(key, _)| key == "CLAUDE_CONFIG_DIR"));`

The base really did carry a credential location, so "the reserved
copies are dropped" is a statement about this fixture.

## `fn a_completed_invocation_releases_in_the_contracts_order_and_reports_the_result() {`

-----------------------------------------------------------------------
5. Supervision, release, and the resource ledgers
-----------------------------------------------------------------------

## `fn a_completed_invocation_releases_in_the_contracts_order_and_reports_the_result() {`

A completed invocation stops, removes, unmounts the view and removes the
intent — in that order — and reports what the container did.

`side_effect_vs_event_ordering`: "stop/rm, view removal, intent removal
**after completion**". Asserted as a sequence of positions in one
ordered trace, not as membership: a release that performed the same four
operations in any other order would satisfy a set.

Second field held constant: the image id, which matches the record; what
varies is only that the container finished.

## `fn a_completed_invocation_releases_in_the_contracts_order_a…` › `let order = [`

The whole sequence, in one chain.

## `fn a_completed_invocation_releases_in_the_contracts_order_a…` › `assert!(`

The three clauses of `side_effect_vs_event_ordering`, each stated on
its own rather than only as a link in the chain above — a chain is
one assertion and the contract is three predicates.

## `fn a_completed_invocation_releases_in_the_contracts_order_a…` › `assert!(at("view:materialized") < at("rt:create"), "{rendered:#?}");`

And the view really is materialised before the create, which is the
physical constraint the module docs record.

## `fn a_completed_invocation_releases_in_the_contracts_order_a…` › `assert!(at("rt:collect") < at("rt:remove"), "{rendered:#?}");`

Collected **before** the release, because `docker logs` answers for a
running container and not for a removed one.

## `fn a_completed_invocation_releases_in_the_contracts_order_a…` › `assert!(fixture.runtime.fake().container_names().is_empty());`

R26 and R19 balance.

## `fn a_container_that_outlives_its_timeout_is_stopped_and_removed() {`

A container that outlives its timeout is stopped and removed, and the
output says so.

`slice_contract.cancellation`: "timeout or shutdown **stops and
removes** the container". The fixture's timeout is `Duration::ZERO`, so
the deadline has passed by the first observation and the supervisor
makes exactly one round trip — `determinism` forbids sleeps and a poll
loop with a real timeout would be one.

Second field held constant: everything except whether the container
terminates — the same image, the same role, the same workspace as the
completing case above.

## `fn a_container_that_outlives_its_timeout_is_stopped_and_rem…` › `let rendered = fixture.trace.rendered();`

Stopped and removed, in that order, and the ledgers balance.

## `fn a_container_that_outlives_its_timeout_is_stopped_and_rem…` › `assert_eq!(`

Exactly one observation: no sleeps, and the loop is bounded by the
deadline rather than by a count.

## `fn output_beyond_the_bound_is_truncated_and_reported_as_limited() {`

Output beyond the bound is truncated and reported as limited.

Without it `ProcessOutput::output_limited` would be `false` for every
container invocation, and `host::run_shell_probe`'s bounded-output
refusal — a real arm of that function — would be reachable at the host
boundary and unreachable at this one. A pre-flight that certifies less
than the one it is paired with is not the parity the packet asks for.

Second field held constant: the exit status, which is 0 in both cells,
so what varies is only how much the container printed.

## `fn output_beyond_the_bound_is_truncated_and_reported_as_lim…` › `let probe = host::run_shell_probe(`

And the probe refusal really is reachable at this boundary.

## `fn a_credential_volume_is_never_created_or_pruned_by_any_disposition() {`

R20: a credential volume is **never created or pruned by a run**, in
every disposition this runner can reach.

`resource_accounting.rows[R20]` is `operator_owned` and
`persistent_output` in **all five** `at_run_end` outcomes — `Complete`,
`Parked`, `Halted`, `BudgetExceeded`, `NoRunFinished`. A run-end outcome
is a fold over the event log and PR6 has no events at all
(`durable_events`: "none"), so what this slice can measure is the set of
dispositions the runner itself reaches, and the five outcomes differ
only in *which* of them ends the last invocation. Each is driven here
and the volume is asserted present afterwards.

The failure this prevents is one no ordinary test looks at: a runner
that tidied up a volume it mounted would destroy operator credentials,
and CLIs "rotate refresh tokens on use, and a discarded rotation forces
re-login" (DESIGN.md:612).

## `fn a_credential_volume_is_never_created_or_pruned_by_any_di…` › `type Disposition = (&'static str, fn(&Fixture));`

One way an invocation of this runner can end.

## `fn container_subtree_production_regions() -> (Vec<(String, String)>, BTreeSet<String>) {`

Every `.rs` file of the container subtree, with each file's **own**
production region.

#### Why this is a function and not three lines at a call site

`PR6-ACCT-002`, which is `PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN` for the
third time in this slice. The census below concatenated the sources and
called `production_region` **once**: that function cuts a source at its
**first** `#[cfg(test)]`, and `src/runner/container.rs` has one, so
everything appended after it — `runtime.rs`, `intent.rs`, `exec.rs`,
`env.rs`, `view.rs` — was cut away entirely. `census.rs` and `resolve.rs`
were not in the list at all. The census was reading one file and
reporting on seven, and its positive control happened to live before the
cut, so it stayed green while measuring almost nothing.

So: the directory is **enumerated**, not listed; each file is cut on its
own; and the caller is handed the per-file regions so it can assert the
domain did not shrink.
The two answers: the production regions in the domain, and the files
deliberately outside it.

Membership is derived from **where `container.rs` declares the module**,
which is the tree's own rule and is written at the top of that file:
"Keep every `#[cfg(test)]` declaration at the BOTTOM". `production_region`
cuts at the first `#[cfg(test)]`, so a `mod x;` above the cut is a
production module and one below it is test-only. Deriving it this way
rather than listing exclusions is what makes a *new* file a failure here
instead of a silent addition to either set.

A test-only file has no `#[cfg(test)]` of its own — its gate is at the
declaration site — so `production_region` of it is the **whole file**,
and including one would put every fixture's `docker volume create` into
a census of production vocabulary.

## `fn a_launch_that_fails_after_a_committed_effect_still_releases_everything() {`

Every exit of `launch` releases what it reached **even when the effect
was already committed**.

`PR6-ACCT-003`, the axis the fail-fast grid beside this one does not
carry. That grid makes a *primitive* fail — a runtime armed failing, a
view that refuses to materialise — so at every exit the effect never
happened. The funnel's other failure mode is the opposite one: it runs
the primitive and *then* consults the `After` phase
(`container::funnel`), which is what an `Injection::Error` at `After`
models and what a real `docker create` that succeeds and whose following
inspect fails does. The state at the exit is therefore strictly larger:
the record is published, the directory exists, the container exists.

The intersection is **{which site} × {effect committed}**, and the
committed column is the one that was empty. `Container.WriteIntent` is
in it because that exit was a bare `?` until this round: a `Before`
failure there has written nothing, so the exit looked harmless, and an
`After` failure leaves a durable R26 record with no container and no
view.

In every cell: R26's container and record and R19's view are all gone,
and R20's volume is untouched.

Second field held constant: the request, the role and the recorded image
id are identical in all eight cells; only the armed site and phase move.

## `fn a_launch_that_fails_after_a_committed_effect_still_relea…` › `if phase == HookPhase::After {`

The premise of the committed column: at `After` the
primitive really did run, so there was something to release.

## `fn a_launch_that_fails_after_a_committed_effect_still_relea…` › `assert!(`

R26 and R19 balance, whichever cell this is.

## `fn a_launch_that_fails_after_a_committed_effect_still_relea…` › `for (_, volume) in VOLUMES {`

R20 is untouched in every cell.

## `fn every_at_run_end_outcome_is_driven_through_its_mechanism_and_the_ledgers_balance() {`

The five `at_run_end` outcomes are **driven**, each through the
mechanism its row names, and every physical resource is checked
afterwards.

`PR6-ACCT-008`. The tables were transcribed and counted — five constant
strings, four `"released"` values, one seeded census — and a table copied
into a test is a table. There was no R19 outcome table at all, and the
R20 disposition grid asserted "one per outcome" by checking that a vector
had five elements.

#### What this slice can and cannot drive

A run-end outcome is a fold over the event log and PR6 has
`durable_events: "none"`, so `Complete`, `Parked`, `Halted` and
`BudgetExceeded` are not states this slice can enter. What each of them
*names* is a **mechanism**, and R26's lifecycle sentence enumerates
them: "released on complete (stop/rm, view removed, intent removed),
**cancel**, or **shutdown**", with `NoRunFinished` "reclaimed … at the
next write-command start". Every mechanism in that sentence is
reachable here, so the table below maps each outcome to one and the
mapping is asserted **total** — a sixth outcome, or an outcome with no
mechanism, fails here rather than being counted.

#### Per resource, not per site

INV-22 is "every physical or logical owned resource has, for every
lifecycle state, exactly one accounting class and exactly one
non-overlapping inventory row". The site-mapping test proves one row per
*effect site*, which is a different statement: it cannot see the staged
intent record, the implicitly created named volume, or a standalone
view. So the ledger here is over the **resources** a container
invocation owns, each named with its row, and each observed after every
mechanism.

## `fn every_at_run_end_outcome_is_driven_through_its_mechanism…` › `enum Mechanism {`

The mechanism a run-end outcome disposes of R19/R26 through.

## `enum Mechanism` › `Complete,`

"released on **complete** (stop/rm, view removed, intent
removed)".

## `enum Mechanism` › `Cancel,`

"…, **cancel**, …" — the invocation was stopped before it
finished on its own. `slice_contract.cancellation`: "timeout or
shutdown stops and removes the container".

## `enum Mechanism` › `Shutdown,`

"…, or **shutdown**" — the launch itself was refused and
everything it reached was released.

## `enum Mechanism` › `Census,`

R26's fifth cell: "reclaimed when the owner or its incarnation
is dead", at the next write-command start.

## `fn every_at_run_end_outcome_is_driven_through_its_mechanism…` › `const OUTCOMES: &[(&str, &str, &str, &str, Mechanism)] = &[`

`decisions.resource_accounting.rows[{R19,R20,R26}].at_run_end`,
transcribed, with the mechanism this slice drives each through.

## `fn every_at_run_end_outcome_is_driven_through_its_mechanism…` › `let mechanisms: BTreeSet<Mechanism> =`

Total: every outcome has a mechanism, and every mechanism the
lifecycle sentence names is used by an outcome.

## `fn every_at_run_end_outcome_is_driven_through_its_mechanism…` › `assert!(`

R20's premise: the operator's volume is there before anything
runs. `persistent_output` is a claim about a resource that
exists.

## `fn every_at_run_end_outcome_is_driven_through_its_mechanism…` › `let fixture = Fixture::new(&format!("outcome-{outcome}-live"), false);`

The cancellation clause: "timeout or shutdown stops and
removes the container". The container is left **running**
by the fake, so the supervisor really does reach its
deadline with something to stop — a fixture whose
container exits on start would take the ordinary
completion path and be a fifth copy of the cell above.

## `fn every_at_run_end_outcome_is_driven_through_its_mechanism…` › `fixture`

The launch is refused mid-sequence and releases what it
reached. R26's "shutdown" is the run stopping before an
invocation could finish, which is the same disposal path.

## `fn every_at_run_end_outcome_is_driven_through_its_mechanism…` › `let mut hooks = RecordingHooks::new(fixture.trace.clone());`

Seeded rather than run: the owner is a *dead* incarnation,
which by construction is not this process.

## `fn every_at_run_end_outcome_is_driven_through_its_mechanism…` › `assert!(`

The per-resource ledger, for the two mechanisms that fall
through: R26's container, R26's record (both halves), R19's
directory, R20's volume.

## `fn every_physical_resource_of_a_container_invocation_maps_to_exactly_one_row() {`

Every physical resource a container invocation owns has exactly one row.

`PR6-ACCT-008`'s second half, and INV-22's own sentence: "exactly one
accounting class and exactly one **non-overlapping** inventory row per
`decisions.resource_accounting`". The site-mapping census proves one row
per *effect site*; a site is not a resource, and the resources with no
site of their own are exactly the ones nothing was checking — the staged
intent record, the anonymous volume a `VOLUME` declaration creates, and
the named volume `docker create` creates implicitly.

The table is the resources, not the sites, and each row names where in
this tree that resource is disposed of. Everything here is asserted
against the packet's row text rather than against the code.

## `fn every_physical_resource_of_a_container_invocation_maps_t…` › `const RESOURCES: &[(&str, &str, &str, &str)] = &[`

`(resource, row, class while it exists, what disposes of it)`.

## `fn every_physical_resource_of_a_container_invocation_maps_t…` › `let rows: BTreeSet<&str> = RESOURCES.iter().map(|(_, row, ..)| *row).collect();`

(1) One row per resource, and the rows are the three this slice owns.

## `fn every_physical_resource_of_a_container_invocation_maps_t…` › `let mut by_row: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();`

(2) One class per row, over every resource in it: a row whose
resources disagreed about their class would be two rows.

## `fn every_physical_resource_of_a_container_invocation_maps_t…` › `for (_, _, class, _) in RESOURCES {`

Every class named is one `decisions.resource_accounting.classes`
declares.

## `fn every_physical_resource_of_a_container_invocation_maps_t…` › `let undisposed: Vec<&str> = RESOURCES`

(3) The R20 row is the only one with no disposer, and that is the
whole of `enforcement_domains.operator_owned`.

## `fn every_physical_resource_of_a_container_invocation_maps_t…` › `let site_names: BTreeSet<&str> = ContainerSite::ALL.iter().map(|site| site.name()).collec…`

(4) Every disposer that names a site names one of the frozen eight,
so a resource cannot be disposed of by something outside the funnel.

## `fn the_container_subtree_can_only_inspect_a_volume() {`

Nothing in the container subtree can create or prune a volume.

The runtime assertion above measures the dispositions a test drove; this
measures the *domain* — `enforcement_domains.operator_owned`: "R20
credential volumes: **never created or pruned by a run**". The seam has
one volume method and it returns a `bool`, and the `docker` CLI issues
exactly one volume subcommand, which is `inspect`.

**The domain has a control** (`PR6-ACCT-002`): every file of the subtree
is enumerated from the directory rather than listed by hand, every one
contributes a non-trivial production region, and the *set* of files is
asserted to contain the seven this slice wrote. A future `#[cfg(test)]`
hoisted to the top of any of them, or a new module added beside them,
fails here rather than silently emptying the census.

## `fn the_container_subtree_can_only_inspect_a_volume()` › `let files: BTreeSet<&str> = regions.iter().map(|(name, _)| name.as_str()).collect();`

-- the control on the domain itself --------------------------------

## `fn the_container_subtree_can_only_inspect_a_volume()` › `assert_eq!(`

The positive control: the read-only inspection really is there, so a
census that had stopped finding anything fails here rather than
reporting silence. It lives in `container.rs` **below** the old cut
point's neighbours, so it is also evidence the domain is whole.

## `fn the_container_subtree_can_only_inspect_a_volume()` › `let funnel = &regions`

-- and the vocabulary census is not the whole claim -----------------
`docker create` creates an absent named volume **implicitly**, with
no `volume create` anywhere (measured, docker 29.7.2). No search over
this subtree's text can see that, so the domain census is paired with
the guard that can: every `Mount::Volume` is re-inspected before the
create, in `container::create_container`.
`a_create_whose_named_volume_is_absent_is_refused_before_any_effect`
drives it; this asserts the guard is *in the production region*, so
it cannot be deleted while the vocabulary census stays green.

## `fn the_container_subtree_can_only_inspect_a_volume()` › `let seam = &regions`

And the seam has one volume method.

## `fn the_container_runner_is_object_safe_and_send_and_sync() {`

The container runner is a `Runner` like any other: object-safe, `Send`
and `Sync`.

PR11 turns `run` into a boxed `Send` future behind the same `&dyn
Runner` its callers hold, so a container runner that stopped being
object-safe would fail to compile here rather than at the migration —
the same guard `runner::tests::the_runner_trait_is_object_safe` gives
the host.

## `fn the_container_runner_is_object_safe_and_send_and_sync()` › `let view: Box<dyn GitView> = Box::new(RoleGitView::new(ContainerTrace::off()));`

And a `GitView` is object-safe too, which is what lets the funnel
take `&dyn GitView` and this module hand it a projection.

## `type SeenIntents = Arc<Mutex<Vec<(String, Option<Vec<u8>>)>>>;`

-----------------------------------------------------------------------
5b. Repair R1: confinement, the intent capability, and cwd-independence
-----------------------------------------------------------------------

## `type SeenIntents = Arc<Mutex<Vec<(String, Option<Vec<u8>>)>>>;`

One row per invocation: the container's name, and the bytes of its
intent record — `None` when there was none, which is the state
catalogue survivor `PR6-INTENT-020` describes.

## `struct IntentPeek {`

A `GitView` that reads the container's intent record at the moment the
view is materialised.

`Container.MountGitView` runs **after** `Container.WriteIntent` and
**before** `Container.Create`, so this observes `<R>/containers` at
exactly the point the contract says the record must already be there:
"intent synced before docker create". The record is removed again when
the invocation completes, which is why a test that looked afterwards
could only ever see an absence.

## `fn materialize(&self, request: &GitViewRequest) -> Result<P…` › `let name = request`

The view directory is `<R>/views/<container-name>`, so its file
name is the container's.

## `struct FailingView;`

A `GitView` whose materialisation fails, so `Container.MountGitView` can
be the step a launch dies at.

## `fn the_public_constructor_withholds_every_category_from_every_role() {`

The **public constructor** withholds every category from every role that
receives a worktree, with no builder call at all.

`PR6-CORRECTNESS-011` / `PR6-ENUM-002`. `ContainerRunner::new` used to
default to `Confinement::none()` with the real set added by an optional
`with_confinement`, so the runner a caller gets by construction
withheld nothing: `PR6-ENUM-002`'s mutation is literally "omit
`with_confinement` at a construction site and submit
`identity.private_root` as a worker workspace", and `-011`'s is "apply
configured confinement only to `ExecutionRole::Implement`". This runner
is built with **`new` and nothing else**, and the grid crosses all four
withheld categories with all five roles, so a rule that held for one
role fails here.

The two probe roles are the other half of the grid rather than an
omission: a probe receives no worktree at all
([`receives_a_worktree`]), so a hostile workspace cannot reach it — and
that is asserted as "no mount source is under the hostile path" rather
than assumed.

## `fn the_public_constructor_withholds_every_category_from_eve…` › `let hostile: Vec<(Withheld, PathBuf)> = vec![`

One hostile workspace per category, each written from the type that
owns that path rather than read back out of `Confinement`.

## `fn the_public_constructor_withholds_every_category_from_eve…` › `let mut planned = 0_usize;`

The control: the role's own worktree plans on the same runner, so the
refusals above are about the path and not about `plan` being broken.

## `fn the_public_constructor_withholds_every_category_from_eve…` › `assert!(`

And nothing was created on the way to any of it.

## `fn the_execution_root_and_its_worktree_namespaces_are_withheld_and_one_worktree_is_not() {`

The execution root **and each of its three worktree namespaces** are
withheld, and any one worktree is not.

`PR6-CORRECTNESS-013`. `Confinement::of_run` named no worktree path at
all — the fixtures added siblings by hand, so the production helper was
unmeasured — and a Gate handed the run's execution root received every
task and merge worktree in one mount. Withholding only the root would
leave `<root>/tasks`, which still holds two, so the namespaces are
withheld too.

The expected paths are built from
[`crate::workspace_manager::execution_root_of`] and the packet's own
three namespace names, **not** read back from `Confinement::entries` —
that would be the function's own oracle.

Second field held constant: the run identity and the repository; what
varies is only which directory of the worktree namespace is offered.

## `fn the_execution_root_and_its_worktree_namespaces_are_withh…` › `let namespaces: Vec<PathBuf> = vec![`

`decisions.workspace_candidates.manager`: "tasks/k<key>-g<gen>,
merge/s<seq>", plus `snapshots`.

## `fn the_execution_root_and_its_worktree_namespaces_are_withh…` › `let confinement = Confinement::of_run(&fixture.identity, &fixture.repo);`

(a) `of_run` names each of them, under the sibling-worktree category.

## `fn the_execution_root_and_its_worktree_namespaces_are_withh…` › `let mut refused = 0_usize;`

(b) and a role handed one is refused, naming the category.

## `fn the_execution_root_and_its_worktree_namespaces_are_withh…` › `let mut planned = 0_usize;`

(c) The over-refusal control, which is what this fix could most
easily get wrong: withholding the namespace must not withhold the
worktrees inside it. Each of the run's three real worktrees plans,
for each worktree role — a container receives *one* worktree, and
which one is the engine's decision, not this runner's.

## `fn every_role_writes_and_syncs_its_own_six_field_intent_before_its_container_is_created()…`

Every role — **both probe kinds included** — writes and syncs its own
six-field intent record, and it is on disk before its container is
created.

Catalogue survivor `PR6-INTENT-020`: an agent probe container created
with no intent record passed, because the suite exercised both probe
kinds and never asserted that each writes its own record.
`T-CONTAINER.boundary` is "the RunnerPreflight shell and agent probe
containers are container invocations **like every other**", so the grid
is all five roles rather than the two the finding names.

The record is read at `Container.MountGitView`, which is between the
write and the create: after the invocation completes the record is gone,
so a test that looked afterwards could only ever see an absence. The six
field values are literals from this module's constants and from the
request's own `InvocationId` — never from `plan.launch.intent`, which is
the runner's own answer.

Second field held constant: the run, the incarnation and the image;
what varies is the role, and with it the invocation.

## `fn every_role_writes_and_syncs_its_own_six_field_intent_bef…` › `assert_eq!(`

R1 wrote this against the backslash-rewrite encoding that repair
R2 replaced with a percent-encoding. Decode through the accessor
the census itself uses, which asserts the round-trip rather than
one side of it -- strictly stronger than comparing the stored
bytes to a hand-written transform.

## `fn every_role_writes_and_syncs_its_own_six_field_intent_bef…` › `let rendered = fixture.trace.rendered();`

**Synced**, and before the create. The durability trio is counted
across the five invocations and each record's own two entries are
ordered against that container's own `create`.

## `fn a_container_is_created_and_started_only_under_its_own_intent_record() {`

A container cannot be created or started except under **its own**
published intent record.

`PR6-CORRECTNESS-012` / `PR6-ENUM-001`.
`expected_failures_refusals[6]` is "container start without an intent is
**impossible by construction**", and it was impossible only by nobody
having written the bypass: `create_container` and `start_container` were
public, took a bare `ContainerName`, and a
`ContainerRunner::start_existing(name)` added tomorrow would have
compiled. They now take an `IntentWritten`, and this pins the two things
the type system cannot say on its own:

1. the proof cannot be minted for a record that is not there, or that is
   not a `ContainerIntent`;
2. a proof for **another** container is refused, before any effect — so
   "an intent was written" cannot stand in for "this container's intent
   was written".

The third leg is a compile error and has no test: `start_container` has
no parameter that names a container other than the proof.

## `fn a_container_is_created_and_started_only_under_its_own_in…` › `let fixture = Fixture::new("intent-capability", false);`

`exit_on_start: false`, so a started container stays `Running` and the
control below observes the start rather than the decorator's own
exit.

## `fn a_container_is_created_and_started_only_under_its_own_in…` › `let refusal = crate::runner::container::intent::IntentWritten::certify(&root, &mine)`

(1a) Absent: the proof cannot be minted at all.

## `fn a_container_is_created_and_started_only_under_its_own_in…` › `std::fs::create_dir_all(crate::runner::container::intent::containers_dir(&root))`

(1b) Present and not a record: still no proof. "The record could not
be parsed" and "the record is gone" are different answers and only
one of them is an absence.

## `fn a_container_is_created_and_started_only_under_its_own_in…` › `let mut hooks = RecordingHooks::new(fixture.trace.clone());`

The control: a real record certifies, so (1a) and (1b) are about the
record and not about `certify` never succeeding.

## `fn a_container_is_created_and_started_only_under_its_own_in…` › `fixture.trace.clear();`

(2) A proof for another container is refused, before any effect.

## `fn a_container_is_created_and_started_only_under_its_own_in…` › `crate::runner::container::create_container(`

The control: the same call with the matching proof creates, so the
refusal above is about the name and not about the spec.

## `fn a_launch_that_fails_at_any_step_releases_everything_it_reached() {`

A launch that fails at **any** step releases everything it reached, and
answers with the original cause.

`PR6-CORRECTNESS-003` / `PR6-ENUM-003`. There are four ways out of
`ContainerRunner::launch` before a `Launched` value exists, and until
repair R1 three of them returned through `?` with nothing to release —
so `Runner::run`'s own release never ran and the container, the view and
the intent all survived. The fourth, the reported-image-id mismatch,
released **fail-fast**: a failing `Container.Stop` skipped the rm, the
view and the intent *and masked the integrity refusal*.

The grid is {failure point} × {cleanup healthy, `Container.Stop`
failing} — the intersection, because a cleanup that runs and a cleanup
that runs *after an earlier step failed* are different claims. In every
cell: the error names the original cause, and R19/R26 balance.

Second field held constant: the request, the role and the recorded image
id, which match in every cell except the one whose subject is a
mismatch.

## `fn a_launch_that_fails_at_any_step_releases_everything_it_r…` › `assert!(`

(1) The cause, never the cleanup's own failure.

## `fn a_launch_that_fails_at_any_step_releases_everything_it_r…` › `assert!(`

(2) R26 and R19 balance: nothing survives, whichever step
failed. A failing `Stop` does not stop the rm — `docker rm
--force` removes a running container — so the ledgers still
balance and the residue clause is the honest record of which
step could not be taken.

## `fn a_launch_that_fails_at_any_step_releases_everything_it_r…` › `if *point != Where::Start {`

(3) The container was never started, except in the cell whose
subject is a failing start.

## `fn a_launch_that_fails_at_any_step_releases_everything_it_r…` › `let attempts_stop = matches!(point, Where::Create | Where::Mismatch | Where::Start);`

(4) A failing `Stop` is *named* rather than swallowed, at
every exit where the cancel attempts one.

That is every exit **past** the create call, including the
one where the create itself failed (`PR6-ACCT-003`): the
funnel runs its primitive before consulting the `After`
phase, and `DockerCli::create` reads the reported image id
back afterwards, so a `Container.Create` that returns `Err`
may have left a container. The cancel cannot tell, so it
attempts the stop and the removal — both tolerant of
already-gone against a real daemon — and reports whatever the
runtime answered. This fake is armed to fail *every* stop,
including one against a container that was never created,
which is why the Create cell now names a stop failure too.

## `fn a_launch_that_fails_at_any_step_releases_everything_it_r…` › `let sites: Vec<String> = fixture`

And every later step was still attempted: the remove, the
view and the intent all have their sites in the trace
after the stop that failed.

## `fn the_working_directory_is_the_runners_own_for_every_role() {`

The working directory is the **runner's**, for every role, and never the
image's.

`PR6-CORRECTNESS-006`, the half `CreateSpec` can carry. A probe's
`workdir` was `None`, which hands the working directory to the image's
`WORKDIR` — so what a probe certified depended on a value the runner had
not read, and the finding's mutation (`None` -> `Some("/")`) changed
nothing any test could see. Both values are pinned here, so the mutation
dies in either direction.

## `fn the_working_directory_is_the_runners_own_for_every_role()` › `for request in requests(&fixture.task_a) {`

Both values are declared mounts, so no role runs in a directory the
runner did not give it.

## `fn a_cwd_relative_or_absent_path_refuses_every_role_before_any_effect() {`

A `PATH` that resolves against the working directory is refused, for
every role, before any effect.

`PR6-CORRECTNESS-006`, the half that matters. A probe has no worktree and
an attempt has one, so their working directories differ **by design**;
with a relative `PATH` component the repository's own worktree is on the
executable search path and repository content decides which `claude` the
attempt runs while pre-flight certified another. Both refusals are here
— the relative component, and the empty base that was the production
default and supplied no `PATH` at all.

Second field held constant: the workspace, the image and the run; what
varies is the base and the role.

## `fn a_cwd_relative_or_absent_path_refuses_every_role_before_…` › `assert!(`

Nothing was created on the way to any of them: the refusal is in
`plan`, which performs no effect.

## `fn a_cwd_relative_or_absent_path_refuses_every_role_before_…` › `let runner = fixture.runner();`

The control: the same runner over an absolute-only base plans and
supplies that value to every role, probe and attempt alike — which is
the property the refusals exist to protect.

## `fn every_role_gets_a_read_only_root_and_one_ephemeral_scratch_mount() {`

Every role's container gets a **read-only root** and exactly one
ephemeral scratch mount, and no other writable surface without a host
source.

`PR6-CORRECTNESS-008` / `PR6-ENUM-005`, the half a machine with no
container runtime can assert — which is the Windows guest, and which is
why it is here as well as in the gated test that runs the write.

## `fn every_role_gets_a_read_only_root_and_one_ephemeral_scrat…` › `for mount in plan.mounts() {`

Every other mount has a source the coordinator can name — a host
path or an operator-owned volume — so the mount list is the whole
of what the container may write and none of it is anonymous.

## `const PREFERRED_IMAGES: &[&str] = &[`

-----------------------------------------------------------------------
6. Docker-gated: what the fake cannot prove
-----------------------------------------------------------------------

## `const PREFERRED_IMAGES: &[&str] = &[`

The references the gated tests prefer, in order.

**These tests never pull.** `non_goals[1]` is "implicit image pull", and
a fixture that pulled would exercise the behaviour the slice forbids on
the very runtime the refusal is meant to be proven against. So the image
is *discovered* among what the machine already holds. `upstroke-test/git:v1`
is first because it is the only local image carrying both a shell and
`git`, and because its `UPSTROKE_IMAGE_MARKER` is how "the container runner
starts from the **image** environment" is measured rather than asserted.

## `const GIT_IMAGES: &[&str] = &["upstroke-test/git:v1"];`

Images that carry `git`. A subset, named separately because the
Git-view proof needs one and the others do not.

**One entry, and `alpine/git` is deliberately not the second.** That
image declares `VOLUME /git`, so every container created from it leaves
an anonymous volume behind that `docker rm --force` does not remove —
measured here, 29 of them from one run of this suite, which is
`PR6A-ANONYMOUS-VOLUMES-LEAK`. A fallback that breaks
`DOCKER-SUBSTRATE.md`'s "leave the daemon as you found it" on somebody
else's machine is worse than a loud, counted absence.

## `const MARKER_IMAGE: &str = "upstroke-test/git:v1";`

The image whose environment carries a marker this suite can recognise.

## `const CREDENTIAL_ENV_IMAGE: &str = "upstroke-test/credenv:v1";`

The image whose **own environment** sets credential-location variables.

`PR6-CORRECTNESS-007` cannot be measured against an image that sets
none: the defect is that `docker create --env` overlays the image
environment, so a key the runner omits is a key the image supplies.
`DOCKER-SUBSTRATE.md` records how it is built, from a base the machine
already holds and with no network.

## `const CREDENTIAL_ENV_CONTROL: (&str, &str) = ("GH_CONFIG_DIR", "/image/gh");`

The image variable that is **not** a credential location, so "the
withheld keys were overridden" is distinguishable from "the image
environment was wiped".

## `fn skipped(reason: &str) {`

What a Docker-gated test does when there is no runtime.

It **reads** the reason rather than returning silently, so a skip that
stopped saying why would not compile.

## `fn no_image(reason: &str) {`

What a Docker-gated test does when the runtime holds no usable image.

Loud under the same variable as a missing runtime: a machine with Docker
and no image would otherwise pass these tests without touching it.

## `fn discover(docker: &dyn ContainerRuntime, preferred: &[&str]) -> Result<(String, String)…`

A reference the runtime holds, with its id, or the reason there is none.

## `fn real_policy(image_id: &str) -> RunnerPolicy {`

A container policy naming a real image id, and no credential volumes.

R20 volumes are **operator-owned** and `persistent_output`; a test that
created one would be creating operator state on the machine it runs on,
which is the very thing the row forbids a run from doing. So the gated
suite records none, and `a_credential_volume_is_never_created_or_pruned_by_any_disposition`
carries the volume obligation against the fake.

## `fn image_environment_of(`

The **reserved** part of the recorded image's own environment, read from
the daemon.

DESIGN.md:259-260: "the container runner [starts] from the image
environment; each supplies role-scoped `HOME`, `PATH`, and credential
locations". A gated fixture is the one place in this suite that can
honour the first clause literally, because it has a real image to read;
the fake fixtures state an equivalent base as a literal. Either way the
runner is given a base carrying an absolute-only `PATH`, which
`ContainerEnvironment::certify_path` now requires.

**Filtered to the reserved keys**, and that is the point rather than an
economy: `docker create --env` *overlays* the image environment, so a
variable the runner does not name still reaches the child. Passing the
whole image environment back would restate every one of those in the
runner's own `--env` list and make
"overlays a runner-owned base rather than replacing it" unmeasurable —
the marker assertion below would be comparing the fixture with itself.

**Not a self-oracle.** This reads the *image's* declared environment out
of the daemon; the assertions are about what a process *inside a
container* sees and resolves, which a different mechanism decides.

## `fn real_identity(root: &Path, repo: &Path, run_id: &str) -> RunIdentity {`

A `RunIdentity` for a gated test, under a scratch private root.

`run_id` is a **parameter** because the container name is
`upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>` and carries
no private root: two gated tests sharing a run id and an invocation
ordinal produce the same container name, and `cargo test` runs them
concurrently. Measured: the first version of this suite failed with
`Conflict. The container name ... is already in use`. In production the
run id is a ULID and the collision cannot arise; in a fixture it is
whatever the fixture writes, which is why it is written per test.

## `const GATED_RUNS: &[(&str, &str)] = &[`

One run id per gated test. Distinct by construction, and asserted so.

## `("outside", "01KZR1GATED000000000000001"),`

Repair R1. The ids carry the round rather than continuing the
`…GATED<letter>` sequence, and that is not cosmetic: a container name
is `upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>` and
carries no worktree, so two *trees* whose gated suites pick the same
run id and invocation ordinal fight over one container name on a
shared daemon. Measured: repair round R3 added a gated test with
`01KZGATEDG000000000000000G` at the same time this one did, and the
two collided with `Conflict. The container name … is already in use`.

## `("credenv", "01KZR3BGATED00000000000001"),`

Repair R3b, with its own round prefix for the reason above.

## `struct LeaveNoResidue {`

Leave the daemon as we found it, even if an assertion panics.

`DOCKER-SUBSTRATE.md`'s first rule. `Drop` rather than a line at the end
of each test, because the line at the end of a test does not run when
the test fails — which is exactly when a container is most likely to be
left behind.

## `fn real_docker_runs_from_the_recorded_image_id_and_composes_over_the_image_environment() {`

The container runner executes the recorded image id, composes **over**
the image environment, and runs in the role's worktree.

Three separately droppable claims against the real runtime:

* the runner **supplies** `PATH` — DESIGN.md:260, "each supplies
  role-scoped `HOME`, `PATH`, and credential locations" — and the value
  the child sees is the one the runner named. A key the runner did *not*
  name and the image did (`UPSTROKE_IMAGE_MARKER`) reaches the child
  anyway, which is what "overlays a runner-owned base rather than
  replacing it" means and is the half a `PATH` assertion alone cannot
  make.
* the adapter's overlay key lands.
* the working directory is the role's worktree mount.

**The first claim used to be its opposite** — "the composed
`CreateSpec.env` does not name `PATH`" — and repair R1 inverted it
deliberately, not to make anything pass. `ContainerEnvironment::inherited`
composed an empty base, so the *image's* `PATH` decided which binary a
bare program name resolved to; with a relative component in it, a probe
(no worktree) and the attempt it certifies (a worktree) resolve
different binaries. `PR6-CORRECTNESS-006`. The old assertion was true
and was a statement that the runner supplied nothing.

Second field held constant: the role and the workspace; what varies
across the three claims is which part of the environment is read.

## `fn real_docker_runs_from_the_recorded_image_id_and_composes…` › `let plan = runner.plan(&request).expect("plans");`

The runner supplies `PATH`, and every component of it is absolute.

## `fn real_docker_runs_from_the_recorded_image_id_and_composes…` › `assert!(`

And it did not *replace* the image environment: the marker key is one
the runner never names, and it is read back from inside the container
below. Without this the `PATH` assertion above would be consistent
with a runner that had thrown the image environment away.

## `fn real_docker_runs_from_the_recorded_image_id_and_composes…` › `assert_eq!(`

R26 and R19 balance against the real daemon.

## `fn real_docker_refuses_a_reviewer_write_to_its_read_only_mount() {`

`expected_failures_refusals`: "**reviewer write attempt fails**".

DESIGN.md:610: "a `:ro` mount makes the reviewer's read-only
*mechanically* perfect instead of flag-deep". The control is the same
command in the `Implement` role over the same workspace: it writes, and
the file appears on the host. Without it, a test in which nothing could
write would pass.

Second field held constant: the command, the image and the workspace;
what varies is the role.

## `fn real_docker_refuses_a_reviewer_write_to_its_read_only_mo…` › `let spec =`

The redirection is captured **inside** the container, because
`DockerCli::collect` returns only what `docker logs` wrote to its
own stdout and discards the container's stderr entirely
(`PR6A-DOCKERCLI-MERGES-STDERR-INTO-STDOUT`). Measured on docker
29.7.2: `docker logs` really does separate the two streams, so
that is a repairable defect in the CLI adapter rather than a
property of the runtime — and this test does not depend on
either way.

## `fn real_docker_refuses_a_reviewer_write_to_its_read_only_mo…` › `outcomes.push((`

Both streams, because `DockerCli::collect` merges the container's
stderr into its stdout — `docker logs` interleaves them on a
container without a TTY. Recorded as
`PR6A-DOCKERCLI-MERGES-STDERR-INTO-STDOUT`; the assertion is
written so it holds either way rather than pinning the residual.

## `fn real_docker_confines_a_gate_to_its_mount() {`

`expected_failures_refusals`: "**gate write outside mount fails**", and
DESIGN.md:400's whole sentence.

Repository-controlled gate code — "which no agent permission surface can
ever bound" (DESIGN.md:610) — is given every withheld path by absolute
name and asked to read it and to write it. The assertions are on the
**host**, because that is what the claim is about: a container is free
to create whatever it likes inside its own writable layer, and none of
it may reach the coordinator.

The control is in the same command: the gate reads its own workspace,
which it *can* see. A test in which the container could read nothing at
all would pass without the confinement doing anything.

## `fn real_docker_confines_a_gate_to_its_mount()` › `assert!(`

The control: the gate can read its own worktree.

## `fn real_docker_confines_a_gate_to_its_mount()` › `for marker in [`

And it saw none of the withheld content.

## `fn real_docker_confines_a_gate_to_its_mount()` › `for ((category, path), original) in withheld.iter().zip(&before) {`

The host is byte-identical: whatever the container wrote stayed in
the container.

## `fn real_docker_confines_a_gate_to_its_mount()` › `assert_eq!(repo::git_ok(&repo_dir, &["rev-parse", "HEAD"]), head);`

And the coordinator's Git is unmoved.

## `fn real_docker_a_gate_write_outside_every_declared_mount_fails() {`

`expected_failures_refusals[5]`: "**gate write outside mount fails**" —
the refusal itself, observed inside the container.

`PR6-CORRECTNESS-008` / `PR6-ENUM-005`, and the distinction that produced
them. `real_docker_confines_a_gate_to_its_mount` proves **"the host is
unharmed"**: it explicitly permits container-layer writes and asserts on
host bytes. That is true, and it is weaker than the contract's sentence —
with no read-only root filesystem the gate's
`printf owned >/outside-role-mount` exited **0**, and a test can prove a
true, weaker statement indefinitely while the stated guarantee does not
hold. So this test asserts the **write fails**, from the container's own
report, and never looks at the host at all.

The grid is {a path outside every declared mount} × {a declared writable
mount}, and the second column is not decoration: a container in which
*nothing* could be written would satisfy the first column while being
unusable, so the two controls — the role's own worktree and the declared
scratch surface — are what say the confinement is a boundary rather than
a brick.

The hostile paths are chosen to cover the three shapes a write can take:
the root of the container filesystem, a directory the image itself
populates, and — the interesting one — a **sibling of the role's own
mount**, `/upstroke/escape`, which a naive "only paths under the mount
targets are writable" implementation would let through.

## `fn real_docker_a_gate_write_outside_every_declared_mount_fa…` › `const OUTSIDE: &[&str] = &[`

Outside every declared mount, and inside the two that are writable.

## `fn real_docker_a_gate_write_outside_every_declared_mount_fa…` › `"/upstroke/escape",`

A sibling of the role's own mount target.

## `fn real_docker_a_gate_write_outside_every_declared_mount_fa…` › `let plan = runner.plan(&request).expect("plans");`

The mount set the claim is about, taken from the plan rather than
assumed, so "outside every declared mount" is checked against the
list the container is actually given.

## `fn real_docker_a_gate_write_outside_every_declared_mount_fa…` › `for path in &inside {`

The controls: the role's own worktree and the declared scratch
surface are writable, so the refusals above are a boundary and not a
container that can do nothing.

## `fn real_docker_the_daemon_holds_exactly_the_specs_mounts_and_a_read_only_root() {`

The daemon's own container carries **exactly** the spec's mounts and a
read-only root.

`PR6-ENUM-005`'s surviving mutation is not about the spec at all: it is
"append `--mount type=bind,source=/tmp,target=/outside` directly to
Docker's argv, **bypassing `CreateSpec.mounts`**". Every fake test sees
the unchanged spec, and a gated test that only writes to paths it knows
about never asks the daemon what the container really has. So this test
asks: it reads `.Mounts` and `.HostConfig.ReadonlyRootfs` back off the
created container and compares them to the plan, and an argv-appended
mount is a destination the plan does not name.

The container is created through the funnel and never started, which is
what lets it be inspected: `Runner::run` releases on both paths, so a
container that had run would be gone before anything could look at it.

## `fn real_docker_the_daemon_holds_exactly_the_specs_mounts_an…` › `crate::runner::container::fake::preclean_names(`

Pre-clean before the create, not after the teardown.

`reviews/FINDINGS.md` §16: `LeaveNoResidue` above is correct and
cannot help, because no in-process cleanup runs when the process is
SIGKILLed. Every component of this name is fixed — `repo_key()`, the
run id from `GATED_RUNS`, `INCARNATION_1`, and `gate_id(0)`'s
deterministic invocation hash — so the name a previous SIGKILLed run
left behind is exactly the name this `docker create` is about to ask
for. The recurrence is what makes the pre-clean meaningful.

## `fn real_docker_the_daemon_holds_exactly_the_specs_mounts_an…` › `let mut hooks = crate::runner::container::NoHooks;`

Write the intent, materialise the view, create — and stop there.
The view is the runner's own projection rather than a bare directory:
a linked worktree's `.git` pointer file is a bind **source** of the
create, so a directory-only view fails `docker create` outright.

## `fn real_docker_the_daemon_holds_exactly_the_specs_mounts_an…` › `assert!(planned.len() >= 4, "{planned:?}");`

The control: the comparison is not two empty sets.

## `fn real_docker_a_worktree_binary_cannot_shadow_the_certified_cli() {`

A binary planted in the worktree cannot become the CLI the attempt runs,
and a probe and an attempt resolve the same name to the same thing.

`PR6-CORRECTNESS-006`, end to end. DESIGN.md:612: "Probes run through
that same runner, or pre-flight could certify a host CLI/version
different from the one the attempt executes." A probe has no worktree
and an attempt has one, so their working directories differ **by
design**; with `PATH=.:/usr/bin` the attempt's `claude` is whatever the
repository put in the worktree and pre-flight certified something else.

Two cells, and they are the two halves of the repair:

1. an image environment whose `PATH` resolves against the working
   directory is **refused before any effect** — no container is created
   at all, checked against the daemon by label;
2. under the absolute-only `PATH` the runner supplies, the planted
   binary is not resolvable by name in either the probe or the attempt,
   and a name that *is* on the path resolves to the same absolute file
   in both.

The control is inside the same command: the gate proves the shim is
really there and really executable in its own worktree, so "not found"
is a statement about resolution and not about the fixture.

## `fn real_docker_a_worktree_binary_cannot_shadow_the_certifie…` › `let shim = mine.join("claude");`

The shim the repository controls, named as a CLI this engine drives.

## `fn real_docker_a_worktree_binary_cannot_shadow_the_certifie…` › `let hostile = ContainerRunner::new(`

(1) The refusal, before any effect.

## `fn real_docker_a_worktree_binary_cannot_shadow_the_certifie…` › `let runner = ContainerRunner::new(`

(2) The guarantee, under the PATH the runner supplies.

## `fn real_docker_a_worktree_binary_cannot_shadow_the_certifie…` › `assert_eq!(answers[0].1["PWD"], BoundaryLayout::DEFAULT_WORKSPACE);`

The two really do run in different working directories — the premise
of the finding, asserted rather than assumed.

## `fn real_docker_a_worktree_binary_cannot_shadow_the_certifie…` › `assert_eq!(`

The control: the shim is there, and it is executable.

## `fn real_docker_a_worktree_binary_cannot_shadow_the_certifie…` › `assert_eq!(`

Neither resolves it, and both resolve a name that is on the path to
the same absolute file.

## `fn real_docker_a_git_dependent_gate_sees_only_the_role_view() {`

`proof_tests[1]`: "**Git-dependent gate sees only the role view**",
against a real container.

The four properties of DESIGN.md:612, each read out of a real `git`
running inside the boundary: the exact detached HEAD, the exact index
(`status --porcelain` is empty on a clean worktree), no engine refs, and
objects that resolve. The coordinator's refs are re-read afterwards, so
"without exposing **or mutating**" is both halves.

## `fn real_docker_a_git_dependent_gate_sees_only_the_role_view…` › `let git = "git -c safe.directory='*' -C /upstroke/workspace";`

`safe.directory` because the host paths are owned by the coordinator's
user and the container's process is not it — an ownership check, not a
confinement one.

## `fn real_docker_a_git_dependent_gate_sees_only_the_role_view…` › `for name in &planted {`

Nothing was exposed and nothing was mutated: the coordinator's refs
are all still there and still where they were.

## `fn real_docker_adapter_parsing_matches_the_host_table() {`

`decisions.tests_acceptance.parity`: "host and container runners produce
identical **adapter parsing**".

The table, the fixtures and the expectations are PR4's — `runner::tests::
adapter_parse_parity` was written for exactly this and its doc comment
says so — and the **only** thing this test varies is the `&dyn Runner`.
It is a real chain, spec -> runner -> `ProcessOutput` -> `AgentAdapter::
parse`, because the claim is about the seam: an adapter never learns
which runner produced the output it reads, and nothing but a runner
actually producing it proves that.

## `fn real_docker_adapter_parsing_matches_the_host_table()` › `assert_eq!(container_rows.len(), 3);`

The table is not empty and really did vary, so equality is a claim.

## `fn real_docker_withholds_an_image_credential_variable_from_a_role_that_takes_none() {`

The recorded image sets a credential location, and a role that takes
none does not receive it **inside the container**.

`PR6-CORRECTNESS-007`, against the daemon. The unit-level assertion is
about the composed vector, and the composed vector is not the
container's environment: `docker create --env` **overlays** the image's
own, so a key the runner omits is a key the image decides. This runs a
gate — repository-controlled code, the one thing no agent permission
surface bounds — inside an image whose `ENV` sets `CODEX_HOME` and
`CLAUDE_CONFIG_DIR`, and reads the variables back from inside.

Second field held constant: the same image and the same command in both
halves; only the role moves. `GH_CONFIG_DIR` is the control — an image
variable that is not a credential location — so a runner that wiped the
image environment rather than overriding two keys of it fails here.

## `fn real_docker_withholds_an_image_credential_variable_from_…` › `let declared = docker`

The premise, read from the daemon rather than assumed: this image
really does set a credential location.

## `fn real_docker_withholds_an_image_credential_variable_from_…` › `let runner = ContainerRunner::new(`

A policy that DOES record a credential volume for codex would be
creating operator state; the withholding is about the *variable*, and
the mount is separately asserted by
`the_credential_volume_is_mounted_exactly_when_its_location_is_supplied`.

## `fn real_docker_a_container_contains_a_daemonised_descendant() {`

A container contains its **descendants**, including one that has left
the leader's session.

`PR6-ENUM-006`. `invariants_introduced[0]` is "container contains
descendants", and nothing in this suite observed one: the timeout
fixture runs a single `sleep` and checks one container liveness bit, so
a cancellation that terminated or forgot only the leader would pass it.

The descendant is `setsid`-detached, which is exactly what escapes a
host process-group kill (`agent::proc`'s whole subject), and it is
observed **in the host's own process table** rather than by timing: a
container's processes are ordinary host processes in another namespace,
so `/proc/<pid>/cmdline` can name them. Each `sleep` carries a marker
argument nothing else on the machine uses.

The intersection: {leader, detached descendant} × {container running,
container reclaimed}. The two "running" cells are the control — without
them a test in which nothing ever started would pass.

## `fn real_docker_a_container_contains_a_daemonised_descendant…` › `const LEADER_MARKER: &str = "903222";`

Distinct markers, so the leader and the descendant are told apart by
value and not by order.

They are the **durations** the two `sleep` calls are given, so they
have to be numbers — `sleep 999111r3b` answers `invalid number` and
exits 1, which is a container that ends before its timeout and a
fixture that measures nothing. Two implausible second counts, far
apart in value, are what makes them recognisable in `/proc` without
making them unrunnable.

## `fn real_docker_a_container_contains_a_daemonised_descendant…` › `fn pids_with(marker: &str) -> Vec<String> {`

Every pid whose argv contains `marker`, read from `/proc`.

`contains` over argv entries rather than a substring of the whole
buffer, so this process's own command line — which carries the
marker as a literal in the binary, not in argv — cannot match.

## `fn real_docker_a_container_contains_a_daemonised_descendant…` › `let mut request = gate_request(`

The leader spawns a detached descendant and then blocks. The runner's
own timeout is what stops the container, which is
`slice_contract.cancellation`: "timeout or shutdown stops and removes
the container".

## `fn real_docker_a_container_contains_a_daemonised_descendant…` › `let seen = std::thread::scope(|scope| {`

The controls, sampled from another thread while the invocation is in
flight: both processes really are on this machine.

## `fn real_docker_a_container_contains_a_daemonised_descendant…` › `let leader = pids_with(LEADER_MARKER);`

The container is gone, and so is everything it contained. The
descendant left the leader's session, so a cancellation that killed
only the leader's process group would leave it here.

## `fn real_docker_a_container_contains_a_daemonised_descendant…` › `assert!(`

The descendant first: it is the invariant's subject, and a leader
assertion that fires before it would report the wrong thing about a
cancellation that left both alive.

## `fn every_gated_test_of_this_lane_is_counted() {`

Every Docker-gated test this lane adds is on the list that counts them.

`every_docker_gated_test_is_named_and_present` in the substrate's own
suite closes both directions across `src/runner/**`; this is the lane's
half, so a name added here without being listed fails in this file
rather than in another lane's.

## `fn every_gated_test_of_this_lane_is_counted()` › `"real_docker_a_gate_write_outside_every_declared_mount_fails",`

Repair round R1.

## `fn every_gated_test_of_this_lane_is_counted()` › `"real_docker_withholds_an_image_credential_variable_from_a_role_that_takes_none",`

Repair round R3b.
