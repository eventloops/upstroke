# `src/topology/fold/region.rs`

Extended notes for [`src/topology/fold/region.rs`](../../../../src/topology/fold/region.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Rendering helpers a refusal message is written from.

## `pub(super) fn describe_region(paths: &PathSet) -> String {`

A region as a refusal names it.

The empty prefix list is spelled out rather than printed as `[]`: an empty
region and an unread one are different answers — [`PathSet::prefixes`] says
so — and a refusal that rendered the first as an empty pair of brackets
would read like a formatting accident next to `the whole repository`.

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
