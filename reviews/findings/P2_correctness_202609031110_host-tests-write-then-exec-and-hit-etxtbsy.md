---
id: W2-HOST-TESTS-WRITE-THEN-EXEC-ETXTBSY
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha:
location: src/runner/host/tests.rs:7179
provenance: pre_existing
first_bad:
guard: the slice that next changes src/runner/host/tests.rs, or whoever meets the failure again
---

## Failure sequence

`an_empty_path_entry_never_reaches_the_workspaces_own_copy_of_a_bare_name` (`src/runner/host/tests.rs:7179`) writes an executable through `marker_shim` (`:5329`) and immediately spawns it. In a gate run at `d8f4d13` the spawn failed with `"an empty entry before a real installation: a raw spawn: Text file busy (os error 26)"` — **ETXTBSY**, a concurrently-forking thread in the same process still holding a write descriptor at `execve`. A textbook write-then-exec race under a parallel harness, not a logic error. **Both functions are byte-identical from `1cbdccd` through `ae2a58f`** (`marker_shim` sha256 `f666ed74…`, 701 bytes; the test `098f21e8…`, 4489 bytes), so the race travelled unchanged through the M4, M5 and M6 splits

## What the change that takes this up should do

Owner, as the ledger records it: the slice that next changes `src/runner/host/tests.rs`, or whoever meets the failure again.

Pre-existing, not reproducible on demand, and fixing it inside a split packet would put a concurrency change in a refactor's diff. **Both prescriptions this finding has carried are refuted, which is the most useful thing in the row**: `drop` plus `sync_all` closes nothing, because the writer is `std::fs::write` and it already drops its handle; and rename-into-place does not help either, because a `fork` that inherits the descriptor inherits it whatever the path is called. A repair must demonstrate that it addresses **fd inheritance across a `fork` in another harness thread**. Misattributed by construction — the failure lands on whichever test happens to be spawning. Full derivation: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
