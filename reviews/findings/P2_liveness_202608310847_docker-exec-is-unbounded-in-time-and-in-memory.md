---
id: PR7-STD-CONTAINER-EXEC-UNBOUNDED
severity: P2
disposition: deferred
category: liveness
pr: 7
reviewed_sha:
location: src/runner/container.rs
provenance: undetermined
first_bad:
guard: project owner
---

## Failure sequence

The Docker subprocess primitive has no timeout or cancellation protocol and captures both streams without a pre-allocation bound (`src/runner/container.rs`)

## What the change that takes this up should do

Owner, as the ledger records it: project owner.

**Raised by the standards review of `3e5212d98b20e2cf72d2fc3982746c7e87de4034`, filed to the sweep work-list as a §9 observation, and routed here from PR41 second review record `reviews/2026-08-28-pr41-frontier-review-788c714.md`, whose first finding identified the misclassification.** It cites **§9's MUST** — *"Every subprocess integration MUST define and test: … timeout, cancellation, and descendant-process cleanup"* — and the same section's requirement that stdout/stderr size behaviour be defined and tested. `CODING_STANDARDS.md` §1 refuses an ad hoc in-code exception for a MUST, and the site's doc comment argues only about **stream separation** — it says nothing about timeout, cancellation or bounds — so there is no rationale to weigh even at SHOULD strength. Enforcement map row **§§8–9 filesystem, persistence, and processes** (mechanism: behavioural tests; platform CI; the active effect denylist); subprocess timeout and capture bounds are not among the automated parts that row names and this finding cites no test or denial, so **review-only**. **Canonical fields, because `MAINTAINING.md`'s nine-column contract governs *pull-request* ledgers and this table's schema is its own four columns:** severity **P2**; provenance **pre_existing**; category **liveness**; failure sequence — `docker logs` hangs or emits unbounded output -> `Command::output` waits with no timeout and allocates complete stdout and stderr buffers before returning -> the runner's later truncation runs only after those vectors exist, so it bounds the log and not the capture -> a container operation blocks the runner indefinitely or drives it toward OOM, and no cancellation path exists to stop it. **Region** — `exec_streams` with the doc comment that is its whole stated rationale, `3e5212d` `container.rs` 1516-1547: `8998739ca68035a8f8e538a3f8c0783835664bbf69ba395d03318322063f6c5f`. The digest is recorded to relocate the site inside a file that has moved; it is not a W10.4 salvage claim and does not exempt this row from re-derivation. **Why it is here and not in the sweep:** the sweep is for SHOULD-level conformance; §1 sends a MUST deviation to an owner, which is the same test that routed the other seven.

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P2** here is this migration's judgement from the consequence described above, not the reviewer's own word.
