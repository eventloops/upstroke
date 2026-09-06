---
id: PR5-R2-OBJECT-GROUP-TAKES-NO-SITE
severity: P3
disposition: deferred
category: correctness
pr: 5
reviewed_sha:
location: rundir.rs
provenance: undetermined
first_bad:
guard: project owner
---

## Failure sequence

`PR5-WORKSPACE-048`. All six Object-group APIs hard-code their `ObjectSite` internally — `candidate_stage`, `candidate_write_tree`, `snapshot_commit_tree`, `candidate_commit_tree`, `proposal_cherry_pick`, `repair_materialize` — while the Ref group takes `site: RefSite`. `manager` says every effect "goes through typed funnel APIs that take a typed site", so the asymmetry is real rather than an artefact of the measurement, and no compile fixture probes it

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

**Carried: widening six public signatures and every caller is a design change, not a repair-round edit.** Recorded as `NOT_PRESENT` by the re-measurement — there is no parameter to delete — but the absence is the finding. The tree already owns the mechanism that would prove it: `rundir.rs`'s `build_refusals()` compiles six fixtures against this crate's rlib and asserts rustc's own error **codes** (E0061, E0308, E0451/E0603/E0063, E0599, E0382) against a control that must compile. It has no Object-group case because there is nothing yet to refuse. If the owner reads `manager` as requiring the parameter, the repair is mechanical and the harness is waiting

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
