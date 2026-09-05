---
id: PR7-STD-CONTAINER-LEXICAL-CONFINEMENT
severity: P2
disposition: deferred
category: security-trust
pr: 7
reviewed_sha:
location: src/runner/container/exec.rs
provenance: undetermined
first_bad:
guard: project owner
---

## Failure sequence

Confinement uses a lexical prefix comparison as its entire filesystem-containment decision (`src/runner/container/exec.rs`)

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

**Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §8's MUST-tagged containment bullet — *"Path containment checks MUST account for `..`, absolute paths, symlinks/reparse points, and platform-specific prefixes as appropriate to the security boundary. Lexical normalization alone does not prove filesystem containment."*, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§§8–9 filesystem, persistence, and processes** (mechanism: behavioural tests; platform CI; the active effect denylist). Containment is not among the automated parts that row names and this finding cites no test or denial, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — a mount source reaches a withheld path through a symlink or a differently-spelled prefix -> `violations` decides containment with `withheld.starts_with(source)` and resolves nothing -> the mount is judged not to hand the container a withheld path -> the container is given the public log or the repository root and the confinement claim is false **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region** — `violations` with its doc comment and the `starts_with` that is the whole check, `3e5212d` `exec.rs` 316-332: `2c7edcd309cca055fb992a25c73504f197fe17a5e5e04fffd611ff4f417e81f4`.

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
