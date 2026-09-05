---
id: PR7-CREATEINTEGRATION-ORDER-BACKWARDS
severity: P3
disposition: deferred
category: docs-contract
pr: 7
reviewed_sha:
location: src/topology/effects.rs:1696
provenance: undetermined
first_bad:
guard: project owner — the post-v0.2 pass over PR3's layer
---

## Failure sequence

`src/topology/effects.rs:1696` says `RefSite::CreateIntegration => Adjacent::Before(DurableEvent::RunStarted)`, and `Adjacent::Before` is documented three lines above as *"the effect is designed to be durable **before** the append is"*. `decisions.pr_sequence[8].slice_contract.side_effect_vs_event_ordering` says **"run_started before integration ref"**, and P8 creates the ref after P6 appends. The registry states this site's order axis backwards

## What the change that takes this up should do

Owner, as the ledger records it: project owner — **the post-v0.2 pass over PR3's layer**.

**Carried by owner ruling, 2026-08-24: recorded clearly and revisited once v0.2 is complete.** Not cosmetic — `Adjacent` "decides `EffectSiteId::observable_orders`, which is what the registry's order axis ranges over", so for a `fault_row: t_runstart` site the fault-injection registry demands evidence for `effect_before_event`, an ordering the production code never produces, and never demands `event_before_effect`, the one it does. **Why nothing caught it:** the only test over the value is `the_observable_orders_are_the_ones_the_adjacency_admits`, which checks that `observable_orders` agrees with `adjacent` — a function used as its own oracle, §4's class, so it is green for either value. Measured: flipping the token fails exactly two tests, `effects::tests::the_checked_in_effect_sites_json_is_what_the_enums_generate` and `topology::effects::tests::every_site_carries_the_row_fault_row_scope_and_adjacency_the_design_gives_it`, both transcriptions of the same claim. The edit is one token; the consequence is that G2 evidence for this site is owed against the other order. `src/topology/effects.rs` is the file `ff0490a` names by name

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
