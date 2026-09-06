---
id: PR110-SITE-CENSUS-MATCHES-EFFECT-SITE-NAMES-BY-SUBSTRING
severity: P2
disposition: deferred
category: correctness
pr: 110
reviewed_sha:
location: src/effects/tests.rs:2625
provenance: pre_existing
first_bad:
guard: the slice that next changes the site census in src/effects/tests.rs
---

## Failure sequence

`every_site_the_inventory_declares_has_a_funnel_that_names_it_or_is_recorded_absent` (`src/effects/tests.rs:2625`) decides that a funnel names a site by plain substring containment — `if source.contains(&variant)` at `:2677` — so **a longer variant satisfies a search for a shorter one**: `WorktreeSite::RemoveExecutionRoot` (`src/workspace_manager.rs:747`) satisfies a search for `WorktreeSite::Remove`. Remove the exact shorter literal while keeping the longer one and the census stays green while the removed site goes unnoticed. **The exposure is a class and it is enumerable**: every group's sites share one funnel module, so a within-group prefix collision is a same-file collision, and at `ae2a58f` there are **ten collision pairs over six shorter variants in four groups** — `WorktreeSite::{Add, Remove, RemoveStaging}`, `SnapshotSite::Remove`, `ContainerSite::Remove`, `EventSite::Append`

## What the change that takes this up should do

Owner, as the ledger records it: the slice that next changes the site census in `src/effects/tests.rs`.

Pre-existing and **not activated** by #110: all six shorter variants are still present as exact literals — not merely as substrings — in their own funnel modules, measured by counting matches not followed by an identifier byte, so the collisions have nothing to hide yet. Verified by the steward before proposing it and by #110's reviewer independently. **Fix the class, not the pair**: repairing only the collision the reviewer named leaves the other nine. A count #110's body quoted was under-counted by this same weakness and was stripped under ruling 10; **the finding survives that, because the census weakness is independent of whether any body quotes a number.** Same family as `PR103-CENSUS-DOMAIN-CANNOT-DECIDE-EXCLUSIVE-TEST-REACHABILITY` and `PR107-CONTAINER-LINT-CENSUS-DOMAIN-IS-A-DIRECTORY-WALK`. Full table of collisions: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
