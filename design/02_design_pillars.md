## 2. The nine pillars

| # | Pillar | One line |
|---|---|---|
| P1 | Plan-agnostic ingestion | Any plan (Claude Code plan-mode markdown, checklists, JSON, task-master output) → typed task DAG via an annotation grammar |
| P2 | Agent-agnostic conducting | Drives official agent CLIs headlessly through an adapter trait; no native loop, no API calls, no subscription proxies |
| P3 | Per-task model routing | Difficulty chains + blast-radius floors + designer-suggested tiers + user override |
| P4 | Verification-driven escalation | Gates → structured strong-model review → retry via session resume → escalate rungs; cross-vendor second opinion |
| P5 | Git discipline | Engine owns git; commit-per-task; v0.2 worktrees, merge queue, conflict→fix-task |
| P6 | Ops backbone | Event-sourced JSONL, resumable, cost ledger + budgets, dry-run preview, TOML, CI-embeddable |
| P7 | Two-phase lifecycle | Interactive frontier design (question exhaustion → decisions record) → headless execution bound to the same work unit |
| P8 | Interrupt-driven interaction | Non-blocking questions; park affected tasks, continue the rest; human = top escalation rung |
| P9 | Capacity engine | Connect your plans; late-bound tier→(model, pool) selection with conserve / value-max / spend-down strategies and affinity-aware assignment |

The design's emphasis is P4, P8 and above all P9; the competitive analysis that ranks the pillars this way is maintained in the strategy record outside this repository and is not part of the engine's contract.
