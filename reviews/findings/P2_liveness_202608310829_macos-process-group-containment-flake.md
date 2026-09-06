---
id: PR7-MACOS-PROCESS-GROUP-FLAKE
severity: P2
disposition: deferred
category: liveness
pr: 7
reviewed_sha:
location: src/runner/host.rs:5565
provenance: undetermined
first_bad:
guard: project owner / whichever slice next opens src/runner/host.rs
---

## Failure sequence

**`runner::host::tests::every_role_reaches_the_containment_points_of_this_platform` fails intermittently on `test (macos-latest)` and nowhere else**, asserting *"review: the child did not lead its own process group, so the pre-exec containment step did not run for this role"* — `left: [false]`, `right: [true]`, at `src/runner/host.rs:5565` **as it stands at `75da796`**. **Measured over the last 20 CI runs on this branch, 13 of which completed a macOS job: 11 success, 2 failure** — and then a **third sample at the same sha**: `gh run rerun 32999498916 --failed` re-ran only the failed jobs of `75da796`'s run, without a push, and `test (macos-latest)` **passed**, taking the run to 9/9 success. So the tally is 12 success / 2 failure over 14 completed macOS jobs, and one head has now produced both outcomes with the tree byte-identical, which is what makes it a flake rather than a defect in the head. Both failures are this test, at `cca1276` (14:00) and `75da796` (18:24) on 2026-08-26; every other completed macOS job on the branch back to `f6ed9f1` passed. The Linux, Windows and both other MSRV legs pass in the same runs, and the guest and this box never reproduce it. **Not caused by the diff it appeared on**: `d17bcf2..75da796` is `reviews/FINDINGS.md`, a new review record, and two doc-comment stampings in `run.rs` and `run/tests.rs` — nothing that can reach a process group

## What the change that takes this up should do

Owner, as the ledger records it: **project owner / whichever slice next opens `src/runner/host.rs`**.

**Recorded with its rate rather than described, and not chased in this slice.** The assertion is that the spawned child leads its own process group after the pre-exec step; a macOS runner under load losing that for one role out of a grid, twice in thirteen, is either a real race in `pre_exec` ordering or a runner-side artifact, and **this session cannot tell those apart** — it has no macOS host, and the two observations are CI logs. **What a repair would need first** is a way to reproduce: a macOS runner the slice can drive, or a CI job that runs this one test in a loop and reports a rate. Adding either is out of scope here and neither is a `src/` change. §12 is the precedent for carrying a flake with numbers; `PR5-MACOS-CLIPPY-NEVER-RUN` in §2 is the standing observation that this project has no macOS host at all, which is the same gap one gate over

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
