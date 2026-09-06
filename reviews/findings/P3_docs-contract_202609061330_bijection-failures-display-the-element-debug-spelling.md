---
id: SWEEP-RESIDUE-AUTHORITY-001
severity: P3
disposition: deferred
category: docs-contract
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/bijection.rs:79
provenance: pre_existing
first_bad: —
guard: the `src/topology/effects/bijection.rs` sweep, or any change to those three `#[error]` texts; `ResidueElement::wire_name` and its `Display` now exist for it to adopt
---

## Failure sequence

`BijectionFailure::ResidueElementNotConstructed`, `ResidueElementNotRecovered` and
`ResidueElementMisclassified` each write `{element:?}` into their `#[error]` text ->
a reader of a failed ST-07 bijection sees `IndexLock`, the derive's `Debug` spelling ->
the same element is `index_lock` in every document the registry serialises, so one element
has two names and neither is stated to be the other

## What the change that takes this up should do

Write `{element}` rather than `{element:?}` in the three texts.
`src/topology/effects/residue_authority.rs` gained `ResidueElement::wire_name` and a
`Display` forwarding to it on branch `sweep/topology-effects-residue-authority`, pinned to
the serde spelling by `every_residue_element_displays_the_spelling_serde_writes`, so the
adoption is the three format specifiers and whatever assertion in
`src/topology/effects/tests.rs` quotes the current text.

This is the same defect as `SWEEP-WORKTREE-008` in a second file; that finding names
`src/workspace_manager/worktree.rs:118` and this one names these three. Both are outside the
`residue_authority.rs` sweep's bound, which is why the vocabulary half landed there and the
adoption did not.
