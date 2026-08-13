# proposals/

Dated design proposals: the entry point of the design lifecycle.

```
proposal  →  council critique  →  decision record  →  implementation  →  review
(here)       (files beside it)     (decisions/)        (commits/PRs)     (reviews/)
```

The contract that keeps this folder safe:

- **DESIGN.md remains the only living authority.** A proposal binds nothing.
  When one survives the council, the verdict lands as a record in
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
  version estimate), `Filed:` (date). A proposal may be revised while in
  council; note material revisions under the status block rather than silently
  rewriting.
- **Critiques live beside their proposal** as
  `<proposal-stem>-critique-<family>.md` (family = model family per DESIGN.md
  §11.3 — the council's seat identity). Critiques are dated records of what a
  seat said about a specific revision: append, don't rewrite.
- **Cross-link freely**, both directions, same as `decisions/`.

## Index

- [2026-08-13 — v0.5 — portfolio coordination and cross-story scheduling](2026-08-13-v0.5-portfolio-coordination.md):
  coordinate multiple active stories as one portfolio — shared work, hidden
  dependencies, contention, parallelism.
  Critiques: [claude](2026-08-13-v0.5-portfolio-coordination-critique-claude.md).
- [2026-08-13 — v0.3 — public run viewer](2026-08-13-v0.3-public-run-viewer.md):
  a read-only public projection of the event log — tactus building tactus,
  with receipts.
- [2026-08-13 — v0.2 — structured review-finding telemetry](2026-08-13-v0.2-review-finding-telemetry.md):
  extend the verdict contract with a `findings` array; additive event fields,
  export schema 2; telemetry never changes a verification outcome.
