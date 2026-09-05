---
id: SWEEP-CONFIG-PARSE-011
severity: P3
disposition: deferred     # the writer is src/gates.rs, a flat module not yet on the sweep queue
category: correctness
pr: 150
reviewed_sha: 425ad55b9703ed58542ee322e6a266d7501bdd93
location: src/gates.rs:315
provenance: pre_existing
first_bad:
guard: the sweep of `src/gates.rs` when its family joins the queue, or the next change to `run_all`
---

## Failure sequence

`[[gates]]` names two gates `lint fast` and `lint-fast` (or `a/b` and `a_b`, or two
names that agree in their first 64 characters) -> `parse_gates` accepts both, since it
compares names as written and they differ -> `gates::run_all` writes each gate's log as
`<task>-<attempt>-<filename_component(name)>.log`, and `util::filename_component` maps
every character outside `[A-Za-z0-9._-]` to `-` and truncates to 64 -> both gates write
`<task>-<attempt>-lint-fast.log` in the same attempt directory, and the second `fs::write`
replaces the first -> the first gate's log is gone, and a failure summary that names
`lint fast` points at a log `lint-fast` wrote.

`parse_gates` now refuses an exact repeat (this pull request), which closes the case an
operator writes by hand; the folded collision is a property of the log writer, and refusing it
at parse would bake `filename_component` into the config reader.

## What the change that takes this up should do

Key the log file by the gate's position as well as its folded name -- the engine already
identifies a gate by index (`invocation(n)`, "which gate, not which run of it") -- for
example `<task>-<attempt>-g<n>-<name>.log`, so two gates can never share a log whatever
their names fold to. Update the two tests in `src/gates.rs` that assert log file names and
any status or export reader that opens them by name.
