---
id: PR7-STD-QUESTION-PAYLOAD-COMPONENT
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

The question-payload write boundary interpolates an unvalidated component into an authoritative path (`src/rundir.rs`)

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

**Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034` and routed here rather than to the cleanliness sweep.** It cites §14's opening MUST — *"Code MUST validate them before granting filesystem, process, git, capacity, or state-transition authority"*, persisted run data being named there as a trust-boundary input, on a security boundary. `CODING_STANDARDS.md` §1 grades evidence to requirement strength: a SHOULD deviation needs a concrete reason in the code, a MUST deviation needs *"an explicit, reviewed change to this standard or to the controlling design—not an ad hoc exception"*. The in-code rationale that discharged the rejected lossy-path SHOULD findings cannot discharge this one. **Open** until the owner rules the boundary adequate as designed, amends the standard, or schedules the repair. Enforcement map row **§14 security and trust boundaries** (mechanism: behavioural/security tests; the active effect denylist; pull-request review), status *review-only where no named test or denial is cited* — none is cited, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing** (the site predates the standard); category **security-trust**; failure sequence — a component reaches `write_question_payload` from persisted or model-authored input -> it is interpolated into `questions.join(format!("{component}.json"))` with no validation -> a component containing a separator or `..` escapes the questions directory -> a write lands outside the run directory the funnel is supposed to bound **On salvage, corrected.** An earlier revision said each row is *"salvageable by hash per W10.4"*. W10.4 is a non-binding clause of a **Decided proposal**, and what it authorises is salvaging a prior review's output where the **whole reviewed file** is byte-identical since that review's sha, re-deriving otherwise. It says nothing about region-level salvage and does not authorise it. The digest below is recorded for a narrower purpose that stands on its own: **relocating this row's site inside a file that has moved**, which a line number cannot survive. It is not offered as W10.4 compliance and does not exempt this row from re-derivation if the file changes. **Region** — `write_question_payload` with its doc comment and the join it performs, `3e5212d` `rundir.rs` 819-831: `0f0459b9b9e906df032ec7c988288be68764a99b2f16ee37d4d106c7b60a05f6`. **Grounds corrected 2026-08-28.** An earlier revision of this row said the cluster cites §8. Five of the seven cite §14; the owner corrected the grounds and the routing conclusion is unchanged, because §14 is MUST-tagged too.

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
