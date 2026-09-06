# `src/validate/graph.rs`

Extended notes for [`src/validate/graph.rs`](../../../src/validate/graph.rs).

Contracts and rationale for the graph validation sweep. Item names identify the corresponding code.

## Module

The plan's dependency graph, as `analyze` checks it: duplicate ids, unknown
`depends` targets, cycles, and the artifact wiring that only warns.

Split out of `super` on 2026-09-03 (`1292653`) and swept on 2026-09-05.
`check_graph` is still the single entry point `analyze_captured` calls and
every message it produced before the sweep is produced unchanged; what
changed is inside. The cycle search is keyed by id rather than by position
in `plan.tasks`, and walks an explicit path instead of recursing, so
nothing here indexes a collection and nothing grows the call stack with the
plan — a chain as long as the plan is a loop, not a recursion, whatever the
thread's stack (see `the_cycle_search_needs_no_call_stack_for_a_long_chain`
below). Every function is total over any `Plan` a caller can build: an
unknown id, a `produced_by` of `None` or a duplicate reaches a `continue`
or a refusal, never an index, an `unwrap_or` or an `unreachable!`, and the
second attribute below is what makes that a build error rather than a
reading.

**The denial is restored rather than inherited.** `super` carries
`#![allow(clippy::disallowed_methods)]` because `write_normalized_json`
writes the normalized plan with `fs::write`, and a module-level allow reaches
every child of the module it sits in. Nothing here writes anything — these
are pure functions over a parsed `Plan` — so that allowance has no business
extending here, and this line is what stops it. That is also what keeps this
file out of `effects/allowlist.toml`: an allowance is what that file records,
and this module takes none.

## `Module` — `#![deny(clippy::indexing_slicing, clippy::unreachable)]`

§7's panic surface, mechanised for this file ahead of the crate-wide
`[lints]` entry `standards/SWEEP.md` says is owed: the sweep left no index,
slice or `unreachable!` here, and this keeps it so. The tests below are under
it too, since `clippy.toml` takes no test allowance for either lint yet.

## `check_graph`

Duplicate ids, unknown `depends` targets, then cycles — all collected so a
broken plan reports everything in one run. On a clean graph, artifact
wiring that contradicts the dependency order is surfaced as warnings.

Duplicates are listed in id order and unknown targets in document order.
The cycle search runs only when both lists are empty: an edge that resolves
to no task, or to two, has no dependency order to check, and reporting a
cycle through it would name a graph the author never wrote.

# Errors

`UpstrokeError::Validation` carrying every duplicate id and every unknown
dependency, or — on a graph where every id names one task and every edge
resolves — one dependency cycle, written as the ids along it with the first
repeated at the end (`a -> a` for a task that depends on itself).

## `check_graph` — `let index = index_by_id(plan);`

From here every id names exactly one task, which is what makes an
id-keyed index a faithful picture of the plan.

## `index_by_id`

Id → task. Faithful only once every id names one task: on a plan with a
duplicate the map would keep one of the two tasks and drop the other, which
is why `check_graph` refuses duplicates before building it.

## `find_cycle`

One dependency cycle as the ids along it, the first repeated at the end, or
`None` when the graph has none.

A depth-first search from each task in document order, following each
task's `depends_on` in its own order, so the cycle a plan reports is the
same on every run and every platform, and the same one the recursive search
this replaced reported. `path` is the chain of tasks the search is inside,
outermost first, each with the dependencies it has yet to follow; `on_path`
is that chain as a set, and the two change together at each push — root
initialization and dependency descent both push a new entry onto both — and
at the one shared pop below, so membership is a lookup rather than a scan.
A dependency
already on the path closes a cycle, which is the path from that task down
plus the edge back to it. A task whose dependencies have all been followed
is `finished`, and an edge into a finished task leads into a subgraph
already known to be acyclic, so it is not followed again: a diamond is not
a cycle, and no task is expanded twice.

An edge to an id the index lacks is skipped: it belongs to no cycle, and
`check_graph` has refused the plan before this runs.

## `check_artifact_wiring`

A task that `needs` an artifact should depend — directly or transitively —
on its producer, or execution order cannot guarantee the artifact exists.
The plan is frozen (§5), so this warns rather than inventing edges.

A task that needs an artifact whose recorded producer is the task itself
is not warned about, as `design/09` specifies. `plan.artifacts` records one
producer per artifact, and the markdown adapter records the first task
that declares `out=`. Every declaration remains available in the tasks'
`artifacts_out`, including declarations by dependencies of the needing
task. The design gives multiple declarations no update, conflict or error
semantics. This check uses the recorded producer without assigning such
semantics to the other declarations.

An artifact no task produces is not in `plan.artifacts` at all — the
markdown adapter warns about it while assembling the plan — and one whose
`produced_by` is `None` is treated the same way: there is nothing to wire.

## `depends_transitively`

Whether `target` is reachable from `task` along `depends_on` edges. A
depth-first walk seeded from the task's own dependencies, each id expanded
at most once, so it terminates on any graph — the cycle check has run by
the time this is called, but nothing here relies on that. An edge is a
match when its id is `target`, whether or not the index has it; an edge to
any other id the index lacks is a dead end. `check_graph` refuses a plan
with an unknown id before this runs, so both halves of that sentence
describe an index that holds every id in play.

## `task`

A task with an id and its dependencies, every other field at rest. A
struct literal rather than a builder so a field added to `Task` fails
here (§12: fixtures derive their field lists from the production type).

## `needing`

`task`, and it needs the artifact named.

## `producing`

`task`, and it produces the artifact named.

## `check`

The warnings of a plan the check accepts, or the rendered refusal.

## `a_cycle_entered_from_a_tail_is_reported_from_its_first_repeated_task` — `let plan = plan(`

`x` is not on the cycle; the report starts where the path re-enters.

## `a_cycle_the_first_task_does_not_reach_is_still_found_from_the_first_task_on_it` — `let plan = plan(`

Roots are taken in document order, not id order: `z` first, then
`c`, so the cycle is reported from `c` rather than from `b`.

## `duplicates_and_unknown_targets_are_reported_together_and_before_any_cycle` — `let plan = plan(`

The second `b` depends on itself; with `b` duplicated the cycle
search does not run, so the refusal lists the two structural
problems and nothing else.

## `a_task_needing_and_declaring_what_an_earlier_task_also_declares_is_not_warned_about_through_the_adapter` — `let raw = "## D1\n<!-- upstroke: id=d1 depends=d2 needs=contract out=contract -->\n\n\`

The markdown adapter records `d1` as the one producer of `contract`
and keeps `d2`'s claim only in its `artifacts_out`. `d1` depends on
`d2` and needs what both declare: accepted input whose meaning the
design leaves undefined (`SWEEP-GRAPH-009`), not a proven update. The
base is silent on it, and a warning that `d1` needs what it produces
itself was the one a pass on `2bbf35b` showed this input producing.

## `the_cycle_search_needs_no_call_stack_for_a_long_chain` — `const LENGTH: usize = 50_000;`

Fifty thousand tasks, each depending on the next, checked on a
thread with a 256 KiB stack: a search that recursed once per task
would overflow it long before the end of the chain, and the last
task closes the chain into a cycle so the whole of it is walked.
