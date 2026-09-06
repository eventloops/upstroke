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
guard: the vocabulary half landed in src/topology/effects/residue_authority.rs (queue row 24) as ResidueElement::wire_name and its Display; what is left is the adoption here, in src/workspace_manager/worktree.rs
---

## Failure sequence

`Residue(element)` displays `{element:?}`, the Debug spelling of a closed enum (`IndexLock`), in an operator message -> `ResidueElement` has serde `snake_case` names and no `Display` -> the message's vocabulary is the derive's, not a chosen one

## What the change that takes this up should do

`src/topology/effects/residue_authority.rs` (queue row 24) owns the vocabulary and now has the
`Display`: its sweep, on branch `sweep/topology-effects-residue-authority`, added
`ResidueElement::wire_name` and a `Display` forwarding to it, pinned to the serde spelling by
`every_residue_element_displays_the_spelling_serde_writes`. What is left is the adoption here —
`{element}` rather than `{element:?}` — and
`every_verify_failure_displays_as_a_lowercase_fragment_carrying_its_fields` pins that the text ends
with the element's Debug name, so it is one assertion's change. The adoption was outside that
sweep's bound, which is why it did not land with the `Display`. `SWEEP-RESIDUE-AUTHORITY-001` is the
same defect in `src/topology/effects/bijection.rs`

Recorded by the `src/workspace_manager/worktree.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
