---
id: SWEEP-FOLD-001
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/events/log.rs:965
provenance: pre_existing
first_bad:
guard: the change that gives the stable-prefix barrier a positioned replay
---

## Failure sequence

`TopologyFold::parse_log` reports a bad line as `FoldError::RewrittenLog { line, .. }`,
so a log that fails to *parse* names the line it failed on. `TopologyFold::replay`
folds the parsed events in order and propagates the `FoldError` of the first event
that does not apply, with no index and no identity of that event.

The one production caller is the stable-prefix barrier in `src/events/log.rs`, which
wraps it as `BarrierError { step: CheckedReplay, path, detail: error.to_string() }`.
The operator is handed the log's path and a sentence such as "`task_dispatched` names
task 7, which this run has no entry for" for a file that may hold thousands of events,
several of them `task_dispatched` for task 7. Nothing in the report says which record
was refused, and re-deriving it means folding the log again by hand.

## What the change that takes this up should do

Not by wrapping `replay`'s error. `src/topology/fold/tests.rs` pins, in
`refused_live_and_on_replay` and in
`every_guarded_event_is_refused_the_same_way_live_and_on_a_hostile_replay`, that the
live path and a replay of the same prefix refuse with an **equal** `FoldError` — the
observable form of "one transition function for a live run and for a replay". The live
path has no line number to carry, so adding one to the replay path alone breaks that
equality, and adding one to both would make every refusal carry a field the live
emitter cannot fill.

The position belongs to the layer that has the events vector. The barrier already holds
both `events` and the fold; it can fold with the index in hand and put the refused
event's ordinal and kind in `BarrierError::detail` beside the unchanged `FoldError`
text. That keeps `FoldError` comparable between the two paths and still tells the
operator which record to look at.
