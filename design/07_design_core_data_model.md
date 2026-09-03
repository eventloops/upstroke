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
    effort: Option<Effort>,           // low | medium | high | xhigh | max — role policy, then pin,
                                      // then tier default; each built-in adapter states it explicitly
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

v0.2 replaces the terminal edge with `Reviewing ─► AwaitingMerge(candidate) ─► Merged | AwaitingRepair(fix task)`. There is no pre-merge `Done`: **dependency readiness is `Merged`** — a dependent's worktree must branch from an integration head that already contains its dependencies' code. `Ready`, the attempt phases, and `MergeVerifying` are derived views; the durable fold stores candidates, repair lineage, and the one prepared merge transaction (decided 2026-08-12; the shipped protocol is in §15).
