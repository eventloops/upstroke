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

**Backlog adapters (v0.2+):** Jira, Azure DevOps work items, GitHub Issues. These feed **Phase 1, not Phase 2** — a backlog item is not a plan: no dependency DAG, no acceptance criteria a gate can check, no tier annotations, no conventions brief. The importer emits a *draft* plan that the designer then subjects to question exhaustion (§5); execution still runs only frozen, annotated plans. Feeding a backlog straight to Phase 2 would point unattended agents at under-specified stories, which is the failure the two-phase lifecycle exists to prevent. Invariant 2 holds by subprocessing the vendor's own CLI (`az boards`, `acli`, `gh`) from a separate `tactus import` command — the network stays out of the engine and reuses auth the user already has. **Write-back is a different seam:** transitioning the item on commit, attaching branch and shas, moving it to Blocked when a question parks the task is a `Notifier` over the event log (§8), not a plan adapter. `Task` gains an `external_ref` so a run traces back to the item that spawned it.

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

1. **Gates first** — configured commands (compile, tests, lint), sequential, short-circuit; output tail (8 KB) becomes feedback. Gates are what make cheap models affordable: objective, free, and they catch most small-model failures before any frontier tokens are spent. Evidence-gate axes adopted from the field's best practice: **an empty diff can never pass** ("done" claims require changed code), red tests block, and **test provenance is enforced for Test tasks — a new test must fail on the base commit and pass on HEAD**, or it proves nothing. The **secret-leak axis belongs here too, not with the reviewer**: added lines are checked for credential shapes deterministically, or by a scanner the user configures as a gate command. A regex beats a frontier model at this, costs nothing, and runs on every attempt — model judgement should carry the axes that actually need judgement.
2. **Review** — a read-only worker profile receives task + acceptance + conventions brief + the engine-captured diff, and must end with a fenced JSON verdict (`pass`, `reasons`, `required_changes`). The engine parses the last fenced block; one re-ask on unparseable, then it counts as failure. The reviewer prompt includes an anti-sycophancy instruction: its job is to find reasons to fail, not to agree.
3. **Cross-vendor second opinion** — for paths matching configured globs, a second reviewer from a *different model family* judges the same diff (e.g. GPT-via-Copilot reviewing Claude-written code). Different families share fewer blind spots; one Copilot subscription makes this a `--model` flag rather than a second product. Both verdicts must pass, and the two reviewers are **independent** — neither is told the other's verdict, because a reviewer who knows the change was already approved stops looking. Turned on per override with `second_opinion = "different-vendor"` rather than applied to every blast-radius path by default: §11.5's cost argument applies here too, and an unrecognised value for that key is a hard config error, because a typo must not silently delete a verification layer. **"Vendor" here means model family, not CLI**: Copilot serves Anthropic models too, so an agent-id comparison would happily pair `claude-opus-5` with itself through a different harness and keep every blind spot. Where the configured second opinion cannot resolve — no other family at that tier has an adapter that probes — the run refuses at pre-flight rather than quietly running one pass.

   The same family axis settles a defect this ladder had until step 9: at the frontier rung the implementer's binder and the reviewer's binder resolve identically, so **a frontier task was reviewed by the model that wrote it**. The reviewer now rebinds to a different family whenever it would otherwise be the *same model* (exact identity, not family similarity — sonnet judged by opus is a genuine second look). That rebind is opportunistic: on a single-vendor install it warns — naming the tasks — and reviews same-model rather than refusing, because nobody asked for it. It is also suppressed when a second opinion is already configured for the task — rebinding there would resolve both passes onto the same different-family model and drop the original family's review entirely, which is worse than the self-review it was avoiding. Who reviews is recorded in `run_started`, so a CLI installed between a run and its resume cannot quietly become its judge; a log that predates that record re-derives and says so, because an *absent* record is not a record of "no reviewers". The recorded cross-family reviewer stays opportunistic on resume too: refusing to continue over a judge that may never have judged anything costs more than it protects, and the per-attempt record names who judged each attempt either way.
4. **Retry, then escalate** — failure feedback (gate log or `required_changes`) goes back to the *same rung* via session resume where the adapter supports it (in-context feedback lands far better than a fresh start); `attempts_per` exhausted → next rung, fresh session, accumulated feedback summary included. Chain exhausted → **the human is the top rung**: an `Unblock` question with full context. Declined or unanswered under CI mode → task `Failed`, dependents `Blocked`, independent work continues.
5. **Security lens (v0.2)** — the cross-vendor second opinion generalizes the reviewer from a single pass into a **list of passes, each with a lens and a pass rule** (shipped in step 9: passes run in order, short-circuiting like gates, and share one review budget rather than each taking a full one); a mandatory security review is then that same mechanism with an adversarial prompt and, ideally, a different model family. Scoped through the existing blast-radius overrides rather than applied globally: a mandatory frontier security pass on every task roughly doubles review spend, while scoping it to `src/auth/**` and `migrations/**` costs almost nothing and hits where blast radius already said to look. **Its ladder dispatch differs deliberately** — a security finding must never enter the retry-until-it-passes loop, which is how a real finding gets laundered into a commit. It goes to an `Unblock` question with the finding attached instead of round the rungs again.

## 12. Interaction model (P8)

- **Questions are events**, scoped to `affected_tasks`. Exactly those park in `AwaitingInput`; the scheduler keeps draining everything else — in v0.1's sequential mode by skipping ahead to the next ready independent task, in v0.2 across parallel worktrees.
- **Raised eagerly** — at detection, not at attempt: the designer resolves most at design time; at runtime a worker can flag uncertainty in its outcome and the reviewer can emit a `needs-human` verdict, both of which raise the question immediately while unrelated work proceeds.
- **Pre-filtered by the architect**: question + decisions record → frontier profile → "already answered?" Only novel questions reach a human, and every one that does is logged as a `design_defect`.
- **Hard block has a precise definition**: the runnable frontier is empty and every remaining task transitively depends on an open question. Anything less keeps running.
- **Channels**: `tactus answer <id>` and attached-terminal prompts in v0.1, desktop notifications in v0.1, Telegram/Slack notifier plugins in v0.2 (delivery only — answers always arrive as events, so a run survives its notifier). `tactus answer` writes a file beside the question rather than appending to the log, keeping `events.jsonl` single-writer; the engine ingests it and records the `question_answered` event itself, on its next scheduler turn if it is live or at the next resume if it is not. Which channel a hard block uses is not a mode question alone: `on_block` at an attached terminal prompts, and the identical config detached waits for `tactus answer` up to `[interaction] wait_on_block_secs`.
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

`connect` enumerates **credential profiles, not just installed binaries**: one vendor can back several pools — two Claude Max accounts, say — and the binder selects between them per attempt through the provider's own profile mechanism, an environment variable on the subprocess rather than a token the engine ever handles (invariants 2 and 5). Whether the CLI honours profile selection is a `probe()` axis like any other, verified at pre-flight instead of discovered mid-spend. Several same-kind pools change three things: estimates must **attribute** usage per profile rather than aggregate it (local-log parsing reads per-account state, so summing reports one healthy pool where there is one exhausted and one fresh); `reserve` becomes asymmetric, since a plan bought for unattended work has no interactive use to protect; and independent reset windows turn a rate limit from *wait* into *rebind and continue* (§10.5) — the single biggest practical gain for an overnight run. Affinity still governs the order: prompt caches are per-account, so the binder drains one pool toward its reserve and then switches, rather than round-robining and paying cache-cold on every task.

**Strategies** (`routing.strategy.mode`):

- `conserve` — classic cost minimization: route down aggressively, escalate only on failure, defer frontier-hungry tasks toward window resets when a pool is projected to run dry.
- `value-max` — subscription yield management: prepaid capacity that would expire unused has zero marginal cost, so surplus near a reset biases default tiers **up** (spend-down mode) — Opus for implementation, frontier review everywhere — subject to `min`/`max` bounds and the reserve floor. *No shipped tool does this (verified Aug 2026); it is the headline.*
- `deadline` — wall-clock first: maximize parallel throughput within capacity, spilling to API dollars when justified by a configured $/hour ceiling.

The ledger accounts every attempt in both currencies: API-equivalent dollars (honestly labeled — subscription spend is notional) and pool units drained. Where a worker's route reports no spend at all — Copilot's does not (§16) — the total it contributes to is marked `?` rather than presented as complete, since a cross-vendor review makes that the normal case rather than a corner of it. Budgets exist per run ($), per task ($), and per pool (fraction).

**Sequencing:** v0.1 ships the capacity engine **read-only** — the dry-run preview and `tactus capacity` show each pool's estimated remaining capacity, resets, and what each strategy *would* do. v0.2 wires it into live routing. This de-risks estimator fragility before any routing depends on it, and the preview alone is the demo that sells the product.

## 14. Execution engine — v0.1 (sequential)

- **Pre-flight:** clean working tree required; every gate command resolves; every referenced agent binary probed (`probe()` logs version + capabilities — Copilot's CLI auto-updates and has shipped breaking flag removals, so capability probing is not optional); plan parses cycle-free; capacity snapshot taken.
- **Run branch:** `tactus/run-<ulid>` from HEAD; the user's branch is never dirtied.
- **Order:** stable topological sort (ties by plan order), with skip-ahead past `AwaitingInput` tasks to the next ready independent task.
- **Per task:** materialize prompt (body + acceptance + artifacts_in + conventions brief) → agent runs in repo root → engine captures `git diff` → gates → review(s) → **engine commits** `[tactus] <task-id>: <title>` on pass.
- **Rollback on failed attempt:** `git checkout . && git clean -fd` back to the last commit — unless the retry resumes the same session, in which case the tree stays and the *cumulative* diff is re-gated.
- **Timeouts:** per-attempt wall clock (default 30 min, per-profile override); timeout = attempt failure with partial transcript as feedback.

## 15. Event log, resume, run layout (P6)

The run directory is **split in two**, by who is allowed to read each half:

```
<repo>/.tactus/runs/<run-id>/     # run-id = ULID — the ops surface
    events.jsonl                  # append-only source of truth
    plan.normalized.json          # the frozen plan this run executes
    artifacts/                    # conventions-brief.md, decisions-record.md, contracts
    questions/<question-id>.json  # rendered question payloads for notifiers
    answers/<question-id>.json    # answers dropped by `tactus answer`
    run.lock                      # advisory; OS-released, so a crash leaves nothing stale
    report.json                   # projection of the log for humans; never read back
~/.tactus/runs/<run-id>/          # agent-authored — outside every agent's reach
    transcripts/<task>-<attempt>.json
    reviews/<task>-<attempt>-review.json
    settings/<task>-<attempt>.json    # the per-attempt permission surface
    gates/<task>-<attempt>-<gate>.log
tactus.toml                       # repo-root config, checked in
```

The split is enforcement, not tidiness. A reviewer is a read-only agent pointed at the workspace, so *anything in the workspace is reachable* — including the implementer's transcript, which invariant 3 says is exactly not the evidence a reviewer should judge on. Deny rules cannot close that on their own: gates execute repository code the implementer just wrote, and that code reads any workspace path no permission rule ever sees. So the agent-authored half lives where there is no path to it, and the deny rules on `.tactus/**` become defence in depth rather than the mechanism. Writes there are denied outright — with the log load-bearing, an agent that could append to it could forge a `task_committed`.

Every transition is an event `{ts, event, task?, attempt?, rung?, profile?, data}` — including `question_raised`, `question_answered`, `design_defect`, `capacity_snapshot`, `pool_exhausted`, and `spend_down_engaged`. `status`, the ledger, and the capacity view are pure folds over this file.

**One fold, not two.** The engine never mutates run state directly: it appends an event and folds it back in through the same function `resume` and `status` use to rebuild state from the file, and it applies the event *as it will be read back* rather than as constructed. A live run and a replay of its own log are therefore the same computation, not two that agree by inspection. Two things deliberately do not survive replay — a session id and its `resume_next` flag, because both describe a conversation that believed it had left edits in a working tree that a crash has since rolled back (§14 pairs session-resume with tree retention precisely so the two never diverge).

`tactus resume <run-id>` replays, verifies the run branch HEAD matches the last committed event (mismatch = refuse with an explanation), re-probes agents, re-snapshots capacity, and continues — parked questions intact. That HEAD check has one deliberate exception, because git and the log cannot be written atomically: §14 commits, reads the sha back, scrubs the tree, and only then appends `task_committed`, so a process that dies inside those three git calls leaves the branch one commit past its own record. Where that commit sits directly on the recorded head *and* carries the message this engine would have written for the task whose last attempt passed, resume adopts it rather than refusing — the alternative is telling the operator to reset away work that already passed its gates and its review, and to spend the attempt again. Anything short of the whole shape is still foreign history, and still refused. It also refuses when the frozen plan's hash moved, when routing resolved differently (a recorded rung is an index into a chain, so a changed chain silently means a different tier), when the branch is gone, and when another process holds the run.

**Gates are taken from the record, not re-derived — and not refused over.** `run_started` records each effective gate in full (name, command, shell, timeout) and a resume rebuilds and runs *those*, exactly as it reads the review plan from the record rather than re-resolving who judges. This is the property a live run already has for free: config is parsed once at pre-flight and gates execute from memory, so a mid-run edit to `tactus.toml` cannot change what a running task is verified against. Honouring the same snapshot across an interruption is what makes every `task_committed` in one log mean the same thing — and it matters concretely once runs self-host, because the workspace an implementer edits *contains the `tactus.toml` its own gates come from*. Refusing on a mismatch was the first design and was worse in both directions: it left the weakened-gate case detected but the run dead, and it made a gate edit that the run's own reviewed task legitimately committed permanently unresumable. A config that differs today is a warning naming the difference, not an error; the edit simply applies to the next run. Logs predating the record re-derive and warn, saying whether the recorded gate *names* still match — which is proof rather than suspicion when they do not. `shell` is recorded because it is half of what a command means (`cmd = "true"` always passes under `sh` and is not a program at all under `cmd.exe`); the portability that argued against pinning it does not exist anyway, since `private_dir` already records an absolute host path. An attempt the log ends mid-flight is settled as `attempt_interrupted`: recorded in the ledger with unknown spend, but not counted against the rung's allowance, because nothing judged the code — the same rule §19 applies to an outage.

## 16. Agent adapters (P2)

**Claude Code** (v0.1): `claude -p` via stdin, `--output-format json` (result, session id, cost/usage parsed defensively), `--model`, `--max-turns`, `--resume <session-id>` for same-rung retries. Permissions: never the skip-all flag — the adapter materializes a per-run `.claude/settings.json` granting file tools plus `Bash(<each gate cmd>)` to edit profiles and read-only tools to reviewers. Docs: https://docs.claude.com/en/docs/claude-code/headless (flags verified Aug 2026).

**GitHub Copilot CLI** (v0.1): the multi-vendor pool — Claude, GPT, and Gemini models through one harness and one subscription. **Route A ships; ACP does not, and the reason is the same one that makes this the churniest adapter.** Neither `--acp` nor `--stdio` appears in GitHub's programmatic reference, so there is no documented surface to pin known-good behavior against — and pinning per version is precisely what this adapter must do. ACP also needs a persistent bidirectional JSON-RPC session, where the rest of v0.1 spawns a process, feeds it, and reads what came back. `probe()` records `acp` as a capability axis regardless, so Route B stays a change inside one file once it is documented and stable.

Route A concretely: `-s` (response only, no decoration), `--no-ask-user`, `--model=`, and granular `--allow-tool='shell(cargo test)'` / `--deny-tool=` mapping one-to-one onto profile permissions — never the `--allow-all*` / `--yolo` class (§20). **The prompt goes on stdin and `-p` is never passed**: GitHub documents `echo … | copilot` as a programmatic form and documents that piped input is *ignored* when `-p` is also given, so passing both would silently discard the real prompt. Stdin is also the only delivery that survives Windows, where npm installs `copilot.cmd` and `cmd /C` caps the command line near 8 KB — well under a review prompt carrying 60 KB of diff.

What this route does not give us is recorded honestly rather than assumed: no JSON envelope, so no session id, no usage, and no cost — Copilot attempts appear in the ledger unpriced rather than free — and no documented session resume, so §11.4's same-rung retry starts fresh with accumulated feedback. Both are `Caps` axes the engine already dispatches on, and both default *pessimistic* here (advertised in `--help` or assumed absent), because claiming a capability this CLI lacks breaks every retry rather than merely degrading one. Its billing moved to AI Credits in June 2026 with legacy annual plans keeping request multipliers — both shapes are handled by the capacity engine, not the adapter. Docs: https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference.

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
# Both ceilings sum REPORTED dollars, so they bound only the routes that report
# any. A run whose implementer is Codex and whose reviewer is Claude Code is
# bounded on the review half alone — the Codex half reports tokens and no price
# (§21), and is bounded by its own subscription window instead. The ledger says
# so with `?`; this comment exists because `--budget 15` otherwise reads like a
# guarantee about the whole run. A token-denominated ceiling is v0.2 capacity
# work if it is ever wanted.

[routing.strategy]
mode = "value-max"                  # conserve | value-max | deadline
spend_down_after = 0.7              # >70% of window left near reset → bias tiers up

[routing]                           # chains are ABSTRACT TIERS — the binder picks models and pools
fix       = { chain = ["small", "mid", "frontier"], attempts_per = 2 }
implement = { chain = ["mid", "frontier"], attempts_per = 2 }
review    = { tier = "frontier" }   # remaining kinds keep derived defaults

[[routing.overrides]]                 # at least one of start_at / second_opinion
paths = ["src/auth/**", "migrations/**"]
start_at = "frontier"                 # optional — omit to add a reviewer without raising the floor
second_opinion = "different-vendor"   # binder must add a reviewer from another model family

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

- Unattended agents run with pre-granted, **narrow** permissions materialized per profile (Claude Code settings; Copilot `--allow-tool`; Codex `--sandbox`). The skip-all-permissions class of flags is never used, with one scoped exception decided alongside the v0.2 runner (§21): under a runner that is genuinely external — container-per-attempt — Codex runs its `external-sandbox` mode, because a container kept at standard hardening with the CLI's sandbox stood down beats one granted `SYS_ADMIN` so the inner sandbox can initialise. The exception exists only while the external boundary does. Edit profiles get no network tools; gates are the only commands they may run; reviewers are read-only.
- The engine refuses dirty trees, never force-pushes, never touches remotes.
- Plans are data, but an agent executing a malicious plan step with edit rights is not: trust in the plan's source is a prerequisite. For untrusted plans, run in a container or dedicated user; the engine never elevates.
- Before public launch: re-verify each provider's terms on headless/automated CLI use. This includes two specifics the design deliberately leaves to that check. **Multi-account pooling** (§13): profiles the operator holds and pays for is a different question from pooling accounts across people, which is account sharing and out of scope whatever the answer. **Spend-down** (§13's `value-max`): using prepaid capacity is not a violation, but a strategy whose stated purpose is to consume quota before it expires sits close to the usage pattern rate limits exist to shape — which is why `conserve` is the derived default and spend-down is opt-in. In both cases the mechanism is the vendor's own sanctioned surface, so an unfavourable answer costs a config option rather than an architecture. The official-CLI-only stance is the defensible posture; keep it clean.

## 21. Versioned scope

**v0.1 — the conductor works (sequential).** Claude Code plan-mode adapter + annotation grammar; Claude Code AND Copilot CLI adapters (Copilot promoted to v0.1 — it buys cross-vendor models and a second pool in one move); sequential engine with skip-ahead; run branch + engine-owned commits + rollback; gates with evidence axes; reviewer with structured verdicts + optional cross-vendor second opinion; retry-with-resume + rung escalation with the human as top rung; questions with CLI/desktop delivery and `tactus answer`; event log, resume, status, ledger; capacity engine **read-only** (`tactus connect` discovery, preview + `tactus capacity`); `validate` and `--dry-run`; pre-flight probing.

Build order (each step leaves a runnable binary): 1 IR + config + validate → 2 plan adapter + annotations → 3 Claude Code adapter → 4 sequential engine + git ownership → 5 gates → 6 reviewer + verdicts → 7 retry/escalation ladder + human rung + questions → 8 events/resume/status/ledger → 9 Copilot adapter + cross-vendor review → 10 connect + capacity read-only + dry-run + polish.

**v0.1 definition of done:** in a real git repository — real gates, real agent CLIs, real spend, nothing mocked; seeded for the purpose counts, inherited history is not required — a 3–5 task annotated plan completes end-to-end where (a) a small-model task passes gates first try, (b) a gate failure recovers via same-rung session-resume feedback, (c) one task escalates a rung and passes, (d) one question parks a task while an independent task proceeds, answered via `tactus answer`, and (e) the summary reports per-task attempts, models, API-equivalent cost, and per-pool drain, with the dry-run having previewed capacity beforehand. Then kill the engine mid-run and `resume` finishes it.

**v0.1 was met on 2026-08-10.** A five-task plan ran unattended against a scratch repository through all five criteria and the kill/resume test, and the engine was then used on a real published library. `acceptance/RESULT.md` records both, with the evidence for each criterion and the engine defects found: **three from the acceptance run and a fourth from the real-library run that followed** — all fixed. (The count is stated per-run wherever it appears, because "three" and "four" are both true of different things and the difference is which run is being talked about.) The definition of done above said "on a real repo" until after the run, and was tightened afterwards to say which part of "real" it meant — that is a clarification made with hindsight, and the ambiguity it removes is the one that had the README calling a scratch repository real. Released as `0.1.0`; `0.0.1` was a name reservation only.

**v0.2 — parallel + capacity-driven.** Worktree-per-task; **execution runner — container-per-attempt as an optional layer, decided 2026-08-11, rationale below**; tokio DAG scheduler with per-agent semaphores; readiness = Merged; overlap serialization from path hints; merge queue + conflict→fix-task; capacity-driven routing live (conserve / value-max / spend-down, reserve floors, rate-limit adaptation); affinity assignment (streak batching + measured switch costs from decision logs); Telegram/Slack notifiers; **OpenAI Codex adapter (landed 2026-08-11, ahead of the rest of v0.2)**; Aider adapter + local pool; task-master/JSON/checklist plan adapters; `export-decisions`.

**Why the Codex adapter came first, and what it turned out to be.** Copilot was promoted into v0.1 because it bought cross-vendor models and a second pool in one move; this one buys the second pool *directly*, and that turned out to be the binding constraint rather than a convenience. §13's capacity engine assumes several subscriptions with independent windows, and v0.1 shipped able to drive exactly one — so a week of real work exhausts a single vendor's quota and the engine stops, with the design's whole answer to that sitting unreachable. Everything else in v0.2 is throughput; this is capacity, and capacity is what ran out first.

**Its implementer path is Linux-only, and that is a platform fact rather than a CLI one.** Codex sandboxes through an external helper: `codex doctor` reports a path for it on Linux and `none` on Windows. With nothing to enforce a boundary, Windows `exec` — which forces `approval_policy = never` — degrades to read-only and then *accepts `--sandbox workspace-write` while writing nothing*, exit 0, no warning; run `01KZRMHA28M5CM88VAXP613X9P` spent both attempts on empty diffs before parking to ask for access it had. Its only writing mode there (`--approve-for-me`) auto-approves writes anywhere on the filesystem, including outside the repository, which §14's `git clean -fd` rollback cannot undo — so §20 rules it out and the adapter refuses at build time (§19). On Linux the same flags behave: writes land inside the workspace and are blocked outside it, both measured, so implementation is open there. Containerising it needs `--security-opt seccomp=unconfined --cap-add SYS_ADMIN`, or the sandbox fails to initialise and produces the same empty diff by a third route. All measured against codex-cli 0.147.0 with ChatGPT-plan auth on 2026-08-11; `src/agent/codex.rs` carries the detail.

**The reviewer seat works everywhere and is the immediate win** — `read-only` is enforced on every platform, the family is genuinely non-Anthropic, and a judge that spends nothing on the Claude window is worth having by itself. Verified end to end on run `01KZRN48A4ZK3AEDST3RJ8HMA4`: the first §11.3 cross-family review this project has ever actually run, after claiming the capability since v0.1. It also carries a `Caps` axis the others do not — usage without pricing — recorded per attempt and rendered as `?`, exactly as §13 says an unpriced route should be.

**The runner layer (v0.2): the container is the floor, not the ceiling.** The Codex findings raised the obvious question — with agent sandboxes this uneven (Codex has none on Windows; Copilot's deny-by-default is admitted unverifiable in its own adapter), why not run every agent in an OS-level container and stop caring about their surfaces? The answer that survived scrutiny is a *layer*, not a replacement; the premise is about 60% true, and the design is knowing which 60%. What a container uniquely buys, in order: it is the first mechanism in this design that confines **gate-executed repository code** — gates run the diff's own build scripts with the tactus process's full authority, which no agent permission surface can ever bound and which is why §15 moved transcripts out of the workspace rather than trying; a `:ro` mount makes the reviewer's read-only *mechanically* perfect instead of flag-deep, ending the reviewer-edits-what-it-judges class outright; and an image with version-pinned CLIs makes the mid-run self-update that killed acceptance run 1 structurally impossible. What it cannot replace: **the network**. An agent's entire function is a network conversation with its vendor, so the container cannot close the channel, and selective egress — allow the vendor API, deny everything else — is a proxy project, not a docker flag. Until one exists, §20's no-URL-grant agent policy remains the only control on the largest exfiltration channel, which alone kills "we don't need to care about the agents." Adapters also keep every duty that is not filesystem confinement — prompt delivery, output parsing, resume semantics, rate-limit phrasing, each CLI's suppress-prompts flag; the permission surface is roughly a fifth of each adapter, and the runner touches only that fifth.

**Runner design commitments, recorded now so the build inherits them.** (1) A runner is orthogonal to an adapter: `[runner]` config selects `host` or `container` (image, mounts), the adapter builds the command, the runner decides where it executes — adapters never learn about containers, and the runner learns nothing about agents beyond which per-agent credential volume to mount (persistent volumes, not ephemeral copies: some CLIs rotate refresh tokens on use, and a discarded rotation forces re-login). This is the same seam §23's runner-fleet model and v0.3's GitHub Action plug into, so the layer is on the roadmap's path regardless. (2) Defence in depth stays the default: agent surfaces remain ON inside the container wherever they work; the container catches what they miss. (3) Codex under a runner uses its `external-sandbox` mode — measured 2026-08-11: its own sandbox needs `seccomp=unconfined` plus `SYS_ADMIN` to initialise under Docker's defaults, and granting the container more so the inner layer can grant less is the wrong trade; one standard-strength boundary beats two weakened ones. §20's ban on the skip-sandbox class gains exactly that one scoped exception, stated there. (4) Sequenced with worktree-per-task because both redesign where an attempt executes; building them separately is building it twice. On Windows the honest cost is named now: container-per-attempt means the repository living WSL-side for filesystem performance — an operator-environment migration, not a footnote. Until the runner exists, the zero-code path stands: run the conductor itself on Linux or WSL, where all three CLIs work, the engine is best-tested since the lock rework, and the Windows-only Codex implementer refusal opens by construction. That path, not the reviewer seat, is where the quota relief lives — in the frontier-implementer regime the implementation half dominates spend (§23.2 as scoped), so relief means moving the *worker* off the Claude window; a free cross-family reviewer is worth having for §11.3's own sake, not as the savings.

**v0.2 definition of done:** a plan with two independent branches runs at `max_parallel = 3`, visibly interleaves in `status --follow`; one deliberate merge conflict is auto-resolved by a spawned fix task; one question is answered from a phone while the run keeps moving; and near a window reset with surplus capacity, spend-down mode observably biases assignments up-tier — with the ledger proving what each pool paid.

**v0.3 — direction.** The design pane (interactive Phase-1 product) and a web dashboard, both as thin clients over the event log; a GitHub Action wrapping `tactus run`; the design-defect feedback loop surfacing into the designer prompt; and routing *prediction* — a frontier model predicting rung and cost at `--dry-run`, shipped only if §23.2's calibration test passes. Learned routing from exported decisions is parked indefinitely at personal scale — single-digit samples per routing cell, and quarterly model churn decays the dataset about as fast as it grows (`decisions/2026-08-11-design-council.md`); the telemetry keeps landing because it is what makes small data interpretable, not because it will train anything.

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

### 23.1 Deployment model and the enterprise path (recorded 2026-08-10)

- **Per-seat is the deployment model, and it retires two risks rather than managing them.** Tactus runs on a developer's own machine, subprocessing a CLI signed in as *that developer* — through corporate SSO where there is one. Every call is a named seat-holder using their own seat, so there is no service account, no shared credential, and nothing to argue about with per-named-user licensing. The tempting alternative — a fleet of shared runners under a service account — is plausibly a ToS violation on most subscription plans and should not be built without written terms saying otherwise. The same model clears the licence question: internal per-seat use is *use*, and the AGPL's obligations attach to distribution or to offering a modified version over a network, so an enterprise adopting it internally owes nothing. **The commercial licence earns on a hosted aggregation layer, not on the CLI.**
- **A shared pool cannot be estimated from one seat, and v0.1's estimator says so honestly by accident.** Per-seat deployment means every §13 source except provider endpoints is *local*: Alice's signals, Alice's self-metering, Alice's logs. Against an org-level pool — Copilot premium requests, a Bedrock account, a Team allowance — forty instances each estimate a shared resource from a fortieth of the evidence. `Remaining::AtMost` stays *correct* there (unseen draw only ever reduces what is left) but the bound degrades toward vacuous. **v0.2's answer is a pool flag, not a better estimator:** an org-shared pool returns `Unknown` with a note naming why, rather than an upper bound that flatters. Provider endpoints are the only genuinely org-level signal and §13 already rates them fragile — a hint, never a floor. Log aggregation solves it properly and is a server, i.e. the company step.
- **The enterprise pitch is governance, not arbitrage.** Capacity yield is a consumer-surplus story; an org with a budget buys more seats. What an org cannot buy from any vendor is a cross-vendor account of what its agents did: an append-only log per run, the engine-captured diff as ground truth, a reviewer provably from a different model family, narrow permissions, and per-team cost attribution across vendors. One clause of this pitch is perishable and gets its own bullet below — reviewer diversity is an on-ramp, not a moat; the log, the pen, and the attribution are what survive any model menu. **Two of those features already ship without being built for it:** repo-level `tactus.toml` is policy distribution (a required second opinion on `src/auth/**` is committed to git and reviewable in a PR), and `reserve` reinterprets cleanly from "headroom for your own interactive work" to "headroom for your colleagues".
- **Model diversity is a commodity ingredient; the moat is the pen and the pools.** Recorded because the first draft of the bullet above nearly got it wrong. Two facts pull in opposite directions, and both belong here. The on-ramp: §11.3 keys verification on model *family*, not vendor, and Copilot alone serves three families — `copilot/gpt-5.3-codex` reviewing work implemented by `copilot/claude-sonnet-5` is genuine cross-family review, priced in premium requests from the one seat a Copilot Business org already pays for. Pins, not procurement: the "who would buy a second vendor for review?" objection dissolves, for the largest corporate segment, into an awareness problem. The flipside, which is what demotes this from moat to on-ramp: the same fact proves the ingredient sits in Microsoft's warehouse. The counter-positioning argument — *no vendor ships cross-model review; it is an admission* — holds for model vendors and **dies for distributors**: GitHub made the admission the day Claude entered the model picker, and cross-model review inside Copilot is a dropdown away from products it already ships. What survives that dropdown, in descending durability: **the pools** — every model in Copilot's picker drains one metered allowance on one billing relationship with one reset clock, and no vendor can spend the other vendor's subscription, so cross-pool economics (P9) is untouched by any model menu; **the record** — Copilot-reviews-Copilot is the platform grading its own homework at the product level whatever the weights, and audit logic turns on who *operates* the check rather than on whether the machinery is sound, so the attestation of an independent engine holding an append-only log survives total commoditization of model access; **the locality** — review gates the *commit*, on the developer's machine, on any host, including the GitLab and self-hosted estates where GitHub's review products do not exist. **Near-term kill criterion, sibling to the cross-pool one in §23:** the day GitHub ships cross-model review as a first-class gate — likelier and sooner than cross-pool routing — every pitch of the form "only we can put a second family's eyes on the code" is dead. Sell the pen, the pools, and the pre-commit gate from that day; stop selling model diversity the day before.
- **The interface wedge is abstraction, not aggregation — and the distinction is a kill criterion of its own.** A unified *prompt box* routing typed input to whichever CLI is the trap: it exposes the intersection of every backend, adds a fourth interface rather than removing three, and competes with Cursor and Claude Code on the interactive work that is their whole product and a footnote in ours. What tactus already does is stronger and stranger — it unifies the agent CLIs by **deleting** their interfaces. The plan file is the interface; a run touches Claude Code and Copilot without the operator ever opening either. §21's v0.3 clients therefore read and answer, never prompt: what every run did across every repo and pool, and what is parked waiting on a human. **The abstraction survives because it makes the differences legible rather than averaging them away** — `Caps` is that mechanism, dispatching on `session_resume` and `cost_reporting` so a retry that cannot resume starts cold *and says so*, and a route that reports no spend renders `?` rather than `$0.00`. An abstraction that hid those would be the lowest common denominator; one that surfaces them is the union plus an honest inventory of what is missing.
- **Corporate adoption relocates the design phase into a ceremony that already exists: refinement.** §5's two-phase lifecycle assumed the operator does the decomposition alone; a team already does it every sprint, on a calendar, with acceptance criteria and a Definition of Ready. A refined story maps onto the IR nearly 1:1 — key → `id`, story → `implement`, bug → `fix`, spike → `design`, summary → `title`, description → `body`, blocked-by links → `depends_on`, acceptance criteria → `acceptance`, component → `path_hints` — so the gap is *translation, not authoring*: an importer under §9's posture (subprocess `gh`, `acli`, the ADO CLI; never HTTP of our own), not a cognitive tool. The developer's day: pick up one or two stories at standup, import to plans, dry-run at zero spend, launch on your own seat (the deployment model above), spend the morning on work that needs a person, answer any parked question, review two run branches that each arrive *already reviewed by a different model family*. **The `design_defect` log becomes a refinement-quality metric no agile tool produces:** a badly refined story parks on a recorded question naming exactly what refinement failed to settle. Today that cost is a developer's half-day of clarification, absorbed invisibly; under tactus it is an event attributable to a story and aggregable per sprint — a Definition of Ready with a failure signal. The honest filter this adds: refinement discipline is rare and most backlogs are too vague to execute, but tactus *measures* that gap story-by-story instead of assuming it away.
- **What the corporate frame reorders.** The backlog importer displaces plan-authoring assistance as the highest-leverage unbuilt item — the plan already exists in the tracker, and import deletes the authoring step; writeback (the run's ledger line as a comment on the story) closes the loop where the team already looks. Cost-per-story is the manager-legible metric the ledger already computes, and it is what flips the adoption vector from *preference-push* (an individual who likes unattended work — a minority taste) to *process-pull* (a team watching $4/story with the reviewing model named). v0.2 parallelism gains weight: "pick up two stories" wants two worktrees, not a queue. The cheapest positioning artifact is documentation — a "Definition of Ready for agents" note stating what acceptance criteria must look like to be executable. And the first-party shadow must be named, because it extends §23's absorption risk: **assign-issue-to-agent lanes already exist** (GitHub ships issue → Copilot coding agent → PR today). The durable differentiation in that lane is exactly the fallback wedge §23 records: tactus runs on the developer's own seats and pools rather than metered cloud minutes, gates the commit on the developer's machine with a verdict whose record an independent engine holds, and leaves an audit log neither vendor can see the whole of — the durable subset, per the model-diversity bullet above. Nothing here is v0.1: the near-term version is one developer hand-translating two stories in ten minutes, and the importer automates the ten minutes.
- **Sequencing, so none of the above becomes a distraction:** prove the loop unattended (§21's acceptance run), use it on real work until it would survive a stranger's scrutiny, then find two or three teams with genuine multi-vendor spend and build what they actually ask for. The enterprise feature list guessable from here — dashboards, RBAC, SSO config — is almost certainly wrong. **The one thing worth doing early is cheap:** confirm against real enterprise terms whether agent CLIs may run under anything other than a named seat, because a "no" reshapes the roadmap and a "yes" opens the runner-fleet model this section otherwise rules out.

### 23.2 What the first real runs measured (recorded 2026-08-10)

- **Review is charged per attempt, so attempt count dominates cost — and §13's `conserve` framing names the wrong lever.** Measured on one task, same base commit and same reviewer, with only `attempts_per` differing: escalating on the first failure cost **$2.73** over two attempts, while retrying on the cheap rung cost **$3.21** over three — *despite* the cheaper arm using the cheaper worker throughout. A frontier review costs the same whatever rung it judges, and it was 44–77% of spend across four runs, so one extra attempt costs more than one cheaper worker saves. "Route down aggressively, escalate only on failure" therefore optimises the smaller half of the bill and can lose money doing it; what reduces spend is **fewer attempts**, which often means starting *higher*. Two things keep this honest. The cheap rung does genuinely recover — §21(b)'s same-rung retry is real, and a retry succeeded here on the third attempt — so this is an argument about price, not capability. And the shape the data points at is inexpressible today: `attempts_per` is one `u32` per kind (`config.rs`), not per rung, so "one shot on the cheapest rung, a retry higher up" is a v0.2 config change rather than a settings tweak. **When cost has to come down _while the implementer is cheap_, the lever is the reviewer, not the worker** — a cheaper judge on early rungs — and that trade must be made deliberately, because on this evidence the reviewer is the half that earns its keep: it rejected an emission that built clean and passed all 722 tests but was not a compile-time constant, and so would have failed CS0133 in a consumer's build. No gate can catch that. **The scoping matters and the emphasis above is deliberate:** every run behind these numbers started at `small` and the ones that succeeded landed at `small` or `mid`, so nothing here measures a frontier *implementer*. The sentence beside it — a frontier review costs the same whatever rung it judges — is what says the ratio must invert: review is a roughly fixed cost per attempt, while implementation scales with tier and with how much agentic work the task takes. Review's 44–77% share is therefore a fact about cheap workers, not a law. Read as a general finding it would send someone optimising the wrong half of a frontier-implemented run, which is the regime the Codex adapter (§21) exists to make affordable, and the one this project still has no numbers for. That gap is now recordable rather than merely regrettable: `AttemptRecord.usage` carries the tokens a CLI reports even when it reports no dollars, because a run that did not record its usage can never be re-measured.
- **The routing dataset is better than §10 implies and the prize is smaller than it sounds — bound it before building anything.** §10 promises `export-decisions` "emits the dataset a learned router would train on" and v0.3 lists learned routing. Two corrections, pulling opposite ways. In its favour: **escalation yields paired observations** — `small failed → mid ok` is two models attempted against an identical task, treatment varying with the task held constant, produced free as a side effect of the ladder. That is a better structure than most off-policy settings ever get, and the label (passed every gate and an independent frontier reviewer) is objective and adversarially generated, which is rare in this domain. Only one direction is censored: when the cheap rung succeeds, nothing learns whether the expensive one would have, and buying those cells means occasionally double-running on purpose. Against it: **a perfect oracle is worth only the attempts it would have skipped, measured at 15–25% of spend** — real at scale, transformative for nobody — and the residual doubt is about *features*, not sample count, since the task that defeated both cheap attempts here read as trivial from its text and was hard for a reason living in the codebase's semantics rather than in anything a feature vector recovers. **The cheap test is to ask a frontier model to predict rung and cost against runs whose outcome is already known**; if it is calibrated, ship that as a `--dry-run` step and drop the learned policy entirely. One methodological finding stands behind all of the above and generalises past it: two runs of an identical configuration on one task produced two *different failure modes* — a review rejection and a parked question — so a single-run A/B comparison of agent behaviour is not evidence, however clean its numbers look.

## 24. References

- Claude Code headless mode: https://docs.claude.com/en/docs/claude-code/headless — and overview: https://docs.claude.com/en/docs/claude-code/overview (flags verified Aug 2026)
- Copilot CLI programmatic reference: https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference · running programmatically: https://docs.github.com/en/copilot/how-tos/copilot-cli/automate-copilot-cli/run-cli-programmatically (verified Aug 2026)
- Companion research: *Prior-Art & Competitive Landscape (Aug 2026)* and *Round-2 Competitive Intelligence* — the competitive matrix, closest-competitor profiles, moat ranking, and demand evidence backing §2 and §23.
