---
id: SWEEP-ANNOTATION-002
severity: P3
disposition: deferred     # the sites are in drafts.rs (row 65) and assemble.rs (row 64)
category: docs-contract
pr: 169
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/plan/markdown/drafts.rs:35
provenance: pre_existing
first_bad:
guard: `Draft::annotation` and the two `annotation()` calls in `assemble`
---

## Failure sequence

`Draft` holds `ann: Option<Annotation>` and `Draft::annotation` returns
`self.ann.clone().unwrap_or_default()`. `assemble` calls it once per draft to reserve explicit
ids and once more per draft to build the task, so every annotation — six `String` vectors — is
cloned twice, and the clone exists to satisfy the borrow checker: `assemble` needs `ann` while
it moves `draft.title`, `draft.body` and `draft.acceptance` out of the draft. §6 names that
clone as the exception to justify, not the default.

## What the change that takes this up should do

Hold `Annotation` (whose `Default` is "absent") rather than `Option<Annotation>` in `Draft`,
return `&Annotation` from `annotation`, and destructure the draft in `assemble` so the fields
move and the annotation is borrowed or moved with them. `Annotation` keeps `Clone` until then;
`annotation.rs` is swept and the derive is documented.
