## 17. Configuration reference

Config splits along its natural seam (v2.1): **pools are user-level** — subscriptions travel with the person, discovered and written by `upstroke connect` — while **routing and gates are repo-level overrides** on derived defaults. A fresh repo runs with zero config.

`~/.upstroke/pools.toml` (written by `connect`, hand-editable):

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

Repo-level `upstroke.toml` — overrides only; everything below has a derived default:

```toml
[engine]
on_task_failure = "halt"            # halt | continue
max_parallel    = 1                 # >1 requires v0.2
max_merge_repairs = 2               # autonomous generations per original task; then HUMAN
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
review    = { tier = "frontier", timeout_secs = 5400 }
                                      # independent budget per pass; includes one format re-ask

[routing.effort]                    # optional role-wide standard; outranks pins and tier defaults
implementation = "xhigh"
review = "max"

[[routing.overrides]]                 # at least one of start_at / second_opinion
paths = ["src/auth/**", "migrations/**"]
start_at = "frontier"                 # optional — omit to add a reviewer without raising the floor
second_opinion = "different-vendor"   # binder must add a reviewer from another model family

[[pins]]                            # optional determinism — otherwise the binder chooses
tier  = "frontier"
agent = "claude-code"
model = "claude-opus-4-8"
effort = "max"                      # optional; default is the tier's when no role policy applies.
                                    # Validated at load —
                                    # the provider rejects an unknown level with a 400 mid-turn,
                                    # so a typo would otherwise cost a whole attempt.

[[gates]]
name = "check"
cmd  = "cargo check --all-targets"
timeout_secs = 600

[[gates]]
name = "test"
cmd  = "cargo test"
timeout_secs = 1200
```
