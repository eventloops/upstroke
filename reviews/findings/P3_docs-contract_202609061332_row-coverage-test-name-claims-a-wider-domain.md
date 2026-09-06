---
id: SWEEP-VOCAB-003
severity: P3
disposition: deferred
category: docs-contract
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/effects/tests.rs:912
provenance: pre_existing
first_bad:
guard: the sweep of `src/topology/effects/tests.rs`, queue row 27 of `standards/SWEEP.md`
---

## Failure sequence

`src/topology/effects/tests.rs` names a test
`every_external_and_process_local_row_has_at_least_one_claimed_site` and opens
it by quoting `outputs`: "every such row has at least one Topology/Shared
site". What it iterates is `ResourceRow::ALL`, the fifteen rows that are
variants of the enum.

R20 is an external-physical row of `resource_accounting.rows` and is not one of
the fifteen. It is excluded deliberately — credential volumes are
`operator_owned`, "never created or pruned by a run", quoted at the guard in
`src/runner/container.rs:299` that refuses an invocation rather than let
`docker create` conjure the volume — so it has no effect site and could not
have a claimed one. The test is therefore correct in what it does and wider in
what it says: read at its name, it asserts a property of every
external-physical and process-local-OS row in the ledger; read at its body, it
asserts a property of the fifteen rows a run can act on.

The cost is a reader who takes the name for the claim and concludes that the
suite would catch a ledger row that acquired an effect nothing maps. It would
not: a row absent from the enum is absent from the domain.

The enum's own documentation now states both exclusion rules and says in as
many words that a claim quantified over `ResourceRow::ALL` is a claim about
the fifteen (`src/topology/effects/vocab.rs`, swept as queue row 26), and
`topology::effects::vocab::tests::no_row_of_the_logical_domain_and_not_operator_owned_r20_is_a_variant`
pins the excluded numbers so a sixteenth row cannot arrive without the rules
being revisited. The test's own name is in a file this sweep's own-file bound
excludes.

## What the change that takes this up should do

Rename the test to the domain it ranges over — for example
`every_row_a_run_can_act_on_has_at_least_one_claimed_site` — and put the R20
exclusion in the comment beside the `outputs` quotation, so the quotation and
the iteration describe the same set.
