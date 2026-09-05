---
id: PR5-ANSWER-MODULE-COLUMN
severity: P3
disposition: deferred
category: docs-contract
pr: 5
reviewed_sha:
location: src/rundir.rs:899
provenance: undetermined
first_bad:
guard: PR6/PR7 implementer (the slice that next opens src/topology/effects.rs)
---

## Failure sequence

`effect_sites.json` ships `"module": "src/interaction.rs"` for `Answer.StageWrite`, `Answer.PublishRename` and `Answer.Ingest`; the `AnswerSite::` literals are at `src/rundir.rs:899`, `:912` and `:934` and nowhere else. The column is `EffectSiteId::module()`, generated from `src/topology/effects.rs`

## What the change that takes this up should do

Owner, as the ledger records it: PR6/PR7 implementer (the slice that next opens `src/topology/effects.rs`).

**The artifact's claim is corrected; the column is not, and cannot be from here.** `effects/funnel-modules.json` is generated beside `effect_sites.json` from the tree's own answer, carries every site and names the three that disagree, and is compared byte-for-byte — so a gate report now carries the correction alongside the claim. The column itself lives in a file frozen under the owner ruling of 2026-08-20, and moving the three funnel bodies to satisfy it is the other thing a slice may not do: they close over `rundir`'s private `funnel`/`RunDirHooks`, and `mechanism` (2) is the packet's own placement. Sol ruled this a low defect (`PR5-CONF-018`) and Fable a preference; the disagreement is over whether a false `module` column matters when enforcement is unchanged, and it is narrow either way — both files are allowlisted funnel modules and `interaction.rs`'s delegations are denied as wrappers

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
