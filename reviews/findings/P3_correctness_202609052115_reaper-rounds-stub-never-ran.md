---
id: PR172-REAPER-ROUNDS-STUB-NEVER-RAN
severity: P3
disposition: deferred
category: correctness
pr: 172
reviewed_sha: b0ff0edf6629bd105ccecbbc89dcf7f51a6c765e
location: src/agent/proc.rs:4542
provenance: pre_existing
first_bad: W2-HOST-TESTS-WRITE-THEN-EXEC-ETXTBSY
guard: deferred: a second site of `W2-HOST-TESTS-WRITE-THEN-EXEC-ETXTBSY`, whose owner is "whoever meets the failure again"; the change that takes this up gives the stub fixture a way to run its script that a concurrent fork in another test thread cannot refuse (a retry on `ETXTBSY` in the reaper's own `execv` path is production code and belongs to a change that owns the reaper's container half), and until then a box red of this test with an unrun stub is this row
---

## Failure sequence

`the_reaper_performs_as_many_rounds_as_the_machine_needs` writes its docker stub with
`std::fs::write` and immediately drives `reclaim_labeled_containers`, whose forked child `execv`s the
stub -> a child forked at that moment by another test thread still holds the stub's write descriptor
until it gets scheduled and closes its inherited descriptors -> `execv` refuses with `ETXTBSY` and the
child `_exit(127)`s -> the listing is empty, nothing is killed, and the assertion reads "the reaper
stopped early: it killed 0 of 12". Three sightings on tactusbox on 2026-09-05, every one with the
stub directory left behind holding `docker-stub` and no `argv.log`, so the script never ran at all:
13:10 (pid 886067, a run of another branch, before PR #172's branch existed), 20:41 (pid 3034456, at
`b0ff0edf` under a parallel `--lib` filter) and 21:33 (pid 3315508, at `a328b6f` in the full suite).
Each passes alone at the same head, in about a quarter of a second. The box runs the suite on 32
threads; CI's three platforms passed it at both heads.

## What the change that takes this up should do

The same thing `W2-HOST-TESTS-WRITE-THEN-EXEC-ETXTBSY` asks for, at this second site. Nothing in PR
#172 touches `reclaim_labeled_containers`, `list_labeled_containers`, `read_bounded` or the stub
fixture; the three sightings are recorded here so the next one is a fourth, with the directory
evidence that says which class it is.
