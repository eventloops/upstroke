---
id: PR4-DESIGN-ROLE-SCOPED-ENV
severity: P3
disposition: deferred
category: docs-contract
pr: 4
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: project owner
---

## Failure sequence

**A wording ambiguity inside one paragraph of DESIGN.md.** :260 says the runner *"supplies role-scoped `HOME`, `PATH`, and credential locations"*; :262-264, three lines later, says *"Probe and execution compose the **same** base, mounts, reserved values, and overlay, so pre-flight certifies the environment that will actually spend."* Probe and execution are **different roles**, so a per-role `HOME` or `PATH` value makes pre-flight certify an environment the attempt will not run in — the second sentence constrains how the first must be read

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

Raised by the independent final confirmation as `PR4-CONF-001`, which read :260 alone. PR4 resolved it by scoping **credential locations** by role while `HOME`/`PATH` stay the host boundary's own, and grounded that in :263, packet :331-333 and :341-342 — the only reading that satisfies both sentences. Two pre-existing tests already enforced the second sentence by name, so the alternative reading would have required deleting a guard on the passage it implements. **Recorded rather than closed** because the ambiguity is in the source document, not in the code, and the same shape as `PR3-RUNNER-DIGEST`. If the owner reads :260 as requiring per-role values, PR4's disposition is the thing to revisit, and it is a design change rather than a repair

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
