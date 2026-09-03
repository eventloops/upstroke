## 5. The work unit: a two-phase lifecycle (P7)

Every piece of work lives in one unit (a "pane" in the eventual UI; a run directory today) with two phases that have opposite attention models.

**Phase 1 — Design (interactive, frontier-tier).** The user and a frontier model iterate on the work with constant feedback. The designer's explicit objectives, in its prompt:

1. Produce the task breakdown (typed tasks, dependencies, acceptance criteria, path hints).
2. **Question exhaustion:** enumerate every decision execution will face; resolve each with the user *now*, while the human is present and cheap to consult.
3. Emit three artifacts: the **task plan** (annotated markdown), the **conventions brief** (one page, injected into every downstream prompt), and the **decisions record** (every resolved ambiguity, with rationale).
4. Annotate each task with a suggested tier and minimum tier (`tier=`, `min=`).

**Phase 2 — Execution (headless, interrupt-driven).** The plan is frozen; the engine takes over. Runtime questions pass through a pre-filter before ever reaching the human: the question plus the decisions record go to the frontier (architect) profile — *"was this already answered?"* Only genuinely novel questions escalate to the user.

**The defect loop:** every question that reaches the human at runtime is, by definition, a design-phase defect. It is logged as one (`design_defect` event, with the question and eventual answer), and the accumulated defects become review material for the designer prompt. The system learns to need the user less.
