---
id: SCHEMA4-PUBLIC-WRITE-PATH-UNGATED
severity: P2
disposition: deferred
category: security-trust
pr: 
reviewed_sha: 
location: src/events/log.rs:796
provenance: pre_existing
first_bad: 
guard: the project owner — the PR12 activation slice
---

## Failure sequence

A library consumer can durably write schema-4 topology state through the checked funnel using
public API only, with no write-side activation check. Three public calls suffice: construct
`RunStarted4 { schema: TOPOLOGY_SCHEMA, … }` (25 fields, all `pub`, no `#[non_exhaustive]`,
`src/topology/events.rs:600`); validate with `TopologyLine::round_trip` (`src/events/log.rs:1242`);
open with `EventLog::open` (`:466`) and commit via `append_topology(site_for(&body), …)` (`:796`,
`:1064`), which delegates straight to `append_topology_hooked` (`:809`) with no ceiling test.
`TOPOLOGY_ACTIVATION` and `MAX_READABLE_SCHEMA` appear nowhere in `src/events/log.rs` — activation
gates *reading* only. The resulting log is state the same binary's own resume refuses by name
(`SchemaRefusal::TopologyLogUnreadable`, `src/topology/schema.rs:338`, raised at `:241`).

## What the change that takes this up should do

Add a write-side refusal with a red-first witness and a killed mutation, **and** account for
the legacy funnel's unvalidated `RunStarted.schema` field in the same change: a schema-4-only guard
would leave schema 3 still accepting any `u32`.

What this row does **not** claim: it is not a breach of the inert-by-default premise. That premise
is behavioural — production's only mint stamps schema 3, no CLI arm reaches the topology
coordinator, and the read ceiling is enforced by four const assertions. What it denies is the
stronger guarantee that a released library cannot create schema-4 logs at all, which was never
true; the legacy funnel already accepts any `pub u32` in `RunStarted.schema`, and `std::fs` binds no
downstream crate. Repair was ruled out of scope for the promotion because narrowing
`src/topology/` visibility would break public `EventSite` signatures and frozen `compile_fail`
doctests pinned to specific failure reasons.

Recorded in `reviews/FINDINGS.md` §37, carried by owner amendment with the instruction that the panel must find it triaged in the ledger rather than discover it. Severity is this migration's judgement.
