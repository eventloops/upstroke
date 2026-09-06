---
id: SWEEP-EFFECTS-REGISTRY-DUPLICATE-SYNTHETIC-RECORD
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/registry.rs:211
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/effects/tests.rs` (queue row 27), or any change that makes a reader count `synthetic` rather than search it
---

## Failure sequence

`Evidence::RecoveryProven`'s `synthetic` field documents itself as "One record
per element the site's class lists", and the format does not enforce the "one".
`validate_entry` asks two questions of the list — every element the site lists
has at least one record (`MissingSyntheticElement`), and every record's element
is one the site lists (`UnlistedSyntheticElement`) — and neither is violated by
a list that names one element twice.

So: a registry document for `Object.CandidateCommitTree`, whose class lists
`UnreferencedObject` and `TemporaryObjectFile`, may carry three records —
`UnreferencedObject` twice and `TemporaryObjectFile` once — and `insert`
accepts it. `check_bijection` then reads every record and holds each to
`constructed`, `recovered` and `classified`, so the duplicate admits no false
claim: a second record that lies is reported exactly like a first one that
lies, and two truthful records are redundant rather than wrong. What is left is
a stated contract the format does not hold the document to, and a
`synthetic.len()` that is not the element count any reader would expect it to
be.

The same shape, and the same absence of consequence, holds for
`Evidence::NotExecuted`'s `sequences`: a record may name one fast sequence
twice. `check_bijection` asks `claimed.contains(&sequence.name())`, which is
membership, and reports an unknown name once per occurrence.

## What the change that takes this up should do

Decide which of the two the document is, and make the code say it. Either the
"one record per element" sentence is the contract, in which case the format
gains a refusal for a repeated element (and one for a repeated sequence name)
with a test that a duplicate is rejected; or a list is a bag and the sentence is
reworded to say so, with a test that a duplicate is accepted and that the
bijection still reads every record. Do not add a refusal without the test:
tightening a format against a shape that admits no false claim is managed debt,
not a repair, and the reason it is being tightened belongs in the commit.
