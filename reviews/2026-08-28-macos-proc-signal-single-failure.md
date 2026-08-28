# 2026-08-28 — one unexplained macOS failure in `agent::proc`, not yet a flake

**Status: a single observed failure with a recorded fingerprint. No rate has been
measured, so this is *not* named a flake.**

**The authority for that, corrected.** An earlier revision of this record cited
`CODING_STANDARDS.md` §12. **That section says nothing about flakes, rates, re-runs or
intermittency** — the words "flake", "numerator" and "denominator" do not appear
anywhere in that document, and §12 is about deterministic tests and censuses. The
practice being followed is this repository's own, recorded at **`reviews/FINDINGS.md`
§12**, which measures a pre-existing flake at *"one failure in ~31 runs (~3%)"* and says
in its last line *"which is why the rate is written down here as a number instead of as
'occasionally'"*. That is a **precedent this record follows**, not a rule it is
compelled by, and it is cited as such from here on.

**Why n=1 here, when two denominators are visible.** This head's CI shows one failure
and one green re-run of the same commit, which is a 1-of-2 among attempts; that is a
number, and it is deliberately not called a rate. Two attempts of one commit on one
hosted runner is an opportunistic sample, not a designed trial: the second attempt is
not an independent draw from the same distribution — it ran later, on a different
machine allocation, after the first had already perturbed nothing observable. The
recorded practice measures ~31 runs and 80,000 runs, not two. What is honest is
therefore: **observations 1, rate unmeasured**, with the attempt count stated rather
than hidden.

It is recorded now so the fingerprint exists and the class is visible, not to license
re-running until green.

## The fingerprint

| field | value |
|---|---|
| Test | `agent::proc::tests::a_blocked_terminal_signal_still_wakes_a_suspended_host` |
| Assertion site | `src/agent/proc.rs:7966:9` |
| Message | `helper status: exit status: 143` |
| Suite result | `FAILED. 1714 passed; 1 failed; 31 ignored; 0 measured; 0 filtered out; finished in 215.25s` |
| Platform | `test (macos-latest)` only — `lint`, `lint (macos)`, `lint (windows)`, all three MSRV legs, `test (ubuntu-latest)` and `test (windows-latest)` all success at the same sha |
| Run | `33162906210`, **attempt 1**, at `c3e5b20a2b8db9ffd8bb59684cd225bdefe43c2f` |
| Retrieve it | `gh run view 33162906210 --attempt 1 --log-failed` (rc=0, 330,968 bytes) |
| Preserved | `~/tactus-artifacts/flakes/2026-08-28-macos-proc-signal-attempt1-run33162906210.log` |

**What 143 means — the third reading, and the first that the formatter supports.**

Two earlier revisions of this record were wrong about this, in different directions. The
first said the helper "was killed on a bound", i.e. a timeout. The second said it was
"killed by the signal the test sent". **Both are refuted by the one word the log
actually prints.**

Rust's Unix `ExitStatus` Display prints **`signal: 15 (SIGTERM)`** for a process
terminated by a signal and **`exit status: N`** for one that exited with a code. The log
says `exit status: 143`. So the helper was **not** killed by a signal: it **exited, with
code 143**.

Where 143 is constructed is not a guess: `128 + terminating` appears once, at
`src/agent/proc.rs:2301`, in the monitor's terminating path, and it runs when the atomic
`PENDING_TERMINATION` holds a signal number. **Everything past that point is where the
three earlier readings went wrong, and this one deliberately stops there.**

**What the evidence establishes:** the helper reached the monitor's terminating path with
`PENDING_TERMINATION == SIGTERM` and exited 143, rather than finishing its work and
exiting 0, which is what `assert!(status.success(), …)` at line 7966 requires.

**What it does not establish, and what earlier revisions wrongly asserted:**

- **Which route set `PENDING_TERMINATION`.** It has several writers. One is the
  terminal-signal handler. Another is `Supervisor::finish` at
  `src/agent/proc.rs:1883-1889`, which assigns SIGTERM when `reaper.cleanup` fails —
  **no signal need have been handled at all.** A released worker exiting normally, a
  reaper cleanup failure, and the monitor reaching `_exit(143)` produces this exact
  fingerprint. A third revision of this record said the handler "ran"; it is not shown.
- **That reaching `_exit` rather than dying in `raise` means anything.** It does not, and
  the reason is in this test's own setup: the helper is spawned with
  `UPSTROKE_BLOCK_SIGNAL` set to **SIGTERM** — its tag `job-control-blocked` selects
  SIGTERM at `proc.rs:7466` — and the monitor unblocks **only SIGCONT**
  (`proc.rs:2127`). A blocked signal stays pending and `raise` returns zero, so falling
  through to `_exit` is the **expected** behaviour here, not an anomaly. The claim that
  a "defensive fallback" fired unexpectedly is **withdrawn**.
- **The guard-mask initialisation race** an earlier revision named. Guard initialisation
  completes before the supervised child is spawned and long before the test's `kill`, so
  that sequence does not fit. Withdrawn and not replaced.

An earlier revision also claimed that every other `_exit` in that file passes zero or one.
That is false, and **the correction an intervening revision offered was itself a wrong
count** — arrived at by a `grep -c` that counted a mention in a doc comment alongside the
calls. Both are withdrawn, and no count replaces them: the claim was never load-bearing,
because the construction that yields this status is unique in the file, and asserting an
incidental tally is how a checked-sounding fact enters without being checked.

**What is left is smaller and still worth recording.** A supervised helper reached the
terminating path when the test required a clean exit, on macOS only, once. The route is
unknown, the rate is unmeasured, and this record says so rather than choosing among the
candidates.

**The rule, stated once, because an earlier revision stated two and they contradicted.**
It first carried `reviews/FINDINGS.md` §12's rule by analogy — *"this flake until proven
otherwise"* — and then said the opposite a few lines later. **The analogy does not hold and
is withdrawn.** §12's oracle identifies its flake; this fingerprint does not identify
anything, so the rule here is weaker and is the only one:

> **A failure matching this fingerprint is *unresolved* until the reaper-cleanup path is
> ruled out.** It is not this observation until shown so, and it is not a regression until
> shown so. What the match establishes is which monitor path ran, and the cause is open.

A failure that does **not** match — in particular one reading a signal termination rather
than an exit status — is a different failure and this record does not cover it.

**The fingerprint's matching rule, stated so it can be applied — and its width, stated so
it is not trusted too far.** Match on the test name, the assertion site
(`src/agent/proc.rs:7966`), and the status **form**: `exit status: 143`, never
`signal: 15 (SIGTERM)`, because that distinction is the whole correction above. Do **not**
match on the suite totals or the elapsed time; those move with the tree.

**A match does not identify the cause.** Because `PENDING_TERMINATION` has several
writers, a red matching this fingerprint is **this observation *or* a regression in reaper
cleanup**, and the two are not separable from the test's output. So the rule is: a
matching red is not automatically "this failure" — it is *this fingerprint*, and it still
needs the cleanup path ruled out. That is weaker than the rule
`reviews/FINDINGS.md` §12 states for the flake it measured, and it is weaker on purpose:
that flake's oracle identifies its cause and this one's does not.

## Why it is not attributable to the change it appeared under

It first appeared on `c3e5b20`, a head of pull request #40. That diff is
**documentation only**: `git diff --name-only 3e5212d c3e5b20 | grep -vc '\.md$'`
returns **0**, over 11 files and 830 insertions.

**"Provably could not reach the failing code" is too strong, and is withdrawn.** That
head changes `DESIGN.md` and `decisions/README.md`, and both are compiled **into the
test binary** by `include_str!` — `src/export.rs:1091`, `:1153` and `:1168`. So the
binary that ran was not byte-identical to the base's, the tests that scan those strings
did different work, and they run in the same process as the timing-sensitive signal
test. That is a concrete path, however thin, by which a markdown change can alter
whether a race manifests. It does not attribute the failure to #40; it removes the word
"provably".

**What survives, and it is weaker than the previous sentence made it sound.** No markdown
change alters the *logic* of signal delivery in `agent::proc`: the handler, the mask
manipulation and `wait_for_exit` are untouched, and `git diff` proves no file under `src/`
differs. So **no new defect was introduced into that code**.

That is the whole of it. An earlier revision went on to say the failure therefore "cannot"
be a behavioural regression and "can only" be pre-existing — **and that does not follow from
the paragraph above it**, which had just conceded that changed embedded text alters the
binary and the concurrent workload. A latent race that manifests *because* the workload
changed is caused, in the ordinary sense a maintainer cares about, by the change that
altered it. Both sentences are withdrawn.

**What is left is a distinction, not an exoneration:** the diff cannot have introduced the
defect, and it may well have changed whether the defect showed. Which of those happened is
unmeasured, and that is the reason to measure rather than a reason to attribute or to
dismiss.

This is a **structural** argument about the code, not a probabilistic one, and the
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
how a rate never gets measured. #40's basis is that diff plus the eight leaf successes this
run did have — the `CI` workflow has **nine** leaf jobs, three `lint`, three `msrv` and
three `test`, besides the `upstroke-ci` aggregate, and attempt 1 was eight successes and
one failure with `upstroke-pr-policy` green in its own workflow. An earlier revision of
this record said "ten green legs", a number that counts nothing on this run; #40's body
retracted the same phrase, and repeating it here would have left the retraction
half-applied across two documents. The re-run's green is **corroborating, not
load-bearing**.

## The class — this is the third, and the second on macOS

| prior | test | platform | measured rate |
|---|---|---|---|
| `reviews/FINDINGS.md` §12 | `agent::proc::tests::pid_directed_termination_kills_a_suspended_tree_without_continue` | Linux | 1 in ~31 (~3%) |
| `PR7-MACOS-PROCESS-GROUP-FLAKE` (§2) | `runner::host::tests::every_role_reaches_the_containment_points_of_this_platform` | macOS only | 2 in 14 completed macOS jobs |
| PR #36 | `workspace::tests::hard_killed_snapshot_owner_is_reclaimed_before_resume` | Linux 1 in 80,000; Windows 1 in 44 | measured |

This one sits at the **intersection of the first two**: it is in the same module as
§12's and turns on the same subject — a *suspended* process that must be woken or
reaped — and it is macOS-only like `PR7-MACOS-PROCESS-GROUP-FLAKE`. Unlike either, its
corrected mechanism points at the **code under test** rather than at the environment. That is the
observation worth carrying: `agent::proc`'s supervision code has now produced two
distinct suspended-process timing failures, and macOS has produced two distinct
process failures. Neither pattern is visible from one record alone.

## What is owed, and deliberately not done now

**A measured rate, when there is a controlled macOS environment.** Not now, for three
reasons. #40 does not need it — its merge rests on the narrowed structural argument
above, that no markdown change alters signal-delivery logic. A rate measured on a hosted
runner nobody controls measures that runner's load profile rather than the code; PR #36
taught this exact lesson, where a deliberate CPU-load arm did **not** reproduce the
elevation and pointed away from CPU as the driver. And macOS runner time is the
scarcest resource here.

**#36's harness does not transfer.** It measures Linux and a local Windows guest; this
failure is macOS-only so far, and there is no equivalent macOS guest on the build box.

**A `reviews/FINDINGS.md` §2 row** naming this with an owner. Not added here on
purpose: pull request #42 is already open with seven new rows in that same table, and
a second branch editing it would manufacture a conflict between two of this seat's own
changes. The row lands once #42 does.

## The recording hazard this exposed

`gh run rerun <id> --failed` re-runs **in place, as attempt 2 of the same run id**.
Afterwards `gh run view <id> --log-failed` returns attempt 2 — green — and the run
reads as a success with no trace of the failure. An attempt to save this log *after*
triggering the re-run returned 0 bytes; `--attempt 1` returned 330,968. **Capture
attempt-N evidence before re-running**, and cite the `--attempt N` form so a later
reader can reach it.
