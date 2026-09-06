---
id: PR136-PASS3-P3-KILL-PAIRING-IS-A-TOCTOU-ORACLE
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha: ead3573882c931f9c7eaf0846a81be3bffd404a8
location: src/workspace_manager/tests.rs:9421
provenance: introduced_by_feature
first_bad: PR136-P4-KILL-RESULT-DISCARDED
guard: withdrawn, not repaired: the try_wait observation, the already_exited fields and the per-shape assertion are deleted, which removes the defect…
---

## Failure sequence

`SampledChild::kill` read `try_wait` and then `kill`, two observations at two instants -> if the child exits between them, on Unix it is an unreaped zombie and `kill(SIGKILL)` returns **success**, so `kill_error` is empty and `already_exited` is false -> the per-shape assertion saw nothing, another shape's kills satisfied the global floor, and a shape that killed nothing passed; on Windows the same split gives `ERROR_ACCESS_DENIED` after `try_wait` recorded false, a nondeterministic false red on a first-class target. The source's claim that the race was "one-sided, a false red and never a false green" was wrong

## What the change that takes this up should do

**withdrawn, not repaired**: the `try_wait` observation, the `already_exited` fields and the per-shape assertion are deleted, which removes the defect completely because the defect was the added observation. `kill_error` stays recorded and printed beside the per-shape counts, so pass 1's §7 discard stays closed without an unsound claim on top of it. Reproduced independently here: `Child::kill` on a zombie returns `Ok` and the following `wait` reports the child's own exit 0. **A sound version cannot be built inside this process** — two instants cannot decide it — and needs the child watched from outside, which is `src/runner/**`'s ground

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
