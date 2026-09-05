---
id: PR128-PARENT-GIT-TRACE-MAKES-ABSENCE-AN-ERROR
severity: P2
disposition: deferred
category: correctness
pr: 128
reviewed_sha: dfc238c63fd7db4c9f9d8ab5f41113ad8ad56617
location: src/workspace_manager.rs:3035
provenance: pre_existing
first_bad: 7a83e69
guard: deferred to the sweep of src/workspace_manager.rs, queue row 11, with the reproduction: run with GIT_TRACE=1 and query a missing object
---

## Failure sequence

`read_only_git` inherits the environment, so `GIT_TRACE=1` puts trace text on stderr for every command -> a lookup that recognises absence only as "exit 1 with both streams empty" stops recognising it -> `object_exists` returns a Git error instead of `false` under standard Git tracing, and the engine's missing-object refusal is never produced; an unborn `HEAD` fails the same way

## What the change that takes this up should do

deferred to the sweep of `src/workspace_manager.rs`, queue row 11, with the reproduction: run with `GIT_TRACE=1` and query a missing object. The repair is for the helper that runs Git: either a trace-proof environment for inspections, or an absence test that does not depend on stderr being empty; both are decisions about how this project runs Git, which is the parent's

Recorded by the PR #128 `src/workspace_manager/residue.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
