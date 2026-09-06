---
id: SWEEP-REGION-001
severity: P3
disposition: deferred
category: docs-contract
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold.rs:177
provenance: pre_existing
first_bad:
guard: queue row 40, `src/topology/fold.rs` — the sweep of the module that declares `FoldError`
---

## Failure sequence

`FoldError::InvalidLeaseDisposition` declares `recorded: String` and `expected: String`, and
its sole construction site — `check_lease_disposition` in `src/topology/fold/region.rs` —
fills both from a `LeaseDisposition`, a `Copy` enum of three variants. The typed value is in
hand at the construction site and is thrown away one line later.

Two consequences. A caller or a test that wants to know *which* disposition was refused compares
strings: at `ee5dc81f` the only assertions on this variant in the tree are two
`matches!(…, FoldError::InvalidLeaseDisposition { .. })` in `src/topology/fold/tests.rs`, which
pin no field, and a test that wanted more had to be written against the rendered sentence. And the
variant carries `fate: &'static str`, which is `"closes"` at that one construction site and can
be nothing else: every caller of `check_lease_disposition`
(`check_attempt_finished`'s `Closed` settlement, `check_attempt_interrupted`,
`check_generation_closed`) passes a generation that closes, which is why the function passes
`false` to `GenerationLease::expected`. A field with one reachable value reads as a dimension
the error varies over and does not.

This is a typing finding, not a wrong answer: the refusal fires on exactly the right events and
its sentence is correct.

## What the change that takes this up should do

Carry `recorded: LeaseDisposition` and `expected: LeaseDisposition` on the variant and give
`LeaseDisposition` a `Display` (or keep `region.rs`'s `disposition_name` and have the
`#[error]` attribute call it), so the values stay typed to the edge and the message keeps its
present words. Drop `fate` unless a second construction site with a different fate arrives with
it. Both are edits to `src/topology/fold.rs`, outside the bound of the row 37 sweep that found
this, which is why the disposition is named in `region.rs` rather than typed here.
