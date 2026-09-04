# `src/runner/invocation.rs`

Extended notes for [`src/runner/invocation.rs`](../../../src/runner/invocation.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

The typed identity every Runner process carries.

`decisions.admission_and_leases.permits.invocation_identity` **enumerates**
it, and the enumeration is the type:

> InvocationId = (key, generation, attempt, role, ordinal) with role in
> {worker, gate(n), review_pass(n), review_reask(n)}, or (sequence, role,
> ordinal) with role in {gate(n), review_pass(n), review_reask(n)}, or
> (probe, target: Agent(name) | Shell, ordinal) at pre-flight (the shell
> probe is target Shell, non-slotted; agent probes are slotted); unique per
> process; a retry attempt has a new attempt number; **deterministic in the
> sequential substrate**; every RunnerRequest carries it

Three closed forms, nine role/target members, and no fourth shape. The
properties follow from the shape rather than from a generator:

* *unique per process* — distinct tuples render distinctly
  ([`InvocationId::render`] is injective; `distinct_tuples_render_distinctly`
  crosses every field).
* *a retry attempt has a new attempt number* — `attempt` is a field, so a
  retry that did not change it is a value equal to the one before it.
* *deterministic in the sequential substrate* — the rendering is a pure
  function of the tuple. Nothing here reads a clock, a pid, or a random
  source. This is load-bearing beyond fidelity: `crash_reconstruction`
  builds container names as
  `upstroke-<repo_key>-<run_id>-<incarnation>-<invocation-hash>` "so
  **deterministic** InvocationIds never collide across incarnations and no
  earlier ownership evidence is overwritten".

PR4 owns the type and its properties. **PR7 assigns them**:
`decisions.sequential_substrate.runner` — "RunnerRequest carries invocation:
InvocationId from PR4 (assigned by PR7, new per attempt)". No ledger, no
broker and no allocation policy lives here.

## `pub enum AttemptRole {`

The role of an invocation identified by `(key, generation, attempt, …)`.

The packet's first form: "role in {worker, gate(n), review_pass(n),
review_reask(n)}".

## `pub enum AttemptRole` › `Worker,`

The worker process of this attempt.

## `pub enum AttemptRole` › `Gate(u32),`

Gate `n` of this attempt's gate list.

## `pub enum AttemptRole` › `ReviewPass(u32),`

Review pass `n`.

## `pub enum AttemptRole` › `ReviewReask(u32),`

Re-ask `n` of a review pass.

## `pub enum SequenceRole {`

The role of an invocation identified by `(sequence, …)`.

The packet's second form: "role in {gate(n), review_pass(n),
review_reask(n)}" — **without** `worker`. A separate type rather than a
runtime check on [`AttemptRole`], because "a sequence has no worker" is then
a compile error at the call site instead of a refusal at run time. INV-20
draws the same line: "every completion is bound to (key, generation,
attempt) or (sequence, candidate)" — a sequence integrates candidates other
processes produced, so there is no worker of a sequence to identify.

## `pub enum SequenceRole` › `Gate(u32),`

Gate `n` of this integration transaction.

## `pub enum SequenceRole` › `ReviewPass(u32),`

Review pass `n`.

## `pub enum SequenceRole` › `ReviewReask(u32),`

Re-ask `n` of a review pass.

## `impl AttemptRole` › `const fn token(self) -> &'static str {`

The token this role renders as.

## `impl SequenceRole` › `const fn token(self) -> &'static str {`

The token this role renders as.

## `fn parse(text: &str) -> Option<Self>` › `for (token, build) in [`

Longest token first: `review_pass` and `review_reask` share no
prefix, but `gate` must not swallow a token that merely starts the
same way if one is ever added.

## `pub enum InvocationId {`

The identity of one Runner process — one of the packet's three forms.

A closed enumeration rather than a string, because the value is a key in
four separate ledgers (R3's slot pairs, R4's invocation registrations, PR6's
container names and intent paths, PR7's completion identity check) and
because every property the packet states about it is a property of the
tuple. An opaque string can hold a value no form describes; this cannot.

## `pub enum InvocationId` › `Attempt {`

`(key, generation, attempt, role, ordinal)`.

## `pub enum InvocationId` › `ordinal: u32,`

Which invocation of this role within this attempt, dense from 0. The
role index says *which* gate; the ordinal says which run of it, so a
re-dispatch inside one attempt is a new identity rather than a
reused one.

## `pub enum InvocationId` › `Sequence {`

`(sequence, role, ordinal)`.

## `pub enum InvocationId` › `Probe {`

`(probe, target: Agent(name) | Shell, ordinal)` at pre-flight.

## `pub enum InvocationId` › `ordinal: u32,`

Which pre-flight this is. Probe identities repeat across
incarnations by construction — the packet says so, and says how it
is handled: "because probe identities repeat across incarnations,
every container name and intent path additionally carries the
coordinator incarnation id".

## `pub const LEGACY_GENERATION: GenerationId = GenerationId(0);`

The generation the legacy sequential engine assigns.

The contract's `invariants_introduced[1]` is "legacy engine assigns
legacy-scoped values". The legacy engine has no generations — it never
re-dispatches a task from a fresh worktree — so every value it assigns sits
in generation 0 and says so through [`InvocationId::legacy_attempt`]. The
scope is real rather than decorative: a legacy run is schema-1..3 and a
generation-bearing run is schema-4, and no run changes schema between
epochs (INV-23), so the two never share a ledger.

## `pub const MAX_LEN: usize = 70;`

The longest value the enumeration can render.

Not a policy number: it is the maximum of [`InvocationId::render`] over the
whole domain, which `the_longest_value_the_domain_can_render_is_the_limit`
computes from `u32::MAX` and the longest role token. Deriving it this way is
what stops the validator refusing a value the domain contains — the failure
mode a hand-picked limit has. The only construction it can therefore refuse
is an over-long agent name in the probe form.

## `const SEPARATOR: char = '.';`

Every character a rendered id may carry.

PR6 puts this value inside a container name and inside the file name
`<R>/containers/<name>.intent` (`decisions.pr_sequence[7].scope`), so a
value carrying a path separator, a space, or a control character is a value
that names a different file than the one the record says. `.` is the field
separator, so no component may contain one.

## `impl InvocationId` › `pub const fn attempt(`

`(key, generation, attempt, role, ordinal)`.

## `impl InvocationId` › `pub const fn legacy_attempt(`

`(key, generation, attempt, role, ordinal)` in the legacy engine's
generation. See [`LEGACY_GENERATION`].

## `impl InvocationId` › `pub const fn sequence(sequence: SequenceId, role: SequenceRole, ordinal: u32) -> Self {`

`(sequence, role, ordinal)`.

## `impl InvocationId` › `pub fn probe(target: ProbeTarget, ordinal: u32) -> Result<Self, UpstrokeError> {`

`(probe, target, ordinal)`.

### Errors

[`UpstrokeError::Refused`] when the target names an agent whose id
carries a character outside `[0-9A-Za-z_-]`, or is long enough to push
the rendering past [`MAX_LEN`]. Every other form is infallible: its
fields are integers, and their longest rendering *is* [`MAX_LEN`].

## `pub fn probe(target: ProbeTarget, ordinal: u32) -> Result<S…` › `if let ProbeTarget::Agent(agent) = &target {`

The *component*, not only the whole rendering. `.` is inside the
charset the whole value is checked against, so an agent named
`claude.code` would render a value that passes `validate` and yet
splits into four components no form has — writable and unreadable.

## `impl InvocationId` › `pub fn render(&self) -> String {`

The value as it is recorded: injective over the whole domain.

The grammar, one line per form:

```text
k<key>.g<generation>.a<attempt>.<role>.o<ordinal>
s<sequence>.<role>.o<ordinal>
p.shell.o<ordinal>   |   p.agent-<id>.o<ordinal>
```

The leading component is `k…`, `s…` or `p`, which no other form can
produce, so the forms are disjoint; within a form the component count is
fixed and no component may contain the separator, so two distinct tuples
differ in some component and therefore in the rendering.

## `impl InvocationId` › `pub fn parse(value: &str) -> Result<Self, UpstrokeError> {`

Rebuild a recorded identity.

The domain is closed on the way back in as well as on the way out: a
value that is not one of the three forms is refused rather than carried
as an opaque string, so a record cannot smuggle a fourth shape into a
ledger keyed by this type.

### Errors

[`UpstrokeError::Refused`] when `value` is not the rendering of any tuple.

## `impl InvocationId` › `pub const fn probe_target(&self) -> Option<&ProbeTarget> {`

The probe target, when this identity is a pre-flight probe.

INV-18 accounts the two targets differently — "every agent CLI
invocation incl. agent probes acquires its atomic {agent, pool?} pair
while gates and the shell probe register without slots" — so the target
is readable from the identity and not only from the request's role.

## `fn validate(value: &str) -> Result<(), UpstrokeError> {`

Refuse a rendering no funnel could have written.

## `fn parse_forms(value: &str) -> Option<InvocationId> {`

The inverse of [`InvocationId::render`], or `None`.

## `fn field(component: &str, tag: &str) -> Option<u32> {`

One `<tag><digits>` component. Rejects a leading `+`, a leading zero on a
multi-digit number, and anything else `u32::from_str` would accept but
`render` would never produce, so `parse ∘ render` is a bijection and not
merely a left inverse.

## `mod tests` › `const KEYS: [u32; 3] = [0, 1, 12];`

-----------------------------------------------------------------------
the grid, and what bounds it
-----------------------------------------------------------------------

Every numeric field is a `u32`, so no grid can be exhaustive. What a grid
has to catch is a rendering that *drops* a field, *conflates* two, or
loses a separator. Dropping and conflation are caught by a full Cartesian
product in which every field varies independently — if the rendering is a
function of fewer fields than the tuple has, two grid points collide.
Three values per field is the smallest set that also distinguishes "uses
the value" from "uses whether the value is zero". A lost separator is a
different defect (adjacent fields concatenate), so it gets its own table
of pairs chosen to collide under exactly that mutation.

## `mod tests` › `fn grid() -> Vec<InvocationId> {`

Every identity the grid describes.

## `mod tests` › `const fn grid_size() -> usize {`

The grid's size, computed from the grid's *definition* — the product of
the dimensions — so a renderer that lost a field cannot also lower the
number it is compared against.

## `fn the_grid_varies_every_field_independently()` › `assert_eq!(BTreeSet::from(KEYS).len(), 3);`

Fixture hostility as a distinct-value count, per field, not as prose.

## `mod tests` › `fn the_enumeration_has_exactly_the_nine_members_the_packet_names() {`

The packet enumerates nine role/target members across the three forms:
four attempt roles, three sequence roles (no worker), two probe targets.

## `mod tests` › `fn the_three_forms_render_as_the_packet_spells_them() {`

Expected values written by hand from the packet's field order, never
from `render`. A rendering that reordered, dropped, or re-spelled a
field fails here even if it stayed injective.

## `fn distinct_tuples_render_distinctly()` › `let tuples: BTreeSet<&InvocationId> = ids.iter().collect();`

Uniqueness within a run is therefore structural: it does not depend
on a generator not colliding, and no expected value here came from
the constructor.

## `mod tests` › `fn adjacent_fields_cannot_be_confused_for_one() {`

A lost separator concatenates two adjacent fields. Each pair below is
two distinct tuples whose renderings become equal under exactly that
mutation, so the pair fails the moment a `.` is dropped.

## `mod tests` › `fn the_longest_value_the_domain_can_render_is_the_limit() {`

[`MAX_LEN`] is the domain's own maximum, so the validator can never
refuse a value the enumeration can produce.

## `fn the_longest_value_the_domain_can_render_is_the_limit()` › `assert_eq!(`

Written out rather than computed: `k` + 10 digits, `.g` + 10, `.a` +
10, `.review_reask` + 10, `.o` + 10.

## `mod tests` › `fn the_same_tuple_always_renders_the_same_value() {`

"deterministic in the sequential substrate" — the rendering is a pure
function of the tuple, so the same identity built twice is the same
value. A ULID, a pid, or a counter fails this.

## `fn the_same_tuple_always_renders_the_same_value()` › `let mut noise = 0u64;`

Work between the two constructions, so anything reading a clock or a
monotonic nonce has had the chance to move.

## `mod tests` › `fn a_retry_is_a_new_attempt_number_and_a_new_identity() {`

"a retry attempt has a new attempt number".

## `mod tests` › `fn parse_refuses_what_no_form_can_render() {`

The domain is closed on the way in. Every value here is one a reader
might plausibly be handed — including the opaque forms this type used to
accept.

## `fn parse_refuses_what_no_form_can_render()` › `"legacy-t1-a2",` (trailing)

the old open-ended scope form

## `fn parse_refuses_what_no_form_can_render()` › `"01K3Q9V0Z3B8N9RJ4F2A6C7D8E",` (trailing)

a ULID

## `fn parse_refuses_what_no_form_can_render()` › `"k1.g1.a1.worker",` (trailing)

a field short

## `fn parse_refuses_what_no_form_can_render()` › `"k1.g1.a1.worker.o1.x2",` (trailing)

a field long

## `fn parse_refuses_what_no_form_can_render()` › `"k1.g1.a1.boss.o1",` (trailing)

a role outside the enumeration

## `fn parse_refuses_what_no_form_can_render()` › `"s1.worker.o1",` (trailing)

worker is not a sequence role

## `fn parse_refuses_what_no_form_can_render()` › `"k1.g1.a1.gate.o1",` (trailing)

an indexed role without its index

## `fn parse_refuses_what_no_form_can_render()` › `"x1.g1.a1.worker.o1",` (trailing)

an unknown form tag

## `fn parse_refuses_what_no_form_can_render()` › `"k1.g1.a1.worker.1",` (trailing)

an untagged field

## `fn parse_refuses_what_no_form_can_render()` › `"k01.g1.a1.worker.o1",` (trailing)

a leading zero render never writes

## `fn parse_refuses_what_no_form_can_render()` › `"k1.g1.a1.worker.o4294967296",` (trailing)

past u32

## `fn parse_refuses_what_no_form_can_render()` › `"p.agent-.o1",` (trailing)

an empty agent name

## `fn parse_refuses_what_no_form_can_render()` › `"p.agent-a b.o1",` (trailing)

outside the charset

## `fn probe_refuses_a_target_that_would_not_survive_a_containe…` › `let longest = "a".repeat(50);`

`p.agent-<name>.o<ordinal>` spends `p`, two separators, `agent-`,
`o` and up to ten ordinal digits: 1 + 1 + 6 + 1 + 1 + 10 = 20. So
MAX_LEN leaves exactly 50 characters for the name, and the boundary
is asserted on both sides rather than described.

## `mod tests` › `fn the_wire_form_is_the_bare_string() {`

The wire form is the bare rendered string, pinned against payloads
written here rather than against this type's own output.

## `fn the_wire_form_is_the_bare_string()` › `assert!(serde_json::from_str::<InvocationId>("\"legacy-t1-a2\"").is_err());`

A value outside the enumeration does not become an InvocationId by
being written into a record.
