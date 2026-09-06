---
id: PR7-NARROWED-SURFACE-19-UNCALLED
severity: P3
disposition: deferred
category: correctness
pr: 7
reviewed_sha:
location: engine/topology.rs
provenance: undetermined
first_bad:
guard: PR8/PR12, or whichever slice next opens these modules
---

## Failure sequence

**Nineteen items in `engine::topology` have no caller at all — not in production, not in a test — and `pub` was what kept the compiler from saying so.** Narrowing `engine::topology` to `pub(crate)` (the frontier review of `75da796`, finding 1) made rustc report **328 items** dead in a lib build, which is what `production_effect = "none"` means and is silenced by `#![cfg_attr(not(test), allow(dead_code))]` in `engine/topology.rs` and `engine/assembly.rs`. **Nineteen survive that gate**, being dead in the *test* build too, and each now carries its own `#[allow(dead_code)]` naming this row: `attempt.rs` `key`; `candidate.rs` `base`; `emit.rs` `discharging`, `wrote_nothing`; `seams.rs` `harness` ×2; `startup.rs` `into_parts`, `lock` ×2, `locked`; `recover.rs` `reader` ×3, `owner` ×2, `bytes` ×2; `run.rs` `PartlyImplemented`, `owes`, `warnings`, `defer_round`. Counted at `610106b`

## What the change that takes this up should do

Owner, as the ledger records it: **PR8/PR12, or whichever slice next opens these modules**.

**This is the slice's own most-recurrent class, found by the compiler rather than by a reviewer, and two entries were already known.** `pr7/STATE.md` records "`PartlyImplemented` has no inhabitants", and S5 round 6 recorded that a doc cited `LoopBranch::owes`, "which has zero call sites" — both stood because the `pub` surface made every item externally reachable in principle, so `dead_code` never fired. Seven review rounds and a withheld mutation catalogue did not find the other seventeen; one visibility change did. **Not deleted here, and the reason is that each is a judgement.** Several are typestate accessors that exist so a proven value can be taken apart (`bytes`, `owner`, `reader`, `into_parts`) and the tree argues for keeping some of them on their own docs; `PartlyImplemented` is a variant the ladder may yet construct. Deleting nineteen items across seven files at the end of a repair round, each needing its own reading, is the shape PR5's round 7 was reverted for. **What is enforced meanwhile**: the allows are per item, not per module, so a *new* uncalled item is still an error at `-D warnings`, and this row is the list a future reader diffs against

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
