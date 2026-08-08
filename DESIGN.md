# tactus — Design Document v2.1

> **Name:** `tactus` — the Renaissance term for the shared steady pulse every performer synchronizes to. Verified free on crates.io and npm (2026-08-08, live API check). Known adjacent collision: AnthusAI/Tactus, an alpha Lua DSL for agent orchestration (~3★) — assessed as tolerable, but the decision is deliberate: we differentiate hard rather than share ground by accident. **Action on repo creation: publish a placeholder crate immediately.**

**Status:** v2 — consolidates the original architecture, the two-phase lifecycle, the interaction model, the capacity engine, and two rounds of competitive research (companion reports: *Prior-Art & Competitive Landscape*, *Round-2 Competitive Intelligence*), plus the v2.1 late-binding refinement: connect your plans; tiers bind to concrete models and pools at attempt time.
**Language:** Rust · **License:** MIT OR Apache-2.0 · **Form factor:** single static binary, Windows first-class

---

## 1. Summary

`tactus` is a headless orchestration engine for AI coding agents. A frontier model and the user design a piece of work together in an interactive session; `tactus` then executes that plan unattended — normalizing it into a dependency graph of typed tasks, dispatching each task to an existing coding-agent CLI (Claude Code, GitHub Copilot CLI, Aider) with a **model chosen per task**, verifying every result through objective gates and strong-model review, escalating failures up an explicit model chain, and **scheduling all of it against the user's actual subscription capacity** so that prepaid frontier-tier quota never expires unused.

It never edits a file, never implements an agentic loop, and never calls a model API. It is the conductor, not an instrument — and it treats your Claude Max windows, Copilot credits, API dollars, and local models as one portfolio to be spent optimally.

When it gets stuck at 2am it doesn't stop the run: it parks only the blocked branch, keeps everything else moving, and pings you as the top rung of the escalation chain.

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

Competitive reality (Aug 2026, from round-2 research): **P1, P3, P5, P6 are table stakes** — the field does versions of them. **P4's cross-vendor arbitrage, P8's park-and-continue scheduler, and above all P9 are the wedge**: no shipped tool routes work across subscription quota pools, and none biases tiers *up* when prepaid capacity would otherwise expire. First parties (Claude Code agent teams, GitHub Agent HQ, Codex) are absorbing the commodity pillars quarter by quarter, but cross-vendor arbitrage and cross-pool yield are structurally out of their reach — no vendor routes your spend to a competitor.

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

## 4. Invariants

1. **Agents edit files; the engine owns git.** Agents are instructed never to commit. The engine creates branches, stages, commits, and (v0.2) merges.
2. **The engine never speaks HTTP.** All model interaction happens inside agent subprocesses.
3. **Ground truth is the diff, not the transcript.** Gates check, reviewers judge, and feedback quotes `git diff` captured by the engine.
4. **Every state transition is an event.** State is derived by replaying `events.jsonl`. Resume = replay + continue.
5. **Official CLIs only.** No ToS-violating proxies, ever — the trust wedge is part of the product.
6. **Questions never stop the runnable frontier.** A question parks exactly the tasks it affects; the run hard-blocks only when nothing remains runnable.
7. **Capacity is estimated conservatively.** Safety margins on every pool, a reserve floor for the user's own interactive use, and rate-limit signals treated as ground truth over any estimate.

## 5. The work unit: a two-phase lifecycle (P7)

Every piece of work lives in one unit (a "pane" in the eventual UI; a run directory today) with two phases that have opposite attention models.

**Phase 1 — Design (interactive, frontier-tier).** The user and a frontier model iterate on the work with constant feedback. The designer's explicit objectives, in its prompt:

1. Produce the task breakdown (typed tasks, dependencies, acceptance criteria, path hints).
2. **Question exhaustion:** enumerate every decision execution will face; resolve each with the user *now*, while the human is present and cheap to consult.
3. Emit three artifacts: the **task plan** (annotated markdown), the **conventions brief** (one page, injected into every downstream prompt), and the **decisions record** (every resolved ambiguity, with rationale).
4. Annotate each task with a suggested tier and minimum tier (`tier=`, `min=`).

**Phase 2 — Execution (headless, interrupt-driven).** The plan is frozen; the engine takes over. Runtime questions pass through a pre-filter before ever reaching the human: the question plus the decisions record go to the frontier (architect) profile — *"was this already answered?"* Only genuinely novel questions escalate to the user.

**The defect loop:** every question that reaches the human at runtime is, by definition, a design-phase defect. It is logged as one (`design_defect` event, with the question and eventual answer), and the accumulated defects become review material for the designer prompt. The system learns to need the user less.

## 6. Architecture

```
            ┌────────────── Phase 1: Design (interactive) ─────────────┐
 user ◄────►│ frontier designer → tasks + conventions + decisions +    │
            │ per-task tier/min annotations                            │
            └───────────────────────────┬──────────────────────────────┘
                                        ▼ (plan frozen)
 plan.md ──► PlanAdapter ──► Plan IR (typed task DAG + artifacts)
                                   │
                                   ▼
                     Router (config ⊕ annotations ⊕ user override)
                     + CapacityEngine (pools, strategies, affinity)
                                   │
        ┌────────────── Scheduler ──┴──────────────────────────┐
        │  v0.1 sequential (skip-ahead past parked tasks)      │
        │  v0.2 tokio DAG + worktrees + merge queue            │
        │                                                      │
        │  Workspace ──► AgentAdapter ──► official CLI proc    │
        │      ▲               │                               │
        │      │               ▼                               │
        │    Gates ◄─── Outcome (diff, usage, session)         │
        │      │                                               │
        │      ▼                                               │
        │  Reviewer (read-only; optionally cross-vendor)       │
        │      │                                               │
        │      ▼ fail → retry (resume) → escalate rung → human │
        └──────┬───────────────────────────────────────────────┘
               ▼
      events.jsonl ──► resume · status · ledger · questions ──► notifiers (CLI / desktop / Telegram / Slack)
```

| Component | Responsibility |
|---|---|
| **PlanAdapter** | Parse a raw plan into the IR. One per format; `sniff()` for auto-detection. |
| **Router** | Resolve each task to an escalation chain of abstract tiers from three sources (config defaults, designer annotations, user override), applying blast-radius floors and `min` clips. |
| **Binder** | Resolve each tier to a concrete (agent × model × pool) at attempt time from everything connected — scoring capability fit (catalog), capacity headroom under the active strategy, and affinity; rebinds across pools on rate limits. Pins force fixed bindings. |
| **CapacityEngine** | Track every quota pool (windows, credits, dollars, local), estimate remaining capacity, expose strategy decisions (conserve / value-max / spend-down) to the router and scheduler. |
| **Scheduler** | Drain the DAG. Sequential in v0.1 — advancing past parked tasks to the next ready independent task. Parallel with worktrees in v0.2. |
| **Workspace** | Git state: run branch, per-task commits; v0.2 worktree-per-task + merge queue. |
| **AgentAdapter** | Turn a `TaskRun` into a subprocess of an official CLI, supervise it, parse the outcome. One file per agent. |
| **Gates** | Configured shell commands (compile/test/lint) in the workspace; failure logs become retry feedback. |
| **Reviewer** | Ordinary read-only worker profile emitting a structured verdict; optionally a different vendor from the implementer. |
| **Interaction** | Question/answer events, parking semantics, notifier plugins, CI degradation. |
| **Event log** | Append-only JSONL; source of truth for state, resume, status, questions, ledger, and the future decision-export dataset. |

## 7. Core data model

```rust
struct Plan {
    source: PlanSource,               // adapter id + original hash
    tasks: Vec<Task>,
    artifacts: Vec<Artifact>,         // conventions brief, decisions record, contracts
}

struct Task {
    id: TaskId,
    kind: TaskKind,                   // Design | Implement | Fix | Refactor | Test | Docs | Chore
    title: String,
    body: String,
    depends_on: Vec<TaskId>,
    acceptance: Vec<String>,
    path_hints: Vec<String>,          // globs — blast-radius routing + v0.2 overlap prediction
    suggested_tier: Option<Tier>,     // from the designer (advisory)
    min_tier: Option<Tier>,           // clips the chain start (binding)
    artifacts_in: Vec<ArtifactId>,
    artifacts_out: Vec<ArtifactId>,
}

struct WorkerProfile {                // v2.1: an optional PIN — tiers bind late by default;
                                      // a pin forces a fixed binding for one tier
    name: String,
    agent: AgentId,                   // claude-code | copilot | aider
    model: String,
    pool: PoolId,                     // which capacity pool this profile drains
    permissions: PermissionMode,      // Edit | ReadOnly
    max_turns: Option<u32>,
    extra_args: Vec<String>,
}

struct Outcome {
    status: OutcomeStatus,            // Completed | AgentError | Timeout | RateLimited
    diff: String,                     // engine-captured
    session_id: Option<String>,
    usage: Option<Usage>,
    cost_usd: Option<f64>,            // API-equivalent, as reported
    pool_drain: Option<PoolDrain>,    // pool units consumed (tokens / credits / $)
    transcript_path: PathBuf,
    duration: Duration,
}

struct Question {
    id: QuestionId,
    kind: QuestionKind,               // Unblock | ApproveSpend | Continue | Clarify
    affected_tasks: Vec<TaskId>,      // exactly these park
    context: String,                  // includes architect pre-filter result
    options: Vec<String>,
}

struct Verdict { pass: bool, reasons: Vec<String>, required_changes: Vec<String> }
```

### Task state machine

```
Pending ─► Ready ─► Running(attempt n, rung r) ─► Gating ─► Reviewing ─► Done
              ▲            ▲                         │           │
              │            │        feedback         │           │
              │            └──(retry same rung ──────┴───────────┘
              │            │   or escalate rung+1)
              │            ▼
              │      AwaitingInput ◄─── question raised (parks THIS task only)
              │            │ answer event
              │            ▼
              └──────── Ready (re-enters queue)

   chain exhausted ─► escalate to HUMAN (a question) ─► answered ─► retry
                                                     └─ declined ─► Failed ─► dependents Blocked
```

v0.2 appends `Done ─► AwaitingMerge ─► Merged | Conflicted(spawns fix task)`, and **dependency readiness is `Merged`, not `Done`** — a dependent's worktree must branch from an integration head that already contains its dependencies' code.

## 8. Trait surface

```rust
trait PlanAdapter {
    fn id(&self) -> &'static str;
    fn sniff(&self, raw: &str) -> bool;
    fn parse(&self, raw: &str) -> Result<Plan>;
}

trait AgentAdapter {
    fn id(&self) -> &'static str;
    fn probe(&self) -> Result<Caps>;         // ran at pre-flight: version + flag capabilities
    fn build(&self, run: &TaskRun) -> Result<Command>;
    fn parse(&self, out: &ProcessOutput) -> Result<Outcome>;
}
// Caps: json_output, session_resume, cost_reporting, read_only_mode, acp, model_list

trait Gate {
    fn name(&self) -> &str;
    async fn check(&self, ws: &Workspace) -> GateResult;   // Pass | Fail { log }
}

trait Notifier {
    fn id(&self) -> &'static str;                          // cli | desktop | telegram | slack
    async fn ask(&self, q: &Question) -> Result<()>;       // delivery only; answers arrive as events
    async fn info(&self, msg: &RunEvent) -> Result<()>;    // milestones, completion, budget alerts
}

trait CapacitySource {
    fn pool(&self) -> PoolId;
    async fn estimate(&self) -> Result<CapacityEstimate>;  // remaining, window ends, confidence
}
```

Deliberate omissions: no `Router` trait (config-evaluating struct until a second policy exists) and no `Executor` trait beyond `AgentAdapter` (a native agentic loop remains explicitly out of scope; the seam exists if that ever changes).

## 9. Plan ingestion (P1)

**v0.1 adapters:** Claude Code plan-mode markdown (primary) and the annotation grammar that upgrades *any* markdown. **v0.2:** generic checklist, JSON schema, and claude-task-master import — turning the most popular DAG generator into an upstream feeder rather than a competitor.

Parsing rules (markdown): each `##`/`###` section or top-level checklist item becomes a task (heading → title, body → body); a bullet list under `Acceptance` / `Done when` / `Success criteria` → acceptance; file paths in the body are collected into `path_hints`; **default dependencies are document order** (task N depends on N−1) unless annotations say otherwise.

Annotation grammar — HTML comments, invisible in rendered markdown:

```markdown
## Design the pagination API
<!-- tactus: id=api-design kind=design depends= tier=frontier out=api-contract -->

## Fix off-by-one in list endpoint
<!-- tactus: id=fix-obo kind=fix depends=api-design min=mid needs=api-contract paths=src/api/** -->
```

Attributes: `id`, `kind`, `depends` (empty = none, breaking the chain), `tier` (designer suggestion), `min` (binding floor), `needs`/`out` (artifact wiring), `paths` (globs). Unknown attributes warn, never error. Un-annotated plans still run: kinds by keyword heuristic, dependencies by document order, artifacts defaulting to a conventions brief from the first Design task.

## 10. Routing (P3) — three sources, then capacity and affinity

Assignment resolves in layers:

1. **Config baseline:** each `TaskKind` maps to an escalation chain, e.g. `fix = { chain = ["small","mid","frontier"], attempts_per = 2 }`.
2. **Blast-radius floors:** path-glob overrides truncate the chain start (`src/auth/**` starts at frontier). Blast radius beats nominal difficulty.
3. **Designer annotations:** `tier=` is advisory (becomes the chain start if it outranks the baseline), `min=` is binding (clips anything below it).
4. **User override:** the dry-run routing preview is where the user edits any assignment before spend.
5. **Late binding (v2.1):** chains are abstract tiers; the **binder** resolves each tier to a concrete (agent × model × pool) per attempt from everything the user has connected — scoring capability fit against the model catalog, capacity headroom under the active strategy (spend-down may raise the effective start, never below `min`), and affinity (ties break toward the previous task's binding; same-profile streaks batch). Rate-limit failover is the binder rebinding the same rung to another pool. Pins force a fixed binding where determinism matters.

**The affinity gradient** (context-switch cost, warmest → coldest): resume the *same session* (whole conversation cached) → new session, *same model*, within the provider's cache window (prefix hits on the system-and-repo preamble — the mechanism behind the ~97% cache-read rates heavy Claude Code users see) → same vendor, different model (cache-cold, harness-warm) → different vendor (cold everything: full context re-ingestion plus a different harness reading the conventions brief fresh). Copilot adds a useful middle rung: a cross-*vendor model* switch without a harness switch. v0.1 implements affinity as a tie-break plus streak batching; the full switch-cost model waits for real decision-log data — guessing reload costs is worse than measuring them.

Every routing decision and outcome is logged with the task's features. `tactus export-decisions` (v0.2) emits the dataset a learned router would train on.

## 11. Verification ladder (P4)

An **attempt** = agent run → gates → review. The ladder:

1. **Gates first** — configured commands (compile, tests, lint), sequential, short-circuit; output tail (8 KB) becomes feedback. Gates are what make cheap models affordable: objective, free, and they catch most small-model failures before any frontier tokens are spent. Evidence-gate axes adopted from the field's best practice: **an empty diff can never pass** ("done" claims require changed code), red tests block, and **test provenance is enforced for Test tasks — a new test must fail on the base commit and pass on HEAD**, or it proves nothing.
2. **Review** — a read-only worker profile receives task + acceptance + conventions brief + the engine-captured diff, and must end with a fenced JSON verdict (`pass`, `reasons`, `required_changes`). The engine parses the last fenced block; one re-ask on unparseable, then it counts as failure. The reviewer prompt includes an anti-sycophancy instruction: its job is to find reasons to fail, not to agree.
3. **Cross-vendor second opinion** — for paths matching configured globs (default: the blast-radius set), a second reviewer from a *different model family* judges the same diff (e.g. GPT-via-Copilot reviewing Claude-written code). Different families share fewer blind spots; one Copilot subscription makes this a `--model` flag rather than a second product. Both verdicts must pass.
4. **Retry, then escalate** — failure feedback (gate log or `required_changes`) goes back to the *same rung* via session resume where the adapter supports it (in-context feedback lands far better than a fresh start); `attempts_per` exhausted → next rung, fresh session, accumulated feedback summary included. Chain exhausted → **the human is the top rung**: an `Unblock` question with full context. Declined or unanswered under CI mode → task `Failed`, dependents `Blocked`, independent work continues.

## 12. Interaction model (P8)

- **Questions are events**, scoped to `affected_tasks`. Exactly those park in `AwaitingInput`; the scheduler keeps draining everything else — in v0.1's sequential mode by skipping ahead to the next ready independent task, in v0.2 across parallel worktrees.
- **Raised eagerly** — at detection, not at attempt: the designer resolves most at design time; at runtime a worker can flag uncertainty in its outcome and the reviewer can emit a `needs-human` verdict, both of which raise the question immediately while unrelated work proceeds.
- **Pre-filtered by the architect**: question + decisions record → frontier profile → "already answered?" Only novel questions reach a human, and every one that does is logged as a `design_defect`.
- **Hard block has a precise definition**: the runnable frontier is empty and every remaining task transitively depends on an open question. Anything less keeps running.
- **Channels**: `tactus answer <id>` and attached-terminal prompts in v0.1, desktop notifications in v0.1, Telegram/Slack notifier plugins in v0.2 (delivery only — answers always arrive as events, so a run survives its notifier).
- **Spend approvals**: `ask_before` thresholds (e.g. frontier escalation projected over $N, or any run past its soft budget) raise `ApproveSpend` questions instead of silently spending.
- **CI mode** (`interaction = "never"`): questions degrade to parked-task reporting; exit status distinguishes clean completion from completion-with-parked-tasks.

## 13. Capacity engine (P9)

The router's economics depend on which pool pays. Pools have different shapes, and the engine models them explicitly:

| Pool kind | Example | Unit | Reset shape |
|---|---|---|---|
| Subscription windows | Claude Max 5x/20x | tokens (est.) | 5-hour rolling + weekly cap |
| Metered credits | Copilot on AI-Credit billing (post-Jun 2026) | credits ≈ $ | monthly allowance + PAYG |
| Legacy request pools | Copilot annual plans | premium requests × per-model multiplier | monthly |
| API keys | Anthropic/OpenAI direct | dollars | none (budget only) |
| Local | home-server models via an OpenAI-compatible endpoint | unlimited | none (hardware-bound) |

**Estimation sources**, in trust order: (1) rate-limit signals from the CLIs — ground truth; a `RateLimited` outcome immediately marks the pool exhausted, demotes or parks frontier-hungry tasks, and sets a retry-at-reset timer; (2) self-metering of everything the engine spawned; (3) ccusage-style parsing of local agent logs, which captures the user's *interactive* sessions drawing from the same pool; (4) optionally, provider usage endpoints where they exist — treated as fragile (several are reverse-engineered and break silently) and never load-bearing. Estimates are always conservative: a per-pool `safety_margin` (usage on other machines is invisible to local log parsing) and a `reserve` floor that keeps headroom for the user's own interactive work.

**Discovery — `tactus connect`.** Pools are connected, not configured: `connect` scans PATH for official CLIs, checks auth state, detects each plan's quota shape, enumerates available models, and writes the user-level pools file. Tier classification comes from a **capability catalog** shipped with the binary (static data — the no-HTTP invariant holds), with a pragmatic prior for unknowns: providers price their own models, so per-model multipliers and per-token rates rank capability. A model absent from the catalog is never auto-selected — pin it or update. Decision logs later calibrate the catalog with measured pass rates per tier and task kind.

**Strategies** (`routing.strategy.mode`):

- `conserve` — classic cost minimization: route down aggressively, escalate only on failure, defer frontier-hungry tasks toward window resets when a pool is projected to run dry.
- `value-max` — subscription yield management: prepaid capacity that would expire unused has zero marginal cost, so surplus near a reset biases default tiers **up** (spend-down mode) — Opus for implementation, frontier review everywhere — subject to `min`/`max` bounds and the reserve floor. *No shipped tool does this (verified Aug 2026); it is the headline.*
- `deadline` — wall-clock first: maximize parallel throughput within capacity, spilling to API dollars when justified by a configured $/hour ceiling.

The ledger accounts every attempt in both currencies: API-equivalent dollars (honestly labeled — subscription spend is notional) and pool units drained. Budgets exist per run ($), per task ($), and per pool (fraction).

**Sequencing:** v0.1 ships the capacity engine **read-only** — the dry-run preview and `tactus capacity` show each pool's estimated remaining capacity, resets, and what each strategy *would* do. v0.2 wires it into live routing. This de-risks estimator fragility before any routing depends on it, and the preview alone is the demo that sells the product.

## 14. Execution engine — v0.1 (sequential)

- **Pre-flight:** clean working tree required; every gate command resolves; every referenced agent binary probed (`probe()` logs version + capabilities — Copilot's CLI auto-updates and has shipped breaking flag removals, so capability probing is not optional); plan parses cycle-free; capacity snapshot taken.
- **Run branch:** `tactus/run-<ulid>` from HEAD; the user's branch is never dirtied.
- **Order:** stable topological sort (ties by plan order), with skip-ahead past `AwaitingInput` tasks to the next ready independent task.
- **Per task:** materialize prompt (body + acceptance + artifacts_in + conventions brief) → agent runs in repo root → engine captures `git diff` → gates → review(s) → **engine commits** `[tactus] <task-id>: <title>` on pass.
- **Rollback on failed attempt:** `git checkout . && git clean -fd` back to the last commit — unless the retry resumes the same session, in which case the tree stays and the *cumulative* diff is re-gated.
- **Timeouts:** per-attempt wall clock (default 30 min, per-profile override); timeout = attempt failure with partial transcript as feedback.

## 15. Event log, resume, run layout (P6)

```
.tactus/
  runs/<run-id>/                  # run-id = ULID
    plan.normalized.json
    events.jsonl                  # append-only source of truth
    artifacts/                    # conventions-brief.md, decisions-record.md, contracts
    transcripts/<task>-<attempt>.json
    questions/<question-id>.json  # rendered question payloads for notifiers
tactus.toml                       # repo-root config, checked in
```

Every transition is an event `{ts, event, task?, attempt?, rung?, profile?, data}` — including `question_raised`, `question_answered`, `design_defect`, `capacity_snapshot`, `pool_exhausted`, and `spend_down_engaged`. `status`, the ledger, and the capacity view are pure folds over this file. `tactus resume <run-id>` replays, verifies the run branch HEAD matches the last committed event (mismatch = refuse with an explanation), re-probes agents, re-snapshots capacity, and continues — parked questions intact.

## 16. Agent adapters (P2)

**Claude Code** (v0.1): `claude -p` via stdin, `--output-format json` (result, session id, cost/usage parsed defensively), `--model`, `--max-turns`, `--resume <session-id>` for same-rung retries. Permissions: never the skip-all flag — the adapter materializes a per-run `.claude/settings.json` granting file tools plus `Bash(<each gate cmd>)` to edit profiles and read-only tools to reviewers. Docs: https://docs.claude.com/en/docs/claude-code/headless (flags verified Aug 2026).

**GitHub Copilot CLI** (v0.1): the multi-vendor pool — Claude, GPT, and Gemini models through one harness and one subscription. Route A: `copilot -p` with `--model`, `-s`, `--no-ask-user`, and granular `--allow-tool='shell(cargo test)'` mapping one-to-one onto profile permissions. Route B (preferred once stable for us): ACP — `copilot --acp --stdio`, JSON-RPC with `session/new`, `session/prompt`, streaming, cancel, and permission control. Operational posture: churniest adapter in the fleet (the CLI auto-updates and has removed programmatic flags without deprecation), so `probe()` gates every run and the adapter pins known-good behavior per version. Its billing moved to AI Credits in June 2026 with legacy annual plans keeping request multipliers — both shapes are handled by the capacity engine, not the adapter. Docs: https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference.

**Aider** (v0.2): `--yes`, `--model`; brings local models via OpenAI-compatible endpoints — the free pool for the home-server tier.

Adapter rule inherited as an invariant: subprocess the real binary, official CLIs only, no spoofed headers — ToS safety is a feature, not a compliance chore.

## 17. Configuration reference

Config splits along its natural seam (v2.1): **pools are user-level** — subscriptions travel with the person, discovered and written by `tactus connect` — while **routing and gates are repo-level overrides** on derived defaults. A fresh repo runs with zero config.

`~/.tactus/pools.toml` (written by `connect`, hand-editable):

```toml
[pools.claude-max]
kind = "subscription-window"
agent = "claude-code"
window = "5h"
weekly = true
sources = ["signals", "self", "local-logs"]
safety_margin = 0.15
reserve = 0.20                      # headroom for the user's interactive sessions

[pools.copilot]
kind = "credits"                    # "request-pool" on legacy annual plans
agent = "copilot"
sources = ["signals", "self"]
monthly_allowance = "auto"

[pools.local]                       # v0.2
kind = "unmetered"
agent = "aider"
endpoint = "http://homeserver:11434/v1"
```

Repo-level `tactus.toml` — overrides only; everything below has a derived default:

```toml
[engine]
on_task_failure = "halt"            # halt | continue
max_parallel    = 1                 # >1 requires v0.2
shell           = "powershell"      # gate shell; default = platform native

[interaction]
mode      = "on_block"              # never | on_block | on_milestone
notify    = ["cli", "desktop"]      # + "telegram", "slack" in v0.2
ask_before = { frontier_escalation_over_usd = 5.0 }

[budgets]
run_usd  = 15.0                     # API-equivalent; omit = unlimited
task_usd = 4.0

[routing.strategy]
mode = "value-max"                  # conserve | value-max | deadline
spend_down_after = 0.7              # >70% of window left near reset → bias tiers up

[routing]                           # chains are ABSTRACT TIERS — the binder picks models and pools
fix       = { chain = ["small", "mid", "frontier"], attempts_per = 2 }
implement = { chain = ["mid", "frontier"], attempts_per = 2 }
review    = { tier = "frontier" }   # remaining kinds keep derived defaults

[[routing.overrides]]
paths = ["src/auth/**", "migrations/**"]
start_at = "frontier"
second_opinion = "different-vendor" # binder must add a reviewer from another model family

[[pins]]                            # optional determinism — otherwise the binder chooses
tier  = "frontier"
agent = "claude-code"
model = "claude-opus-4-8"

[[gates]]
name = "check"
cmd  = "cargo check --all-targets"
timeout_secs = 600

[[gates]]
name = "test"
cmd  = "cargo test"
timeout_secs = 1200
```

## 18. CLI surface

```
tactus connect                     # discover installed agent CLIs, auth, plans; writes ~/.tactus/pools.toml
tactus design <brief>              # v0.3: interactive design phase (until then: Claude Code plan mode)
tactus validate <plan>             # parse, task table, routing + capacity preview
tactus run <plan> [--dry-run] [--budget <usd>] [--config <path>]
tactus resume <run-id>
tactus status [<run-id>] [--follow]
tactus answer <question-id> [--option N | --text "..."]
tactus capacity                    # all pools: remaining, resets, active strategy effect
tactus export-decisions <run-id>   # v0.2: routing dataset (JSONL/CSV)
```

`--dry-run` executes everything except agents: parse, route, and print task → kind → chain (with source of each decision: config/annotation/override) → gates → pool + strategy effect, at zero spend. It exists from day one; it is both the config-iteration loop and the sales demo.

## 19. Failure handling

| Failure | Detection | Handling |
|---|---|---|
| Agent binary missing / probe failure | pre-flight | refuse to start |
| Agent spawn error | engine | halt run (environment, not task) |
| Agent non-zero / timeout | adapter | attempt failure; feedback = stderr/transcript tail |
| Rate-limited | adapter signal | pool marked exhausted; task deferred to reset or demoted per strategy (never below min) |
| Gate failure | gate runner | attempt failure; feedback = log tail |
| Review failure | verdict | attempt failure; feedback = required_changes |
| Chain exhausted | router | `Unblock` question to human (top rung); declined/CI → task Failed, dependents Blocked |
| Question parked, frontier non-empty | scheduler | continue independent tasks |
| Runnable frontier empty | scheduler | hard block (interactive) / end run reporting parked tasks (CI) |
| Budget or pool budget exceeded | ledger | stop scheduling; run ends `BudgetExceeded` |
| Merge conflict (v0.2) | merge queue | auto-spawn Fix task on `mid` in a rebased worktree |
| Engine crash / power loss | — | `tactus resume` replays the event log |

## 20. Safety and permissions

- Unattended agents run with pre-granted, **narrow** permissions materialized per profile (Claude Code settings; Copilot `--allow-tool`). The skip-all-permissions class of flags is never used. Edit profiles get no network tools; gates are the only commands they may run; reviewers are read-only.
- The engine refuses dirty trees, never force-pushes, never touches remotes.
- Plans are data, but an agent executing a malicious plan step with edit rights is not: trust in the plan's source is a prerequisite. For untrusted plans, run in a container or dedicated user; the engine never elevates.
- Before public launch: re-verify each provider's terms on headless/automated CLI use. The official-CLI-only stance is the defensible posture; keep it clean.

## 21. Versioned scope

**v0.1 — the conductor works (sequential).** Claude Code plan-mode adapter + annotation grammar; Claude Code AND Copilot CLI adapters (Copilot promoted to v0.1 — it buys cross-vendor models and a second pool in one move); sequential engine with skip-ahead; run branch + engine-owned commits + rollback; gates with evidence axes; reviewer with structured verdicts + optional cross-vendor second opinion; retry-with-resume + rung escalation with the human as top rung; questions with CLI/desktop delivery and `tactus answer`; event log, resume, status, ledger; capacity engine **read-only** (`tactus connect` discovery, preview + `tactus capacity`); `validate` and `--dry-run`; pre-flight probing.

Build order (each step leaves a runnable binary): 1 IR + config + validate → 2 plan adapter + annotations → 3 Claude Code adapter → 4 sequential engine + git ownership → 5 gates → 6 reviewer + verdicts → 7 retry/escalation ladder + human rung + questions → 8 events/resume/status/ledger → 9 Copilot adapter + cross-vendor review → 10 connect + capacity read-only + dry-run + polish.

**v0.1 definition of done:** on a real repo, a 3–5 task annotated plan completes end-to-end where (a) a small-model task passes gates first try, (b) a gate failure recovers via same-rung session-resume feedback, (c) one task escalates a rung and passes, (d) one question parks a task while an independent task proceeds, answered via `tactus answer`, and (e) the summary reports per-task attempts, models, API-equivalent cost, and per-pool drain, with the dry-run having previewed capacity beforehand. Then kill the engine mid-run and `resume` finishes it.

**v0.2 — parallel + capacity-driven.** Worktree-per-task; tokio DAG scheduler with per-agent semaphores; readiness = Merged; overlap serialization from path hints; merge queue + conflict→fix-task; capacity-driven routing live (conserve / value-max / spend-down, reserve floors, rate-limit adaptation); affinity assignment (streak batching + measured switch costs from decision logs); Telegram/Slack notifiers; Aider adapter + local pool; task-master/JSON/checklist plan adapters; `export-decisions`.

**v0.2 definition of done:** a plan with two independent branches runs at `max_parallel = 3`, visibly interleaves in `status --follow`; one deliberate merge conflict is auto-resolved by a spawned fix task; one question is answered from a phone while the run keeps moving; and near a window reset with surplus capacity, spend-down mode observably biases assignments up-tier — with the ledger proving what each pool paid.

**v0.3 — direction.** The design pane (interactive Phase-1 product) and a web dashboard, both as thin clients over the event log; a GitHub Action wrapping `tactus run`; the design-defect feedback loop surfacing into the designer prompt; learned routing from exported decisions.

## 22. Adopted from the field (with credit)

- **Fresh-context-per-stage** discipline — every worker starts clean and receives only curated artifacts (Context Foundry).
- **Evidence-gate taxonomy** — empty-diff refusal, red-test blocking, test provenance (fail-on-base/pass-on-HEAD), secret-leak axis — and the anti-sycophancy reviewer stance (Loki Mode).
- **ACP (`--acp --stdio`)** as the durable programmatic surface for the Copilot adapter (GitHub).
- **Notifier transport abstraction** and the "subprocess the real binary, no spoofed headers" ToS posture (ductor).
- **Local-log usage parsing** as a capacity source that sees interactive sessions too (ccusage lineage).

## 23. Risks and kill criteria

- **First-party absorption** is the dominant risk: single-vendor multi-agent orchestration is commoditizing quarterly (Claude Code agent teams, GitHub Agent HQ, Codex subagents). Durable value concentrates in cross-pool yield (P9), cross-vendor arbitrage (P4), and neutrality — the parts no single vendor is incentivized to build. **If a first party ships cross-pool capacity-aware routing with spend-down, P9's moat is gone: pivot to neutrality + arbitrage as the sole wedge.**
- **Estimator fragility:** provider usage endpoints break silently; hence signals-first trust order, read-only capacity in v0.1, and log-parse fallbacks.
- **Catalog staleness:** model rosters churn monthly; unknown models are never auto-selected, the catalog ships with releases, and pricing-derived priors bridge gaps.
- **Adapter churn:** Copilot's CLI has removed flags without deprecation; probing at pre-flight and per-version pinning are load-bearing, not nice-to-haves.
- **Context Foundry threshold:** if it ships its "adaptive pipeline" escalation *and* passes ~500 stars, it becomes a real OSS competitor on P3/P4 — differentiate on P8/P9.
- **Name:** tactus collides with a tiny alpha DSL in the same space; acceptable, decided deliberately. Publish the crate placeholder immediately; revisit only if that project grows teeth.

## 24. References

- Claude Code headless mode: https://docs.claude.com/en/docs/claude-code/headless — and overview: https://docs.claude.com/en/docs/claude-code/overview (flags verified Aug 2026)
- Copilot CLI programmatic reference: https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference · running programmatically: https://docs.github.com/en/copilot/how-tos/copilot-cli/automate-copilot-cli/run-cli-programmatically (verified Aug 2026)
- Companion research: *Prior-Art & Competitive Landscape (Aug 2026)* and *Round-2 Competitive Intelligence* — the competitive matrix, closest-competitor profiles, moat ranking, and demand evidence backing §2 and §23.
