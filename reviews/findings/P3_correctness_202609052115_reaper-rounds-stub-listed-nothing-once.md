---
id: PR172-REAPER-ROUNDS-STUB-LISTED-NOTHING-ONCE
severity: P3
disposition: deferred
category: correctness
pr: 172
reviewed_sha: b0ff0edf6629bd105ccecbbc89dcf7f51a6c765e
location: src/agent/proc.rs:4542
provenance: undetermined
first_bad: —
guard: deferred: one red in one run, on a build box, in a test PR #172 does not touch; passes alone on both the changed and the unchanged tree; a rate is owed before anything is called a flake (§12), so this row exists to make the next sighting a second one rather than a first
---

## Failure sequence

`the_reaper_performs_as_many_rounds_as_the_machine_needs` ran on tactusbox at `b0ff0edf` inside
`cargo test --lib -- termination:: effects:: rundir::` (Linux, the box's slot target directory,
under the gate lock) -> `reclaim_labeled_containers` against the `unbounded` stub recorded no
`kill` at all -> the assertion "the reaper stopped early: it killed 0 of 12" fired with an empty
set on the left. Rerun alone at the same head: passes in 0.26 s. Rerun alone with PR #172's change
stashed, on the unchanged tree: passes in 0.27 s. PR #172 does not touch `reclaim_labeled_containers`,
`list_labeled_containers`, `read_bounded` or the stub fixture.

## What the change that takes this up should do

Establish a rate before a cause: run the test under the same parallel `--lib` filter on the box and
in CI's Linux leg and count. If it recurs, read the stub's `argv.log` from the failing run — the
fixture keeps the directory only on success, so the change that takes this up first makes the
failure path keep it — and look at whether `ps` was listed at all (a `read_bounded` that saw no
byte within `REAPER_DOCKER_TICKS`, or an `execv` of the stub that failed under load). If it does
not recur across that count, delete this file.
