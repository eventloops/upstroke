---
id: PR110-CONTAINMENT-COMMENT-STATES-A-FALSE-GUARANTEE
severity: P2
disposition: deferred
category: security-trust
pr: 110
reviewed_sha:
location: src/workspace_manager/containment.rs:83
provenance: pre_existing
first_bad:
guard: the slice that next changes src/workspace_manager/containment.rs, remove_intent, remove_execution_root or Slot::validate
---

## Failure sequence

`src/workspace_manager/containment.rs:83` states *"every deletion **in this subsystem** goes through `WorkspaceManager::contained`, which compares **canonical** paths, so a resolved link cannot carry a removal outside the root."* **It is FALSE, not stale** — recorded in those words deliberately, because "pre-existing, referent updated" reads as a bookkeeping nit and this is a false containment assertion in a security comment. Of the six deletion sites in the subsystem's production region at `ae2a58f`, **one** goes through `contained()`: the checkout removal in `remove_worktree` (`src/workspace_manager.rs:1215-1216`). `remove_intent` (`:842`) reaches `fs::remove_file` after `slot.validate()?` and `self.revalidate()?` with **`contained()` never called**; `remove_execution_root` (`:760`, `:766`) does not call it either, and neither do `remove_worktree`'s own `locked`-file removal (`:1232`) or its admin-tree removal (`:1256`). `contained()` has exactly one production call site in the whole subsystem

## What the change that takes this up should do

Owner, as the ledger records it: the slice that next changes `src/workspace_manager/containment.rs`, `remove_intent`, `remove_execution_root` or `Slot::validate`.

**What actually provides the containment, which the comment does not name**: `Slot::validate` (`src/workspace_manager/naming.rs:189`) calls `safe_component` (`:136`). So the subsystem is safe on that path **by a different mechanism than the one documented** — the dangerous case, not the harmless one, because a refactor that weakens `safe_component` or adds a deletion path skipping `validate` will be reading a comment promising a guard that does not run there. The real guard is load-bearing on two of three `Slot` arms (`Staging` holds a `u64` and has nothing to validate, `:192`), so the **documented** guard is the real one on none of them. A **three-state trace** is why nobody caught it: the claim was false at base, made **vacuous** by the split, and false again once the repair widened its referent — so at any single state it reads as either a pre-existing defect or a clean repair. Ruled out of scope by #110's pass-2 reviewer. Full table of the six sites: **§43**

Appended to `reviews/FINDINGS.md` §2 by §43, the 2026-09-03 W1/W2 decomposition review, which recorded it as pre-existing: neither introduced nor activated by the diff in front of it. The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
