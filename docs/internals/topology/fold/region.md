# `src/topology/fold/region.rs`

Extended notes for [`src/topology/fold/region.rs`](../../../../src/topology/fold/region.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Rendering helpers a refusal message is written from.

Nothing here decides anything: every item turns a value the fold already holds
into the words a `FoldError` carries, and `check_lease_disposition` is the one
exception — it compares two values and builds the refusal when they disagree.
None of it touches the filesystem, the clock or a process, so each item is a
function of its arguments alone and is tested as one.

**Display spellings, not wire spellings.** The names here are prose for an
operator reading a refusal — `human-required`, `lineage-held`, `in flight` —
and the event log spells the same values `human_required`, `lineage_held`.
That is the convention the rest of the fold already keeps
([`GenerationClass::name`] returns `open with no attempt`), and it is why none
of these functions is derived from a `serde` tag or from `Debug`.

## `pub(super) fn describe_region(paths: &PathSet) -> String {`

A region as a refusal names it.

The empty prefix list is spelled out rather than printed as `[]`: an empty
region and an unread one are different answers — [`PathSet::prefixes`] says
so — and a refusal that rendered the first as an empty pair of brackets
would read like a formatting accident next to `the whole repository`.

A prefix reaches this from the event log, so the list is rendered in the order
the log recorded it and nothing is sorted or de-duplicated: `PathSet` compares
as a `Vec`, so the two sides of the one comparison that prints this — the
predicted region against the region a task's frozen hints derive, in
`check_attempt.rs` — differ whenever their orders differ, and a rendering that
sorted would print two identical strings for a refusal that fired.

## `pub(super) fn ineligible_detail(why: Ineligible) -> String {`

Why the queue would not admit this candidate, as a clause the refusal's
sentence continues into (`it is not eligible: …`).

The two lineage arms are one word apart and mean different things: a candidate
that overlaps the region a lineage holds is inside that lineage's work, and one
that overlaps the region an *older* lineage holds is behind it in the queue. The
`root` in each is the lineage's root task, so a reader has the key to look up.

## `pub(super) fn spawn_admission_name(admission: &SpawnAdmission) -> &'static str {`

How a `task_spawned` event's own admission is named, for the refusal that
reports an event and its registered entry disagreeing.

Three spawn admissions face two entry admissions, and the pairing is not
one-to-one: `human_required` is the automatic-repair limit, which a `runnable`
entry legitimately carries, so `check_admission` admits that pair and this
naming is only ever printed for a pair it refuses. The refusal reads `its
admission is X and its entry's is Y`, so every pair it can print must render as
two different words; the module's test block pins that over the pairs
`check_admission` does not handle rather than over the enums.

## `pub(super) fn admission_name(admission: &Admission) -> &'static str {`

How a registered entry's admission is named, the other half of that sentence.

**It is not [`Admission::tag`], which spells the same two words.** That one
feeds `encode_entry`, the registry's content digest: its output is part of an
identity that a replayed log has to reproduce byte for byte. Routing this
diagnostic through it would make rewording a refusal a change to every recorded
digest. The duplication is the point — two callers with different contracts,
one free to change and one frozen.

## `pub(super) fn ordinal(index: u32) -> String {`

A lineage member's position, for the refusal that reports a repair recording
the wrong one. `#0` is the first member; the count and the recorded index are
printed side by side, so the `#` is what tells them apart in the sentence.

## `fn disposition_name(disposition: LeaseDisposition) -> &'static str {`

The three lease dispositions, named.

It is private and has exactly one caller, `check_lease_disposition` below. It
exists because that refusal used to print `format!("{recorded:?}")`: a `Debug`
rendering in an operator-facing message is a message whose text is whatever a
derive currently produces, and it was the module's only value not rendered
through a name of its own.

## `pub(super) fn check_lease_disposition(`

refusals[14]: the disposition an event records must be the one this
generation's holding admits.
The recorded disposition against the one the holding implies.

**Every caller passes a closing generation, and since 2026-08-27 there is no
other kind.** This took a `survives: bool`, and exactly one caller ever
passed `true`: `attempt_finished{Succeeded}`, the settlement that left a
generation open to hand its region to a candidate. That event is no longer a
settlement this fold accepts — `candidate_prepared` is the sole successful
one — so the parameter had a single reachable value and a second value that
documented a rule nothing could exercise.

**The surviving case did not disappear, it moved.** A generation that keeps
its region hands it over through `CandidatePrepared::lease_effect`, which
[`TopologyFold::check_candidate_prepared`] matches against the entry's
lineage — the same decision, on the event that now makes it.
[`GenerationLease::expected`] keeps both arms and its own table test,
because it is the statement of the rule rather than a caller of it.

That is what the `false` at the head of this function is: not a policy this
function chooses, but the one kind of generation its three callers
(`check_attempt_finished`'s `Closed` settlement, `check_attempt_interrupted`
and `check_generation_closed`) can reach it with. `fate` is `"closes"` for the
same reason.

