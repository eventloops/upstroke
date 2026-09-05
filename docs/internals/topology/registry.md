# `src/topology/registry.rs`

Extended notes for [`src/topology/registry.rs`](../../../src/topology/registry.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The task registry: what a schema-4 run stores a task *as*.

**INV-04 — [`TaskKey`] is the only storage identity, and display ids are
validated projections.** A key is a dense index assigned at registration;
the id a person types, a plan writes, and a report prints is a field on the
entry that key names. Every later slice — the checked fold, the candidate
queue, the merge queue, repair lineage — addresses tasks by key, so the one
place a display id is interpreted is here, where it is checked.

That inversion is what the merge queue needs. A run that spawns repair tasks
has ids nobody wrote down in the plan, and a design keyed on the display
string has to answer awkward questions about what happens when two of them
collide, or when a plan happens to contain the id the queue was about to
generate. Keyed on a dense index, both answers are structural: a display id
names exactly one entry or the registry refuses to exist, and the id space
the queue generates into is reserved against originals up front.

### What an original entry is derived from

Originals come from two frozen inputs and nothing else: the run's
`plan.normalized.json` and its [`RunStarted`] record — the resolved chains
with their exact rung bindings, the review plan, and the effort policy.
Both are already immutable for the life of the run, which is what lets a
resume, a replay, and a fresh reader rebuild the identical registry rather
than three that agree by inspection.

[`TaskRegistry::digest`] is the authentication value over that derivation.
A reader rebuilds the originals and compares; a mismatch means the frozen
plan or the run record moved underneath the log and is refused rather than
folded on a guess. The digest is consumed from the schema-4 fold onwards —
this slice establishes it and proves it deterministic.

Dynamic (merge-repair) entries are the merge queue's, and it does not exist
yet: this module reserves their id namespace and carries their shape, and
nothing in production registers one.

## `pub struct TaskKey(pub u32);`

Storage identity for one task in one run: dense from 0, assigned in plan
order for originals and equal to the registry's length at the event that
registers a dynamic task.

Deliberately not the display id. See the module documentation for why.

## `impl TaskKey` › `pub fn index(self) -> usize {`

Position of this key's entry in the dense registry.

## `pub enum Origin {`

Where an entry came from.

## `pub enum Origin` › `Original,`

Written in the plan the run froze.

## `pub enum Origin` › `MergeRepair,`

Registered by a merge rejection. No production producer yet — the merge
queue lands in a later slice; the variant is here because it is part of
what an entry *is*, and therefore part of what the digest covers.

## `impl Origin` › `fn tag(self) -> &'static str {`

The token this origin contributes to the canonical serialization.

## `pub struct Lineage {`

A repair's place in the lineage it belongs to. `None` on an original.

## `pub struct Lineage` › `pub root: TaskKey,`

The original task this lineage descends from.

## `pub struct Lineage` › `pub parent: TaskKey,`

The entry whose rejection produced this one.

## `pub struct Lineage` › `pub index: u32,`

Run-local monotonic index within the lineage, and the number that
appears in the repair's display id.

## `pub struct FrozenRung {`

One rung's frozen execution identity, exactly as the run resolved it.

## `pub enum Admission {`

Whether an entry may be dispatched, or is waiting for a human to name a
binding for it.

## `pub enum Admission` › `Runnable,`

The frozen ladder has rungs; the scheduler may dispatch it.

## `pub enum Admission` › `HumanBinding { options: Vec<String> },`

The ladder clipped to nothing, so there is no binding to run and the
entry cannot move until an answer records an explicit one-off binding.

Reachable only for a repair whose floor and its root's ceiling do not
intersect, which is the merge queue's business and has no producer here.
An original's ladder is whatever its run resolved, and a run that
resolved nothing is refused at construction instead.

## `impl Admission` › `fn tag(&self) -> &'static str {`

The token this admission contributes to the canonical serialization.

## `pub struct FrozenLadder {`

The escalation ladder frozen for one entry.

## `pub struct FrozenLadder` › `pub tiers: Vec<Tier>,`

The resolved tiers, in escalation order.

## `pub struct FrozenLadder` › `pub attempts_per: u32,`

Attempts allowed on each rung before escalating (§10.1).

## `pub struct FrozenLadder` › `pub rungs: Vec<FrozenRung>,`

Each rung's exact binding, aligned with `tiers`. Empty only for an
entry admitted as [`Admission::HumanBinding`].

## `pub struct FrozenLadder` › `pub floor: Option<Tier>,`

The task's binding `min=` clip, or `None` where it set no floor. This is
what a repair spawned from this entry intersects its own floor with.

## `pub struct FrozenLadder` › `pub ceiling: Option<Tier>,`

The highest tier this ladder reaches — the policy ceiling a repair
descended from this entry may not exceed. `None` on an empty ladder.

## `pub struct FrozenLadder` › `pub effort: ResolvedEffortPolicy,`

The run's resolved effort standard. Carried per entry rather than
referenced, because a dynamic entry is embedded whole in the event that
registers it and has to be readable without the run header beside it.

## `pub struct FrozenTaskSpec {`

Everything about a task that is not its identity, its dependencies, or how
it is run — frozen at registration.

## `pub struct FrozenReviews {`

The review identity frozen for one entry.

The run-level record ([`crate::review::ReviewPlan`]) resolved these; they
are copied onto the entry for the same reason the effort policy is, and
they are the inputs [`crate::review::ReviewPlan::passes_for`] consumes at
attempt time. Which of them actually runs still depends on the rung the
implementer bound to, so that choice stays where it was.

## `pub struct FrozenReviews` › `pub enabled: bool,`

Whether verification was deliberately enabled when the run froze.

## `pub struct FrozenReviews` › `pub alternative_available: bool,`

Whether the run deliberately retained an anti-self-review alternative.

## `pub struct FrozenReviews` › `pub pass_timeout_secs: u64,`

The independent per-pass wall-clock allowance.

## `pub struct FrozenReviews` › `pub second_opinion: Option<PassBinding>,`

This task's §11.3 second opinion, where its paths asked for one.

## `impl FrozenReviews` › `pub fn bindings(&self) -> Option<crate::review::ReviewBindings<'_>> {`

The three bindings pass selection reads, or `None` when this run froze
verification off.

**The one place `enabled` gates the plan.** It was read at the plan
assembler and nowhere else, so anything else that wanted to know what
this entry's review standard *is* had to remember the flag on its own.
Two readers of one rule is how the two disagree, and the disagreement
this closes is a live one: the fold judges a candidate's success against
the obligation and the assembler dispatches the passes that discharge
it.

The flag is load-bearing here even though `plan_for`'s disabled branch
resolves no `primary` either — `enabled: false` beside a resolved
reviewer is a shape the *wire* admits, and a fold reads logs rather than
configurations.

## `impl FrozenReviews` › `pub fn obliged_lenses(&self) -> Vec<crate::review::Lens> {`

The lenses this entry obliges, in order — `[]` when it obliges none.

What a successful attempt's record has to carry, and the reason it can
be asked without the implementer's binding is
[`crate::review::obliged_lenses`]'s.

## `pub struct TaskEntry {`

One registered task.

## `pub struct TaskEntry` › `pub display_id: TaskId,`

The id a plan wrote or the merge queue generated. Display only: it is
validated on the way in and projected on the way out, and nothing
stores a relationship by it.

## `pub struct TaskEntry` › `pub deps: Vec<TaskKey>,`

Dependencies as keys — what readiness is actually computed over.

## `pub struct TaskEntry` › `pub display_deps: Vec<TaskId>,`

The same dependencies as the plan wrote them, kept so the legacy
projection is a copy rather than a reconstruction.

## `pub struct TaskEntry` › `pub allowed_agents: Vec<String>,`

The agents this run's pre-flight actually probed — the allow-list every
binding on this entry is drawn from, including one a human names for a
repair whose ladder clipped to nothing.

Recorded per entry rather than referenced from the run header for the
same reason the effort policy is: a dynamic entry is embedded whole in
the event that registers it and has to be readable without the header
beside it. Kept in the order `run_started` recorded, because that record
is frozen and this value is part of what the digest authenticates.

## `impl TaskEntry` › `pub fn legacy_task(&self) -> Task {`

This entry projected back to the [`Task`] shape schemas 1–3 read.

Lossless by construction: everything a `Task` holds is either the
display id, the display dependencies, or a field of the spec. That is
what makes the registry a re-encoding of the frozen plan rather than a
summary of it, and it is the property the projection-parity tests check
by comparing serialized bytes.

## `pub struct TaskRegistry {`

Every task in one run, addressed by [`TaskKey`].

## `pub struct TaskRegistry` › `originals: usize,`

How many leading entries came from the frozen plan.

The boundary [`Self::digest`] is defined over. Everything after it was
registered by an event, which carries it complete and is its own
authority.

## `pub enum RegistryError {`

Why a registry could not be derived, or could not be trusted.

## `const REPAIR_PREFIX: &str = "merge-fix-";`

The id namespace merge repairs are generated into.

## `const REPAIR_INDEX_WIDTH: usize = 4;`

Zero-padded width of a repair's lineage index
(`decisions/2026-08-12-merge-queue-execution-topology.md`: `merge-fix-0001-<task>`).

## `pub fn repair_display_id(lineage_index: u32, root: &TaskId) -> String {`

The display id a merge repair takes.

The one place the pattern is written. [`is_reserved_display_id`] refuses
everything this can produce, so the generator and the refusal cannot drift
into disagreeing about what the reserved namespace is.

## `pub fn is_reserved_display_id(id: &str) -> bool {`

Whether a display id falls inside the reserved repair namespace.

Deliberately a superset of what [`repair_display_id`] emits: the index is
matched at four digits *or more* so a run that ever exceeds 9999 repairs
cannot generate an id a plan was allowed to take, and the prefix is matched
without regard to ASCII case so `MERGE-FIX-0001-x` cannot be smuggled past
it. Reserving more than is generated costs a plan author a hyphenated id
nobody writes; reserving less costs a collision between a plan's task and a
repair, which is the thing a storage identity exists to make impossible.

## `impl TaskRegistry` › `pub fn originals(plan: &Plan, started: &RunStarted) -> Result<Self, RegistryError> {`

Derive the original entries: the frozen plan's tasks, in plan order,
against the chains, review plan, and effort policy the run recorded.

Derive the original entries from a run record that names no probed
agents, giving every entry an empty allow-list.

A legacy [`RunStarted`] has no probed-agent record — the field is
schema 4's — so this is the whole of what such a record supports. **A
schema-4 derivation must use [`Self::originals_with_agents`]**: the
allow-list is a digest input, so originals rebuilt through this
constructor authenticate against a schema-4 log only if that log probed
nothing, which no run does.

### Errors

Every [`RegistryError`] [`Self::originals_with_agents`] produces.

## `impl TaskRegistry` › `pub fn originals_with_agents(`

Derive the original entries: the frozen plan's tasks, in plan order,
against the chains, review plan, and effort policy the run recorded,
with the agents its pre-flight probed.

Every refusal here is a statement that the two inputs do not describe
the same run. That matters more than it looks: this is the construction
a reader repeats to check [`Self::digest`], so an input pair it accepted
loosely would authenticate a registry nothing else agrees with.

### Errors

A [`RegistryError`] naming the first way the plan and the run record
disagree about the run they describe.

## `impl TaskRegistry` › `if enabled && reviews.second_opinion.len() != plan.tasks.len() {`

Aligned by index, exactly as the review plan is (its own record
refuses a misalignment when it is written). Checked again rather than
assumed: a registry rebuilt on replay is rebuilt from the file, and
the file is the only thing it may take as given.

## `impl TaskRegistry` › `pub fn register(&mut self, entry: TaskEntry) {`

Add an entry a schema-4 event registered.

Infallible on purpose: whether this key is the next dense index, whether
its display id is free, and whether its ladder is one an attempt could
climb are all decided by the checked fold *before* it applies the event
that registers the entry. Repeating those checks here would put a second
authority on the same question, and the one place a dynamic entry can be
refused is the transition that introduces it.

Does not move [`Self::digest`]: see that method for why.

## `impl TaskRegistry` › `pub fn key_of(&self, display_id: &str) -> Option<TaskKey> {`

The key a display id names, or `None` if this run has no such task.

## `impl TaskRegistry` › `pub fn legacy_tasks(&self) -> Vec<Task> {`

Every entry projected back to the [`Task`] shape schemas 1–3 read, in
key order — which for originals is plan order.

This is the legacy projection: a `Plan` rebuilt around it serializes to
the same bytes as the one the registry was derived from, so a status, a
report, and an export taken through the registry are the same bytes as
one taken through the plan.

## `impl TaskRegistry` › `pub fn originals_len(&self) -> usize {`

How many entries came from the frozen plan.

## `impl TaskRegistry` › `pub fn digest(&self) -> String {`

The authentication value over this registry's **original** entries.

`sha256:<hex>` of the canonical encoding over the originals alone, in
the `sha256:<hex>` shape the normalized plan's digest uses so a log
carries one shape of digest rather than two.

Deliberately not a digest of everything registered. A reader
authenticates a registry by rebuilding the originals from
`plan.normalized.json` and `run_started` and comparing; a dynamic entry
has no frozen input behind it to rebuild *from*, and is authenticated
instead by arriving complete inside the event that registers it. A
digest that widened as repairs were registered would be a value no
reader could ever recompute, and it would do so silently.

This is the half of the pair that is *narrow* on purpose.
[`Self::canonical_bytes`] is the whole registry; the two are the same
bytes exactly when nothing dynamic has been registered.

## `impl TaskRegistry` › `pub fn verify_digest(&self, recorded: &str) -> Result<(), RegistryError> {`

Refuse a registry that does not match a recorded digest.

## `impl TaskRegistry` › `pub fn canonical_bytes(&self) -> Vec<u8> {`

The exact bytes [`Self::digest`] hashes.

**Frozen.** The field order below, and the set of fields in it, are part
of what a recorded digest means; changing either re-dates every digest
ever recorded. A new field goes in at the end behind a new version tag,
never in the middle.

`allowed_agents` is the one field that arrived after the encoding was
written and did *not* take a new tag. It is not an extension: it is part
of what an entry has always been (`decisions.task_registry.task_entry`),
deferred by one slice on the explicit ruling that no digest is recorded
in between. Nothing has ever written a `upstroke.registry.v1` value
without it, so there is no reader for which the two versions differ, and
a second tag would claim a compatibility history this format does not
have. The next field to arrive will be a real extension and takes v2.

Every value is written length-prefixed as `<byte length>:<bytes>;`, so
the encoding is injective — two registries that differ anywhere produce
different bytes, and no arrangement of one entry's text can imitate
another's. Nothing here is a float, a hash-map iteration, or a
locale-dependent rendering, which is what makes the value identical in
another process on another platform.

## `impl TaskRegistry` › `fn encode(&self, entries: usize) -> Vec<u8> {`

The canonical encoding of this registry's first `entries` entries.

One encoder, two readers: [`Self::digest`] takes the originals and
[`Self::canonical_bytes`] takes everything. Writing it once is what
makes "the digest is the whole-registry encoding when nothing dynamic
exists" a fact about the code rather than a coincidence between two
copies of a format — and a dynamic entry that no encoder ever visited
would be a value nothing downstream could compare.

The count is part of the encoding, so a prefix of a longer registry is
never the encoding of a shorter one.

## `fn keys_by_display_id(plan: &Plan) -> Result<BTreeMap<String, TaskKey>, RegistryError> {`

Dense keys by display id, refusing a duplicate or a reserved id on the way.

## `fn chains_by_task<'a>(`

The recorded chains, indexed by the task each names.

Matched by display id rather than by position: the run writes them in plan
order, but a registry rebuilt from a file has no standing to assume the file
still says so. The coverage has to be exact in both directions — a chain for
a task the plan does not have, or a task with no chain, means the plan and
the record are not describing one run.

## `fn frozen_ladder(`

One task's ladder, frozen from the chain its run recorded.

## `admission: Admission::Runnable,`

An original is admitted by the ladder its run resolved, and an empty
one was refused above. The human-gated admission belongs to a repair
whose clip emptied its ladder, which nothing here produces.

## `fn field(out: &mut Vec<u8>, value: &str) {`

---------------------------------------------------------------------------
Canonical serialization
---------------------------------------------------------------------------

## `fn field(out: &mut Vec<u8>, value: &str) {`

One length-prefixed value: `<byte length>:<bytes>;`.

## `fn encode_entry(out: &mut Vec<u8>, entry: &TaskEntry)` › `strings(out, entry.allowed_agents.iter());`

In the order `run_started` recorded, not sorted: the record is frozen,
and two runs that probed the same agents in different orders resolved
their bindings against different lists.

## `mod tests` › `type BreakRecord = fn(&mut RunStarted);`

One way to damage a run record, for the refusal tables.

## `mod tests` › `type MoveInput = fn(&mut Plan, &mut RunStarted, &mut Vec<String>);`

One way to move a single digest input, for the coverage table.

The probed agents are a third input rather than a field of the run
record: a legacy [`RunStarted`] has no place to record them, and they
are a digest input all the same.

## `mod tests` › `type MoveField = fn(&mut TaskEntry);`

One way to move a single field of one already-built entry.

## `mod tests` › `type PermuteTask = fn(&mut Task);`

One way to permute a list one task wrote in a deliberate order.

## `mod tests` › `fn sample_plan() -> Plan {`

Plan order, display-id order, and topological order all disagree here,
so a projection that quietly used one where it meant another shows up
rather than passing by coincidence.

## `mod tests` › `fn sample_effort() -> ResolvedEffortPolicy {`

The effort standard the sample record freezes. Written once, so a
fixture that expects it cannot drift from the record that carries it.

## `fn review_plan(tasks: usize) -> ReviewPlan` › `second_opinion: (0..tasks)`

Only some tasks ask for one, so a slot read at the wrong index
lands on a different answer.

## `mod tests` › `fn varied_chain(task: &str) -> ChainSummary {`

A ladder that belongs to one task and to no other.

Every component the registry freezes — the tier list, the attempts
allowance, and each rung's agent, model and pin — is derived from the
task's own id. Reading the wrong task's chain therefore yields a wrong
*value*, where [`chain`] yields the same value for every task and so
cannot tell a keyed lookup from a positional one.

## `mod tests` › `fn unordered_chain(task: &str) -> ChainSummary {`

A chain that records its rungs in an order that is neither ascending nor
descending, so the derived `ceiling` is neither end of the list.

Every other fixture records an ascending ladder, because that is what an
escalation ladder is — and while the tiers ascend, the ceiling is the
list's maximum *and* its last element at once. A ceiling read off the end
of the list rather than taken over the whole of it is invisible
everywhere else, and so is one read off the front of a descending list.
Nothing validates the recorded order, so a record can put the top rung in
the middle, and there all three derivations disagree: the maximum is
`frontier`, the first is `mid`, and the last is `small`.

## `mod tests` › `fn varied_plan() -> Plan {`

The sample plan with a different binding floor on each task, so the
derived `floor` distinguishes an entry as well as its chain does.

## `mod tests` › `fn varied_started_for(plan: &Plan) -> RunStarted {`

The varied plan's run record — chains and review slots that name the
task they belong to, written in an order the plan does not share.

Plan order is `zeta, alpha, mid`; the record writes `alpha, mid, zeta`.
That is a derangement, so no task's chain sits at its own index and the
chain at index 0 is not the first task's: a positional read and a
first-chain read each land on a ladder that says whose it really is.

## `fn varied_started_for(plan: &Plan) -> RunStarted` › `second_opinion: vec![`

Distinct models rather than the sample's palindromic
`[None, Some, None]`, which a reversed read reproduces exactly —
and a distinct *agent* on each occupied slot for the same reason
one level down. The sample's occupied slot holds `copilot`, which
is also `alternative`'s agent, so while every occupied slot said
`copilot` the model's source was discriminated and the agent's
was not: an agent copied from the alternative reviewer, or
hard-coded to that literal, produced the fixture's own answer.
Each component of each slot now names the task it belongs to, and
[`the_fixtures_give_every_binding_component_its_own_literal`]
holds the whole set apart.

## `mod tests` › `fn expected_ladder(plan: &Plan, task: &str) -> FrozenLadder {`

The ladder the varied fixture declares for one task, restated from the
fixture's own inputs rather than read back off the registry under test.

## `mod tests` › `fn projection_plan() -> Plan {`

The sample plan plus two tasks the fixture log never reaches.

A report lists the tasks that ran in the order they ran and appends the
rest in **plan order**, so the untouched tail has to be longer than one
task for a reordered projection to be visible at all. With a single
untouched task the comparison passes under a registry that sorts its
entries by display id, which is exactly the defect it is here to catch.

## `mod tests` › `fn dependency_order_plan() -> Plan {`

A plan whose one wide dependency list is written in an order that is
neither its sorted order nor the reverse of it.

[`sample_plan`]'s widest list is `alpha, zeta`: two entries, already in
lexicographic order. A registry that sorted `display_deps` on the way
in reproduces it exactly, and one that reversed them is indistinguishable
from one that sorted them descending — two entries cannot tell those
apart. Four entries in a scrambled order can, and plan order is chosen
so the resolved keys `0, 2, 1, 3` are equally unordered: a sort of
*either* representation is a list this fixture does not contain.

## `mod tests` › `fn artifact_order_plan() -> Plan {`

The wide-dependency plan, with artifact lists wide enough to have an
order and written in one that is neither sorted nor reversed.

[`task`] gives every task exactly one artifact on each side, and the
parsed fixture plans carry no task with two of either. A one-entry list
cannot tell a registry that kept its order from one that sorted it, so
without this fixture both artifact lists are named by tests that no
ordering defect could fail.

## `mod tests` › `fn sample_agents() -> Vec<String> {`

The agents the sample run's pre-flight probed.

Not the agents the chains bind, and not a sorted list. A real run's
probe finds every configured CLI, of which the ladder binds some; a
fixture whose allow-list happened to be the set of bound agents would be
reproduced exactly by an encoder that derived the allow-list from the
rungs instead of reading the record. `copilot` is here and is bound by
nothing; `claude-code` is bound and is here; the padded, multi-byte and
over-length entries are here and are bound by nothing.

The order is the record's: neither ascending nor descending by bytes
(`"  Codex-CLI  "` sorts first, the `a`-run second, `claude-code` third,
`copilot` fourth, `ÜBER…` last), so an encoder that sorted or reversed
the list writes bytes this fixture does not contain.

## `mod tests` › `fn originals_of(plan: &Plan, started: &RunStarted) -> Result<TaskRegistry, RegistryError>…`

The derivation every fixture here goes through, with the sample run's
probed agents.

## `mod tests` › `fn keys_are_dense_and_assigned_in_plan_order() {`

-----------------------------------------------------------------------
INV-04: the key is the identity, the display id is a projection
-----------------------------------------------------------------------

## `fn dependencies_are_stored_as_keys_and_projected_as_written…` › `("mid", vec![1, 0], vec!["alpha", "zeta"]),`

Written `alpha, zeta`, which is keys 1 then 0: resolved by id
and kept in the order the plan wrote them, never sorted and
never taken from the dependent's own position.

## `fn dependencies_are_stored_as_keys_and_projected_as_written…` › `let plan = dependency_order_plan();`

The keys above are `1, 0` and so cannot be a sorted list, but the
display side of the same task is `alpha, zeta` — already sorted, and
therefore silent about whether the plan's order was kept or merely
reproduced by accident. The wide fixture says which, and says it of
each representation on its own rather than of the pair moving
together.

## `fn dependencies_are_stored_as_keys_and_projected_as_written…` › `for (what, ordered) in [`

The fixture has to be able to see a sort before either assertion
above means anything: a list that was already ordered would satisfy
both a registry that kept it and one that ordered it.

## `fn dependencies_are_stored_as_keys_and_projected_as_written…` › `for (position, (key, display)) in omega.deps.iter().zip(&omega.display_deps).enumerate() {`

Each representation names the same dependency at the same position:
that is the pairing the two lists claim, and it is what makes a sort
of one alone a contradiction rather than a difference of opinion.

## `fn dependencies_are_stored_as_keys_and_projected_as_written…` › `assert_eq!(`

And the projection is the written order too — the whole point of
keeping a second copy is that a legacy reader sees what the plan
said.

## `fn artifact_lists_keep_the_order_the_plan_wrote_them_in()` › `let plan = artifact_order_plan();`

The same defect as the dependency lists, in the two remaining fields
whose order the encoder writes: every other fixture holds exactly one
artifact on each side, so a registry that sorted either list is
indistinguishable from one that copied it.

## `fn artifact_lists_keep_the_order_the_plan_wrote_them_in()` › `for (what, list) in [`

Neither list may already be in an order a sort would produce, or the
assertions above hold just as well for a registry that sorted it.

## `fn artifact_lists_keep_the_order_the_plan_wrote_them_in()` › `assert_eq!(`

The projection carries the written order back out to a legacy reader,

## `fn artifact_lists_keep_the_order_the_plan_wrote_them_in()` › `let baseline = registry.canonical_bytes();`

and the digest authenticates it: were a permutation to encode alike,
one recorded digest would accept both records.

## `fn the_frozen_ladder_is_the_chain_the_run_recorded()` › `for entry in registry.entries() {`

Every entry rather than the first. The sample records one chain shape
for all three tasks, so a check of entry 0 alone says nothing about
whether the other two were given a ladder at all — which is why the
association itself is proved on the varied fixture below rather than
here.

## `fn each_entry_takes_the_chain_recorded_for_its_own_display_…` › `let plan = varied_plan();`

The scope requirement: an original's attempts, agent, model and pin
come from *its own* `run_started` chain. The sample fixture cannot
witness that — one chain shape repeated makes a keyed lookup, a
positional lookup and a first-chain lookup indistinguishable. Here
the three ladders differ in every component and the record writes
them in a derangement of plan order, so each wrong lookup produces a
wrong ladder for at least one task.

## `fn each_entry_takes_the_chain_recorded_for_its_own_display_…` › `let ladders: Vec<&FrozenLadder> = registry`

The fixture has to discriminate before anything below means
something: two equal ladders would satisfy any lookup at all.

## `fn each_entry_takes_the_chain_recorded_for_its_own_display_…` › `for entry in registry.entries() {`

Every entry's complete ladder, addressed by the display id whose
chain it had to have taken — and reachable by its key, which is the
identity everything after this slice addresses it by.

## `fn each_entry_takes_the_chain_recorded_for_its_own_display_…` › `let first_chain = started.chains[0].task.as_str();`

And the two substitutions named explicitly, so a regression to either
fails here instead of passing on a fixture that cannot see it.

## `fn the_ladder_ceiling_is_the_highest_tier_recorded_not_an_e…` › `let plan = plan_of(vec![task("alpha", &[])]);`

`ceiling` is the maximum of the recorded tiers, and every other
fixture records them ascending — which is what an escalation ladder
is, and which makes the maximum, the last element and (for the
one-rung chain) the first element the same value. A ceiling taken
from an end of the list rather than over the whole of it therefore
produces the expected answer everywhere else in this module.

## `fn the_ladder_ceiling_is_the_highest_tier_recorded_not_an_e…` › `assert_eq!(`

The fixture has to be able to see the difference before the assertion
below means anything.

## `fn the_ladder_ceiling_is_the_highest_tier_recorded_not_an_e…` › `assert_eq!(`

And the floor is the task's own `min=`, which does not follow the
recorded order at all: here it is the tier the list ends on.

## `fn frozen_reviews_take_each_task_s_own_second_opinion_slot()` › `let plan = varied_plan();`

The sample's slot pattern is a palindrome, and its one occupied slot
holds the same binding as `alternative`. A read that walked the slots
backwards therefore reproduces it exactly, and so does one that
copied the alternative reviewer into every entry. The varied fixture
gives each occupied slot an agent *and* a model naming the task it
belongs to, which neither substitution can imitate.

Both components are read. While only the model was, the agent's
source was free: every occupied fixture slot said `copilot`, so an
agent taken from `alternative` — which says `copilot` too — or
hard-coded to that literal produced the expected answer, and the
slot's own recorded agent was never consulted by anything.

## `fn frozen_reviews_take_each_task_s_own_second_opinion_slot()` › `for entry in registry.entries() {`

And the run-level bindings the occupied slots could have been taken
from are still beside them, holding literals none of the slots share.

## `mod tests` › `struct SourceCase {`

One value in the run record, and the entry field it is supposed to feed.

`mutate` moves exactly one recorded value; `restore` copies back the one
field the case claims that value lands in. Both halves are load-bearing.
Without the first, a constructor that ignored the record and wrote a
literal passes, because the field it wrote never depended on the record
at all. Without the second, a constructor that read the record at the
wrong field passes, because *something* moved.

## `fn moving_one_recorded_value_moves_exactly_the_entry_field_…` › `let cases: [SourceCase; 13] = [`

The distinction this draws is derivation against encoding. The
canonical-bytes table takes an entry that is already built and moves
one field of it, which proves the serializer writes that field — and
says nothing about where the builder got it. A constructor that
wrote the literal `Effort::Low` into `effort.small`, or `true` into
`reviews.enabled`, encodes just as faithfully and satisfies every
fixture that never moves the source underneath it.

So each case here moves one value of the `RunStarted` the registry is
derived *from*, and the built entry has to follow it — in that field
and in no other. `effort.review` is the worked example for the second
half: the sample record resolves it to `high`, which is also what it
resolves `effort.frontier` to, so a constructor that read the
frontier standard where it meant the review standard stays invisible
until exactly one of the two moves.

## `fn moving_one_recorded_value_moves_exactly_the_entry_field_…` › `SourceCase {`

The reviewer bindings are restored one component at a time rather
than whole, so a constructor that put the recorded agent in the
model field fails here as well: restoring the agent alone leaves
the misplaced value behind for the comparison to find.

## `fn moving_one_recorded_value_moves_exactly_the_entry_field_…` › `for (index, (entry, base)) in moved.entries().iter().zip(baseline.entries()).enumerate()`

Every entry rather than the first. These are run-level standards
and belong on all three, so an entry that stayed where it was
names one the constructor filled in from something other than the
record it was handed.

## `mod tests` › `fn restore_agent(entry: &mut Option<PassBinding>, base: &Option<PassBinding>) {`

Copy one binding's agent back, leaving its model where it was found.

## `mod tests` › `fn restore_model(entry: &mut Option<PassBinding>, base: &Option<PassBinding>) {`

Copy one binding's model back, leaving its agent where it was found.

## `mod tests` › `struct SlotCase {`

One recorded second-opinion slot, and the entry component it feeds.

The table above moves run-level standards, where the claim is that
*every* entry follows the value. A second-opinion slot is the opposite
claim: it belongs to one task, so the entry at that index has to follow
it and the others have to stay exactly where they were.

## `struct SlotCase` › `slot: usize,`

The plan index whose entry is the only one allowed to move.

## `fn moving_one_second_opinion_slot_moves_exactly_that_entry_…` › `let cases: [SlotCase; 7] = [`

The run-level table has a case for each component of the primary and
the alternative reviewer, and none for the second opinion — the one
reviewer binding that is *per task*. Every test that named it read
the slot back off a fixture instead, and while both occupied fixture
slots said `copilot`, so did `alternative`: an agent copied from the
alternative reviewer, or hard-coded to that literal, was the fixture's
own answer and no test could see the difference.

So each case here moves one component of one recorded slot and
requires the entry at that index to follow it — in that component,
in no other component of that entry, and in no other entry. The
varied record is the fixture because the sample's occupied slot holds
`alternative`'s exact binding and cannot discriminate the two.

## `fn moving_one_second_opinion_slot_moves_exactly_that_entry_…` › `SlotCase {`

The empty slot filled, rather than an occupied one emptied: a
constructor that decided which entries get a second opinion from
anything but the record — the sample's alternating pattern, say —
leaves `alpha` empty here.

## `fn moving_one_second_opinion_slot_moves_exactly_that_entry_…` › `for (index, (other, base_other)) in`

A slot belongs to one task. Any other entry that followed it is a
slot being broadcast rather than read at the task's own index.

## `fn the_fixtures_give_every_binding_component_its_own_litera…` › `let plan = varied_plan();`

The systematic defect this slice kept producing, stated as a fixture
property rather than patched one case at a time: where two
independently meaningful components share a literal, the one a test
names can be read from the other — or hard-coded to the value both
happen to hold — and still produce the expected answer. Enumerating
every identity literal the discriminating record carries and refusing
a repeat closes that for components no test names yet as well as for
the ones it does.

The sample record is deliberately not enumerated: it feeds the frozen
digest vector and cannot move, and its occupied second-opinion slot
holds `alternative`'s exact binding. That is precisely why the varied
record exists and why it is the one that has to stay discriminating.

## `fn the_fixtures_give_every_binding_component_its_own_litera…` › `assert_eq!(`

The count is this guard's own guard: a collection loop that stopped
recording would otherwise pass by finding nothing to collide.

## `mod tests` › `fn the_reserved_namespace_covers_every_id_the_repair_generator_can_emit() {`

-----------------------------------------------------------------------
Reserved display ids
-----------------------------------------------------------------------

## `fn the_reserved_namespace_covers_every_id_the_repair_genera…` › `for index in [0u32, 1, 9, 99, 999, 1000, 9999, 10_000, u32::MAX] {`

The generator and the refusal are checked against each other rather
than each against a literal, so a change to one that the other does
not follow fails here instead of colliding at run time.

## `fn the_reserved_namespace_covers_every_id_the_repair_genera…` › `assert_eq!(`

Two literal ids, anchoring the shape the decision wrote down. They
are samples and cannot be more than that: any pair of inputs can be
answered from a table keyed on exactly those inputs. What makes the
suffix *the root* rather than a string that matched twice is the
relation asserted in
`a_repair_id_is_its_prefix_its_index_and_the_root_display_id_itself`.

## `fn the_reserved_namespace_covers_every_id_the_repair_genera…` › `for outside in [`

And the namespace stops where it should: reserving everything would
refuse ordinary plans, and reserving only the literal prefix would
let a plan take an id a five-digit lineage would later generate.

## `fn a_repair_id_is_its_prefix_its_index_and_the_root_display…` › `const PREFIX: &str = "merge-fix-";`

The frozen framing, written out rather than read from `REPAIR_PREFIX`
and `REPAIR_INDEX_WIDTH`: an expectation composed from the production
constants would follow them wherever they moved, and holding them
still is half of what this test is for.

## `fn a_repair_id_is_its_prefix_its_index_and_the_root_display…` › `for root in [`

Roots that no table can be keyed on: none is a fixture task id or
appears anywhere else in this module, `0042` and `-` are not task ids
at all, and `merge-fix-0042-kestrel` is itself shaped like a repair
id. Every root is crossed against every index, so a suffix chosen by
index and a suffix chosen by root are each wrong somewhere in this
grid; only reading the argument satisfies all of it.

The last four are hostile to *transformations* of the root rather
than to substitutions of it. A generator that reads the argument but
lowercases, trims, truncates or re-encodes it would satisfy every
root above and disagree here: `Kestrel` is not case-stable, the
padded root is not trim-stable, the long root exceeds any plausible
truncation bound, and `café-kestrel` has more bytes than characters.
`TaskId` is a transparent string that preserves what it is given, so
each of these is a legal root and the suffix must come back
byte-for-byte.

## `fn a_repair_id_is_its_prefix_its_index_and_the_root_display…` › `assert_eq!(`

The relation, which is the whole point: the suffix is not a
string this pair happens to produce, it *is* the root display
id. A generator that chose the suffix any other way — from the
index, from a list of known roots, from nothing — disagrees
here for at least one cell of the grid above.

## `fn an_original_may_not_take_a_reserved_repair_id()` › `const RESERVED: &str = "merge-fix-0001-alpha";`

Written out rather than asked of `repair_display_id`: a refusal test
that derives the id it expects to be refused from the generator
agrees with that generator however wrong it is, and would keep
passing while the namespace it defends moved out from under it.

## `mod tests` › `fn an_incomplete_run_record_cannot_authenticate_a_registry() {`

-----------------------------------------------------------------------
Refusals: the plan and the run record must describe one run
-----------------------------------------------------------------------

## `fn a_record_that_does_not_describe_the_frozen_plan_is_refus…` › `let plan = sample_plan();`

A binding recorded against the wrong tier: same count, wrong meaning.

## `fn a_record_that_does_not_describe_the_frozen_plan_is_refus…` › `let plan = sample_plan();`

A review identity that does not line up with the task list would give
one task another task's reviewer.

## `mod tests` › `const SAMPLE_DIGEST: &str =`

-----------------------------------------------------------------------
registry_digest
-----------------------------------------------------------------------

## `mod tests` › `const SAMPLE_DIGEST: &str =`

The value another process on another platform has to reach from the same
inputs. Written down rather than derived beside the code, which would
agree with any bug in it — and reproduced once from a separate
implementation of the documented encoding rather than copied out of this
one, so it pins the format and not merely today's output.

## `mod tests` › `const SAMPLE_CANONICAL_BYTES: usize = 2522;`

The length of the exact bytes [`SAMPLE_DIGEST`] is taken over.

Pinned beside the digest because the two fail differently: a hash
mismatch says something moved, and the byte count says whether the
encoding grew, shrank, or merely rearranged. Re-derived from the
documented framing at the same time as the digest.

## `mod tests` › `const SAMPLE_CANONICAL_BYTES: usize = 2522;`

2520 before the rename, 2522 after. `upstroke` is exactly two characters
longer than `tactus`, and the delta is exactly two -- so precisely one
renamed string sits inside the canonical encoding (the domain field) and
nothing else about the serialization moved. Any other change would not
land exactly on the rename's length difference.

## `fn the_registry_digest_is_its_frozen_vector()` › `assert_eq!(registry_of(&sample_plan()).digest(), SAMPLE_DIGEST);`

Built again from scratch: no interior iteration order, no address,
no clock.

## `fn a_record_that_names_no_probed_agents_derives_an_empty_al…` › `let plan = sample_plan();`

The two-argument derivation is the legacy one: a schema-1..3
`RunStarted` has nowhere to record what pre-flight probed, so every
entry's allow-list is empty — and that is a different registry, with
a different digest, from the one the same plan derives under a run
that probed anything at all. Asserted rather than assumed because the
difference is exactly what stops a schema-4 log from authenticating
against originals rebuilt through the wrong constructor.

## `fn a_digest_mismatch_is_refused_and_a_match_is_not()` › `let mut moved = sample_plan();`

What the refusal is actually for: the frozen plan moved by one field
under a log that recorded the digest of the plan it started with.

## `fn the_digest_covers_every_field_it_authenticates()` › `let cases: [(&str, MoveInput); 30] = [`

One mutation per digest input. Each must move the digest, and no two
may move it to the same place: a field left out of the canonical
serialization shows up as a digest that did not move, and a field
written without its own length prefix shows up as a collision.

## `fn the_digest_covers_every_field_it_authenticates()` › `("alternative reviewer", |_, started, _| {`

The alternative reviewer and the marker that says one was
retained move separately, though a real run moves them together.
Moved as a pair they are one case, and a serialization that wrote
only one of them is authenticated by the other; apart, each has
to reach the digest on its own.

## `fn the_digest_covers_every_field_it_authenticates()` › `("reviews enabled marker", |_, started, _| {`

The sample record enables verification, and nothing else here
moves that marker off its default, so an encoding that dropped it
would be authenticated by every other case in this table.

## `fn the_digest_covers_every_field_it_authenticates()` › `("probed agent value", |_, _, agents| agents[1].push('!')),`

The allow-list, moved four ways. It is the same value on every
entry, which is exactly why a single "the agents changed" case
would be weak evidence: an encoder that wrote it once for the
whole registry rather than once per entry, or that wrote only
its length, or that sorted it, passes that case and fails these.

## `fn the_digest_covers_every_field_it_authenticates()` › `let baseline = registry_of(&sample_plan()).digest();`

Against the baseline computed here, not against the frozen vector: a
field dropped from the canonical serialization moves the baseline
too, and comparing to a stale constant would let every case pass
while proving nothing about coverage.

## `fn changing_one_entry_field_alone_changes_the_canonical_byt…` › `const MOVED: usize = 2;`

What the table above cannot reach. Its mutations move a `Plan` or a
`RunStarted`, and one such edit moves every entry field derived from
it at once — so a field the encoder wrote as a constant would hide
behind a correlated field that really did move. `effort.small` is the
worked example: the sample resolves it to `low`, so an encoder that
wrote the literal `low` there leaves the frozen vector and every
plan-level case exactly where they are.

Here the registry is built normally and then exactly one field of one
already-built entry is written, so nothing else can move with it. A
case that leaves the bytes alone names a field the digest does not
authenticate; two cases that reach the same bytes name a pair of
records a single recorded digest would accept both of.

The entry moved is `mid`, the sample's third and last. It is the only
one with more than one dependency, which is what lets the order of a
dependency list be moved without its contents — and each list moved
apart from the other, rather than the pair of them together, since an
encoder that sorted one is invisible while the other supplies the
difference.

## `fn changing_one_entry_field_alone_changes_the_canonical_byt…` › `("dependency key order", |entry| entry.deps.swap(0, 1)),`

The two order cases are the pair that matters. Each moves one
dependency representation and leaves the other exactly as it
was, so an encoder that sorted `deps`, or one that sorted
`display_deps`, changes bytes here that the untouched
representation cannot account for.

## `fn changing_one_entry_field_alone_changes_the_canonical_byt…` › `("ladder tier order", |entry| entry.ladder.tiers.swap(0, 1)),`

Escalation order is ascending in every fixture here, because that
is what an escalation ladder is. Sorting it is therefore a
no-op on real input and undetectable from values alone.

## `fn changing_one_entry_field_alone_changes_the_canonical_byt…` › `("rung order", |entry| entry.ladder.rungs.swap(0, 1)),`

Rung order runs in lockstep with tier order in every fixture, so
it needs moving on its own for the same reason the tiers do.

## `fn changing_one_entry_field_alone_changes_the_canonical_byt…` › `("allowed agent value", |entry| {`

The allow-list is the one entry field every entry holds the same
value of, so it is the field an encoder is most likely to write
once for the whole registry — or to leave out, since no
plan-level mutation moves it alone. Moved here on one entry only,
with the entries either side of it asserted unchanged, all four
shortcuts are visible: a registry-level write leaves these bytes
where they were, and so does no write at all.

## `fn changing_one_entry_field_alone_changes_the_canonical_byt…` › `assert_eq!(`

The isolation the case claims: one field of one entry moved, and
the entries either side of it are byte-for-byte what they were.

## `fn the_canonical_encoding_cannot_shift_text_between_adjacen…` › `fn adjacent(title: &str, body: &str) -> Plan {`

The sample plan with one task's adjacent title and body replaced.

## `fn the_canonical_encoding_cannot_shift_text_between_adjacen…` › `for [left_title, left_body, right_title, right_body] in [`

Each row is one run of text split two ways across an adjacent pair.
The first is benign — `ab`/`c` against `a`/`bc` produces different
bytes under *delimiter-only* framing (`value;`) as well, so on its own
it witnesses nothing about the length prefix and would keep passing if
the prefix were dropped. The rest are hostile: the text they shift
across the boundary is the framing punctuation itself, so under
`value;` framing the two sides of each pair are the same bytes and
one recorded digest would authenticate both records.

## `fn the_canonical_encoding_cannot_shift_text_between_adjacen…` › `["ab", "c", "a", "bc"],`

Benign, kept for the plain case.

## `fn the_canonical_encoding_cannot_shift_text_between_adjacen…` › `["a;", "b", "a", ";b"],`

The value terminator moved across the boundary.

## `fn the_canonical_encoding_cannot_shift_text_between_adjacen…` › `["é;", "b", "é", ";b"],`

The same, in text where a byte length and a character count
disagree: `é` is one character and two bytes.

## `fn the_canonical_encoding_cannot_shift_text_between_adjacen…` › `["a:", "b", "a", ":b"],`

The length/value separator, for a framing that kept `:` instead.

## `fn the_canonical_encoding_cannot_shift_text_between_adjacen…` › `["x2:;", "y", "x", "2:;y"],`

Punctuation-dense: both delimiters and a digit run that reads
like a prefix of its own.

## `fn the_canonical_encoding_cannot_shift_text_between_adjacen…` › `let bytes = registry_of(&adjacent("é", "b")).canonical_bytes();`

And the prefix is the value's length in bytes, not in characters: a
character count would frame the two-byte `é` as `1:`.

## `mod tests` › `fn round_tripped(plan: &Plan) -> Plan {`

-----------------------------------------------------------------------
Legacy projection parity
-----------------------------------------------------------------------

## `mod tests` › `fn round_tripped(plan: &Plan) -> Plan {`

The plan a registry projects back to, in the frozen plan's own envelope.

## `mod tests` › `fn normalized_bytes(plan: &Plan) -> Vec<u8> {`

Exactly what `plan.normalized.json` holds
(`engine::preflight::normalized_plan_bytes`).

## `fn the_registry_round_trips_the_frozen_plan_byte_for_byte()` › `for (fixture, raw) in [`

Real plans first: whatever the parser produces, including fields no
hand-written fixture would think to set.

## `fn the_registry_round_trips_the_frozen_plan_byte_for_byte()` › `let bare = plan_of(vec![Task {`

Then the shapes a fixture plan does not reach: no dependencies, no
annotations, empty strings, every list empty.

## `fn the_registry_round_trips_the_frozen_plan_byte_for_byte()` › `for plan in [sample_plan(), bare, dependency_order_plan()] {`

The last of these is the wide dependency list: `depends_on` survives
as written, so a registry that ordered it on the way in writes a
`plan.normalized.json` the frozen one does not match.

## `mod tests` › `fn event_log(started: &RunStarted) -> Vec<Event> {`

A log with a committed task, an escalation, and an interrupted attempt,
so the projections under test have something to project.

## `fn the_report_and_status_projections_are_byte_identical_thr…` › `assert_eq!(`

The projection has to be worth comparing: an empty report, or one
whose task order carried no information, would satisfy any of the
above. Tasks that ran come first in the order they ran, and the three
that never ran follow in plan order — which is not display-id order.

## `mod tests` › `struct RunFixture {`

A run directory holding one frozen plan and one log, removed on drop.

Every effect here goes through a funnel that takes a site — the run
directory through `RunDir.CreatePublicDir`, the frozen plan through
`RunDir.WritePlan`, the log through `Event.LegacyOpenLog` and
`Event.LegacyAppend`, the teardown through `RunDir.RemovePublicHusk`.
It has to: `decisions.effect_site_inventory.mechanism` (2) puts a raw
`fs` call in a **topology** module beyond reach of every allow the
allowlist can grant, because the legacy section "never contains a
topology module (src/topology/**, …)" and the funnel section's clause is
about performing effects inside site-taking APIs. PR5 lane D turned the
denial on; this is what it demanded, and nothing about what the test
below proves has changed.

## `impl RunFixture` › `fn new(tag: &str, plan: &Plan, log: &[Event]) -> Self {`

One run directory: the plan, and a log written once.

The plan is rewritten in place by [`Self::reproject`] rather than a
second fixture being built beside this one, because the log would
then be appended twice and `EventLog::append` stamps the wall clock —
two logs, two sets of timestamps, and `Row::run_started_at` carries
them into the export. One log, one set of timestamps, and the only
thing that differs between the two projections is the plan, which is
exactly the claim.

## `impl RunFixture` › `fn reproject(&self, plan: &Plan) {`

Replace the frozen plan, leaving the log alone.

## `fn the_export_projection_is_byte_identical_through_the_regi…` › `let text = String::from_utf8(expected).expect("utf-8 export");`

The comparison has to be over real rows.
