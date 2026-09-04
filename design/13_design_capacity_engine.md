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

**Discovery — `upstroke connect`.** Pools are connected, not configured: `connect` scans PATH for official CLIs, checks auth state, detects each plan's quota shape, enumerates available models, and writes the user-level pools file. Tier classification comes from a **capability catalog** shipped with the binary (static data — the no-HTTP invariant holds), with a pragmatic prior for unknowns: providers price their own models, so per-model multipliers and per-token rates rank capability. A model absent from the catalog is never auto-selected — pin it or update. Decision logs later calibrate the catalog with measured pass rates per tier and task kind.

`connect` enumerates **credential profiles, not just installed binaries**: one vendor can back several pools — two Claude Max accounts, say — and the binder selects between them per attempt through the provider's own profile mechanism, an environment variable on the subprocess rather than a token the engine ever handles (invariants 2 and 5). Whether the CLI honours profile selection is a `probe()` axis like any other, verified at pre-flight instead of discovered mid-spend. Several same-kind pools change three things: estimates must **attribute** usage per profile rather than aggregate it (local-log parsing reads per-account state, so summing reports one healthy pool where there is one exhausted and one fresh); `reserve` becomes asymmetric, since a plan bought for unattended work has no interactive use to protect; and independent reset windows turn a rate limit from *wait* into *rebind and continue* (§10.5) — the single biggest practical gain for an overnight run. Affinity still governs the order: prompt caches are per-account, so the binder drains one pool toward its reserve and then switches, rather than round-robining and paying cache-cold on every task.

**Strategies** (`routing.strategy.mode`):

- `conserve` — classic cost minimization: route down aggressively, escalate only on failure, defer frontier-hungry tasks toward window resets when a pool is projected to run dry.
- `value-max` — subscription yield management: prepaid capacity that would expire unused has zero marginal cost, so surplus near a reset biases default tiers **up** (spend-down mode) — Opus for implementation, frontier review everywhere — subject to `min`/`max` bounds and the reserve floor. *No shipped tool does this (verified Aug 2026); it is the headline.*
- `deadline` — wall-clock first: maximize parallel throughput within capacity, spilling to API dollars when justified by a configured $/hour ceiling.

The ledger accounts every attempt in both currencies: API-equivalent dollars (honestly labeled — subscription spend is notional) and pool units drained. Where a worker's route reports no spend at all — Copilot's does not (§16) — the total it contributes to is marked `?` rather than presented as complete, since a cross-vendor review makes that the normal case rather than a corner of it. Budgets exist per run ($), per task ($), and per pool (fraction).

**Sequencing:** v0.1 ships the capacity engine **read-only** — the dry-run preview and `upstroke capacity` show each pool's estimated remaining capacity, resets, and what each strategy *would* do. v0.2 wires it into live routing. This de-risks estimator fragility before any routing depends on it, and the preview alone is the demo that sells the product.
