---
id: W1-FIXTURES-NOT-RETIRED-W0-AUTH-PART-E-UNFULFILLED
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha:
location: src/validate.rs
provenance: pre_existing
first_bad:
guard: the slice that takes up retirement, which is blocked behind the src/validate.rs scratch-directory row
---

## Failure sequence

W0-AUTH Part E said: retire `fixtures/` and inline the corpus. **`fixtures/` survives** — `bare-plan.md`, `cyclic-plan.md`, `sample-plan.md`, `steps-plan.md`. What PR #104 as landed did achieve, re-derived at `ae2a58f`: every runtime fixture read **outside** `src/validate.rs` is gone; `src/plan/mod.rs` takes the corpus at compile time through `include_str!` (`:82`, `:87`, `:91`) and `src/plan/markdown.rs` and `src/topology/registry.rs` (`:3123-3125`) consume those constants. **`src/validate.rs` is the one remaining runtime reader, with 10 call sites** of the form `opts("fixtures/<name>.md")`; `cyclic-plan.md` is the one file with no constant and its only consumer is `src/validate.rs:739`

## What the change that takes this up should do

Owner, as the ledger records it: the slice that takes up retirement, which is blocked behind the `src/validate.rs` scratch-directory row.

Recorded so an unfulfilled packet clause is not later read as a fulfilled one. It stopped here because `src/validate.rs` is frozen-legacy and every attempt to give its tests a corpus on disk produced a new finding about temporary-directory ownership — five across four repair rounds, then three more at pass 8 — and owner ruling 7 reverted the file rather than ship the ninth. **What is owed**: retirement needs `src/validate.rs`'s tests to stop reading from disk, which is `PR104-VALIDATE-SCRATCH-DIRECTORIES-PREDICTABLE-AND-UNRECLAIMED`'s problem for all ten call sites at once. Doing that row first makes retirement straightforward; doing it second is what produced eight passes. Full derivation: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
