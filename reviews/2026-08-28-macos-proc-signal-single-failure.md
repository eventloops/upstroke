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

Where 143 comes from is not a guess. `128 + 15` is constructed in exactly one place in
`src/agent/proc.rs` — line **2301**, `libc::_exit(128 + terminating)` — at the end of the
terminal-signal handler, after it SIGKILLs the isolated groups, restores `SIG_DFL` and
calls `raise(terminating)`. Every other `_exit` in that file passes 0 or 1. The comment
above it calls that line *"a defensive fallback if a platform returns from `raise`"*.

What the evidence therefore supports, and no more:

- the helper **ran the terminal-signal handler** rather than finishing its work and
  exiting 0, which is what `assert!(status.success(), …)` at line 7966 requires;
- it left that handler through the **`_exit(143)` fallback** rather than dying inside
  `raise` — otherwise the status would read `signal: 15`. On this run that fallback was
  **not** dead code.

**What is not established, and is no longer asserted.** Whether `raise` returned or the
handler reached `_exit` by some other route. Whether the helper's taking the handler at
all is a race with the `finish` write that releases the supervised worker, which happens
one line after the test's `kill`. And in particular the **guard-mask initialisation
race** an earlier revision named: guard initialisation and unblocking complete before
the supervised child is spawned and long before the test sends SIGTERM, so that
sequence does not fit. That hypothesis is withdrawn.

**Why the correction still raises rather than lowers the stakes.** The first reading
implied a harmless environmental artifact. This one says a documented *defensive*
fallback in production signal-handling code executed on a supported platform, in a test
whose entire subject is signal delivery under a blocked mask. The mechanism remains
unexplained, and it is a better reason to measure than either earlier reading was.

**A red matching this fingerprint is this failure until shown otherwise; a red that
does not match it is a regression until shown otherwise.** That is the rule
`reviews/FINDINGS.md` §12 applies to the flake it measures — *"A red on this test after
a push is **this flake until proven otherwise**"* — carried here by analogy, and it is
why the fingerprint is recorded before any rate is.

**The fingerprint's matching rule, stated so it can be applied.** Match on the test
name, the assertion site (`src/agent/proc.rs:7966`), and the status **form** —
`exit status: 143`, not `signal: 15 (SIGTERM)`, because that distinction is the whole
correction above. Do **not** match on the suite totals or the elapsed time; those move
with the tree. A red at 7966 reading `signal: 15` is a *different* failure and this
record does not cover it.

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

**What survives, and is the actual claim.** No markdown change alters the *logic* of
signal delivery in `agent::proc`: the handler at `proc.rs:2288-2301`, the mask
manipulation and `wait_for_exit` are untouched, and `git diff` proves no file under
`src/` differs. So the failure cannot be a behavioural regression introduced by that
diff. It can only be a pre-existing condition, possibly perturbed in timing by a
different test workload — which is a reason to measure it, not a reason to attribute it.

This is a **structural** argument about the code, not a probabilistic one, and the
distinction matters: the recorded practice refuses "it passed on re-run" as a merge
justification precisely because that launders a real intermittent defect the change
*could* have caused. #40's basis is that diff plus the eight leaf successes this
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
