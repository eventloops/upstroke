# proposals/

Dated design proposals: the entry point of the design lifecycle.

```
proposal  →  council critique  →  decision record  →  implementation  →  review
(private)    (files beside it)     (decisions/)        (commits/PRs)     (reviews/)
```

**Closed to new filings since 2026-08-27.** Proposals are now filed in the
private companion repository, engine mechanisms included — see
[the decision](../decisions/2026-08-27-proposals-private.md). The proposals
below stay here: decision records cite them as inputs and those citations must
keep resolving. The first stage of the lifecycle happens privately; every later
stage is unchanged and public.

The rest of this file describes the conventions those filings follow, which the
private folder inherited unchanged.

The contract that keeps this folder safe:

- **DESIGN.md remains the only living authority for product design.**
  A proposal binds nothing. When one survives the council, the verdict lands as a record in
  [`decisions/`](../decisions/README.md) citing the proposal and its critiques
  as inputs, and DESIGN.md gets the compressed edit at decision time.
- **One proposal per file**, named `YYYY-MM-DD-vN.M-<slug>.md`.
  - The date is the filing date — same merge-safety and chronology reasons as
    `decisions/`.
  - `vN.M` is the version the proposal argues it belongs to, **as estimated at
    filing**. It is a filed estimate, not a living fact: this project has
    already promoted Copilot into v0.1, landed the Codex adapter ahead of the
    rest of v0.2, and parked learned routing from v0.3 to indefinitely.
    Reality reassigning the work does **not** rename the file — the header's
    `Target:` line carries the living estimate, and the decision record settles
    it. Omit the segment only when the work is genuinely unplaceable.
- **Every proposal opens with a status block:** `Status:` (`Draft`,
  `In council`, `Decided → <link>`, `Parked`, `Withdrawn`), `Target:` (living
  version estimate), and `Filed:` (date). New or materially revised proposals
  also state `Review:` (`Unreviewed`, `In review`, or `Reviewed` with critique
  links); absence on an older file means unrecorded, not reviewed. Status and
  review evidence are orthogonal: a draft is not reviewed merely because it was
  filed, and a critique does not make it authoritative. A proposal may be
  revised while in council; note material revisions under the status block
  rather than silently rewriting.
- **Critiques live beside their proposal** as
  `<proposal-stem>-critique-<family>.md` (family = model family per DESIGN.md
  §11.3 — the council's seat identity). Critiques are dated records of what a
  seat said about a specific revision: append, don't rewrite.
- **Cross-link freely**, both directions, same as `decisions/`.

## Index

- [2026-08-24 — v0.2 — the G2 pass over PR3's layer](2026-08-24-v0.2-g2-pr3-layer-pass.md): ten
  workstreams over `src/topology/**`, the enforcement layer that ranges over it,
  and the test infrastructure that observes it — behaviour before structure before
  churn. Decided by the freeze charter; runs before PR8.
- [2026-08-22 — v0.3 — repository hazard map](2026-08-22-v0.3-hazard-map.md):
  path-indexed design defects and review findings, fed back into Phase 1 as
  mandatory question-exhaustion items where they overlap a story's path hints.
  The map generates questions, never answers; absence renders as unknown.
- [2026-08-15 — v0.3+ — proposal disposition ledger and rationale history](2026-08-15-v0.3-proposal-disposition-ledger.md):
  **Unreviewed draft.** Preserve approved, rejected, parked, withdrawn, and
  superseded reasoning with explicit revisit conditions and an eventual curated
  customer-facing projection.
- [2026-08-15 — v0.3+ — blind normalized design council](2026-08-15-v0.3-blind-normalized-design-council.md):
  **Unreviewed draft.** Separate anonymous design, challenge, synthesis,
  implementation, and review seats through a provider-neutral normalized
  envelope; keep the protocol manual until critical parallelism lands.
- [2026-08-15 — v0.2 — pull-request rationale and acceptance traceability](2026-08-15-v0.2-pr-rationale-and-acceptance-traceability.md):
  **Unreviewed draft.** Make truthful what/why, acceptance mapping, risk,
  verification, deferral, and recovery evidence part of every PR review.
- [2026-08-15 — v0.2 — review convergence and defect governance](2026-08-15-v0.2-review-convergence-and-defect-governance.md):
  **Unreviewed draft.** Cap ordinary semantic frontier invocations at three,
  distinguish introduced/fix-regression/pre-existing defects, require
  regression evidence, and prioritize confirmed work through a versioned C/S/L
  matrix.
- [2026-08-15 — v0.2 — streamed agent supervision and independent review deadlines](2026-08-15-v0.2-streamed-agent-supervision.md):
  **Unreviewed draft.** Decode provider streams into sanitized activity,
  preserve bounded private evidence, and give every frontier review pass its
  own fail-closed deadline before parallel self-hosting.
- [2026-08-13 — v0.2 — engine-owned implementation checkpoints and selective rewind](2026-08-13-v0.2-implementation-checkpoints.md):
  **Unreviewed draft.** Preserve bounded private A/B/C checkpoints during one
  implementation so a bad D can rewind to C, while final verification still
  judges the complete task-base-to-result change.
- [2026-08-13 — v0.2 — machine-readable Upstroke commit provenance](2026-08-13-v0.2-upstroke-commit-provenance.md):
  **Unreviewed draft.** Retain `[upstroke]` as the human-visible subject marker
  and augment it with run/task/work/revision Git trailers, a predictable
  `upstroke` display identity, and optional annotated tags that expose accepted
  `Initial work → Fix N` lineages without tagging rejected attempts.
- [2026-08-13 — v0.5 — portfolio coordination and cross-story scheduling](2026-08-13-v0.5-portfolio-coordination.md):
  coordinate multiple active stories as one portfolio — shared work, hidden
  dependencies, contention, parallelism.
  Critiques: [claude](2026-08-13-v0.5-portfolio-coordination-critique-claude.md).
- [2026-08-13 — v0.3 — public run viewer](2026-08-13-v0.3-public-run-viewer.md):
  a read-only public projection of the event log — upstroke building upstroke,
  with receipts.
- [2026-08-13 — v0.2 — structured review-finding telemetry](2026-08-13-v0.2-review-finding-telemetry.md):
  **Unreviewed draft.** Extend the verdict contract with a `findings` array and
  stable lifecycle hooks; additive event fields and export schema 2 preserve
  the rule that telemetry never changes a logically consistent verification
  outcome, while a self-contradictory pass still fails closed without another
  model call.
