---
id: PR5D-PROCESS-FUNNEL-TAKES-NO-SITE
severity: P2
disposition: deferred
category: security-trust
pr: 5
reviewed_sha: 
location: src/runner/host.rs
provenance: pre_existing
first_bad: 
guard: the slice that owns `src/runner/**` (PR6/PR7 implementer)
---

## Failure sequence

`decisions.effect_site_inventory.identity` requires every effectful funnel API to take its
group's site by value and to call `hook(Before, site) -> primitive -> hook(After, site)`. PR4's
process funnel does neither: `HostRunner::run` threads a `SpawnHooks` observer and consults
containment sub-effect points by name, and `ProcessSite` appears nowhere in the production half of
the tree. `Process.Spawn` and `Process.Terminate` are the only two claimed sites in the inventory
that no funnel names, which is how
`effects::tests::every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent`
found it. So `effect_sites.json`'s `module` column is not true of `Process.*`.

## What the change that takes this up should do

Change PR4's funnel signature so the site travels with the call. This is a shape gap and not
a coverage one — the hooks do fire and are driven under witness and fault at all eight containment
points (`runner::host::tests::every_role_reaches_the_containment_points_of_this_platform`,
`a_fault_armed_at_any_containment_point_stops_any_role`) — so the change is to the signature, not
to the behaviour, and `src/runner/**` is frozen, which is why PR5 could not make it.

Recorded in `reviews/FINDINGS.md` §8, and likewise absent from §35's and §38's audit tables.
