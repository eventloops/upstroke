---
id: SWEEP-TESTS-DIFF-FLAGS-EXT-AND-TEXTCONV-UNWITNESSED
severity: P3
disposition: deferred
category: correctness
pr: 
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager.rs:2622
provenance: pre_existing
first_bad: SWEEP-TESTS-CANDIDATE-DIFF-HAS-NO-TEST-IN-THIS-SUITE
guard: deferred to this file: both need a program on disk or a .gitattributes filter, which is a fixture shape this suite does not have and a half-test…
---

## Failure sequence

`candidate_diff` takes `REVIEW_DIFF_FLAGS`, whose doc says each flag defends against operator config -> the new test witnesses the recorded objects, `--binary` and colour suppression, and not `--no-ext-diff` or `--no-textconv` -> two of the seven flags could be dropped from the shared list with this suite green

## What the change that takes this up should do

deferred to this file: both need a program on disk or a `.gitattributes` filter, which is a fixture shape this suite does not have and a half-test rather than a witness if faked. `capture_diff_is_immune_to_user_diff_config` covers the same flag list for the schema-3 caller in `src/workspace.rs`, so the list is not unwitnessed in the crate

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
