---
id: SWEEP-TESTS-SAMPLED-CHILD-DUPLICATES-THE-FIXTURES-KILLABLE-CHILD
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager/tests.rs:8194
provenance: pre_existing
first_bad: SWEEP-TESTS-KILL-FINGERPRINT-WRITTEN-TWICE
guard: deferred, naming effects/allowlist.toml: this file's funnel row states in the present tense that the one type owning a live std::process::Child…
---

## Failure sequence

`SampledChild` is `KillableGitChild` with the same fields, the same spawn, the same self-recording `kill` and the same argument for why the record must be written inside the call -> `src/workspace_manager/fixture.rs` says so itself, and keeps its copy because a topology module cannot name `Command` -> two implementations of one protocol, and the reason given covers why the fixture has one rather than why this file keeps a second

## What the change that takes this up should do

deferred, naming `effects/allowlist.toml`: this file's funnel row states in the present tense that the one type owning a live `std::process::Child` here is `SampledChild` and is private, and counts this file's `Command::spawn` sites, so collapsing the type edits a reviewed governance record as well as the test. It belongs in the same change as the row below

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
