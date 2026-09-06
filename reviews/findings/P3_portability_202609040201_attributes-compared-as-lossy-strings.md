---
id: SWEEP-PARSERS-008
severity: P3
disposition: deferred
category: portability
pr: 
reviewed_sha: f458cfc6d4470970744c3950f20a5a108ac0d1fe
location: src/workspace_manager/parsers.rs:238
provenance: pre_existing
first_bad: —
guard: deferred: the field types are worktree.rs's (queue row 8) and the comparison is the parent's (row 11); the doc comment on parse_worktree_records…
---

## Failure sequence

the attributes are read with `from_utf8_lossy` into the `String` fields of `WorktreeRecord` -> `assert_publishable` (`src/workspace_manager.rs:1407` at the base) compares `branch` with a refname as identity, which §8 says a lossy string never is -> a branch named with non-UTF-8 bytes on Unix cannot equal any `&str`, and its lossy spelling equals only a refname that itself contains `U+FFFD`, so the comparison can over-refuse on that collision and never under-refuse

## What the change that takes this up should do

deferred: the field types are `worktree.rs`'s (queue row 8) and the comparison is the parent's (row 11); the doc comment on `parse_worktree_records` states what each attribute is read for

Recorded by the `src/workspace_manager/parsers.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
