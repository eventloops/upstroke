---
id: PR7-STEP-D-LINEAGE-ARM-UNWITNESSED
severity: P3
disposition: deferred
category: correctness
pr: 7
reviewed_sha:
location: topology/events.rs
provenance: undetermined
first_bad:
guard: PR8 implementer (the slice that gives the merge queue a repair to spawn)
---

## Failure sequence

**Recovery step (d) handles `LeaseDisposition::LineageHeld` and no test can reach that arm.** Catalogue entry `PR7-PIPELINE-008` adds `if lease == LineageHeld { continue; }` to `settle_interrupted`'s loop and the whole suite stays green. The loop is **already correct** — this is a coverage gap, not a defect

## What the change that takes this up should do

Owner, as the ledger records it: PR8 implementer (the slice that gives the merge queue a repair to spawn).

**Carried with a condition sharper than the one the catalogue implies, and measured.** `LineageHeld` is produced only by `GenerationLease::InheritedLineage`, which only a **repair task** holds, and a repair task exists only after a `task_spawned` carrying `Origin::MergeRepair`. Measured over `effects::production_code`: the only `TaskSpawned {` constructions in the tree are the frozen layer's own definitions (`topology/events.rs`, `topology/fold.rs`) and `engine/topology/scaffold.rs`, which is `#[cfg(test)]`. **No production path in this slice spawns a repair**, so the arm is unreachable by construction rather than by width — PR8's merge queue is what makes it live, not PR11's parallelism. **Why it is carried rather than witnessed**: the fixture would have to seed a `task_spawned` whose `FrozenSpawn.entry` is a registry entry derived outside the fold — the scaffold's `spawn_repair` reads the live registry to build one, and `Damage::extra` is assembled before any fold exists. That is a different construction from the sibling gap `PR7-PIPELINE-010`, which **was** repaired in-slice this round (`Damage::two_tasks`, `steps_d_and_e_reach_every_generation_not_the_first`) because it was the loop-versus-first shape a second task settles

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
