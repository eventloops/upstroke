---
id: PR7-WRAPPERS-EMPTY-DOMAIN
severity: P1
disposition: deferred
category: security-trust
pr: 7
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: project owner — the post-v0.2 pass over PR3's layer
---

## Failure sequence

`effects::externally_reachable_fns` consults the truncating `production_region`, so for `engine/{attempt,coordinator,resume}.rs` and three siblings cut at a `#[cfg(test)] use` the **classification domain is empty**. Production `pub`-declared functions in classified modules are unclassified — 40 externally-reachable names and 20 `pub` fns across the six modules, and a **working bypass was demonstrated**: a `pub(super) fn` below the cut, called from a live topology module, passes clippy and the whole suite

## What the change that takes this up should do

Owner, as the ledger records it: project owner — **the post-v0.2 pass over PR3's layer**.

**Carried: the repair is shared enforcement machinery whose blast radius is every classified module**, which is the shape that made PR5 round 7 a revert. `mechanism` (3)'s guarantee that a topology module cannot reach an effect through a legacy wrapper **does not hold** today, and that is a live-passage failure, not hardening — it is recorded here rather than repaired because the change is to the classifier every other module's enforcement depends on, and PR7 already spent two rounds on this file. Recorded **with its measurement and its bypass** so the next slice inherits evidence rather than a rumour. This is the **fourth and fifth** occurrence of `PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN` (§4): PR7 repaired the two census instances by giving `production_code` a comment-and-string blanker, and this one is the same root cause in the function the blanker does not serve

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P1** here is this migration's judgement from the consequence described above, not the reviewer's own word.
