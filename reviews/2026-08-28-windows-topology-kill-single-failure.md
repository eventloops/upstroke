# 2026-08-28 — two unexplained Windows failures in the topology kill tests, not yet a flake

**Status: a single observed run in which two tests failed together, with their
fingerprints recorded. No rate has been measured, so this is *not* named a flake.**

**The authority for that, corrected.** An earlier revision cited `CODING_STANDARDS.md`
§12. **That section says nothing about flakes, rates, re-runs or intermittency** — the
words "flake", "numerator" and "denominator" appear nowhere in that document. The
practice being followed is this repository's own, at **`reviews/FINDINGS.md` §12**,
which measures a flake at *"one failure in ~31 runs (~3%)"* and says *"which is why the
rate is written down here as a number instead of as 'occasionally'"*. Precedent, not
rule, and cited as such.

Two failures in one run is one observation of a run, not two observations of a rate:
they share a job, a runner, a temp root and a test binary, so they are not independent
trials.

**The denominator that is visible, and why it is still not a rate.** The two sibling
jobs cited below as a control are also two more observations of this leg on this source:
one failure in three same-source Windows jobs. That number is stated rather than hidden.
It is **not** called a rate because the three were not a designed trial — they ran hours
apart on different hosted allocations, and this record's own control argument treats
them as evidence about *attribution*, which needs only that the source was identical,
not as draws from one distribution, which would need much more. **Observations 3,
failures 1, rate unmeasured** is the honest line, and it is weaker than a rate on
purpose.

It is recorded for the same reason as its companion record in this pull request: a
fingerprint is what makes the next red judgeable. It is **not** recorded to license
re-running until green.

## The fingerprint

| field | value |
|---|---|
| Tests | `engine::topology::attempt::tests::kill_after_snapshot_add_reclaims_snapshot_and_releases_its_commit` and `engine::topology::attempt::tests::kill_during_retry_attempt_closes_generation` |
| Assertion sites, verbatim from the Windows log | `src\engine\topology\attempt\tests.rs:1656:10` and `src\engine\topology\scaffold.rs:1356:5` |
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

- **B is a child that panicked instead of aborting — and on Windows the oracle cannot
  say why.** `died_by_abort` failed with `ExitStatus(101)`. The message at
  `src/engine/topology/scaffold.rs:1356` reads that as the injection having stopped
  killing, and **this record does not repeat that reading**, because on Windows the
  oracle cannot support it. `died_by_abort` there is `!status.success() && status.code()
  != Some(101)` (`src/workspace_manager.rs:3884`) — a pure negation of the panic code,
  chosen because Windows exposes no portable `abort` signature. And `run_kill_child`
  sets `.stdout(Stdio::null())` and `.stderr(Stdio::null())` (`:3774-3775`), so the
  child's panic message is discarded and the parent sees only the code.

  **Therefore any child panic produces this exact message.** A child that panicked
  *before* the kill site was armed — a `settle_retry` returning something the arm
  expects not to, say — exits 101 and yields Message B verbatim, while being a genuine
  regression rather than an injection failure. What is established is only: **the child
  exited 101, which is a Rust panic, and the parent required an abort.** Whether it ran
  past the kill site is *not* established, and an earlier revision of this record said
  it was.
- **A is a reclaim failing after a kill.** `settle_interrupted` returned
  `Git { message: "git worktree prune failed … fatal: not a git repository" }` for the
  fixture repo under the job's temp root. The path it names,
  `upstroke-wm-killattempt-7784-0\repo`, is the fixture's own repository directory, and
  `git` reports it is not one.

**What is *not* established.** Whether the two share a cause; whether either is a
production defect or a fixture-lifecycle fault on Windows; whether the `-7784-`
component of that temp path — which has the shape of a process id — indicates a
collision; and, per the correction above, whether B's child reached the kill site at
all. Each is a hypothesis this record deliberately does not assert, because nothing
here measures it.

**The matching rule, stated so the fingerprint can be applied.** Match Message A on the
test name, the assertion-site suffix `engine/topology/attempt/tests.rs:1656:10` after
normalising path separators, and the shape
`git worktree prune failed in <temp>\repo: fatal: not a git repository` — **normalise
the whole temp path away.** `upstroke-wm-killattempt-7784-0` embeds a process id and an
ordinal that differ on every run, so a literal match on it can never fire twice and
would report every recurrence as a new failure. Match Message B on the test name, the
assertion-site suffix `engine/topology/scaffold.rs:1356:5` after normalising path
separators, and exit code **101** — and note that this
signature is *broad*: it also matches a child that panicked for an unrelated reason, so
a red matching B is this observation **or** a regression, and the two are not separable
from the parent's output alone. That is a limitation of the oracle, recorded here rather
than papered over.

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

That is a **structural** argument about attribution, not a rate: the change could not have
caused this failure, which is a different claim from "it passed on re-run" and is the only
one made here.

**On "it passed on re-run", corrected a second time.** An earlier revision said
`CODING_STANDARDS.md` §12 forbids that reasoning; it contains no such rule. A later
revision replaced that with "the recorded practice refuses" it, and **that is also
unsupported** — no document in this repository forbids it, and `reviews/FINDINGS.md` §12
says the opposite for the flake it measured: *"check the failing test name before
treating it as a regression, and **re-run** rather than repairing forward."* So the
restraint this record practises is **this seat's choice, not a repository rule**, and it is stated as a
choice: a green re-run is not offered here as evidence of anything, because the failure it
would launder is unattributed rather than measured. Where §12's flake has a rate and a
ruled-out cause, this one has neither, and re-running an unidentified failure to green is
how a rate never gets measured.

**And this record's structural claim is stronger than its companion's**, which is worth
stating because the companion's had to be weakened. `DESIGN.md` and
`decisions/README.md` are compiled into the test binary by `include_str!`
(`src/export.rs:1091`, `:1153`, `:1168`), so a markdown edit to *those* files does change
the binary. **This diff touches neither**: everything it adds is under `reviews/`, which no
`include_str!` in the tree reads. The binary here really is byte-identical to the base's,
which is the property "provably could not reach the failing code" actually requires. No
file count is stated, because the count changes as this branch commits the reviews of its
own heads and an earlier revision of this sentence went stale exactly that way.

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

**A `reviews/FINDINGS.md` §2 row**, for the same reason its companion gives: the
project-wide ledger has a separate exclusive writer lease while parallel pull requests
are being reconciled. The row is deferred to that serialized ledger writer and the
planned consolidated findings sweep; this record remains the durable provenance until
that work lands.

**A re-run is not evidence and is not offered as any.** The CI conclusion at this head
is whatever the re-run produced; the fingerprint above is from attempt 1 and was
captured before any re-run, because `gh run rerun --failed` re-runs in place as
attempt 2 and makes the failing log non-default — the hazard the companion record ends
with.
