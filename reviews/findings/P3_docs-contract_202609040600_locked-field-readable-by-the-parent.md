---
id: SWEEP-WORKTREE-004
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/workspace_manager/residue.rs:206
provenance: pre_existing
first_bad: SWEEP-WORKTREE-003
guard: deferred: the doc comment on the locked field says why it is readable and what moves
---

## Failure sequence

residue.rs compares `record.locked` with `initializing` directly -> at the reviewed base that was two sites (206 and 329) in a file PR #128 was sweeping; since #128 merged as `95c5bd3` it is one site, and this pull request did not edit that file in either state -> the field stays `pub(super)`, readable by the parent module, until that line moves, so the record is not yet private in full

## What the change that takes this up should do

deferred: the doc comment on the `locked` field says why it is readable and what moves. It was sweep_coordinator's rule of 2026-09-04 that #128's file was not this sweep's to edit while #128 was open; now that #128 has merged, the remaining line belongs to the next change that touches it, and the parent's sweep (queue row 11) reads every call site of this record. Editing a file whose own sweep has just merged, on a round with no further frontier pass, is what this pull request declines to do

Recorded by the `src/workspace_manager/worktree.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
