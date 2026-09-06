---
id: REVIEW212-VARIANT-CENSUS-IGNORES-DISCRIMINANTS
severity: P3
disposition: deferred
category: correctness
pr: 212
reviewed_sha: 2c6d93d66d6144d39a3b1414f2cc7f9b20910a4c
location: src/topology/effects/vocab.rs:1428
provenance: introduced_by_feature
first_bad: 5bb996971d9bc92eb6b1f71528b4a41cb6fcfde4
guard: the next change to `src/topology/effects/vocab.rs`
---

## Failure sequence

Location as first recorded: `src/topology/effects/vocab.rs:1428`,
`declared_variants` and the `is_variant_line` it counts with (as of the
reviewed sha).

Finding 8 of PR #212's frontier pass.

`is_variant_line` accepts a line only when, after stripping the trailing
comma and any `(..)` payload, the identifier begins with an ASCII uppercase
letter and **every remaining character is ASCII alphanumeric**. A variant
written with an explicit discriminant -- `Timeout = 2,` -- leaves
`Timeout = 2` as the identifier, whose remaining characters include a space
and `=`, so the line is not counted.

Add `Timeout = 2,` to `InjectionMode`, update the exhaustive matches the
compiler demands, and forget `InjectionMode::ALL`. `declared_variants` counts
2 where the enum now has 3, `InjectionMode::ALL.len()` is still 2, the two
agree, and `every_all_lists_every_variant_of_its_enum`
(`src/topology/effects/vocab.rs:1507`) passes. The census exists to catch
exactly that omission and cannot.

The positive control in
`the_census_reads_variants_and_not_the_text_that_looks_like_them` does not
reach the gap: its fixture holds `One,`, `Three(Payload),` and, in the
injected-violation half, `Four,` -- a simple unit variant and a tuple
variant, and no discriminant. Standards §12 asks a source instrument to be
exercised over the shapes its subject can take.

## What the change that takes this up should do

Count a variant whose line carries an explicit discriminant, by taking the
identifier as the text before the first of `=`, `(` or `{` and trimming it,
rather than by requiring the whole body to be alphanumeric. Then extend the
positive control's fixture with a discriminant variant and a struct-bodied
one, and witness the change by adding a variant to a real `pub enum` of the
file without touching its `ALL` -- the mutation the census is for -- rather
than by asserting the parser against a fixture the old parser also handles.
