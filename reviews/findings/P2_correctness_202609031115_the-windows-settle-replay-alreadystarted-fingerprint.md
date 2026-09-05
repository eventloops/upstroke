---
id: PR107-WINDOWS-SETTLE-REPLAY-ALREADYSTARTED-FINGERPRINT
severity: P2
disposition: deferred
category: correctness
pr: 107
reviewed_sha:
location: 
provenance: pre_existing
first_bad:
guard: project owner / the slice that next opens the Windows engine::topology::settle harness
---

## Failure sequence

Two `engine::topology::settle` kill tests fail together on the Windows guest with `the log replays: AlreadyStarted` — `kill_after_failed_settlement_rematerializes_question` at `src\engine\topology\settle\tests.rs:1764:56` and `retained_generation_not_continued_after_kill` at `:1807:60`, `test result: FAILED. 1760 passed; 2 failed; 35 ignored`. Run `33785587535`, attempt 1, job `100749444333`, `test (winguest)`, at `9963fb0` on PR #107; `upstroke-ci` concluded failure on the back of it

## What the change that takes this up should do

Owner, as the ledger records it: project owner / the slice that next opens the Windows `engine::topology::settle` harness.

**Open as one unexplained observation, not classified as a flake or regression.** **Its own ID deliberately, and NOT folded into `PR104-WINDOWS-SETTLE-PATH-HINT-FINGERPRINT`**: that row is a predicted-region trailing-slash mismatch with `MalformedEntry { kind: "task_dispatched", key: 0 }`, and **the string `src/aleph` appears zero times in this job log** — checked in a local copy. Same module, same leg, same two tests, **different assertion**; folding two fingerprints into one record is how a class stops being countable. Nondeterministic, established by the same head passing twice and failing once, all `attempt=1` so nothing is hidden inside a row: `33784774150` success, `33785587535` **failure**, `33786611538` success — and the red run was started by a **body edit**, not a code change. **Not a regression from the C-004 repair**: a regression would be deterministic and this is not. What would settle it is wider than the path-hint derivation the sibling row names — whether these two tests build their event log deterministically on Windows at all. **Not rerun**; no licence covers this signature. **Member of `CLASS-INTERMITTENT-SUBPROCESS-KILL-SETTLE-RESIDUE-FAILURES`.** Full evidence: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
