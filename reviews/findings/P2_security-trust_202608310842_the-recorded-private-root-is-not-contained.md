---
id: PR7-STD-PRIVATE-ROOT-NO-CONTAINMENT
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

The recorded private-root locator is accepted without absolute-path or symlink/reparse-point containment validation (`src/engine/topology/recover.rs`)

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

**Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §8's MUST-tagged containment bullet — *"Path containment checks MUST account for `..`, absolute paths, symlinks/reparse points, and platform-specific prefixes as appropriate to the security boundary. Lexical normalization alone does not prove filesystem containment."*, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§§8–9 filesystem, persistence, and processes** (mechanism: behavioural tests; platform CI; the active effect denylist). Containment is not among the automated parts that row names and this finding cites no test or denial, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — a recorded locator names `<R>/runs/<run_id>` where a component is a symlink out of `<R>` -> `authorized_root` checks components lexically, rejecting `..` and requiring the two trailing names, and resolves no link -> the derived root is accepted -> locking and reclamation operate under a root outside the containment boundary the record was supposed to prove **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region** — `authorized_root` with the doc comment and in-line rationale that justify the lexical check, `3e5212d` `recover.rs` 418-458: `b9fd86d19d22b096130ecdee08427816852a062a59f0a0df64b854e95fd10483`.

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
