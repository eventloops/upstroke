---
id: SWEEP-TESTS-REFUSALS-ASSERTED-BY-MESSAGE-SUBSTRING
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha: 95c5bd336986a620f1b36c2e17496717c0edae6c
location: src/workspace_manager/tests.rs:289
provenance: pre_existing
first_bad: —
guard: deferred to this file, once the parent decides its flattening: the parent turns most Refusal values into UpstrokeError::Refused { message }, so a…
---

## Failure sequence

61 assertions in the file are `message.contains(...)` over the text of a refusal -> a message substring is an assertion on the implementation's prose, not on a value, and the file already shows the stronger form where a typed value is reachable (the four snapshot tests compare against a constructed `Refusal`) -> a refusal that changed variant while keeping a word in its text would not be seen

## What the change that takes this up should do

deferred to this file, once the parent decides its flattening: the parent turns most `Refusal` values into `UpstrokeError::Refused { message }`, so a typed comparison at these sites needs `Refusal` reachable from the test, which is `src/workspace_manager.rs`'s call, queue row 11. Converting all 61 blind would also be a diff no reviewer could read against a census, which is the bound this sweep is held to

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
