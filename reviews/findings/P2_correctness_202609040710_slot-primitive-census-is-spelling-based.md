---
id: PR136-PASS3-P2-SLOT-PRIMITIVE-CENSUS-IS-SPELLING-BASED
severity: P2
disposition: deferred
category: correctness
pr: 
reviewed_sha: ead3573882c931f9c7eaf0846a81be3bffd404a8
location: src/workspace_manager/tests.rs:3585
provenance: pre_existing
first_bad: SWEEP-TESTS-SOURCE-CENSUSES-COUNT-UNBLANKED-TEXT
guard: claims corrected, no third census written. every_slot_taking_primitive_refuses_a_hostile_slot_name and slot_taking_fallible_primitives now say the…
---

## Failure sequence

`slot_taking_fallible_primitives_in` includes a `pub fn` only if its signature holds the exact strings `slot: &Slot` and `-> Result<` -> a primitive spelt `target: &Slot`, or returning `std::result::Result<…>`, is invisible to it -> the derived set, the eleven-primitive grid and the scan's own positive control all stay green while that primitive turns caller data into a path with nothing refusing for it; `seen_fns > 30` is a floor saying the scan read something, not the boundary §12 asks for. **This sweep noticed the shape at its start and did not act on it**, which is the honest provenance rather than the reviewer finding something subtle

## What the change that takes this up should do

**claims corrected, no third census written.** `every_slot_taking_primitive_refuses_a_hostile_slot_name` and `slot_taking_fallible_primitives` now say the set is matched by spelling rather than derived, name both invisible spellings, and state that adding a primitive spelt as the existing eleven are fails by name while one spelt otherwise does not. Two text censuses in this file have been walked around three times between them, so the third attempt is not being made; the shape a real derivation needs is the parent's signatures read structurally, which is queue row 11's ground

Recorded by the `src/workspace_manager/tests.rs` sweep; the row is carried out of `reviews/FINDINGS.md` in the words it
was written in.
