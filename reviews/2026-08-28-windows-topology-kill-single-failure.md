# 2026-08-28 — two unexplained Windows failures in the topology kill tests, not yet a flake

**Status: a single observed run in which two tests failed together, with their
fingerprints recorded. No rate has been measured, so under `CODING_STANDARDS.md` §12
this is *not* named a flake.** Two failures in one run is one observation of a run,
not two observations of a rate: they share a job, a runner, a temp root and a test
binary, so they are not independent trials. n=1 run is not a rate.

It is recorded for the same reason as its companion record in this pull request: a
fingerprint is what makes the next red judgeable. It is **not** recorded to license
re-running until green.

## The fingerprint

| field | value |
|---|---|
| Tests | `engine::topology::attempt::tests::kill_after_snapshot_add_reclaims_snapshot_and_releases_its_commit` and `engine::topology::attempt::tests::kill_during_retry_attempt_closes_generation` |
| Assertion sites | `src/engine/topology/attempt/tests.rs:1656` and `src/engine/topology/scaffold.rs:1356` |
| Message A | `settle: Git { message: "git worktree prune failed in C:\\Users\\runneradmin\\AppData\\Local\\Temp\\upstroke-wm-killattempt-7784-0\\repo: fatal: not a git repository (or any of the parent directories): .git" }` |
| Message B | see below — it contains backticks and does not fit a table cell |
| Suite result | `FAILED. 1676 passed; 2 failed; 33 ignored; 0 measured; 0 filtered out; finished in 713.10s` |
| Platform | `test (windows-latest)` only — `lint`, `lint (macos)`, `lint (windows)`, all three MSRV legs, `test (ubuntu-latest)` and `test (macos-latest)` all success at the same sha |
| Run | `33169116985`, **attempt 1**, at `02b739970524a2431d95f71b9a41eabee47a1c96` |
| Retrieve it | `gh run view 33169116985 --attempt 1 --log-failed` (rc=0, 330,691 bytes) |
| Preserved | `~/tactus-artifacts/flakes/2026-08-28-windows-topology-kill-attempt1-run33169116985.log` |

Message B in full, as the log records it:

```
`retry`: the child must have died by `std::process::abort()`, and it ended
ExitStatus(ExitStatus(101)) — a child that reached its own `unreachable!` panics
instead, which means the injection stopped killing
```

(The log prints it on one line; it is wrapped here and nowhere else.)

## What the two messages say, read against the source rather than guessed

Both tests drive the same helper. `kill_after_snapshot_add_reclaims_snapshot_and_releases_its_commit`
calls `kill_child_and_adopt(CHILD, &dir, "after_snapshot_add")` at
`src/engine/topology/attempt/tests.rs:1618`, and `kill_during_retry_attempt_closes_generation`
calls it with the site `"retry"` at line 1683. The helper spawns the test binary as a
child, sets `UPSTROKE_TEST_KILL_SITE`, and requires the child to die by
`std::process::abort()` at that site.

- **B is the injection not firing.** `died_by_abort` failed with `ExitStatus(101)`.
  101 is the Rust panic exit code, and the helper's own message at
  `src/engine/topology/scaffold.rs:1356` states the interpretation the code itself
  intends: a child that *reached its own `unreachable!`* panics rather than aborting,
  "which means the injection stopped killing". So the child ran **past** the kill site.
- **A is a reclaim failing after a kill.** `settle_interrupted` returned
  `Git { message: "git worktree prune failed … fatal: not a git repository" }` for the
  fixture repo under the job's temp root. The path it names,
  `upstroke-wm-killattempt-7784-0\repo`, is the fixture's own repository directory, and
  `git` reports it is not one.

**What is *not* established.** Whether the two share a cause; whether either is a
production defect or a fixture-lifecycle fault on Windows; and whether the `-7784-`
component of that temp path — which has the shape of a process id — indicates a
collision between the parent and an adopted child. Each is a hypothesis this record
deliberately does not assert, because nothing here measures it.

## Why it is not attributable to the change it appeared under

It appeared on `02b7399`, the head of this pull request. That diff is **one file, 106
insertions, markdown**: `git diff --name-only 3e5212d 02b7399 | grep -vc '\.md$'`
returns **0**, and `git diff --name-only 3e5212d 02b7399 -- src/ Cargo.toml Cargo.lock
clippy.toml` returns nothing at all. A markdown-only change cannot alter process kill
injection or worktree reclamation.

**The same source passed the same Windows leg twice, hours earlier.** Pull requests #41
(`ea25033`) and #42 (`31e24cc`) branch from the same base and likewise change no file
under `src/`, verified by the same command. Their `test (windows-latest)` legs — runs
`33157987233` and `33157989853`, both 09:06:19Z — succeeded. This run failed at
11:57:15Z. Identical source, same platform, two successes and one failure, and no
change in between.

That is a **structural** argument about attribution, not a rate. §12 forbids "it passed
on re-run" as a merge justification because that launders an intermittent defect the
change *could* have caused; here the change provably could not, which is a different
claim and the only one made.

## The class

Its companion record in this pull request is a macOS `agent::proc` signal failure. This
one is a Windows failure in `engine::topology`, in different code and a different
subsystem, and the two are **not** claimed to share a mechanism. What they share is
their epistemic status: one observation each, fingerprint recorded, rate unmeasured.

The nearer relative is **pull request #36**, which measures
`workspace::tests::hard_killed_snapshot_owner_is_reclaimed_before_resume` at 1 in 44 on
Windows against 1 in 80,000 on Linux. That is also a hard-kill reclaim race with a
Windows-elevated rate, and failure A above is also a reclaim failing after a kill. The
relationship is **adjacency, not identity**: different test, different module,
different assertion, and no measurement here connects them. It is stated so that a
reader measuring one has a reason to look at the other.

## What is owed, and deliberately not done now

**A measured rate on a controlled Windows environment.** The build box has a Windows
guest and #36's harness already measures a hard-kill reclaim race on it, so — unlike
the macOS record beside this one — the environment for this measurement **does exist**.
It is not run here because this pull request is a record, not a measurement, and
because the guest's Defender exclusions are known to name a stale path, which changes
the timing the measurement would report. Both are conditions to fix before the number
would mean anything.

**A `reviews/FINDINGS.md` §2 row**, for the same reason its companion gives: #42 is
open with seven new rows in that table, and a second branch editing it would
manufacture a conflict between two of this seat's own changes. The row lands once #42
does.

**A re-run is not evidence and is not offered as any.** The CI conclusion at this head
is whatever the re-run produced; the fingerprint above is from attempt 1 and was
captured before any re-run, because `gh run rerun --failed` re-runs in place as
attempt 2 and makes the failing log non-default — the hazard the companion record ends
with.
