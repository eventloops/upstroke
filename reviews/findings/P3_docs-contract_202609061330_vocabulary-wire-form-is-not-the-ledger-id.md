---
id: SWEEP-VOCAB-001
severity: P3
disposition: deferred
category: docs-contract
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/vocab.rs:25
provenance: pre_existing
first_bad:
guard: the change that regenerates `effects/residue-classes.json` and `effect_sites.json` together — it is the only one that can move the spelling and the checked-in artifact in one commit
---

## Failure sequence

`FunnelGroup`, `ResourceRow`, `FaultRow` and `SubEffectPoint` each carry
`#[serde(rename_all = "snake_case")]` and each also carries an accessor that
spells the same value the way the design's own tables spell it —
`FunnelGroup::name` ("RunDir"), `ResourceRow::name` ("R9"), `FaultRow::id`
("T-CAND-OBJ"), `SubEffectPoint::name` ("IdUnread"). The two spellings are not
the same, nothing crosses them, and both reach artifacts a reader opens.

Generate the inventory (`topology::effects::effect_sites_json`) and one record
of `effect_sites.json` reads:

    "site":      "RunDir.PublishCommitRecord",
    "group":     "run_dir",
    "row":       "r9",
    "fault_row": "t_cand_obj",

— three spellings of two identities in four adjacent fields. `site` is the
dotted name built from `FunnelGroup::name`, `group` is serde's rename of the
same enum, and `row` and `fault_row` are neither the ledger's `R9` nor the
matrix's `T-CAND-OBJ`. `effects/residue-classes.json`, which is compared
byte-for-byte by `effects::tests::the_checked_in_residue_class_record_is_what_the_enums_generate`,
carries the same mixture in one record: `"group": "Worktree"` beside
`"row": "r9"`, because its generator
(`effects::tests::artifacts::residue_record`) calls `.name()` for the group and
lets serde spell the row.

Nothing misreads it today: no consumer outside this crate parses those fields,
and the crate round-trips its own form. The cost is paid by a person
cross-reading a gate-attached artifact against the resource ledger or the
transaction fault matrix, who has to know the convention to see that `r9` and
`R9` are one row. That is why this is P3 and not P2.

The two types whose accessor and wire form do agree — `DurableEvent::kind` and
`Host::name` — are what make the other four read as drift rather than as a
convention.

Not repaired in the sweep of `src/topology/effects/vocab.rs` (queue row 26)
because the repair is not confined to that file: changing `rename_all` to
per-variant `rename` regenerates `effects/residue-classes.json` and
`effect_sites.json`, which the sweep's own-file bound and the standing rule
against touching the `effects/` allowlists both exclude.

The divergence is now a pinned fact rather than an accident:
`topology::effects::vocab::tests::the_wire_form_is_not_the_ledger_id_for_the_four_types_that_have_one`
asserts it, and
`topology::effects::vocab::tests::every_vocabulary_value_writes_the_snake_case_of_its_variant`
pins the rule that produces it.

## What the change that takes this up should do

Decide one spelling per identity and make the accessor its single source:
replace `rename_all = "snake_case"` on those four enums with per-variant
`#[serde(rename = "...")]` matching `name()`/`id()`, or keep the wire form and
delete the accessors that contradict it. Then regenerate
`effects/residue-classes.json` and re-run
`effects::tests::the_checked_in_residue_class_record_is_what_the_enums_generate`
and `the_checked_in_effect_sites_json_is_what_the_enums_generate`, and invert
`the_wire_form_is_not_the_ledger_id_for_the_four_types_that_have_one` — that
test is written to fail when this finding is taken up.
