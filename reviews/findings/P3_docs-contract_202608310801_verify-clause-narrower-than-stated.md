---
id: PR5-VERIFY-CLAUSE-NARROWER-THAN-STATED
severity: P3
disposition: deferred
category: docs-contract
pr: 5
reviewed_sha:
location: src/workspace_manager.rs:2802
provenance: undetermined
first_bad:
guard: project owner — for the G2 erratum list
---

## Failure sequence

`slice_contract.proof_tests[8]` says each of **eight** synthetic residue elements "classifies Internal, **fails `Worktree.Verify`**, and forced removal succeeds", and `command_internal_sub_effects` says the same of its synthetic evidence. For **two** of them — `UnreferencedObject` and `TemporaryObjectFile` — the suite asserts `Worktree.Verify` **passes**, and the implementation (`element_breaks_quiescence`, `src/workspace_manager.rs:2802`) says so on purpose. Twelve of the frozen 24 (site, element) pairs satisfy the clause and twelve cannot

## What the change that takes this up should do

Owner, as the ledger records it: project owner — **for the G2 erratum list**.

**The behaviour is right and the sentence is over-general; recorded because an unrecorded live sentence the behaviour does not satisfy is a defect until an owner rules.** Both elements live in the *shared object store*, are R27 ("Git's"), and are left by ordinary Git use — every amended commit leaves an unreferenced object. A `Worktree.Verify` that consulted the object store would fail on essentially every worktree in every real repository, and `decisions.workspace_candidates.generation` requires a quiescent worktree to be **reusable**; forcing the clause would make `OpenNoAttempt` reuse impossible and the tabled recovery non-convergent. Measured rather than argued (PR5-CONF-006, Fable PR5-CONF-003): Sol predicted a survivor and the flip of `element_breaks_quiescence` is **KILLED** — the partition is pinned hard in both directions. What it is pinned *against* is the implementation's own `const fn`, which is the reason this row exists rather than a repair: the suite cannot both hold the packet's sentence and hold the behaviour. **Not repairable in this slice** — the alternative is failing quiescence for every innocent worktree — and the erratum wanted is one clause on `proof_tests[8]` naming the two object-store elements as exceptions

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
