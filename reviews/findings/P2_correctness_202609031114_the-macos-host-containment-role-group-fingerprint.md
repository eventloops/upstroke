---
id: W2-MACOS-HOST-CONTAINMENT-ROLE-GROUP-FINGERPRINT
severity: P2
disposition: deferred
category: correctness
pr: 173
reviewed_sha: 65d3df535268f94705344355a037b91edc26b7f8
location: src/agent/proc.rs:336
provenance: pre_existing
first_bad:
guard: closes when the macOS leg shows no red of this fingerprint across the pull requests that merge in the week after PR #173 merges, or sooner if the owner reads the evidence as sufficient; a red after #173 prints the `GroupObservation` beside `[false]` and is a new row, not this one
---

## Failure sequence

`runner::host::tests::every_role_reaches_the_containment_points_of_this_platform` fails intermittently on macOS with *"`<role>`: the child did not lead its own process group, so the pre-exec containment step did not run for this role"*, and a `test result: FAILED. <n> passed; 1 failed` whose passed-count tracks whichever head it ran on. **Twelve sightings across six branches between 2026-09-01 and 2026-09-03**, every one confirmed by its own `... FAILED` line in its own job log rather than by a mention. **Three are on `master` itself**, and the two earliest — runs `33503020178` and `33535107935`, 2026-09-01 — are at `src/runner/host.rs:5574:13`, the pre-extraction location, so **the failure predates the W2 programme and the `W2-` prefix records when it was found, not when it began**. The rest sit at `src/runner/host/tests.rs:4220:9`, `:4227:9` or `:4229:9`, which is the same assertion relocated by successive splits

## What the change that takes this up should do

Owner, as the ledger records it: project owner / the slice that next opens the pre-exec containment path, once a controlled macOS environment can measure it.

**Open as an unexplained observation, not classified as a flake or regression.** Not diff-caused, on the cleanest counterfactual this programme has produced: `c30aca0`'s delta from `9a7fc22` is `reviews/`-only, `9a7fc22` was green (run `33776069960`, attempt 1) and `c30aca0` is red — the same tree with a markdown file added. Independently, #108 does not touch `runner::host` at all. **The failing role varies across three roles** — `probe(claude-code)` six times, `review` four, `implement` twice — **and one run settles what that means**: run `33777752620` is red on both attempts at the identical commit, naming `probe(claude-code)` then `review`. Direct evidence that any role can lose, consistent with a race in the pre-exec `setpgid` path rather than with anything specific to a role. Whether this is a face of `W1-MACOS-PROC-LATE-REAPER-SELF-SIGTERM` is **open**; the signatures differ and they are deliberately not merged on family resemblance, because that row's repair makes the question answer itself — if this shape stops recurring on heads carrying it, it was the same defect. **Member of `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`.** Full evidence: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.

## 2026-09-06: the cause, measured on the runner CI uses, and the repair (PR #173)

The observation was wrong, not the containment. `child_leads_its_own_group` asked `getpgid(pid)` in `child_created`, right after `spawn` returns, and its note said an exited, unreaped child still answers. That holds on Linux and not on XNU: `proc_exit` moves the process onto the zombie list and marks it invisible to `proc_find`, which `getpgid` uses, so a child that had already run to exit answered `ESRCH` and read as "not leading its own group" although its pre-exec `setpgid(0, 0)` had run (a failed `setpgid` in `pre_exec` makes `spawn` return `Err`, and the test would have panicked "did not run" instead). It selects the roles it does because the three that fail run this test binary with a filter matching no test, which exits in a millisecond or two, and the two that never fail run `sh -c 'exit 0'` through two `exec`s; a parent descheduled for a few milliseconds on the three-core runner looks after the former has died and before the latter has.

Measured at `65d3df5` (the oracle recording its whole lifecycle, decision unchanged), run 34001563243, job 101401235904, `macos-26-arm64` image 20260831.0337.3: the new `an_exited_but_unreaped_child_still_answers_for_its_own_group` failed with "pid 16222: before the look exited, unreaped; getpgid failed: No such process (os error 3); proc_pidinfo(PROC_PIDT_SHORTBSDINFO, zombie) answered pgid 16222 status 5; after the look exited, unreaped" (status 5 is `SZOMB`); `a_child_left_in_this_processs_group_never_answers_for_its_own` passed, so the exited record of a child in the parent's group names the parent's group. Linux passed both at the same head. Sightings to date, all `left: [false]`: eighteen across 2026-08-26 to 2026-09-05 — `probe(claude-code)` eight, `review` six, `implement` three, one (2026-08-26 at `cca1276`) whose role the ledger did not record — and none on `probe(shell)` or `gate`; the last four are runs 33884290866, 33912102180, 33912658339 and 33994257038.

**Repair:** on macOS alone, `GroupObservation::leads_own_group` lets a `getpgid` `ESRCH` fall through to the exited record's `pbsi_pgid`; every other errno, and any other platform, stays `false`. The grid's assertion keeps its sentence and its expected value and now prints the observation beside them. `PR7-MACOS-PROCESS-GROUP-FLAKE` is this fingerprint at its first sighting and is folded into this row; `PR125-CLOSE-GROUP-ORACLE-CANNOT-SEE-A-ZOMBIE-ON-DARWIN` was the hypothesis, is confirmed, and is closed by the repair.

**Kept open for confirmation only**, as `PR125-CLOSE-MACOS-READY-RED-CAUSE-UNKNOWN` was after PR #172: the mechanism is measured and repaired, and the rate before the repair was one red in roughly twenty macOS jobs, so a week of merges without this fingerprint is the confirmation, and a red carrying an observation that says `getpgid` answered a group other than the child's own would be a containment failure this row never was. The harness-killed-by-SIGTERM shape `reviews/FINDINGS.md` §43 lists beside this one is not explained by this cause and is not claimed.
