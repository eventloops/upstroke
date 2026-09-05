# `src/topology/mod.rs`

Extended notes for [`src/topology/mod.rs`](../../../src/topology/mod.rs).

## Module

The v0.2 execution topology uses event schema 4. The compatibility boundary is defined in
[DESIGN.md §15](../../../design/15_design_event_log_resume_run_layout.md); the build order and
remaining v0.2 work are in [§21](../../../design/21_design_versioned_scope.md).

This module declares nine children:

| Module | Responsibility |
|---|---|
| [census](../../../src/topology/census.rs) | Bounded exploration of checked fold transitions |
| [effects](../../../src/topology/effects.rs) | Typed effect sites, hooks and fault-injection records |
| [events](events.md) | Schema-4 event types and wire contracts |
| [fold](../../../src/topology/fold.rs) | Checked transitions shared by live execution and replay |
| [leases](../../../src/topology/leases.rs) | Generation, candidate and repair-lineage holdings |
| [paths](../../../src/topology/paths.rs) | Predicted and actual repository regions |
| [queue](../../../src/topology/queue.rs) | Candidate order and integration eligibility |
| [registry](../../../src/topology/registry.rs) | Task storage identities and frozen entries |
| [schema](schema.md) | Schema selection, header probing and reader compatibility |

Schema-4 writing machinery exists. The crate-private
[engine topology driver](../../../src/engine/topology.rs) creates and drives topology runs;
its [emit path](../../../src/engine/topology/emit.rs) writes through
[EventLog's topology append methods](../events/log.md). This build does not expose that driver
through the production CLI. The [engine facade](../../../src/engine/mod.rs) keeps it
`pub(crate)`, and [schema selection](../../../src/topology/schema.rs) keeps
`TOPOLOGY_ACTIVATION` at `Inactive`: fresh production runs write schema 3 and production readers
accept schemas 1 through 3. `WriterSelector::TopologyPreview` selects schema 4 for the topology
machinery exercised by tests. Existing runs continue through the sequential engine; there is no
in-flight upgrade from schema 3 to schema 4.
