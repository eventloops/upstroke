---
id: PR7-STD-OWNERSHIP-PROOF-UNCANONICAL
severity: P2
disposition: deferred
category: security-trust
pr: 7
reviewed_sha:
location: src/rundir.rs
provenance: undetermined
first_bad:
guard: project owner
---

## Failure sequence

The private-half ownership proof falls back to an uncanonicalized public path when canonicalization evidence is unavailable (`src/rundir.rs`)

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

**Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §14's MUST-tagged fail-closed bullet — *"Security-sensitive comparisons and decisions MUST fail closed on malformed, contradictory, or unavailable evidence; availability fallbacks must not silently grant more authority."*, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§14 security and trust boundaries** (mechanism: behavioural/security tests; the active effect denylist; pull-request review), status *review-only where no named test or denial is cited* — none is cited, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — the marker is written while a spelling denotes one public directory -> that spelling is later retargeted to another while the remaining recorded fields still match -> canonicalization of the public directory then fails, so the proof falls back to the uncanonicalized path and renders it lossily -> the comparison is spelling against spelling, the private half is treated as this run's, and the retain-on-disagreement path never fires. The retarget step is stated because equal spellings alone do not establish two directories **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region 1 of 2** — the proof's documented contract through the fallback, the lossy rendering and the disagreement comparison they feed, `3e5212d` `rundir.rs` 1451-1595: `c4256e0a23cc312185222314f3af0f1d1cf353f7c8720cf0e07178d9082bba5f`. **Region 2 of 2** — the **write side's** rationale, which is where the deliberateness of the paired fallback is actually argued, `3e5212d` `create.rs` 1983-1995: `4500bb448c7bd33285f4d72c9d40366334a65098c35e46ccd4d448bf4b0bfd37`. An earlier revision hashed only the read side's fallback, so an edit to either rationale left the digest verifying while this row's documented-site claim became false — the third time a region on this row covered less than the row claims, and the reason every region is now quoted with what it covers. **Grounds corrected 2026-08-28.** An earlier revision of this row said the cluster cites §8. Five of the seven cite §14; the owner corrected the grounds and the routing conclusion is unchanged, because §14 is MUST-tagged too.

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
