---
id: PR40-CHARTER-BINDS-A-PROPOSAL
severity: P3
disposition: deferred
category: docs-contract
pr: 40
reviewed_sha:
location: proposals/README.md:22
provenance: undetermined
first_bad:
guard: project owner, carried by the documentation-authority pass
---

## Failure sequence

**Two live passages disagree about whether a proposal can bind, and this pull request lands both.** `proposals/README.md:22` states the folder contract — "**DESIGN.md remains the only living authority for product design.** A proposal binds nothing." `decisions/2026-08-24-pr3-layer-freeze-charter.md:169` states the opposite for one proposal: `proposals/2026-08-24-v0.2-g2-pr3-layer-pass.md` "is the pass's plan; it binds when this record lands and cites it". The only `DESIGN.md` edit this pull request makes records sequencing and links to the plan; it does not carry the plan's content. Failure sequence: the G2 pass opens -> one implementer reads `DESIGN.md` as the sole living authority and inherits only that the PR3-layer pass precedes PR8 -> another reads the charter, treats the proposal as binding, and inherits W1 through W10 with their exit criteria -> the two build to different scopes, and each can correctly cite a governing document against the other. Compounding: `decisions/README.md:17` makes a landed record immutable, so after merge the charter's sentence can be superseded by a dated appended section or a successor record but never edited

## What the change that takes this up should do

Owner, as the ledger records it: project owner, carried by the documentation-authority pass.

**Accepted as real and deferred, not fixed — owner disposition 2026-08-29.** Found by the frontier review of `7cf4f9971e2b4a8712ca7afa11e129c734921173`, verdict CHANGES_REQUIRED. Deferred deliberately: the repair is a ruling about how a charter may confer authority on a plan, which reaches `proposals/README.md`, `decisions/README.md` and `DESIGN.md` together, and is wider than the documents this pull request lands. **Revisit in the documentation-authority pass**, and sooner if any slice cites the proposal as binding. Until that ruling, `DESIGN.md` governs, the proposal binds nothing, and the charter's sentence is to be read as scheduling the pass rather than as conferring authority on the plan

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
