---
id: PR5D-ROW-MAPPING-REFUSAL-UNFIXTURED
severity: P3
disposition: deferred
category: docs-contract
pr: 5
reviewed_sha: 
location: src/topology/effects.rs
provenance: pre_existing
first_bad: 
guard: the slice that next edits `src/topology/effects.rs` (PR6/PR7 implementer)
---

## Failure sequence

`expected_failures_refusals[7]` states that a site without a row mapping fails to compile.
The refusal is real and structural — `EffectSiteId::row()` and each group's `row()` are `const fn`
matches with no wildcard, so an unmapped variant is `error[E0004]` — but no fixture in the tree
exercises it, unlike the other four build-refusal clauses, which each have a pinned fixture. A
stated compile-time refusal with no executed proof is the shape of
`PR5-C-DOCTEST-FIXTURES-NEVER-RAN`.

## What the change that takes this up should do

Add a fixture that adds a variant to the enum and asserts the build refusal, with a positive
control so a broken toolchain invocation cannot make every fixture "refuse". It could not be done
from PR5: it requires adding a variant to a frozen enum in a file the 2026-08-20 ruling froze by
name, and a fixture in a separate crate would test its own enum rather than this one. The row says
so rather than pointing at a test that does not exist.

Recorded in `reviews/FINDINGS.md` §8 and counted among its two carried rows by §35.
