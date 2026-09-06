---
id: SWEEP-EFFECTS-REGISTRY-001
severity: P3
disposition: deferred
category: docs-contract
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/registry.rs:662
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/effects/registry.rs` (queue row 23)
---

## Failure sequence

Location as first recorded: src/topology/effects/registry.rs:662 (as of the reviewed sha)

`validate_entry` refuses two shapes with `RegistryError::MislabelledEntry`
whose `found` and `required` fields it fills with the **same** value:

* a `NoExecution` phase carrying `Evidence::Executed`, and
* a `Before`/`After`/`Point` phase carrying `Evidence::NotExecuted`,

each constructed as `found: EvidenceLabel::ExecutionObserved, required:
EvidenceLabel::ExecutionObserved`. The variant's `Display` is

    `{site}`'s `{phase}` entry is labelled {found:?} but its phase requires {required:?}

so a hand-edited `registry.json` that puts an executed-test record on a
no-execution entry — the exact confusion the phase kinds exist to prevent —
is reported to its author as "labelled ExecutionObserved but its phase
requires ExecutionObserved". The label is not what is wrong with the entry;
the evidence *shape* is, and the diagnostic names neither the shape it found
nor the shape the phase admits. §13: diagnostics identify the operation and
make the decision reconstructable; §7: variants follow decisions a caller can
make.

Reached, not theoretical: two of the twenty-five refused cells of
`the_format_admits_exactly_one_evidence_shape_and_label_per_phase_kind` land
here.

## What the change that takes this up should do

Give the two phase/evidence-shape mismatches their own variant — the shape
found and the shape the phase admits, not two copies of one label — or drop
the fields from these two constructions and say what the mismatch is. Then
extend that grid test in `src/topology/effects/tests.rs` to assert the new
variant's fields; this sweep pinned the refusal's **variant** at every one of
the thirty cells but deliberately left its fields unasserted, because
asserting `found == required` would have recorded the defect as the expected
behaviour.
