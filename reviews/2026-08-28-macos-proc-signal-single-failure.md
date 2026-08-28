# 2026-08-28 — one unexplained macOS failure in `agent::proc`, not yet a flake

**Status: a single observed failure with a recorded fingerprint. No rate has been
measured, so under `CODING_STANDARDS.md` §12 this is *not* named a flake.** §12
requires a numerator over a denominator of observed runs before that word is used,
and n=1 is not a rate. It is recorded now so the fingerprint exists and the class is
visible, not to license re-running until green.

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

**143 is 128+15 — SIGTERM, and the SIGTERM is the test's own.** An earlier revision of
this record said the helper "was killed on a bound", i.e. a timeout. That is wrong, and
the test's source disproves it:

- the test **sends SIGTERM itself** — `libc::kill(-pid, libc::SIGTERM)` at
  `src/agent/proc.rs:7960`;
- `wait_for_exit` returns `None` on expiry, so a timeout would have panicked at the
  `.expect("guard with an unblocked mask wakes the suspended host")` on line **7963**,
  a different message at a different line;
- the panic was at **7966**, `assert!(status.success(), "helper status: {status}…")`.

So `wait_for_exit` returned `Some(143)`. The helper **exited inside the bound**, killed
by the signal the test sent, and then failed the success assertion. No timeout occurred
and nothing was killed on a bound.

**What that makes it.** The helper died from the deliberate SIGTERM instead of handling
it and exiting cleanly — which is exactly the contract the test's name asserts, that a
guard with an unblocked mask wakes the suspended host and it terminates successfully.
The likely shape is a race between the guard unblocking its signal mask and SIGTERM
arriving. That is a **signal-delivery race in `agent::proc`**, not runner load.

**This raises the stakes rather than lowering them.** The earlier reading implied a
harmless environmental artifact. The corrected one is closer to a real defect in the
code the test exists to guard — which is a stronger reason to measure it, not a weaker
one. The mechanism remains unexplained and no rate is measured.

**A red matching this fingerprint is this failure until shown otherwise; a red that
does not match it is a regression until shown otherwise.** That is §12's rule and it
is why the fingerprint is recorded before any rate is.

## Why it is not attributable to the change it appeared under

It first appeared on `c3e5b20`, a head of pull request #40. That diff is
**documentation only**: `git diff --name-only 3e5212d c3e5b20 | grep -vc '\.md$'`
returns **0**, over 11 files and 830 insertions. A documentation-only change cannot
alter signal delivery in `agent::proc`.

This is a **structural** argument, not a probabilistic one, and the distinction
matters: §12 forbids "it passed on re-run" as a merge justification precisely because
that launders a real intermittent defect the change *could* have caused. Here the
change provably could not. #40's basis is that diff plus the eight leaf successes this
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
reasons. #40 does not need it — its merge rests on the structural argument above. A
rate measured on a hosted runner nobody controls measures that runner's load profile
rather than the code, which is the "mistake nondeterminism for provenance" error §12
names; PR #36 taught this exact lesson, where a deliberate CPU-load arm did **not**
reproduce the elevation and pointed away from CPU as the driver. And macOS runner time
is the scarcest resource here.

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
