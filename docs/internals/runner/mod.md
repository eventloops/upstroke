# `src/runner/mod.rs`

Extended notes for [`src/runner/mod.rs`](../../../src/runner/mod.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The Runner seam (DESIGN.md §8, §20; INV-18, INV-20, INV-22, INV-23).

> **Runner** — Execute probes, workers, gates, and reviewers on the host or
> in a role-scoped container; owns cwd, mounts, environment, supervision,
> and timeout, never agent semantics or Git. (DESIGN.md:118)

An adapter builds a data-only [`CommandSpec`]; a [`Runner`] decides where
it executes. That split is the whole point of the layer: "adapters never
learn about containers, and the runner learns nothing about agent semantics
beyond which per-agent credential volume to mount" (DESIGN.md:612).

PR4 ships the host half. [`host::HostRunner`] implements the trait, resolves
the `host-v1` [`policy`] for the marker, the owner record and
`run_started(4).runner`, composes the base-plus-overlay environment, and
executes the `RunnerPreflight` shell probe. The container runner is PR6 and
an explicit non-goal here, as are the async surface and the slot broker.

#### Why `run` is synchronous and still shaped like the async one

`decisions.sequential_substrate.runner`: "Runner::run(&RunnerRequest) ->
ProcessOutput synchronous until PR11 (then a boxed Send future)".
DESIGN.md:250-256 says why the shape has to survive that change: every
async trait used behind `dyn` returns a boxed `Send` future, so the trait
must already be object-safe and its request must already be a single
borrowed value. It is, and [`Runner`] is `Send + Sync` so a `&dyn Runner`
can be held across the await points PR11 introduces.

## `pub struct CommandSpec {`

What an adapter hands the runner (DESIGN.md:222).

```text
struct CommandSpec { program: String, args: Vec<String>, env: Vec<(String, String)>, stdin: Vec<u8> } // env is an overlay
```

Data only, and that is load-bearing rather than stylistic: it knows nothing
about where it will run, so the same value is executed by the host runner
and (PR6) by the container runner without an adapter ever learning which.

## `pub struct CommandSpec` › `pub program: String,`

The program to execute, as the **adapter** names it — and the runner
resolves it against the environment the runner composes.

A name, not a location. Until PR6 an adapter's `build` and `probe`
located their CLI on the coordinator host's `PATH` and put the absolute
host path here, which was invisible while the host runner was the only
runner and its boundary *was* this machine. With a second boundary it is
three failures — a CLI pinned in an image and absent on the host refused
before the runtime is asked anything, every spec carrying a path that
names nothing inside the image, and `Caps.version` certifying the host's
CLI while the attempt runs the image's, which is DESIGN.md:612's
sentence exactly. `PR4-ADAPTER-RESOLVES-ON-THE-HOST` in
`reviews/FINDINGS.md` is the entry; [`crate::agent::bin::Invocation::named`]
is the repair.

This is not a new shape for the field. [`crate::gates::ShellKind::spec`]
has always put a bare `sh`, `bash`, `cmd` or `pwsh` here, for every gate
and for the `RunnerPreflight` shell probe; the three agent CLIs were the
exception. **A `String` was always wide enough** — DESIGN.md:222 freezes
`program: String`, and a name fits where a path may not
(`PR4-PROGRAM-PATH-NOT-UNICODE`).

The corollary belongs to the *runner*: resolving a name is now a thing
each boundary does, so each boundary owns which files a name may reach.
`PR6D-HOST-RUNNER-RESOLVES-BY-PLATFORM-SEARCH` in `reviews/FINDINGS.md`
records what the host runner's answer is today and who owns tightening
it.

## `pub struct CommandSpec` › `pub env: Vec<(String, String)>,`

**An overlay**, not the environment. DESIGN.md:258: "`CommandSpec.env`
overlays a runner-owned base rather than replacing it."

## `pub struct CommandSpec` › `pub stdin: Vec<u8>,`

Bytes for the child's stdin. `Vec<u8>` rather than `String` because a
prompt is text but a spec is a command, and the funnel writes bytes.

## `impl CommandSpec` › `pub fn new(program: impl Into<String>) -> Self {`

A spec for `program` with no arguments, no overlay and no stdin.

## `impl CommandSpec` › `pub fn arg(mut self, arg: impl Into<String>) -> Self {`

Append arguments.

## `impl CommandSpec` › `pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {`

Add one overlay entry.

## `impl CommandSpec` › `pub fn stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {`

Set the stdin payload.

## `pub struct AgentId(String);`

The agent a request is bound to.

Matches [`crate::agent::AgentAdapter::id`] — `claude-code`, `copilot`,
`codex` — because that is the identity the credential location, the slot
pair and the catalog are all keyed by.

## `impl AgentId` › `pub fn new(id: impl Into<String>) -> Self {`

The adapter id as its own type.

## `impl AgentId` › `pub fn as_str(&self) -> &str {`

The id as recorded.

## `pub enum ProbeTarget {`

What a probe certifies.

The contract's `invariants_introduced[1]`: "the probe role carries target
`Agent(name) | Shell`". Two targets rather than one flag, because the two
are accounted differently and the difference is an invariant, not a
detail: INV-18 has "every agent CLI invocation **incl. agent probes**
acquires its atomic {agent, pool?} pair while gates **and the shell probe**
register without slots".

## `pub enum ProbeTarget` › `Agent(AgentId),`

One recorded agent's CLI. Slotted.

## `pub enum ProbeTarget` › `Shell,`

The recorded shell executing `exit 0`. Non-slotted.

## `pub enum ExecutionRole {`

Which seat a process occupies (DESIGN.md:224), with the probe target the
contract adds.

## `impl ExecutionRole` › `pub fn all() -> Vec<Self> {`

Every role, with both probe targets.

Written out rather than derived so a role added later has to be added
here too, and so every grid over roles covers both probe targets — the
pair whose accounting differs.

## `impl ExecutionRole` › `pub fn is_slotted(&self) -> bool {`

Whether this role's process takes an atomic `{agent, pool?}` slot pair.

R3: "agent slot + pool slot pair (worker, review, re-ask, agent probe)
… the shell probe and gates are non-slotted". PR4 records the property;
the broker that acts on it is PR11.

## `impl ExecutionRole` › `pub fn label(&self) -> String {`

The role as it is written in a record: `probe(shell)`, `probe(<agent>)`,
`implement`, `gate`, `review`.

## `pub struct RunnerRequest {`

One process the runner is asked to execute (DESIGN.md:223 plus the
contract's `invocation` field).

## `pub struct RunnerRequest` › `pub workspace: PathBuf,`

The child's working directory.

## `pub struct RunnerRequest` › `pub agent: Option<AgentId>,`

The agent whose slot pair and credential location this process uses.
`None` for a gate and for the shell probe.

## `pub struct RunnerRequest` › `pub invocation: InvocationId,`

R4: "invocation registration (all Runner processes incl. gates, agent
probes, and the shell probe)". Not optional — that is the invariant.

## `pub fn worker_request(`

The worker process of one attempt: `ExecutionRole::Implement`, bound to the
agent whose CLI it is, carrying that attempt's worker identity.

One construction point per role, for the same reason
[`crate::agent::probe_request`] and [`host::shell_probe_request`] are one
each. The three fields below do not vary independently in production — the
role decides the slot pair (R3, [`ExecutionRole::is_slotted`]), the agent
binding decides which credential location `host-v1` supplies
([`host::HostEnvironment::compose`]), and the identity form is the one
`decisions.admission_and_leases.permits.invocation_identity` gives a worker
— so a request that carried one without the others would be a request this
crate never sends. Before these existed, `HostRunner`'s own role grid
hand-built the worker and reviewer requests with `agent: None` and a *gate*
identity, which left a `HostRunner::run` that suppressed the containment
hooks for exactly the production shape (`role in {Implement, Review}` **and**
`agent.is_some()`) passing every test in the suite.

## `role: ExecutionRole::Implement,`

"`ExecutionRole::Implement` with the bound agent is what makes this
process slotted (R3) and what tells `host-v1` to supply that agent's
credential location — both properties of the role, not of the call
site."

## `pub fn review_request(`

One reviewer process: `ExecutionRole::Review`, bound to the reviewing
agent, carrying that pass's or re-ask's identity. See [`worker_request`].

A reviewer is an agent CLI, so it is slotted and `host-v1` gives it its
agent's credential location — the same rule as the worker, and the reason
the two share a shape rather than each being spelled out where it is sent.

## `pub fn gate_request(`

One gate process: `ExecutionRole::Gate`, and **no** agent. See
[`worker_request`].

"A gate is repository-controlled code and runs no agent CLI, so it takes no
`{agent, pool}` pair (R3) and `host-v1` hands it no agent's credential
directory." `agent: None` is therefore part of what a gate *is*, not an
omission at the call site — which is why it is written once, here.

## `pub trait Runner: Send + Sync {`

DESIGN.md:227.

Object-safe, and `Send + Sync` so PR11 can turn `run` into a boxed `Send`
future behind the same `&dyn Runner` its callers already hold.

## `pub trait Runner: Send + Sync` › `fn run(&self, request: &RunnerRequest) -> Result<ProcessOutput, UpstrokeError>;`

Execute `request` and return what the process did.

### Errors

A pre-flight refusal (a reserved environment key in the overlay, a
failing shell probe) or a spawn/supervision failure. A non-zero exit is
not an error: it is a [`ProcessOutput`].

## `pub const SPAWN_SITE: EffectSiteId = EffectSiteId::Process(ProcessSite::Spawn);`

---------------------------------------------------------------------------
ST-07 evidence: the containment sub-effect points
---------------------------------------------------------------------------

## `pub const SPAWN_SITE: EffectSiteId = EffectSiteId::Process(ProcessSite::Spawn);`

The site every containment sub-effect point belongs to.

## `pub struct HarnessHooks {`

Wires the process funnel's [`SpawnHooks`] onto PR3's [`HookHarness`].

The funnel names a point; the harness is keyed by `(site, point, mode)`,
because a mode is executed when its fault *fired* rather than when a funnel
walked past the place it would have fired. So one funnel call consults the
harness once per mode the point declares, and the first non-`Proceed`
answer wins. A point with one mode is consulted once; `AmbientJobJoined`,
the only containment point the packet gives an error contract
(`containment_sub_effects`: "failure refuses the write command"), is
consulted for both.

## `impl HarnessHooks` › `pub fn new(harness: Arc<Mutex<HookHarness>>) -> Self {`

Observe through `harness`.

## `impl HarnessHooks` › `pub fn harness(&self) -> &Arc<Mutex<HookHarness>> {`

The harness this observer records into.

## `impl SpawnHooks for HarnessHooks` › `fn point_mode(&mut self, point: SubEffectPoint, mode: InjectionMode) -> Injection {`

One mode, at the coordinate that mode belongs at. A funnel that fires a
point's two modes at two coordinates calls this twice, once each; the
harness is keyed by `(site, point, mode)`, so each lands on its own key.

## `mod tests` › `fn slotting_follows_r3_not_the_predicate() {`

The slotting split is R3's sentence, transcribed here rather than
computed from the function under test.

## `mod tests` › `fn each_role_builder_binds_what_its_role_binds() {`

Each role builder produces its role, its binding and nothing else's,
and carries the spec and the identity through untouched.

The builders are what every fixture and every call site now asks for, so
a builder that named the wrong role — or bound an agent to a gate —
would be wrong everywhere at once and invisible in a grid keyed on the
same builders. The expected values here are written from R3 and from
each role's own sentence, not read back out of the builder.

## `fn each_role_builder_binds_what_its_role_binds()` › `assert_eq!(request.command, spec, "{role}: the command spec");`

The spec is carried, not rebuilt: an overlay or a stdin payload
dropped here would be dropped for every caller at once.

## `fn each_role_builder_binds_what_its_role_binds()` › `let roles: std::collections::BTreeSet<String> = built`

Three builders, three distinct roles, and two of the three bind.

## `fn role_labels_name_the_probe_target()` › `let labels: std::collections::BTreeSet<String> = ExecutionRole::all()`

Two probe targets never render the same, or a record could not tell
a slotted probe from a non-slotted one.

## `fn the_runner_trait_is_object_safe()` › `fn takes_dyn(_: &dyn Runner) {}`

PR11 turns `run` into a boxed Send future behind this same `dyn`.
A trait that stopped being object-safe would fail to compile here
rather than at the migration.

## `mod tests` › `fn invocation_ids_are_unique_within_a_run_incl_agent_and_shell_probes() {`

Proof test: "InvocationId uniqueness within a run incl. agent and
shell probes".

Uniqueness is **structural**, not statistical. The identities of a run
are the tuples the packet enumerates, and distinct tuples render
distinctly (`invocation::tests::distinct_tuples_render_distinctly`
crosses every field). So what this proves is the other half: that the
set of identities a whole run's worth of Runner processes carries —
INV-20's "worker, gate, review, re-ask, agent probe, shell probe" — is
exactly one per process, with no expected value taken from a generator.

## `fn invocation_ids_are_unique_within_a_run_incl_agent_and_sh…` › `fn run_requests() -> Vec<RunnerRequest> {`

One run's Runner processes, in the order the run would produce
them. A function rather than a literal because it is called twice:
a run whose identities are not a function of the run is a run whose
identities are not deterministic.

## `fn run_requests() -> Vec<RunnerRequest>` › `push(`

Pre-flight (INV-23's RunnerPreflight): one non-slotted shell
probe and one slotted probe per recorded agent. The packet's
third form, "(probe, target: Agent(name) | Shell, ordinal)".

## `fn run_requests() -> Vec<RunnerRequest>` › `for task in 0..TASKS {`

The run: every attempt of every task, its gates, and its review
pass — the packet's first form, "(key, generation, attempt,
role, ordinal)".

## `fn run_requests() -> Vec<RunnerRequest>` › `for sequence in 0..SEQUENCES {`

Integration transactions — the packet's second form,
"(sequence, role, ordinal)", whose roles exclude worker.

## `fn run_requests() -> Vec<RunnerRequest>` › `Some(AgentId::new(AGENTS[(sequence % 3) as usize])),`

A reviewer is an agent CLI in this form too, so it
carries its agent. A grid whose sequence reviews bound
no agent would be varying the role and the binding
together and calling it one field.

## `fn invocation_ids_are_unique_within_a_run_incl_agent_and_sh…` › `let expected = 1`

The size comes from the run's shape, written here, not from the
vector under test.

## `fn invocation_ids_are_unique_within_a_run_incl_agent_and_sh…` › `let counted = |prefix: &str| ids.iter().filter(|id| id.starts_with(prefix)).count();`

All three forms are in the set, and each is in it the number of times
the run's shape says.

## `fn invocation_ids_are_unique_within_a_run_incl_agent_and_sh…` › `assert_eq!(`

The binding is R3's rule in every form, and it is a count rather
than a claim: a grid that let the agent binding ride along with the
role would prove the identities of a run this crate never executes.

## `fn invocation_ids_are_unique_within_a_run_incl_agent_and_sh…` › `let again: Vec<String> = run_requests()`

Deterministic in the sequential substrate: the same run yields the
same identities. A generator that mints a fresh value per call — a
ULID, a counter, a clock — fails here, and this is the assertion
`crash_reconstruction` rests on when it builds a container name
"so deterministic InvocationIds never collide across incarnations
and no earlier ownership evidence is overwritten".

## `fn invocation_ids_are_unique_within_a_run_incl_agent_and_sh…` › `let probes: Vec<&RunnerRequest> = requests`

The probes are in the set, and they are accounted the way INV-18
accounts them: agent probes slotted, the shell probe not.

## `fn invocation_ids_are_unique_within_a_run_incl_agent_and_sh…` › `let workers: Vec<&InvocationId> = requests`

"changes with every attempt": the same task, agent and role at two
attempts are two invocations, and they differ in the attempt field
rather than by chance.

## `mod tests` › `pub(crate) struct ParsedRow {`

Every place production code starts a process, named and counted.

`decisions.pr_sequence[5].slice_contract.invariants_introduced[0]`:
"**every** CLI and gate process executes through Runner", and
`gating`: "process funnel sites recorded". Recorded as a table with
counts rather than as prose, because the failure mode is a *new* spawn
appearing somewhere with nobody deciding whether it should have been
routed. A file that grows one fails here until it is classified.

What is scanned: `Command::new`, `.spawn()` and `run_with_timeout` in
the production region of every `src/**/*.rs`. The production region is
the file with every `#[cfg(test)] mod … { … }` block removed by brace
matching at the module's own indentation — sound because
`cargo fmt --check` is a gate, so a module's closing brace is the first
line at exactly that indentation. `src/engine/tests.rs` is a whole test
module (`engine/mod.rs`: `#[cfg(test)] mod tests;`) and is excluded as
one.

The three rows are the only production process starts in the tree, and
each is classified against the passage that puts it there.
One row of the parity obligation: what a runner was asked to run, and
what the adapter made of what came back.

## `mod tests` › `pub(crate) fn adapter_parse_parity(`

The adapter-parsing half of `decisions.tests_acceptance.parity`, as a
function of the boundary.

> host and container runners produce identical **adapter parsing** and
> environment composition

PR6 calls this with its container runner and compares the returned
table against the host's, which is what "dropped in beside it" means:
the fixtures, the specs and the expectations live here once, and the
only thing that varies between the two runs is the `&dyn Runner`.

It is a real chain rather than a stub — spec → runner → `ProcessOutput`
→ `AgentAdapter::parse` — because the claim is about the *seam*: an
adapter never learns which runner produced the output it reads, and
nothing but a runner actually producing it proves that.

The child is the recorded shell echoing an environment variable, so the
fixtures need no agent CLI and no writable scratch file, and no payload
byte ever reaches a command line. The payload and the exit code ride in
on `CommandSpec.env` — the overlay, which is therefore load-bearing
here rather than decorative, and which is itself half of what the
parity clause is about.

## `mod tests` › `const FIXTURES: &[Fixture] = &[`

Two adapters with two different answer shapes (a JSON envelope and
plain text) and both exit dispositions: a parity table whose rows
all parse the same way would prove two runners agree about nothing.

## `mod tests` › `fn script(code: i32) -> String {`

Echo `$UPSTROKE_PARITY_PAYLOAD` and exit `code`, in the recorded
shell's own dialect. The payload is never in the command line, so
nothing here depends on either shell's quoting.

## `mod tests` › `fn the_host_runners_adapter_parsing_table_is_the_one_pr6_must_match() {`

The host side of the parity table, pinned.

The expected rows are written from what each adapter's `parse`
documents — a JSON envelope with `is_error` absent and exit 0 is
`Completed` carrying `result`, its session and its cost; the same
envelope after a non-zero exit is an `AgentError`; and Copilot has no
envelope at all, so it reports no session and no cost even on success.
None of them is read back from `parse`.

## `fn the_host_runners_adapter_parsing_table_is_the_one_pr6_mu…` › `detail: Some("the work is done".to_owned()),`

The failure path reports the agent's own text, and the
envelope's session and cost survive it: spend already
happened.

## `fn the_host_runners_adapter_parsing_table_is_the_one_pr6_mu…` › `let mut statuses: Vec<String> =`

Hostility as counts: two adapters, two statuses, three distinct
details, and both "reports a session" dispositions.

## `mod tests` › `fn the_spawn_site_files_every_role_under_one_context_and_the_count_says_which() {`

Every spawn this slice performs is filed under **one** site, and that
site declares one adjacent event, one fault row and one observable
order.

`decisions.effect_site_inventory.identity` says each group's "variants
are its semantic contexts", and every variant carries "its adjacent
durable event … [and] its fault-matrix row id". PR3's `Process` group
has two variants — `Spawn` and `Terminate` — and `Spawn` is
`After(AttemptStarted)` / `T-ATTEMPT`. PR4 routes five roles through
it, and two of them do not run inside an attempt at all: the shell
probe and the agent probes are `RunnerPreflight`, which
`workspace_candidates.run_creation` orders at **P4**, before P6's
`run_started`. A crash prefix at a probe spawn is therefore
effect-before-`run_started` (T-RUNSTART on a fresh run, T-RESUME on a
resume) while the site it is recorded under says event-before-effect in
T-ATTEMPT.

**The site's own variants are not this slice's to add.** The site enum,
its adjacency and its fault row are `topology::effects` — PR3's, frozen
here, and in its `sites.rs` and `vocab.rs` children since that module was
split — and a probe context would be a *new variant* of an
inventory the packet enumerates. That half is deferred, with an owner,
in `reviews/FINDINGS.md`. What this test contributes is that the
mismatch is counted rather than silent: the two roles are named here,
so ST-07 evidence over `Process.Spawn` cannot be read as covering the
probe prefixes.

**This count discharges nothing about the hooks themselves, and must
not be read as if it did.** Counting that two roles fall outside the
site's declared context proves the mismatch exists; it does not prove
the containment hooks execute on those roles, and a `HostRunner::run`
that passed `NoHooks` for `Probe(_)` would leave this test green. That
obligation is PR4's — `scope`'s "**probes**, workers, gates, reviews go
through the Runner" and `proof_tests[3]`'s "containment sub-effect hook
tests (ST-07 subset)" — and it is discharged at runtime, for all five
roles, by `host::tests::every_role_reaches_the_containment_points_of_this_platform`
and `host::tests::a_fault_armed_at_any_containment_point_stops_any_role`.

## `fn the_spawn_site_files_every_role_under_one_context_and_th…` › `assert_eq!(SPAWN_SITE, EffectSiteId::Process(ProcessSite::Spawn));`

Transcribed from PR3's inventory, not read back from PR4.

## `fn the_spawn_site_files_every_role_under_one_context_and_th…` › `let roles: Vec<(ExecutionRole, bool)> = vec![`

Which roles this slice spawns, and whether each runs inside an
attempt — i.e. after the durable event the site is adjacent to.
Written from the packet's own ordering of a run's phases.

## `mod tests` › `fn the_containment_coordinates_are_pinned_against_written_literals() {`

The eight containment coordinates, pinned as literals.

`containment_sub_effects` writes them out — "Spawn.AmbientJobJoined …,
Spawn.CreatedSuspended …, Spawn.PrivateJobAssigned, Spawn.Resumed …;
Unix: Spawn.ReaperStarted …, Spawn.PreExecPgidAndRegister, Spawn.Exec,
Spawn.Registered" — and every check the suite made on that vocabulary
was derived from the enum it is meant to pin: the generated registry,
the `Display` impl and the serde round trip all read `SubEffectPoint`,
so renaming a variant *and* its `name()` arm together left all of them
agreeing on the new spelling and the suite green. The literal
`Spawn.CreatedSuspended` existed in this tree only inside doc comments.

This is the project's own upheld line — a suite that "compares its own
serialization only against itself" has not pinned anything — applied
where the packet freezes the spelling in prose. The enum is PR3's and
frozen; the assertion is PR4's, because PR4 is the slice that made these
eight coordinates load-bearing.

Two spellings, because there are two: the coordinate the packet writes
(from `name()`) and the wire form the enum serialises to (from
`rename_all = "snake_case"`). Naming the Rust variant in the same row is
deliberate — a rename of the variant itself stops this table compiling,
which is the same failure by a shorter route.

## `fn the_containment_coordinates_are_pinned_against_written_l…` › `const PINNED: &[(SubEffectPoint, &str, &str)] = &[`

(variant, the coordinate `containment_sub_effects` writes, wire form)

## `fn the_containment_coordinates_are_pinned_against_written_l…` › `let decoded: SubEffectPoint =`

And the written literal decodes back to this point: a rename that
kept the encoder and the decoder agreeing would otherwise be
invisible from this direction too.

## `mod tests` › `fn receiver_writes(code: &str, field: &str) -> usize {`

Assignments to `field` **through a receiver**, in every assignment form.

`x.field = …` and **all ten** of Rust's compound assignment operators —
`+= -= *= /= %= &= |= ^= <<= >>=`; never `x.field == …`, and never a
longer field whose name starts with this one.

Two measured fail-open holes, both in the same direction. The literal
`".field ="` misses `+=` — the idiomatic increment, and therefore the
form a second counting rule is most likely to arrive in (S5 round 4).
The five-operator repair for that then missed `&= |= ^=` (not in its
set) and `<<= >>=` (second byte not `=`), which S5 round 5 measured with
`task.attempts_on_rung |= 1;` (`R5-SETTLE-001`). The enumeration is now
the language's, so there is no sixth hole of this shape.

## `fn receiver_writes(code: &str, field: &str) -> usize` › `[b'=', b'=', ..] => false,`

`==` is a comparison, not a write.

## `fn receiver_writes(code: &str, field: &str) -> usize` › `[b'<', b'<', b'=', ..] | [b'>', b'>', b'=', ..] => true,`

`<<=` and `>>=`, whose second byte is not `=`.

## `fn receiver_writes(code: &str, field: &str) -> usize` › `[op, b'=', ..] => {`

The seven single-character compound operators. Rust has
ten in total and this arm used to name five: `&= |= ^=`
fell through it and `<<= >>=` never reached it, so
`task.attempts_on_rung |= 1;` — a bare assignment through
a receiver, inside the domain all three doc sentences
state — left the census green. `R5-SETTLE-001`, and it is
`PR7-R3-CENSUS-WRITE-DOMAIN-PROSE` five operators over.

## `mod tests` › `fn until_depth_zero(value: &str, terminator: u8) -> &str {`

`value` up to `terminator` at **nesting depth zero**, so a comma inside
`format!("{a}-{}", b)` does not end the expression.

The reason this exists: taking a field initializer's value as "everything
up to the first comma" truncates every multi-argument expression before
its arguments, so the site is skipped rather than judged. Measured by S5
round 4 — a planted `stem: format!("{index:02}-{}", display_id)` was
invisible to the census that exists to forbid exactly that.

## `mod tests` › `fn function_body<'a>(code: &'a str, name: &str) -> Option<&'a str> {`

The body of `fn name` in `code`, braces matched, or `None` when `code`
does not define it.

**Per-item, because a whole-file count is not a mapping.** A census that
totals a call across a subtree is green for every arrangement summing to
the same total, so a charge that moves from one function to another is
invisible to it. Reading each function's own body is what turns "two
calls somewhere" into "this one calls it, and so does that one".

The scan is over [`crate::effects::production_code`]'s region, where
comments and string literals are already blanked, so a brace inside
either cannot open or close a body here.

### Panics

When `code` defines `name` more than once: two bodies is two answers and
the caller would silently read the first.

## `fn function_body<'a>(code: &'a str, name: &str) -> Option<&…` › `let mut at = found + needle.len() - 1;`

The signature first: the body opens at the first `{` outside the
parameter list and outside any bracketed bound. A `;` there instead
is a declaration without a body — a trait signature — and running on
would return the *next* item's body.

## `mod tests` › `fn stem_values(code: &str) -> Vec<(usize, String)> {`

Every place production builds a filename `stem`, and the expression it
builds it from.

**Both shapes**: the field initializer `stem: <value>,` and the binding
`let stem = <value>;`. The census counted only the first, so
`coordinator.rs:537` — the live legacy path, and the site the schema-4
assembler was extracted from — was outside its domain entirely.

## `mod tests` › `fn production_sources() -> Vec<(String, String)> {`

[`production_sources_by_path`], keyed by the display form eleven census
tables below are written in: `src`-relative and forward-slashed, so one
table reads the same on both platforms.

**This string is a label, not a path, and no boundary may be decided on
it.** `to_string_lossy` replaces a non-UTF-8 byte with U+FFFD and the
`replace` turns every backslash into a separator, so on Unix — where a
backslash is an ordinary filename byte — a sibling literally named
`src/topology/fold\decoy.rs` arrives here as `src/topology/fold/decoy.rs`
and is indistinguishable from a file inside the directory. Wrapping the
result in `Path::new` afterwards cannot restore what the conversion
destroyed: it re-parses a display string and inherits the same answer
with a `Path` type annotation on it. A census that decides containment
takes [`production_sources_by_path`] and compares the walk's own
`PathBuf`, which is `PR108-CENSUS-PATH-AS-DISPLAY-STRING`'s repair.

## `mod tests` › `fn display_path(relative: &std::path::Path) -> String {`

The display form of a `src`-relative path, for the census tables keyed
by it. See [`production_sources`] for what it is not.

## `mod tests` › `fn production_sources_by_path() -> Vec<(PathBuf, String)> {`

Every `src/**/*.rs`, as (`src`-relative path, production code), with
whole-file test modules left out.

The path is the one the walk built, unconverted, so a caller deciding
whether a file is inside a subtree compares components rather than
characters.

The region is [`crate::effects::production_code`]: the whole file with
comments and string literals blanked and every `#[cfg(test)]` item
removed. Every census below counts over it, and each of the three
properties is load-bearing:

* **Blanked**, because a count over raw text counts prose.
  `src/agent/proc.rs` names `run_with_timeout` eight times, five in code
  and three in doc comments, and a real `run_with_timeout_unbounded`
  bypassing `OUTPUT_LIMIT_BYTES` could be paid for by deleting two
  sentences in the same file. Measured — it was.
* **The whole file**, because the previous region dropped everything
  between a `#[cfg(test)] mod tests;` declaration and the next line that
  is exactly `}`. The `tests.rs` entries of
  `effects::tests::cfg::WHOLE_FILE_TEST_MODULES`
  declare their tests that way, and a
  `Command::new("git").arg("push")` appended after one was invisible
  while the identical lines above it failed the census.
* **Item-wise removal**, not truncation, for the same reason.

## `fn production_sources_by_path() -> Vec<(PathBuf, String)>` › `let test_modules =`

Through the shared resolver: this loop was written out here and in
`events::log::tests`, and a third census then wrote a *different*
rule — `file_stem == "tests"` — which covers only the whole-file
test modules named `tests.rs` and not the rest. `PR7-R5-ATT-001`.

## `fn production_sources_by_path() -> Vec<(PathBuf, String)>` › `assert!(`

The control: a derivation that found nothing would silently count
every test file as production, which is the failure this replaces.

## `fn production_sources_by_path() -> Vec<(PathBuf, String)>` › `let relative = path`

The path, not a rendering of it. Everything a caller decides
about containment is decided on this value; the display form
is derived from it at the last moment, by `display_path`.

## `fn production_sources_by_path() -> Vec<(PathBuf, String)>` › `for (relative, code) in &sources {`

The three controls every census below shares, all of
`PR4-CENSUS-COMMENT-ORACLE`'s class.

The regions are not empty, **file by file**. The aggregate floor below
is the weaker half of this and cannot replace it: it stands at
750,000 against an actual 926,043, so 176,043 non-whitespace bytes may
vanish before it notices, and the two largest files this walk keeps
hold 146,260 between them — inside that headroom. One file's region
collapsing to nothing is exactly what a `#[cfg(test)]` in a comment
used to do, and it is invisible to a sum.

**Necessary, not sufficient.** A per-file floor sees a region that
collapses; it does not see one that is *replaced*.
`PR7-R2C-CHAR-LITERAL-DESYNC`'s refined form removes exactly the
forged lines and adds a probe of the same size, and was measured at
8525 dense bytes both with the attack and without it. What closes that
is `effects::char_literal_end` and `configured_item_end` returning
`start` rather than the file's length — not this.

## `fn production_sources_by_path() -> Vec<(PathBuf, String)>` › `assert!(`

And the blanking removed something. A blanking that silently stopped
working would put every doc comment and string literal back into the
counts, which is how a real ninth `run_with_timeout` entry point was
paid for by deleting two sentences.

## `mod tests` › `fn every_production_runner_request_is_built_by_its_roles_builder() {`

Every `RunnerRequest` production builds is built by the builder for its
role, and there are five roles and five builders.

`scope`: "probes, workers, gates, reviews go through the Runner", and
each of those four words is a role whose request carries three fields
that travel together — the role, the agent binding (R3's slot pair,
`host-v1`'s credential location) and the identity form. A request
assembled at a call site can get one of them wrong; a request assembled
by the role's builder cannot, and a *test* that assembles its own is
how PR4 came to prove containment for a shape production never sends.

So the census is on the construction, not on the shape: one
`role: ExecutionRole::` per builder in the production region of the
tree, and no others anywhere. A new hand-built request — in production
or in a fixture that copied one — shows up here as a row that has to be
classified.

**Two needles, because one of them can be dodged.** A literal written
with field shorthand (`RunnerRequest { command, workspace, role, … }`)
names no variant and would slip past the first needle entirely — the
grid in this very file writes one that way. So the type's own name is
counted beside it, and that count includes the declaration and the
builders' return types, which is why the numbers are what they are.

## `fn every_production_runner_request_is_built_by_its_roles_bu…` › `const EXPECTED: &[(&str, usize, usize, &str)] = &[`

(file, `role: ExecutionRole::`, `RunnerRequest {`, and what they are).

## `fn every_production_runner_request_is_built_by_its_roles_bu…` › `for absent in [`

The four words of `scope`, and the file that would hold a fifth.

## `fn every_production_runner_request_is_built_by_its_roles_bu…` › `"src/engine/assembly.rs",`

Assembles a worker's *command* and must never assemble its
request: the command says what to run and the request says the
role, the boundary and the identity. One module doing both would
be a call site that could choose its own role, which is what
`ExecutionRole::is_slotted` and `host::supplies_credentials` are
derived from.

## `fn every_production_process_start_is_classified()` › `const EXPECTED: &[(&str, usize, usize, usize, &str)] = &[`

(file, `Command::new`, `.spawn()`, `run_with_timeout`) and why.

## `fn every_production_process_start_is_classified()` › `assert_eq!(expected.len(), 5);`

The table names five files, and it is the *set* that is the claim:
adapters, gates, review and the engine appear nowhere in it, which
is what "every CLI and gate process executes through Runner" means
once the migration has happened. All five really do start a process.
PR6's `src/runner/container.rs` is the newest, and its row says why a
`docker` call is one of the things that never crosses the boundary
rather than one that was forgotten.

It was six. The sixth was `src/effects.rs`, whose only
`Command::new(` is inside `DENIAL_FIXTURES` — a string constant whose
whole purpose is to be REFUSED, compiled against `clippy.toml` by
`effects::tests::every_declared_effect_denial_refuses_for_the_reason_
it_declares`. It was a row here only because this census counted
string literals. It counts code now, so a fixture is not a process
start and `src/effects.rs` is named below with the rest of the files
that start none.

## `fn every_production_process_start_is_classified()` › `for (file, why) in [`

The other half of DESIGN.md:612's sentence. "Authoritative Git and
the event log never [cross the boundary]" names two things, and the
table above only sees one of them: `src/workspace.rs` is caught by a
declared `Command::new(` count it would have to lose, but `events.rs`
legitimately starts no process at all, so a Runner call *appearing*
there subtracts from nothing. An event append implemented by
spawning an append helper through the Runner — on every event,
replay included — passed the census above unchanged.

So the event log is asserted by name and by the tokens that would
mean it had acquired a boundary: not just a spawn, but a runner, a
request, or a command spec, any of which is the log deciding where
its writes execute.

## `fn every_production_process_start_is_classified()` › `"src/events/log.rs",`

PR5 moved the writer here. The claim follows the code: this
file is now the only one that writes the log, so it is the
one an append-by-subprocess would have to appear in.

## `fn every_production_process_start_is_classified()` › `for adapter in [`

And an adapter does not *choose* a boundary either. DESIGN.md:117:
an adapter turns a TaskRun into a data-only CommandSpec and "does
not decide where the process runs". Naming a concrete runner in
production is that decision, whether or not it also spawns — which
is the half a spawn-site count cannot see. `capacity` and `connect`
are the two commands that legitimately make their own host runner
because they drive no run and have none to borrow, so they are
named here rather than covered by silence.

## `fn every_production_process_start_is_classified()` › `let code = crate::effects::production_code(&source);`

Code, not prose: a doc comment may name the host runner to
explain why something is the way it is, and several do. The
blanking is what removes them; a `//`-prefixed-line filter left a
trailing `// … HostRunner …` on a code line in place.

## `mod tests` › `fn write_command_containment_has_one_join_site_and_one_mint() {`

Write-command containment is joined in **one** place and proved in
**one** place, and this is the count.

`Contained`'s constructor is private to `runner::host::proof`, a module
with no descendants, so the only mutation that can mint a proof out of a
failed join — `let _ = proc::join_ambient_job(hooks); Ok(Contained::new())`
— is one that can be *written* inside that module, and nowhere else in the
crate: not in `runner::host` itself, and not in its other children. That
makes the class closable by counting: one call to
`proc::join_ambient_job`, one call to `Contained::new`, both inside the
function
`host::tests::the_production_containment_mint_propagates_a_join_refusal_and_mints_nothing`
drives on its failure branch.

A second mint appearing anywhere — a new entry point that "also"
establishes containment, a facade that inlines the step — fails here
until it is classified, which is the half a single failure-path test
cannot cover on its own. Code only: several doc comments name both
symbols, and two of them do it to explain this very rule.

**Three needles, because the named constructor can be walked around.**
`Contained`'s field is private to `runner::host::proof`, and inside *that*
module `Contained(())` builds one without going anywhere near
`Contained::new` — and without touching the establishment counter the
failure-path test reads. Written anywhere else, including `runner::host`
itself, it is now a compile error (`E0423`). So the tuple-struct call is
counted too, which is why
`src/runner/host.rs` shows one (the declaration) and `src/main.rs` shows
two.

## `fn write_command_containment_has_one_join_site_and_one_mint…` › `const EXPECTED: &[(&str, usize, usize, usize, &str)] = &[`

(file, `proc::join_ambient_job(`, `Contained::new()`, `Contained(`,
and why).

## `fn write_command_containment_has_one_join_site_and_one_mint…` › `let counts = (`

`production_sources` already blanks comments and string literals,
so a doc comment naming either symbol — and two of them do it to
explain this very rule — contributes nothing.

## `mod tests` › `fn a_command_is_assembled_in_one_production_place_per_role() {`

Every production call site that **populates** a [`CommandSpec`] payload
field, named and counted.

This is the tripwire for `PR4-CONF-006`'s whole class. That finding was
not "the fixtures forgot stdin"; it was that a production call site
started filling a spec field and no fixture grid learned of it, so an
observer suppression keyed on that field passed every test in the suite.
The same thing is true of the overlay the moment anything sets one: as
of this slice **nothing does**, and `runner::host::tests::
the_role_grid_sends_the_shapes_production_sends` carries an empty
overlay for all five roles *because* that is production's only value.

So the count is on the population, not on the shape. `.stdin(` and
`.env(` are counted across the production region of the tree — both
`CommandSpec`'s builders and `std::process::Command`'s methods spell
them the same way, and each row says which it is. A file that grows one
fails here until somebody decides whether the grids have to carry it.

**A method call is not the only way to populate a field.**
`PR5-FIDELITY-001`: the two spec *constructors* build a `CommandSpec`
with a struct literal, so `env: Vec::new()` at `src/agent/bin.rs`
becoming an argument-dependent overlay is a production site this census
could not see at all — pre-flight would then launch the probe with an
overlay the spending command does not carry, against DESIGN.md:262-264.
So the third column counts struct-literal `env:`/`stdin:` initializers
too, and the constructors are enumerated rows like everything else.
**A worker's command is assembled in exactly one production place, and
a gate's in exactly one other.**

The census this slice most needed and did not have. Two engines now want
the same three command sets — the legacy one assembling them inline at
the moment of use, the schema-4 driver wanting them up front in a plan —
and assembling them twice is this project's dominant defect class. Of
PR7's review findings the expensive ones were all one rule implemented
twice, including two derivations of a task's predicted region that
disagreed on every glob and shipped green (`84a3978`).

**What this pins is input selection, not minting.** Minting was never
duplicated: the crate has two `CommandSpec` constructors,
`gates::ShellKind::spec` and `agent::bin::Invocation::spec`, and both
already say so in their own docs. What was about to be duplicated is
*which* prompt, permissions file, timeout and profile go into them. So
the columns count the two calls that perform that selection —
`AgentAdapter::build`, and `ShellKind::spec` — per file.

`src/review.rs` is here with a non-zero `build` count **and that is the
outstanding half of this work**, not an exemption: the reviewer's
command is still assembled in `review.rs` because its invocation
machinery is a re-ask loop with a per-invocation prompt, which does not
extract by moving one expression. When that lands, its count moves the
way the worker's did — and until then the duplication is a number in a
test rather than a sentence in a review.

## `fn a_command_is_assembled_in_one_production_place_per_role()` › `const EXPECTED: &[(&str, usize, usize, &str)] = &[`

(file, `AgentAdapter::build` calls, `ShellKind::spec` calls, why).

## `mod tests` › `fn an_attempts_ledger_line_is_constructed_in_one_production_place() {`

**An attempt's ledger line is constructed in one production place.**

The fourth one-production-place census, and the one whose subject is
read back out. `AttemptRecord.failure` is what `ladder::next_step`
decides from and what `ladder::spends_allowance` prices, so two
constructions are two answers to "what happened to this attempt" — and
the settlement, the escalation and the allowance would each be reading
whichever one their caller happened to build.

The column counts `AttemptRecord {` struct literals in production code,
anchored to a line of its own for the reason the profile census is:
a return type and the definition contain the same text.

## `fn an_attempts_ledger_line_is_constructed_in_one_production…` › `const EXPECTED: &[(&str, usize, &str)] = &[(`

(file, `AttemptRecord {` literals, why).

## `mod tests` › `fn a_worker_profile_is_constructed_in_one_production_place_per_role() {`

**An invocation's profile is constructed in one production place per
role.**

The third of the one-production-place censuses, and the one that decides
what a process is *allowed to do*. A [`crate::ir::WorkerProfile`] carries
`permissions`, and the two roles want opposite answers: an implementer
is `Edit`, a reviewer is `ReadOnly`. A driver that rebuilt an
implementer's profile and reached for the nearest existing constructor
would get `review::profile_for` — a read-only profile. The worker would
spawn, edit nothing, and report success, and the gates would judge an
empty diff.

The column counts `WorkerProfile {` struct literals in production
code, anchored to a line of its own: a return type (`-> WorkerProfile
{`) and the definition itself (`struct WorkerProfile {`) both contain
the same text and neither constructs anything. Measured without the
anchor this census reported five sites and three files.

## `fn a_worker_profile_is_constructed_in_one_production_place_…` › `const EXPECTED: &[(&str, usize, &str)] = &[`

(file, `WorkerProfile {` literals, why).

## `mod tests` › `fn an_observation_about_an_attempt_is_classified_in_one_production_place() {`

**An observation about an attempt is classified in one production
place.**

The companion to `a_command_is_assembled_in_one_production_place_per_role`,
and it pins the higher-stakes half. A command assembled twice runs the
wrong process; a *classification* made twice decides the wrong thing
about a task — `ladder::next_step` reads an `AttemptFailure` and chooses
retry, escalate, defer, park or fail from it, and **the allowance
decision is derived from the same field**. Two engines calling one diff
different things would not surface as a wrong answer. It would surface
as a task escalating to a pricier tier because the other engine
disagreed about what its diff was.

Columns count the constructors of the two verdict types: `AttemptFailure`
and `ReviewRecord`. `src/engine/classify.rs` holds what was inline in
the legacy verification ladder. `src/engine/attempt.rs` keeps its count
for `review_failure`, which was **already a function** — a pure move of
something already callable is churn, not extraction — and `src/ladder.rs`
keeps its own for the escalation vocabulary it owns.

## `fn an_observation_about_an_attempt_is_classified_in_one_pro…` › `const EXPECTED: &[(&str, usize, usize, &str)] = &[`

(file, `AttemptFailure::new` calls, `ReviewPassOutcome::` uses, why).

## `mod tests` › `fn the_rungs_allowance_is_counted_in_one_production_place() {`

**The rung's allowance is counted in one production place, and decided
in another that everyone consults.**

The fourth single-authority census, and it exists because the pair it
covers had already diverged. S5 round 2 found `TaskFold::attempts_on_rung`
incrementing on every `attempt_started` while `ladder::spends_allowance`
— documented as *"the single production implementation of the allowance
rule"*, total over `FailureKind` — decided the same question at the
settlement. Two production places for one rule, disagreeing on every
interruption, park and outage, against
`transaction_fault_matrix[T-ATTEMPT]`'s "unknown spend, **allowance
refunded**".

The other three censuses were each written after a defect of exactly this
shape, and none of them covered *counting*: they cover which command a
role gets, what an observation means, which profile a role runs under,
and which ledger line an attempt writes. Repairing the divergence alone
would leave the pair free to diverge again on the next edit, which is the
difference between fixing an instance and closing a class.

### The two columns

**Writes** are **assignments through a receiver** — which is what makes a
site a decider of persisted state. There may be exactly the two in the
fold, the increment at the settlement and the reset the escalation
performs onto its new rung, and no others anywhere. A third is a second
counting rule.

**Every assignment operator, not just `=`.** The needle was the literal
`".attempts_on_rung ="`, which does not match `+=` — the most idiomatic
form of the very thing this census counts. Measured by S5 round 4:
planting `ladder.attempts_on_rung += 1;` in a production function left
this census green. That is `PR7-R3-CENSUS-WRITE-DOMAIN-PROSE` one
operator over: the stated domain ("assignments through a receiver")
still exceeded the counted domain. [`receiver_writes`] counts the
compound forms too, and excludes `==`.

**The construction default is deliberately outside this domain, and the
doc said otherwise until `PR7-R3-CENSUS-WRITE-DOMAIN-PROSE`.**
`TaskFold`'s `attempts_on_rung: 0` is a field initializer, not an
assignment, so the needle never matched it while this comment claimed
three sites and the table expected two. **A census whose stated domain is
wider than its counted domain fails open**: a second `TaskFold`
constructor with a non-zero default would move no count, and this
census's whole purpose is that a new writer cannot appear silently.
The domain is now stated as what it counts; widening it to constructors
is a separate needle and a separate claim.

**Consults** are calls to `spends_allowance`. These are *expected* to be
plural: one rule consulted from several places is the shape this census
wants. What it forbids is the alternative — a caller that re-derives the
answer from a `SettlementTransition`, a `Next`, or an attempt number,
which is what `settle_failed` did before `FailureShape` existed and what
this fold did until round 2.

## `fn the_rungs_allowance_is_counted_in_one_production_place()` › `const EXPECTED: &[(&str, usize, usize, &str)] = &[`

(file, `attempts_on_rung` writes, `spends_allowance` calls, why).

## `fn the_rungs_allowance_is_counted_in_one_production_place()` › `const SELF_CHECK: &str = "<self-check>";`

**The control is a corpus entry, not a side call.**
`each_census_needle_covers_the_domain_its_doc_states` proves
`receiver_writes` is right and proves **nothing** about whether this
census calls it — measured: reverting the count below to the
pre-repair literal `code.matches(".attempts_on_rung =")` left that
unit test green, and left a closure-based self-check green too,
because the revert bypasses the closure. A control only binds the
census's own count if it travels through it, so this synthetic file
joins the corpus and is expected in the table like any other: it
carries **four** compound assignments — one from each shape the
needle has had to grow to cover, `+= -=` and `|= <<=` — and one
comparison, and expects 4. The pre-repair literal
`.attempts_on_rung =` scores it **1** (it matches only the `==`), and
the five-operator version scores it **2**. One compound assignment
and one comparison would have scored 1 under both the literal and the
correct needle, because the two errors cancel — measured, and the
reason the control is shaped this way.

## `fn the_rungs_allowance_is_counted_in_one_production_place()` › `let writes = receiver_writes(&code, "attempts_on_rung");`

**Assignments through a receiver**, which is what makes a site a
*decider* of persisted state. `let attempts_on_rung = ...` in the
driver is a local binding of a value it is about to pass, and
counting it would make this census report the consult twice.

## `fn the_rungs_allowance_is_counted_in_one_production_place()` › `use std::path::Path;`

**And every settlement reaches that one place — each of them, not
two between them.** The table above counts *write sites*, which is
what it was written for — but a write site nothing calls is a rule
nothing applies, and that is exactly how the allowance broke on
2026-08-27: `candidate_prepared` became the sole successful
settlement, the increment stayed behind in `apply_settlement`, and
this census went on finding its one write and passing. A successful
attempt spent nothing and nothing said so.

Schema 4 has two settlement appliers — `apply_settlement` for a
failure and `apply_candidate_prepared` for a success — and both must
charge.

**READ THIS BEFORE MAKING THE NEEDLE STRICTER. What follows counts
SPELLINGS, and a count over text cannot enforce a property about
calls.** Three rounds made this needle stricter — a whole-subtree
walk, then a per-applier map, then both `Call` forms — and each
stricter needle lost to a different spelling. The one that defeats
the form below leaves every number it reads unchanged:

    let real_charge = Self::charge_allowance;
    let charge_allowance = |state: &mut Self| {
        if !matches!(&finished.settlement, AttemptSettlement::Retained { .. }) {
            real_charge(state, finished.key, &finished.record);
        }
    };
    charge_allowance(self);

The closure invocation is the only `charge_allowance(` in that body
and the real call is spelled `real_charge(`, so the map still reads
`1` and the subtree total still reads `2` — while a `Retained`
settlement charges nothing, `attempts_on_rung` stays at zero across a
retained retry, and with `attempts_per = 2` the next rejection
derives `0 + 1 < 2` and retries the rung it should have escalated
off, indefinitely. Measured at `823ad36`: with that mutation applied
the census passes and so does the whole suite. A fourth stricter
needle would meet a fifth spelling; do not write one.

**So the two assertions below are narrowed to what a lexical count
establishes, and the property itself is asserted by value
elsewhere.** They enforce that each applier's body NAMES the charge
exactly once and that the fold's production region names it exactly
twice in total. That is worth keeping and is not the property: a
charge that leaves an applier's body is a `0` in the map whichever
helper it moved into, and a third naming anywhere in the subtree
moves the total off two. What they do not and cannot see, stated
rather than left to be inferred: the name bound to something else,
as above; a charge an applier reaches through an intermediate helper;
and any spelling that does not put `charge_allowance(` in the text.
**That escape is open here.**

**What closes it is
[`crate::topology::fold::tests::an_interrupted_attempt_refunds_the_rungs_allowance`],**
which drives `apply_settlement` over the settlement vocabulary and
`apply_candidate_prepared` beside it, and reads `attempts_on_rung`
off the state afterwards. It observes the charge rather than the
characters, so an alias, a closure of the same name, a
fully-qualified path and an intermediate helper are all one thing to
it — which is what a resolved check would have given and what
`clippy.toml`'s `disallowed-*` lists cannot: those DENY a resolved
call, and there is no resolved form of "this function must call that
one", so the property has no expression on that mechanism at all.
The mutation above fails that test naming the retained arm.

**The count here is per applier, because an aggregate is not that
mapping.** A total of two is green for every arrangement that sums to
two, so taking the success charge out of `apply_candidate_prepared`
and putting a second one into `apply_settlement` — failures charged
twice, successes not charged at all — left the total at two and this
census green. Measured, by planting exactly that. The table below is
keyed by applier and asserted whole, so the failure names the applier
that stopped naming the charge alongside the one that gained a
naming, rather than reporting a number that did not move.

**Every spelling, not one.** The needle was the literal
`self.charge_allowance`, and `Self::charge_allowance(self, …)` and a
fully-qualified `RunState::charge_allowance(self, …)` are the same
item to rustc while being invisible to it.
[`crate::effects::census_domain::production_calls`] is asked for both
call forms — `Call::Method` for the dotted receiver, `Call::Free` for
every path form — and their sum is what this counts. `SPELLINGS`
drives that sum, so the control travels through the census's own
counter rather than beside it.

**`SPELLINGS` is a string, so rustc never reads it.** It named
`TaskFold::charge_allowance` for a round — an item that does not
exist, the method being defined on `RunState` — and nothing caught it
because a fixture this census only ever counts over cannot be wrong in
a way the compiler reports. The path is `RunState`'s now, and
`crate::topology::fold::tests::CHARGE_ALLOWANCE` is that exact path
as a compiled `fn` item, so a rename or a move to another type stops
the build there instead of silently emptying a control here. It lives
in the fold's own test module because `charge_allowance` is
`pub(super)` within `topology::fold` and cannot be named from here.

**The fold is a directory, and one level of it is not the subtree.**
It was one file when this was written, and the read below was that
file. The domain is unchanged — the fold's production code — but it
is now the root plus everything beneath it, so reading `fold.rs`
alone would report **zero** charges — both calls this census counts
are in `apply.rs` — and a read of the root cannot see a charge that
lives in a child, nor one that moves between children.

**The walk is [`production_sources`], not a one-level `read_dir`.**
A `read_dir` of `src/topology/fold` claimed this whole domain while
reaching only its direct children, so a helper in
`fold/apply/debit.rs` charging a retained failure a second time left
this count at two and this census green — measured, by planting
exactly that. `CODING_STANDARDS.md` §12 states the rule it breaks: a
positive control inside a truncated domain does not prove that the
whole named domain was scanned. That is also why the mutation which
binds this walk is planted in a GRANDCHILD; the earlier confirmation
planted its third charge in `check_candidate.rs`, a direct child and
so inside the truncated boundary, and confirmed the census fired
without ever testing its depth.

The shared walk recurses, and it drops whole-file test modules
through `census_domain::whole_file_test_modules`, so `fold/tests.rs`
— and any deeper one a later split adds — leaves this domain by
derivation rather than by a `tests.rs` name check.

**The boundary is decided on the walk's own `PathBuf`, BEFORE any
conversion to text.** It was decided on the display form twice: first
`String::starts_with` against a hand-written `"src/topology/fold/"`
with `!rest.contains('/')` for "no deeper", then `Path::new` wrapped
around that same string. The second is the first with a type
annotation on it — `production_sources`'s key has been through
`to_string_lossy().replace('\\', "/")` by then, and nothing
downstream can undo a lossy conversion. On Unix a backslash is an
ordinary filename byte, so a sibling literally named
`src/topology/fold\decoy.rs` — one path component, outside the
directory — normalises into `src/topology/fold/decoy.rs` and is
classified inside the subtree, where a charge planted in it counts
toward the two below. [`production_sources_by_path`] hands out the
path the walk built, so `Path::starts_with` compares the components
the filesystem actually has: `foldx.rs` and `fold\decoy.rs` are both
outside by construction, and `parent()` names the direct children
without scanning for a separator.

## `fn the_rungs_allowance_is_counted_in_one_production_place()` › `let children = walked`

**The floor counts DIRECT children; only the charge count below
widens to the whole subtree.** Widening the scan must not widen the
control. Counting descendants here would let ten files under one
grandchild directory satisfy a floor whose sentence says "children",
and would hold this control green on a tree where every direct child
had gone — which the one-level walk this replaces would have caught.
It would also make the message below true only while no grandchild
exists, which is the same contingent shape as the truncation being
repaired.

## `fn the_rungs_allowance_is_counted_in_one_production_place()` › `assert!(`

The control: a walk that found nothing would count zero charges in a
tree that has two, and the assertions below would then be about an
empty region rather than about the appliers.

## `fn the_rungs_allowance_is_counted_in_one_production_place()` › `let charge_calls = |code: &str| {`

Both call forms, summed: `receiver.charge_allowance(…)` and every
path form of the same call.

## `fn the_rungs_allowance_is_counted_in_one_production_place()` › `const SPELLINGS: &str = "\`

The control, through the counter the census itself uses: the four
spellings that reach the same item, the definition line, and two
near-names that are different items. The literal needle this
replaces scores this body **1**.

## `fn the_rungs_allowance_is_counted_in_one_production_place()` › `assert_eq!(`

And nowhere else in the subtree. The map above is per applier and
says nothing about a third naming — a helper charging a retained
failure a second time is outside both bodies and inside this count.

## `mod tests` › `fn each_census_needle_covers_the_domain_its_doc_states() {`

**Each census needle covers the domain its doc states.**

The class boundary for three findings of one shape, all measured
surviving in S5 round 4: a census whose **stated** domain is wider than
its **counted** domain fails open, and does so in the passing direction,
so nothing ever reports it. `PR7-R3-CENSUS-WRITE-DOMAIN-PROSE` was the
first instance and was repaired by narrowing the prose; these are the
same defect at three more needles, repaired by widening the needle,
because in each case the wider domain is the one the census is for.

Unit assertions over the needles themselves, deliberately: the censuses
that use them are whole-tree and green, and a green whole-tree census is
exactly what a fail-open needle produces.

## `fn each_census_needle_covers_the_domain_its_doc_states()` › `assert_eq!(receiver_writes("state.attempts = 1;", "attempts"), 1);`

`receiver_writes`: every assignment form is a write, `==` is not, and
a longer field name is not this field.

## `fn each_census_needle_covers_the_domain_its_doc_states()` › `let field = stem_values("Inputs { stem: format!(\"{i:02}-{}\", display_id), n: 1 }");`

`stem_values`: a field initializer's value runs to the end of the
statement rather than to the first comma...

## `fn each_census_needle_covers_the_domain_its_doc_states()` › `let binding =`

...and a `let` binding is a stem site too, which is the shape the one
production site on the live legacy path uses.

## `fn each_census_needle_covers_the_domain_its_doc_states()` › `assert!(`

A different identifier that merely ends in `stem` is not one.

## `mod tests` › `fn a_plan_authored_id_never_reaches_a_filename_unsanitised() {`

**A plan-authored id never reaches a filename unsanitised.**

The sixth single-authority census, and the one with the sharpest
consequence. A task's `display_id` is whatever an `id=` annotation said —
`plan/markdown.rs`'s `assemble` takes `Some(explicit) => explicit`
verbatim, and `keys_by_display_id` checks only the reserved `repair-N-`
prefix and duplicates. It then becomes a `stem`, and a stem becomes
`dir.join(format!("{stem}.json"))` in every adapter's
`materialize_permissions`.

`PR7-R3-ATTEMPT-001`: the schema-4 assembler took `display_id` **raw**
while `coordinator.rs:537`, the legacy authority it was extracted from,
wrote `format!("{index:02}-{}", util::filename_component(task.id.as_str()))`.
So `id=../../x` wrote outside the run directory. The extraction dropped a
**guard**, which is the one-rule-two-places class at its worst — a
dropped convenience is a bug, a dropped guard is a vulnerability.

The census is on the **pairing**: wherever a `stem` is built from a
plan-authored id, `filename_component` appears in the same expression.

**It is also on a count, and the two say different things.** This
paragraph used to end "that is what makes it survive a third assembler
being written, which a count could not" — and the control below is now
`assert_eq!(guarded + unguarded, 4)`, so a fourth *correctly guarded*
assembler fails this test. That is deliberate (a new site has to be read
once by a person, and a needle that quietly stops matching reads exactly
like a clean tree) but it is the opposite of what the sentence promised.
`R5-SETTLE-004`.

**What the pairing cannot see, stated rather than left to be found**: a
site that rebinds the id one line earlier — `let id = task.id.to_string();`
and then a stem built from `id` — is not a site at all, so it never
enters either list and the count still reads 4. `coordinator.rs` already
carries a `let task_id = …` seven lines above the stem it builds, so the
shape is not hypothetical. Reaching it needs data flow, not a needle.
`R5-SETTLE-006`.

`util::filename_component_neutralizes_hostile_names` is the other half —
it asserts `"unit/fast"` becomes `"unit-fast"` and that an all-dots
result becomes `"x"`, so `..` is neutralised. This census says the guard
is *reached*; that test says it *works*.

## `fn a_plan_authored_id_never_reaches_a_filename_unsanitised()` › `const PLAN_AUTHORED: &[&str] = &["display_id", "task.id"];`

How this project spells a **plan-authored** task identifier. Both,
because both engines are live: schema 4 froze the annotation onto
`TaskEntry::display_id`, and the legacy coordinator — the path
`upstroke run` still drives — reads `task.id`. The census used to
name only the first, so the site the second one owns, which is the
site the extraction was copied *from*, was outside its domain.

## `fn a_plan_authored_id_never_reaches_a_filename_unsanitised()` › `const SELF_CHECK: &str = "<self-check>";`

**The control is a corpus entry**, for the reason the allowance census
above gives: a unit assertion on `stem_values` proves the helper and
says nothing about whether this census walks the tree with it. This
synthetic file is a guarded site of the exact shape the live legacy
path uses — a `let` binding whose value runs past a comma — so a
reader that matches only `stem:` initializers, or that truncates the
value at the first comma, loses it and the site count below is 3
rather than 4.

## `fn a_plan_authored_id_never_reaches_a_filename_unsanitised()` › `assert_eq!(`

The control comes first and counts **sites**, not guarded ones: a
needle that stops matching must not read as a clean tree, and a site
that loses its guard must be reported as unguarded rather than
disappearing out of the count.

## `mod tests` › `fn a_reviewers_profile_is_accounted_for_at_both_callers() {`

**Every field of a reviewer's `WorkerProfile` is accounted for at both
callers.**

The seventh single-authority census, and the one that closes the class
`PR7-R3-ATTEMPT-001` opened: **the extraction dropped something the
legacy caller supplied.** Three instances now — the sanitiser on a task
id (a **guard**), the reviewer's pool and the retry's pool (**values**) —
found one at a time by three different reviewers.

### The field list comes from the type

PR4's rule, and the reason this census cannot sprawl: the roll is
`crate::ir::WorkerProfile`'s own fields, not a list somebody thought of.
Adding a field to that struct fails this test until the new field is
given a cell, which is the property a census of "did we forget anything"
has to have to mean anything.

### Three cells, and every field has exactly one

- **Identical** — both callers supply the same value by the same route.
- **Differs, cited** — the callers legitimately disagree, and the cell
  carries the §-citation for why. `pool` is the model: §11.3/§13 make a
  cross-vendor second opinion draw on its own subscription, so it is
  looked up from the reviewer's agent rather than inherited.
- **Absent, cited** — neither caller sets it beyond the constructor's
  default, and the cell says what supplies it instead.

A cell is prose and this test cannot check prose. What it checks is that
**the roll is complete** — that no field of the type is missing a cell —
which is exactly the failure all three instances share: nobody had
enumerated what the legacy caller supplies.

## `fn a_reviewers_profile_is_accounted_for_at_both_callers()` › `const ROLL: &[(&str, &str)] = &[`

(field, cell).

## `fn a_reviewers_profile_is_accounted_for_at_both_callers()` › `let ir = std::fs::read_to_string(`

The roll is checked against the TYPE, not against itself.

## `fn a_reviewers_profile_is_accounted_for_at_both_callers()` › `let open = start + ir[start..].find('{').expect("an opening brace") + 1;`

Past the declaration line: `pub struct WorkerProfile {` itself starts
with `pub ` and would otherwise parse as a field.

## `fn every_production_command_spec_payload_is_classified()` › `const EXPECTED: &[(&str, usize, usize, usize, &str)] = &[`

(file, `.stdin(`, `.env(`, struct-literal `env:`/`stdin:`, and what
they are).

## `fn every_production_command_spec_payload_is_classified()` › `code.lines()`

Struct-literal initializers of the same two fields. Anchored
at the start of a line so `.env(` chains and doc prose cannot
contribute, and counted separately so a row says which kind
of population it is.

## `fn every_production_command_spec_payload_is_classified()` › `let expected: BTreeMap<String, (usize, usize, usize)> = EXPECTED`

`PR4-CENSUS-COMMENT-ORACLE`'s class — a census over a file format that
has comments. The control that the blanking removed something is in
`production_sources`, which every census here shares, rather than
repeated four times: it asserts the regions hold strictly fewer
non-whitespace bytes than the sources they came from, and a floor
beneath which the counts would be over nothing.

## `mod tests` › `fn a_command_specs_payload_does_not_depend_on_its_arguments() {`

The two spec constructors' payload is a function of nothing.

DESIGN.md:262-264: "Probe and execution compose the **same** base,
mounts, reserved values, and overlay, so pre-flight certifies the
environment that will actually spend." A probe and a work command differ
in exactly one thing — their **arguments** — so an overlay that varies
with the arguments is an overlay that differs between pre-flight and
spend, and `PR5-FIDELITY-001` is that edit at `bin::Invocation::spec`.

The census above says a site *exists*; this says what it produces. Both
are needed and neither implies the other: a census cannot tell
`Vec::new()` from a conditional, and a fixture that built one spec
cannot tell a constant from a function of its input.

The argument vectors are production's own — every adapter's `--version`
probe, every adapter's `build_args` fresh and resumed, Codex's six
strict-config parser probes' shape, and the gate/shell dialects — so
this is a statement about the values production actually passes and not
about invented ones.

## `fn a_command_specs_payload_does_not_depend_on_its_arguments…` › `let invocation = Invocation::at(if cfg!(windows) {`

(a) `bin::Invocation::spec`, the agent-CLI constructor.

## `fn a_command_specs_payload_does_not_depend_on_its_arguments…` › `type Payload = (Vec<(String, String)>, Vec<u8>);`

One spec's payload: its overlay and its stdin.

## `fn a_command_specs_payload_does_not_depend_on_its_arguments…` › `let mut shell_payloads: Vec<Payload> = Vec::new();`

(b) `gates::ShellKind::spec`, the other one. Every dialect, because
the shell is a field of the record and not a constant.

## `fn harness_hooks_consult_every_mode_a_point_declares()` › `assert!(harness.reached_point(`

AmbientJobJoined declares both modes; CreatedSuspended declares kill
only. The expected pairs come from `containment_sub_effects` ("failure
refuses the write command" for the ambient join alone), not from
`SubEffectPoint::modes`.
