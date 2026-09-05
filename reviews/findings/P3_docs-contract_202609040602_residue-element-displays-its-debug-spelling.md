---
id: SWEEP-WORKTREE-008
severity: P3
disposition: deferred
category: docs-contract
pr: 
reviewed_sha: 0bff83dfa632b80a0373202613f37cce222410f9
location: src/workspace_manager/worktree.rs:118
provenance: pre_existing
first_bad: —
guard: deferred to src/topology/effects/residue_authority.rs (queue row 24), which owns the vocabulary: a Display matching its serde spelling, adopted here
---

## Failure sequence

`Residue(element)` displays `{element:?}`, the Debug spelling of a closed enum (`IndexLock`), in an operator message -> `ResidueElement` has serde `snake_case` names and no `Display` -> the message's vocabulary is the derive's, not a chosen one

## What the change that takes this up should do

deferred to `src/topology/effects/residue_authority.rs` (queue row 24), which owns the vocabulary: a `Display` matching its serde spelling, adopted here. `every_verify_failure_displays_as_a_lowercase_fragment_carrying_its_fields` pins that the text ends with the element's Debug name, so the adoption is one assertion's change

Recorded by the `src/workspace_manager/worktree.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
