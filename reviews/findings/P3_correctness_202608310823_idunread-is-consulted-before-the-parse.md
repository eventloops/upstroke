---
id: PR5-R2-IDUNREAD-BEFORE-THE-PARSE
severity: P3
disposition: deferred
category: correctness
pr: 5
reviewed_sha:
location: 
provenance: undetermined
first_bad:
guard: PR6/PR7 implementer
---

## Failure sequence

`PR5-WORKSPACE-045`. `commit_tree` consults `IdUnread` before parsing the child's printed id, and the three `IdUnread` tests all run against a child that succeeds and prints a well-formed id — so moving the point *after* the parse changes nothing they can see

## What the change that takes this up should do

Owner, as the ledger records it: PR6/PR7 implementer.

**Carried: not constructible through the funnel.** The distinguishing fixture is a commit-tree child that writes its object and then prints a **malformed** id, and the child is real `git commit-tree`, which always prints a valid one. Nothing stubs the child or injects its stdout, and adding a stdout seam to a production Git invocation to test the ordering of a hook is a larger change than the claim. The live passage is `effect_site_inventory.identity`'s R27 clause. What *is* held is that the point fires exactly once, before `After`, and that a kill there leaves a GC-owned object nothing adopts

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
