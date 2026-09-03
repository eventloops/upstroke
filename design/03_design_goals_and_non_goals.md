## 3. Goals and non-goals

**Goals**

- Take a designed piece of work from plan to verified, committed code without supervision, asking only the questions that genuinely block progress.
- Route every task to the cheapest worker that can pass verification — where "cheapest" is measured against the user's real capacity pools, not list prices.
- Maximize the value of prepaid subscriptions: spend-down surplus capacity at the top tier, conserve when scarce, always leave a reserve for interactive use.
- Be agent-agnostic and vendor-neutral: official CLIs only, one adapter file per agent, cross-vendor review as a feature.
- Single static binary; first-class on Windows, macOS, Linux. Resumable, auditable, CI-embeddable.

**Non-goals**

- Not an agent: no file editing, no tool-use loop, no context management, no HTTP client, no model API keys.
- Not a subscription proxy: never spoofs headers or re-exposes a subscription as an API endpoint. Subprocesses of official binaries only.
- Not a UI (the engine): panes, dashboards, and notifiers are thin clients of the event log. The design-pane product is v0.3, built *on* the engine.
- Does not repair bad plans. The design phase exists to make plans good; execution assumes they are.
- No cross-run learning in v0.x — but every routing decision is logged so a learned router is possible later.
