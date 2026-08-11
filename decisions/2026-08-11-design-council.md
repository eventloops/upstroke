# Decision record — multi-model design council for tactus

**Date:** 2026-08-11
**Status:** Decided — adopted as manual practice now; machinery deferred pending the miss/catch record (v0.3 design-pane era at the earliest).
**Inputs:** *Tactus Multi-Model Design Council* and *Tactus Adaptive Agent Routing Engine* (GPT-5.6-sol drafts, kept outside the repo), reviewed against DESIGN.md v2.1 (§2, §5, §11.3, §21, §23.2) in a Fable 5 critique session. Decisions: project owner.
**Siblings from the same session** (originally addenda here, split out per `README.md`): [self-hosting v0.2](2026-08-11-self-hosting-v02.md) · [resume gate config](2026-08-11-resume-gate-config.md) · [Codex reasoning effort](2026-08-11-codex-reasoning-effort.md).

**The idea under decision:** run high-stakes tactus design decisions through multiple independent frontier models — independent proposals, cross-critique, synthesis, adversarial review — instead of a single designer. The sol doc proposes it as a five-phase product subsystem ("council mode").

---

## Verdict

Adopt the council **through the self-hosting/bootstrap lens, manual-first, zero code now**, in a bounded shape:

1. **Reserved for genuinely open, hard-to-reverse decisions** (the class: remote-worker protocol, credential architecture, merge arbitration). Ordinary design stays single-designer under DESIGN.md §5.
2. **At most three generator seats — one per frontier family** (Anthropic, OpenAI, Google; all reachable through existing adapters: Claude Code, Codex, Copilot's model picker). No fourth seat: it is necessarily a family repeat, the least additive chair by §11.3's family-not-harness logic.
3. **Independence until late.** Generators share the same brief and context package — which must include DESIGN.md's relevant sections — and never see each other's proposals during generation. No debate-to-consensus at any phase: the value is structured disagreement, not discussion.
4. **Critique-heavy allocation.** After generation: adversarial cross-family critique of fixed artifacts, one red-team pass on the synthesis, and **evidence experiments for any testable factual dispute**. The human owns synthesis and every user-owned call (scope, risk tolerance, product intent) — §5's question exhaustion against the user survives in full.
5. **Machinery cut:** no divergence analytics, no consensus-extraction engine, no automated council-depth selection, no council-by-default. If machinery is ever built (v0.3, design pane), build the **experiment arm first** — the council commissions spike tasks the engine executes and gates — and the analytics never.
6. **Adopt into house practice immediately, independent of the council:** the decision-ledger format (statuses `accepted / rejected / deferred / unresolved`; critical dissent forces explicit resolution) and the evidence-phase rule (*empirical evidence overrides model opinion*).
7. **Value is judged qualitatively** via a running miss/catch record per council round. No statistical validation will be attempted (see Measured vs. assumed).
8. **First live use:** the next hard v0.2 design decision — merge-queue semantics is the ripest candidate.

## Reasoning

- **Bootstrap economics.** Tactus's own development is a stream of council-grade decisions (the sol doc's §61 prototype list is effectively the backlog). Each gates weeks of implementation at real prices ($3–12/task; rework cycles at frontier rates). A council round at an estimated $30–100 is trivially justified *if the process catches real things* —
- **and it demonstrably does.** This review session is the existence proof: sol proposed, a different family critiqued against the spec, and real defects surfaced before implementation inherited them (collisions with §23.2's measured findings, a missing §5 output contract, an unaudited divergence-extractor trust gap).
- **Coverage update — a position revised during review.** The initial critique held that frontier models mostly converge given the same brief, making parallel generation the weakest phase. Direct evidence dented this: sol's drafts contained non-overlapping, adoptable ideas the Fable-authored spec lacked (failure taxonomy, `selection_origin`, pre-execution difficulty snapshots). Coverage failures are also the *invisible* class — the better design nobody generated leaves no trace — so "the record shows detection dominated" carried selection bias. Second and third family generators therefore earn their seats on open problems.
- **More-intelligence-earlier is measured, not aspirational.** §23.2 recorded that escalating early cost less than retrying cheap ($2.73 vs $3.21) because frontier review is charged per attempt. The design-phase analog compounds, since design errors gate everything downstream.
- **Diminishing returns per seat.** The second family buys the large decorrelation jump; the third, some; a fourth chair repeats a family. Three families is also the current ceiling of the frontier.
- **The fact frontier.** The project's hardest calls were settled by measurement, not reasoning: Codex's sandbox behavior (empty diff with exit 0; `codex doctor` reporting no sandbox helper on Windows; seccomp/SYS_ADMIN required under Docker defaults) settled the runner design; the cost inversion settled routing emphasis. Past the fact frontier, a fourth model adds confidence, not information — the evidence arm is structural, not optional.
- **Sequencing trap avoided.** Building council machinery now would spend the development capacity it exists to save, on a v0.3-era subsystem. The manual rounds *are* the bootstrap; automate only what several real rounds show to hurt (predicted: context packaging and role prompts, not orchestration).

## Measured vs. assumed

**Measured (project record):**
- Frontier review charged per attempt dominated cost while implementers were cheap: 44–77% of spend across four runs; escalate-early beat retry-cheap, $2.73 vs $3.21 (DESIGN.md §23.2).
- Independent review catches what gates miss (the CS0133 emission: built clean, 722 tests passed, still wrong).
- Codex sandbox facts (2026-08-11, codex-cli 0.147.0) — the findings that settled the runner layer; no council would have reasoned its way to them.
- Two identical runs produced two different failure modes → single-run A/B comparisons are not evidence (§23.2).
- This session: cross-family critique surfaced real spec defects; sol generated non-overlapping value.

**Assumed / judged (recorded so honesty survives):**
- Council round cost of $30–100 (derived from task economics; no design run has been priced).
- Diminishing returns after the second family seat (consistent with correlation logic and §11.3; not measured).
- That the manual pain points justifying eventual machinery will be context packaging and prompt consistency.
- Convergence-under-shared-context is still assumed, but *weakly* — held loosely after the sol counter-evidence.

## Rejected options

1. **Full 4–5 designer council with divergence analytics as the default for important work** (the sol doc's maximal shape). Rejected: the fourth seat is a family echo; divergence extraction is an unaudited single point of interpretive failure (it can launder a minority view into "consensus" and nothing checks it); the analytics presume statistical resolution that never arrives at this scale.
2. **Discussion / debate-to-consensus formats.** Rejected: anchoring and groupthink; the sol doc itself opens by rejecting this shape, and the naive reading of "get the models to work together" collapses into it.
3. **Build council machinery now.** Rejected as the sequencing trap above. Revisit against the miss/catch record at v0.3.
4. **Statistical validation of council value** (the doc's telemetry §§35–37). Rejected: at $30–100/sample against noisy long-horizon outcomes, with §23.2's variance finding, samples will never support the comparison. Qualitative miss/catch record instead.
5. **Council as a near-term product wedge.** Rejected: the wedge remains P9/P4 (§2). The productizable kernel, later and narrower: a cross-family *design review pass* in the v0.3 design pane, with the manual practice as its existence proof.
6. **Critique-only council (single generator)** — the reviewing model's own opening position. Superseded by the coverage update: up to three generators, one per family, on open problems.

## Constraints any future formalization inherits (from DESIGN.md)

- Output contract = §5's artifacts, or the result doesn't compile to the IR: task plan with `tier`/`min` annotations, conventions brief, decisions record — plus the decision ledger as a fourth artifact.
- Question exhaustion targets the **user**, mid-process. Models never resolve user-owned decisions; approval at the end is not interrogation at the start. A runtime `design_defect` traced to a council design needs an attributable seat.
- "Family" per §11.3: model family, not CLI/harness — Copilot serving an Anthropic model is not a second family.
- Critical red-team findings dispatch like §11.5 security findings: to an `Unblock`-style question, never into a retry-until-pass loop where they get laundered.
- Council roles are read-only worker profiles; spend runs through the existing budgets / `ApproveSpend` machinery.

## Related dispositions from the same review (recorded lightly)

- **Adaptive routing doc, learning phases (Bayesian → bandit → learned ranking): parked indefinitely — not worth accounting for now.** Two structural reasons at personal scale: single-digit observations per (kind × tier × model) cell, and data half-life — model rosters churn quarterly, so the dataset decays about as fast as it grows. The live path remains §23.2's kill test: the predict-before-running protocol and its miss log decide whether rung/cost prediction ships as a `--dry-run` step.
- **Salvage candidates from the routing doc** (spec changes if adopted — v0.2 `export-decisions`/event schema, justified for *interpretability of small data*, not ML): failure taxonomy on outcomes; full agent identity incl. versions per attempt; `selection_origin` (auto / user-override / pin / exploration); pre-execution difficulty/feature snapshot; marking evidence-free failures (infra) apart from capability failures. **Status: candidates — to be decided when the export-decisions plan is designed, which is the first self-hosted run ([self-hosting v0.2](2026-08-11-self-hosting-v02.md)).** Item #2 has since landed with a concrete shape — see [Codex reasoning effort](2026-08-11-codex-reasoning-effort.md).
- **Process rule from the review itself:** any future sol design draft gets DESIGN.md's relevant sections in its context (at minimum §§3–5, 8, 10–13, 21, 23.2). Both docs were written without the spec and re-derived or contradicted recorded decisions — the failure mode the council doc's own context-manifest rule (§43) exists to prevent.

## Follow-ups approved the same day

All five wrap-up threads from this session were approved by the owner: (1) the `decisions/` folder created under the contract in `README.md`; (2) self-hosting v0.2 confirmed — [its own record](2026-08-11-self-hosting-v02.md); (3) DESIGN.md §21's v0.3 learned-routing line reconciled with §23.2, citing this record; (4) the five telemetry fields left as candidates, to be decided at export-decisions plan time; (5) the gate-config hardening check performed — [its own record](2026-08-11-resume-gate-config.md).
