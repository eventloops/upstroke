---
id: PR136-PASS2-P3-DIED-BY-ABORT-IS-A-NEGATION-FOR-ITS-OTHER-CALLER
severity: P3
disposition: deferred
category: portability
pr: 
reviewed_sha: 142c321144729517024e6f632737d6e79918cc12
location: src/workspace_manager/fixture.rs:475
provenance: pre_existing
first_bad: PR136-PASS2-P2-ABORT-ORACLE-ACCEPTS-AN-EXIT-OF-ONE
guard: deferred to src/workspace_manager/fixture.rs, queue row 9 and the subject of open PR #135, so this session does not edit it: the repair is the one…
---

## Failure sequence

`died_by_abort`'s Windows arm accepts every unsuccessful exit that is not 101 -> this file no longer relies on it alone, but `src/engine/topology/scaffold.rs` does, through `run_kill_child` -> a kill injection that became any non-101 failure there would read as an abort, which is the same defect one file over

## What the change that takes this up should do

deferred to `src/workspace_manager/fixture.rs`, queue row 9 and the subject of open PR #135, so this session does not edit it: the repair is the one made here, a measured abort status rather than a described one, and `abort_probe_helper` is the shape to lift. Reported to the coordinator rather than made, per this sweep's bound on another pull request's file, and routed by them to #135 while it is mid-repair on that file. **The measurement that made the routing obvious**: the Unix legs cannot see this mutation at all — with `Injection::Kill` changed to `process::exit(1)`, `a_kill_at_id_unread_aborts_before_the_id_is_recorded` fails on Linux at both the reviewed and the repaired head, because the Unix arm names `SIGABRT` — so no Linux run was ever going to witness a repair through that test, on this file or on `scaffold.rs`. If #135 lands it first this row becomes fixed there rather than disappearing

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
