---
id: SWEEP-TESTS-KILL-HELPER-LAUNCH-DUPLICATES-RUN-KILL-CHILD
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager/tests.rs:6429
provenance: pre_existing
first_bad: SWEEP-TESTS-SAMPLED-CHILD-DUPLICATES-THE-FIXTURES-KILLABLE-CHILD
guard: deferred, naming effects/allowlist.toml for the same reason as the row above: this file's only Command::output site is that launch, and the funnel…
---

## Failure sequence

`a_kill_at_id_unread_aborts_before_the_id_is_recorded` builds its own re-exec of the test binary with `--exact --ignored --nocapture` and null streams -> `run_kill_child` is that launch, used by `src/engine/topology/scaffold.rs` -> the shape is written twice, and the copy here used `.output()` with both streams nulled where the shared one uses `.status()`

## What the change that takes this up should do

deferred, naming `effects/allowlist.toml` for the same reason as the row above: this file's only `Command::output` site is that launch, and the funnel row counts it. The oracle that made the test wrong is fixed here and is independent of where the child is spawned

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
