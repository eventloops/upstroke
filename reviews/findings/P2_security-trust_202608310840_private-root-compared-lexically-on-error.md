---
id: PR7-STD-PRIVATE-ROOT-LEXICAL-COMPARE
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

The explicit private-root comparison falls back to lexical equality on every canonicalization error (`src/engine/topology/recover.rs`)

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

**Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §14's MUST-tagged fail-closed bullet — *"Security-sensitive comparisons and decisions MUST fail closed on malformed, contradictory, or unavailable evidence; availability fallbacks must not silently grant more authority."*, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§14 security and trust boundaries** (mechanism: behavioural/security tests; the active effect denylist; pull-request review), status *review-only where no named test or denial is cited* — none is cited, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — the two `normalize` calls are independent and each falls back only for the path that failed to canonicalize -> canonicalization of the **recorded** root fails, transiently or on a mount that refuses it, so that side degrades from an identity to a spelling while the explicit root canonicalizes normally -> the comparison is then between a resolved path and a spelling, so an explicit root that merely spells the same is accepted and one that resolves to the same place through a symlink is refused -> the decision reaches the opposite verdict from the one it exists to reach, in the direction of granting rather than refusing, on evidence that was unavailable **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region 1 of 2** — the decision site, the `--private-root` comparison itself, `3e5212d` `recover.rs` 338-342: `5289194ca998e04b98b33aba06400b2abab199a6fdce2c9737693e326f6990c5`. **Region 2 of 2** — its documented rationale and the `normalize` helper it justifies, 460-470: `74cb133ae3a953d0c6a7e7dcf8c25c445203f0cbe52f457c309645e4963b555f`. **Grounds corrected 2026-08-28.** An earlier revision of this row said the cluster cites §8. Five of the seven cite §14; the owner corrected the grounds and the routing conclusion is unchanged, because §14 is MUST-tagged too.

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
