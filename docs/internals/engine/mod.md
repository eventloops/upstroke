# `src/engine/mod.rs`

Extended notes for [`src/engine/mod.rs`](../../../src/engine/mod.rs).

The code is the authority for what it does; this file is the whole of its prose, moved out of
the source verbatim. Each section is headed by the line of code the comment sat above, spelled
as it is in the source, so the heading is the grep string that finds the code.

## Module

Sequential execution engine (DESIGN.md §14) and the verification ladder it
drives (§11.4, §12, §19).

Pre-flight, run branch, then a scheduler that drains the task graph one
attempt at a time: agent run → engine-captured diff → gates with evidence
axes (§11.1) → read-only review with a structured verdict (§11.2) →
engine-owned commit. A failed attempt does not end the task — it feeds the
failure back to the same rung (resuming the session where the adapter
supports it), then escalates a rung on a fresh session with the accumulated
feedback, and finally asks a human, who is the top rung.

The scheduler's defining property is invariant 6: **a question parks only
the tasks it affects.** Everything else keeps draining, and the run
hard-blocks only when the runnable frontier is empty and everything left is
waiting on an answer. That is the moment — and the only moment — a human is
asked.

Every transition here is an event (invariant 4). The engine never mutates
run state directly: it appends to `events.jsonl` and folds the event back in
through [`crate::events::RunState::apply`], the same function `resume` and
`status` use to rebuild state from the file. A live run and a replay of its
own log therefore cannot disagree — there is no second path for them to
disagree along. `report.json` is written from that state as a projection for
humans; nothing ever reads it back.

## `pub(crate) mod topology;`

The schema-4 run lifecycle.

**`pub(crate)`, and that is what makes `production_effect = "none"` true
rather than an erratum.** It was `pub`, on the argument that the capability
types this module *will* carry are to be guarded by compile-fail fixtures
building out-of-process against the public path — and that behind a private
`mod` those refusals collapse to `E0603`, a compile-fail test passing
because the module is unreachable rather than because the token did its
job. That argument is sound and its subject does not exist: the same doc
admitted "no such fixture exists yet: the visibility is set ahead of the
types".

**What the speculative `pub` actually bought was a defect.** `create_run`,
`Started::into_handle`, `TopologyRun::resumed` and `step` formed a
non-`#[cfg(test)]` writer path reachable by any downstream caller of this
library. Such a caller writes P0–P8 schema-4 state; this build's recovery
refuses every schema-4 log at reader ceiling 3; so `upstroke resume` could
not resume what the released library itself created. Found by the frontier
review of `75da796`, finding 1, and it is the reason the module doc one
level down — "a schema-4 run is reachable only from a `#[cfg(test)]` writer
selector" — was false as written.

**Set back when the fixtures exist**, not before: the visibility follows the
types rather than leading them, which is the opposite of the order tried
here. `the_engine_facade_exposes_exactly_the_items_the_packet_enumerates`
now forbids `pub mod ` in this file, so the next attempt has to be
deliberate.

## `pub use crate::agent::{AdapterSource, BuiltinAdapters};`

Re-exported so `engine::AdapterSource` still resolves for callers that
reasonably think of it as the engine's seam.

## `fn run_harness_on(`

The same run, on an explicit [`Runner`].

The boundary is a parameter rather than a `Harness` field because it is not
an injectable stand-in for a collaborator: it is where every process of
this run executes, and DESIGN.md:612 makes it a configured choice —
"`[runner]` config selects `host` or `container`". PR6 passes the container
runner here; PR4 passes [`HostRunner`] and nothing else.

**Private, and it has to be.** `decisions.phase_zero_modules.visibility` is
"pub(super) only where a sibling or tests reference an item; **no new pub
or pub(crate)**; public paths unchanged", and the module's own entry
enumerates the facade without it. The reason is not bookkeeping: this
function drives the *schema-1..3* coordinator, and `invariants[22]` is
"schema-1..3 runs are host-only and no run changes its boundary or image
between epochs". A `pub` here lets a downstream crate execute a legacy run
off-host, with no `RunnerPolicy` to record it and no refusal — and lets the
same run come back on `HostRunner` at the next resume. Private is what
makes that unreachable rather than merely undocumented.

### Errors

Whatever the run refuses or fails on.

## `run_contained(opts, harness, runner, || {`

`NoHooks` is what production passes the process funnel, and the
containment step is threaded the same way: the observer exists so the
step has a drivable failure path (`runner::host::contain_write_command`),
and production arms nothing.

## `fn run_contained(`

The same run, over the containment step it must perform **first**.

Every public entry point above reaches the coordinator through here, so
this one call is what makes `run`, `run_with` and `run_harness` write
commands in INV-18's sense: "on Windows every host child is a member of the
coordinator's ambient kill-on-close Job Object from creation", and
`expected_failures_refusals[1]`, "ambient job cannot be created or joined
(Windows) → write command refuses at startup with a diagnostic". A
downstream crate calling `engine::run_with` is a coordinator exactly as the
CLI is; before this it established nothing, so a kill between
`CreateProcessW` and private-job assignment left the suspended stub alive
and a real ambient failure could not produce the required refusal.

`contain` is a parameter for the same reason `src/main.rs`'s `dispatch`
takes its join: no machine here can make the real one fail, and the
*ordering* between containment and the first thing the coordinator does is
then a testable fact rather than a written-down one
(`a_facade_run_refuses_before_any_effect_when_containment_fails`). It is
not a hole in the guarantee: `Contained` has a private field, so the only
closure that can return one is one that establishes containment.

## `pub fn resume_harness(`

§15: replay, verify the run branch still matches the record, re-probe, and
continue — parked questions intact.

Every refusal below exists because continuing would produce a *wrong*
result rather than merely an awkward one, and each says which of the four
things moved — the run, the plan, the config, or the branch — because that
is what decides what the operator does next.

Note what is *not* a refusal: gates that resolve differently today. Those
are taken from the record and run, so there is nothing to refuse — the
difference is a warning about an edit that does not apply here. A refusal is
for the cases where continuing would be wrong, and continuing under the
gates this run has been using all along is exactly right.

## `fn resume_harness_on(`

The same resume, on an explicit [`Runner`]. See [`run_harness_on`],
including why this is private.

### Errors

Whatever the resume refuses or fails on.

## `fn resume_contained(`

The same resume, over the containment step it must perform first. See
[`run_contained`]: a resume drives a run, so it is a write command, and the
three public resume entry points reach the coordinator only through here.
