---
id: PR5-R1-CFG-TEST-SHRINKS-THE-DOMAIN
severity: P2
disposition: deferred
category: correctness
pr: 5
reviewed_sha:
location: src/agent/bin.rs
provenance: undetermined
first_bad:
guard: PR7+ implementer (the slice that owns effects::externally_reachable_fns)
---

## Failure sequence

`effects::production_region` cuts a file at its **first** `#[cfg(test)]`, so a test-only item placed among production items removes every item below it from the **wrapper-classification** domain — silently, and `mechanism` (3)'s "every pubfn of a legacy or shared module is classified" would then be true of a domain nobody drew. Measured: adding `Invocation::at` inside `impl Invocation` took five of `src/agent/bin.rs`'s functions out of the census. **Scope as of PR7:** `effects::externally_reachable_fns` and the three censuses in `src/runner/container/exec.rs` are what still read the truncating region; the four whole-tree censuses no longer do

## What the change that takes this up should do

Owner, as the ledger records it: PR7+ implementer (the slice that owns `effects::externally_reachable_fns`).

**Two of three parts closed; the third is what this row now is.** (1) The instance is repaired: the constructor lives in a `#[cfg(test)] impl` block below every production item, so `src/agent/bin.rs` is whole again, and the shrink was **loud** — `every_externally_reachable_fn_of_a_legacy_or_shared_module_is_classified` reported the five functions as "invented". (2) The *prohibition* half is closed: PR7 gave the four whole-tree censuses `effects::production_code`, which removes each `#[cfg(test)]` **item** in place instead of truncating, so a mid-file test item no longer takes the rest of the file out of those, and `effects::tests::every_production_region_that_stops_early_stops_at_a_module` pins by name the ten files whose truncating region still stops at something that is not a module. (3) What is **not** closed: `externally_reachable_fns` still calls `production_region`, so those same ten files have a classification domain that ends at their first `#[cfg(test)]`, and six modules have an empty one. Moving it to `production_code` re-derives every classification entry by hand and is a change to the generated inventories, which PR7 does not own

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
