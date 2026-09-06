---
id: PR7-STD-OWNER-RECORD-LEXICAL-AUTH
severity: P2
disposition: deferred
category: security-trust
pr: 7
reviewed_sha:
location: src/engine/topology/recover.rs
provenance: undetermined
first_bad:
guard: project owner
---

## Failure sequence

Owner-record public-directory authentication falls back to lexical spelling when canonical evidence is unavailable (`src/engine/topology/recover.rs`)

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

**Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §14's MUST-tagged fail-closed bullet — *"Security-sensitive comparisons and decisions MUST fail closed on malformed, contradictory, or unavailable evidence; availability fallbacks must not silently grant more authority."*, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§14 security and trust boundaries** (mechanism: behavioural/security tests; the active effect denylist; pull-request review), status *review-only where no named test or denial is cited* — none is cited, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — the record is written while a spelling denotes one directory -> that spelling is later retargeted to another, by replacing a symlink or a mount, while the run id, repo key and incarnation still match -> the filesystem then refuses to canonicalize the public directory, so `canonical_display` returns the spelling on both the recorded and the live side -> string equality authenticates the **new** directory against the old directory's owner record, and the disagreement refusal never fires. The retarget step is stated because equal spellings alone do not establish two directories, and an earlier revision of this row jumped straight from the canonicalization failure to the conclusion **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region 1 of 2** — the decision site, the owner-record public-directory check, `3e5212d` `recover.rs` 632-634: `a332d47443baaa6c12f1f74ee47e06a6b654e16a6bbbd731261d1de46971fb75`. **Region 2 of 2** — its documented rationale and the `canonical_display` helper it justifies, 751-758: `1366553bf35fea1422476857aa79e3b8ac7c76e7e77011c01e84efcea7d0abb1`. **Grounds corrected 2026-08-28.** An earlier revision of this row said the cluster cites §8. Five of the seven cite §14; the owner corrected the grounds and the routing conclusion is unchanged, because §14 is MUST-tagged too.

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
