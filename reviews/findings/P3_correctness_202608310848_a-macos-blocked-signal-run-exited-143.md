---
id: PR43-MACOS-PROC-SIGNAL-FINGERPRINT
severity: P3
disposition: deferred
category: correctness
pr: 43
reviewed_sha:
location: reviews/2026-08-28-macos-proc-signal-single-failure.md
provenance: undetermined
first_bad:
guard: project owner / the slice that next opens src/agent/proc.rs, once a controlled macOS environment can measure it
---

## Failure sequence

One macOS run of `agent::proc::tests::a_blocked_terminal_signal_still_wakes_a_suspended_host` reached the monitor's terminating path with `PENDING_TERMINATION == SIGTERM` and exited 143 instead of completing cleanly; the writer and reason are unresolved, and no rate has been measured

## What the change that takes this up should do

Owner, as the ledger records it: project owner / the slice that next opens `src/agent/proc.rs`, once a controlled macOS environment can measure it.

**Open as an unexplained observation, not classified as a flake or regression.** Durable provenance and the exact matching boundary are in `reviews/2026-08-28-macos-proc-signal-single-failure.md`: test name, assertion site and the status form `exit status: 143` (not signal termination). Run `33162906210`, attempt 1, at `c3e5b20`; one failed and one green attempt are visible, but that opportunistic pair is not promoted to a rate. Several writers can store SIGTERM, including reaper-cleanup and the SIGCONT guard fallback, so a matching red remains unresolved until its writer and reason are established. This row fulfills the record's deferred §2 commitment after the PR #42 serialization boundary; it does not strengthen the record's causal claim.

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
