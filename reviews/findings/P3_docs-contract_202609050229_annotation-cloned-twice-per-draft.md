---
id: SWEEP-ANNOTATION-002
severity: P3
disposition: deferred
category: docs-contract
pr: 169
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/plan/markdown/drafts.rs:35
provenance: pre_existing
first_bad:
guard: `Draft::annotation` and the two `annotation()` calls in `assemble`; the activation rule in `standards/SWEEP.md`
---

## Failure sequence

`Draft::annotation` clones its owned annotation, and `assemble` calls it once to reserve explicit IDs and again to construct each task. The calls duplicate annotation strings and vectors so assembly can retain the annotation while moving other draft fields. The clone is an ownership shortcut that a later refactor can remove.

## Current triage

Re-triaged on 2026-09-05 during PR #169's second repair. The bodies of `Draft::annotation` and `assemble` are unchanged and their files remain unswept; the current edits to other functions in drafts.rs do not activate those bodies under standards/SWEEP.md. This is deferred ownership cleanup, with no failing behavior test or activated MUST breach asserted. The decision does not rely on a historical restriction against editing caller files.

## What the change that takes this up should do

Borrow the annotation while reserving IDs and move it when assembling the task, destructuring the draft to move its other fields. Holding an empty Annotation directly rather than Option<Annotation> is one possible representation. Keep observable ID reservation and task assembly behavior covered while removing the duplicate copies. The Annotation Clone derive remains necessary until its existing callers change.
