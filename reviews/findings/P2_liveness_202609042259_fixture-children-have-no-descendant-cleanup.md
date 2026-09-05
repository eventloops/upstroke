---
id: PR135-REVIEW2-DESCENDANT-CLEANUP-IS-NOT-THIS-FILES-WORK
severity: P2
disposition: deferred
category: liveness
pr: 135
reviewed_sha: 1bcc12e5dfffdb2745f581c19e307ef21a241db0
location: src/workspace_manager/fixture.rs:793
provenance: pre_existing
first_bad:
guard: src/runner/host.rs (queue row 45), which already implements process-group containment
---

## Failure sequence

`run_kill_child` re-execs this test binary and waits for it. A child that spawns a grandchild and
exits leaves the grandchild running; nothing kills it and nothing waits for it. §9 requires a
subprocess integration to define descendant-process cleanup, and this one does not.

## What the change that takes this up should do

Call the host runner's containment rather than building a second copy. Killing a grandchild means
the child leads its own process group — `setpgid` and `killpg` on Unix, a job object on Windows —
which is `unsafe`, platform-split, and needs a native test per leg under §11. Two frontier passes
faulted attempts to grow that machinery inside a fixture. **Out of reach of a fixture, not merely
out of this pull request's scope**: a later pass labelling this P1 or P2 should escalate to the
owner rather than re-defer it to another sweep of this file.
