# upstroke — Design Document v2.1

> **Name:** `upstroke` — the Renaissance term for the shared steady pulse every performer synchronizes to. Verified free on crates.io and npm (2026-08-08, live API check). Known adjacent collision: AnthusAI/Upstroke, an alpha Lua DSL for agent orchestration (~3★) — assessed as tolerable, but the decision is deliberate: we differentiate hard rather than share ground by accident. **Action on repo creation: publish a placeholder crate immediately.**

**Status:** v2 — consolidates the original architecture, the two-phase lifecycle, the interaction model, the capacity engine, and two rounds of research whose companion reports are maintained in the strategy record outside this repository, plus the v2.1 late-binding refinement: connect your plans; tiers bind to concrete models and pools at attempt time.
**Language:** Rust · **License:** Apache-2.0, relicensed from AGPL-3.0-only by [the 2026-09-01 decision](decisions/2026-09-01-relicense-apache-2.md) · **Form factor:** single static binary, Windows first-class

---

This file is the index. The design lives in `design/`, one numbered section per
file. Section numbers are the API: code, reviews, and the standards cite
`DESIGN.md §N`, so a number is never reassigned. New sections append; a retired
section keeps its number and says so.

| § | Section | File |
|---|---|---|
| 1 | Summary | [design/01_design_summary.md](design/01_design_summary.md) |
| 2 | The nine pillars | [design/02_design_pillars.md](design/02_design_pillars.md) |
| 3 | Goals and non-goals | [design/03_design_goals_and_non_goals.md](design/03_design_goals_and_non_goals.md) |
| 4 | Invariants | [design/04_design_invariants.md](design/04_design_invariants.md) |
| 5 | The work unit: a two-phase lifecycle (P7) | [design/05_design_work_unit_lifecycle.md](design/05_design_work_unit_lifecycle.md) |
| 6 | Architecture | [design/06_design_architecture.md](design/06_design_architecture.md) |
| 7 | Core data model | [design/07_design_core_data_model.md](design/07_design_core_data_model.md) |
| 8 | Trait surface | [design/08_design_trait_surface.md](design/08_design_trait_surface.md) |
| 9 | Plan ingestion (P1) | [design/09_design_plan_ingestion.md](design/09_design_plan_ingestion.md) |
| 10 | Routing (P3) — three sources, then capacity and affinity | [design/10_design_routing.md](design/10_design_routing.md) |
| 11 | Verification ladder (P4) | [design/11_design_verification_ladder.md](design/11_design_verification_ladder.md) |
| 12 | Interaction model (P8) | [design/12_design_interaction_model.md](design/12_design_interaction_model.md) |
| 13 | Capacity engine (P9) | [design/13_design_capacity_engine.md](design/13_design_capacity_engine.md) |
| 14 | Execution engine | [design/14_design_execution_engine.md](design/14_design_execution_engine.md) |
| 15 | Event log, resume, run layout (P6) | [design/15_design_event_log_resume_run_layout.md](design/15_design_event_log_resume_run_layout.md) |
| 16 | Agent adapters (P2) | [design/16_design_agent_adapters.md](design/16_design_agent_adapters.md) |
| 17 | Configuration reference | [design/17_design_configuration_reference.md](design/17_design_configuration_reference.md) |
| 18 | CLI surface | [design/18_design_cli_surface.md](design/18_design_cli_surface.md) |
| 19 | Failure handling | [design/19_design_failure_handling.md](design/19_design_failure_handling.md) |
| 20 | Safety and permissions | [design/20_design_safety_and_permissions.md](design/20_design_safety_and_permissions.md) |
| 21 | Versioned scope | [design/21_design_versioned_scope.md](design/21_design_versioned_scope.md) |
| 22 | Adopted from the field (with credit) | [design/22_design_adopted_from_the_field.md](design/22_design_adopted_from_the_field.md) |
| 23 | Risks and kill criteria | [design/23_design_risks.md](design/23_design_risks.md) |
| 24 | References | [design/24_design_references.md](design/24_design_references.md) |

Read §4 in full before touching the engine, the event log, or anything that
handles capacity or questions. §21 is the build order, and it is deliberate.
