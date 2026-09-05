---
id: PR163-ASTRA-RUSTDOC-LINKS
severity: P3
disposition: deferred
category: docs-contract
pr: 163
reviewed_sha: f346f28e6be23c5f9bd3f8decfa64e025dea5c91
location: docs/internals/runner/container/runtime.md:26
provenance: introduced_by_feature
first_bad: 32e87f8170088dc19a73c04bc30184e53014bd44
guard: Future internal-notes maintenance should render representative type and cross-module references with a Markdown parser and require their links to resolve.
---

## Failure sequence

Open the new runtime notes as Markdown and follow the RuntimeOp reference at line 26. The source uses Rustdoc shortcut syntax, [`RuntimeOp`], with no Markdown reference definition. A CommonMark parser emits text, an inline-code token, and text for this reference. It emits no link. The same problem affects the RuntimeError and ContainerRuntime::probe references at lines 45 through 47 and other notes in this migration.

The review's read-only source audit counted 269 occurrences of Rustdoc shortcut syntax across the 11 new notes. The rendered runtime note has only the explicit opening source link. Evidence is in source-audit-v2.json and runtime-note-commonmark.html under the review's evidence-f346f28e6be23c5f9bd3f8decfa64e025dea5c91 directory. The finding concerns lost documentation navigation. It demonstrates no runtime or security regression.

The owner authorized deferral under DOCS_FAST_TRACK.md and STACK_STOP_RULE.md. Preserve this record and its canonical ledger row before final handoff. No additional repair cycle is required for this finding.

## What the change that takes this up should do

Convert Rustdoc shortcut references to ordinary Markdown links or supply explicit reference definitions. Link to the relevant notes section or to source at a matching revision, and check representative cross-module references after rendering.
