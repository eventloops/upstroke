---
id: SWEEP-DRAIN-009
severity: P3
disposition: deferred
category: correctness
pr: 154
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/agent/proc.rs:330
provenance: pre_existing
first_bad:
guard: the sweep of `src/agent/proc.rs` (queue row 51), which owns `run_with_timeout_and_limit`'s stdin feeder; `drain::Drain::start` is the shape to copy
---

## Failure sequence

`run_with_timeout_and_limit` has spawned the child and registered it, then hands stdin to a
feeder with `thread::spawn` -> the OS refuses the thread (`EAGAIN` from `pthread_create`: the
process is at `RLIMIT_NPROC`, or the box is out of threads or of address space for a stack) ->
`std::thread::spawn` panics with "failed to spawn thread" rather than returning the `io::Error`
`thread::Builder::spawn` would -> the coordinator unwinds out of the funnel with the child alive:
on Unix the `termination::Supervisor` guard's `Drop` cancels the reaper, which settles the group,
and on Windows the private Job's kill-on-close handle does the same, so nothing is orphaned — but
an OS resource outcome (§7: "scheduling outcomes") ends the coordinator by panic instead of by a
typed `UpstrokeError::Agent` the engine could record and escalate, and the two pipe readers
started three lines later were changed in PR #154 to return that error and terminate the tree,
so the funnel now handles the same refusal two different ways within one screen of code.

## What the change that takes this up should do

`thread::Builder::new().name("stdin-feeder").spawn(...)`, the shape `drain::Drain::start` takes,
mapping the `io::Error` into `UpstrokeError::Agent` and terminating the child the way the reader
arm added by PR #154 does (Unix: `termination.finish()`, `kill`, `wait`; Windows: `kill_tree`).
The same body's `let _ = pipe.write_all(&stdin_bytes)` discards every write error and not only the
broken pipe its comment names; row 51 should decide whether an `EIO` on stdin is best-effort
(§7's rule: then its observability is defined and its failure path tested) or a supervision
failure.
