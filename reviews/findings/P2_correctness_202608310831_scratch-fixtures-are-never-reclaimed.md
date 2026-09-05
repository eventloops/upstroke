---
id: PR7-SCRATCH-FIXTURE-LEAK
severity: P2
disposition: deferred
category: correctness
pr: 7
reviewed_sha:
location: src/rundir.rs
provenance: undetermined
first_bad:
guard: project owner / whichever slice owns shared test infrastructure
---

## Failure sequence

`src/rundir.rs`'s `scratch` calls `remove_dir_all` at **creation**, keyed by `{tag}-{pid}` — §16 records it in full. PR7 is the slice that pays for it: the suite grew from 1385 tests to **1644**, and the leak scales with the suite

## What the change that takes this up should do

Owner, as the ledger records it: project owner / whichever slice owns shared test infrastructure.

**Carried, unchanged in disposition from §16 and now with a second measurement.** The build box reached **19% of 58.5M inodes**; sweeping leaked fixture directories returned it to **12%** — on the order of **4.1 million inodes** that were leaked test fixtures, roughly a third of everything in use. `df -h` read 31% throughout. Held out of this slice for the reason §16 gives — the repair is a judgement call across 60+ call sites in shared test infrastructure, the PR5-round-7 shape — and mitigated out of tree by a sweeper with a 30-minute age floor so it cannot race a running suite. **PR7 raises the urgency rather than the difficulty**: parallel execution multiplies the fixture count per wall-clock hour. **2026-08-26: on Windows this stopped being a disk problem and became a correctness one.** The guest suite at `5e309a0` returned **16 failures** — fourteen in `engine::topology::emit::tests` and two in `settle::tests` — every one of them `assert!(bytes.is_empty(), "a fresh run has no prefix")` at `emit/tests.rs:324`. The same guest, minutes later, was **green at `040a100`** (1651 + 10, 0 failed), so it is not a regression in the diff. `emit/tests.rs`'s `run_paths` keys its scratch on `{tag}-{pid}-{n}` and **Windows recycles pids**: `%TEMP%` held **11,395** leaked `upstroke-*` directories, and grouping the `upstroke-emit-*` ones by their pid component gave six previous processes with 25-34 directories each. A run that draws a recycled pid finds its "fresh" fixture already populated and fails on the emptiness assertion. Sweeping `%TEMP%` to zero and re-running the same head is the control. **What this changes about the row**: the Linux symptom is inode exhaustion and is mitigated out of tree by a sweeper; the Windows symptom is a **fresh-run fixture that is not fresh**, it is indistinguishable from a real defect in the reviewed head, and no sweeper prevents it — the fix is that a fixture root includes something a recycled pid cannot supply, or removes its own directory at creation the way `rundir::scratch` does. Recorded here rather than repaired for the reason the row already gives: 60+ call sites in shared test infrastructure

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
