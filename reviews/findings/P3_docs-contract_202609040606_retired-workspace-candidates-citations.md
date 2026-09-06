---
id: SWEEP-WORKTREE-014
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha: c0eb8c5d35517b9402f7ddd5dfc46eb0c8c9ee42
location: src/workspace_manager/worktree.rs:5
provenance: pre_existing
first_bad: PR131-P2-INVENTED-DESIGN-AND-TRUST-AUTHORITY
guard: deferred: the mapping is DESIGN.md's to state (a retired-records row for workspace_candidates, or the section that now carries its substance), and…
---

## Failure sequence

`src/` cites `decisions.workspace_candidates.*` at 51 sites, and `DESIGN.md`'s retired-records table names no such record -> a reader following a citation from the code reaches a record retired on 2026-09-03 and no design section -> the tree's design authority for the workspace-candidates rules is unlocatable from the code, which pass 2 found by reading this file's copy of it

## What the change that takes this up should do

deferred: the mapping is `DESIGN.md`'s to state (a retired-records row for `workspace_candidates`, or the section that now carries its substance), and 50 of the 51 citations are other files'; this file quotes the record as the record's own words meanwhile, which is the only claim it can support

Recorded by the `src/workspace_manager/worktree.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
