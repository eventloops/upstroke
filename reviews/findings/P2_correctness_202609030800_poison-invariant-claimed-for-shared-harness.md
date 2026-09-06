---
id: PR119-POISON-INVARIANT-CLAIMED-FOR-SHARED-HARNESS
severity: P2
disposition: deferred
category: correctness
pr: 119
reviewed_sha: 43a9acdcdc0227e2daa2d69f32d73bc68147fa84
location: src/workspace_manager/hooks.rs:194
provenance: pre_existing
first_bad: 7a83e69 (the adapters' `unwrap_or_else(PoisonError::into_inner)` shape predates this pull request)
guard: the claim is fixed at 0d09ca4df07728d55fcb637e2508cd4145aaf1d2 to what holds, HarnessEffects alone, and the type's doc names the four adapters…
---

## Failure sequence

the doc and the body say "a poisoned harness is never recorded into" for the shared `HookHarness` -> only `HarnessEffects` honours it, while `HarnessEventHooks` (`src/events/log.rs:298` at that head) and the run-directory, container and spawn adapters recover a poisoned guard and call `hook` -> open a fast sequence while holding the shared mutex, panic, then drive an Event-family phase -> that adapter records into the abandoned sequence and manufactures the false coverage evidence the claim says is prevented

## What the change that takes this up should do

the claim is fixed at 0d09ca4df07728d55fcb637e2508cd4145aaf1d2 to what holds, `HarnessEffects` alone, and the type's doc names the four adapters that still recover and record; the adapters themselves are deferred to the sweeps of their files, `src/rundir.rs`, `src/events/log.rs`, `src/runner/container.rs` and `src/runner/mod.rs`, which are not edited here

Recorded by the PR #119 `src/workspace_manager/hooks.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
