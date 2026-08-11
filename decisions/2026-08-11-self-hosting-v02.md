# Decision record — self-hosting v0.2 through tactus

**Date:** 2026-08-11
**Status:** Decided
**Provenance:** originally Addendum B of `2026-08-11-design-council.md`; split into its own record on 2026-08-11 when one-decision-per-file became the folder convention (`README.md`). Content unchanged.
**Related:** [design council](2026-08-11-design-council.md) (the session this came out of), [reasoning effort](2026-08-11-codex-reasoning-effort.md), [resume gate config](2026-08-11-resume-gate-config.md) (the hardening this run order depends on).

---

## Verdict

v0.2 development runs through tactus wherever the v0.1 envelope allows, starting now.

## Reasoning

- **The claim is auditable, not asserted.** Engine-owned commits carry `[tactus] <task-id>` and every commit has a run ledger and event log behind it, so "N% of v0.2 was written by tactus" is a `git log --grep` result with receipts — the §23.1 "pen" pitch pointed at our own repo. Report the measured number by task count, and say plainly that difficulty-weighted share is lower.
- **Envelope expectations, stated up front:** isolated v0.2 items flow through the engine (export-decisions, plan adapters, Aider adapter, notifiers); the scheduler rework and runner core may stay interactive or run as single-task frontier plans with heavy review. Every v0.2 defect invites "the tool wrote that" — the answer is the review record and the defect loop, not defensiveness.
- **First run: the export-decisions schema work** — the five telemetry candidates from the council review become its plan, decided at plan time. This closes that loop (the first tactus-built v0.2 tasks implement the salvage from the review that decided to self-host) and starts filling §23.2's recorded gap: no numbers exist for the frontier-implementer regime.
- **Operational:** runs happen WSL-side (all three CLIs work there; the Codex implementer only works there); the predict-before-running protocol applies to every run; misses land in the miss log.
