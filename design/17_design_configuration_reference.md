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

**What `connect` writes.** The file above is an operator's. The one `upstroke connect` writes is a
persisted format with two readers — `config`, for every `run`, `validate` and `capacity` that loads
pools, and `connect` itself on its next run — and it has this shape:

- One `[pools.<name>]` table per agent whose CLI probed and answered discovery, in registry order,
  **named after the agent** (`claude-code`, `copilot`, `codex`) rather than after a plan: discovery
  does not establish a tier, and the pool is the operator's to rename. An agent that is not usable
  on the machine gets a comment saying so and no table.
- The keys: `kind` and `agent` always; `window = "5h"` and `weekly = true` for a
  `subscription-window` pool; `sources = ["signals", "self"]`, the two sources v0.1 reads, never
  `local-logs` or `provider-endpoint`; `safety_margin = 0.15` and `reserve = 0.20`, §13's defaults.
  Then the operator's own keys, written only when the existing file held them under the same pool
  name: `profile`, `monthly_allowance` as a number (`"auto"` is the reader's default and is not
  written) and `endpoint`. `connect` invents none of the three. An allowance must be a positive,
  finite number or the string `"auto"`. An invalid allowance produces a warning and leaves the
  discovered `auto` default; valid `profile` and `endpoint` values are still carried.
- Comments carry what discovery found: a header naming the version, the write time (RFC 3339, UTC)
  and where the model roster came from; per agent, the auth state — one of *signed in*, *NOT signed
  in* and *could not be determined*, never the second for the third — each discovery note on its
  own line, and a sentence saying the `kind` is a default where the CLI could not say.
- Strings and table keys are TOML-encoded to preserve their values. Finite positive allowances
  are written as numbers the configuration reader accepts. Every comment payload is written one
  line per line with forbidden control characters replaced, so a discovery note cannot become a
  setting or make the file malformed.

**When it is rewritten.** Two comparisons against the file already there, because two questions
are asked. *May* it be replaced: only if both complete TOML documents parse to equal tables, or
`--force` is given; otherwise `connect` refuses, prints the file it would have written, and exits
non-zero. Quoted values retain their whitespace and escapes; equivalent string spellings,
formatting and table order do not cause a conflict. Integer and float values remain distinct.
For example, an older writer's integer allowance `10000000000000000` differs from this writer's
float `1e16` and requires `--force` once. No universal cross-version `Unchanged` behavior is
promised. Malformed TOML, including malformed comments, requires `--force`. A read error other
than a missing file is reported even with `--force`.

*Should* it be rewritten: if anything but the first generated write-time header differs,
comments included, so a login between two runs updates the auth comment while an
unchanged machine leaves the file, and the date on it, alone. `--force` carries `profile`,
`monthly_allowance` and `endpoint` over from the pools of the same name in the existing file and
replaces everything else; a file that does not parse carries nothing. Invalid allowances are
omitted with the warning described above. Repeated connects using the same writer leave an
unchanged machine's file alone once its settings and nonvolatile content match.

**Not decided here:** whether the keys above and the default pool name are frozen across versions.
Today the reader warns on an unknown key and refuses an unknown value, no key or name has been
renamed, and nothing migrates one; a renamed default pool name would stop the carrying by name.
That rule is owed and is the owner's to state.

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

**What the repo-level file refuses.** A mistake that would silently delete a control is an error at load, before a lock, a workspace or a run directory exists; a mistake that only degrades what the run can say about itself is a warning that names the key. So: an unknown key in `[runner]`, `[engine]`, `[budgets]`, `[interaction]`, its `ask_before` table, or a `[[gates]]` entry is an error naming the accepted keys — `[engine]` included, because `on_task_failure` and `shell` sit in that table beside the ceilings, and `on_task_failur = "continue"` left to a warning is a halted run the operator asked to continue; an unrecognised `kind`, `on_task_failure`, `mode`, `second_opinion` or effort value is an error, while an unrecognised `shell` warns and takes the platform default; a zero ceiling, timeout or `attempts_per` is an error, as is a gate `timeout_secs` the run record cannot hold (more than 18,446,744,073,709,551 seconds, since `run_started` records it in milliseconds as a 64-bit count), a budget ceiling that is not a positive finite number of dollars, or an `ask_before` threshold that is negative or not finite; and two `[[gates]]` entries may not share a `name`, compared without regard to ASCII case because a case-insensitive filesystem keeps one log file for both, since the name is what a gate's log file and its failure report carry. A resume reads `[[gates]]` by what its log records (§15): a run whose log records its gates takes them from the record, so today's section is compared with them and never refused over — every shape above is a warning naming the recorded gates as what runs — while a run whose log predates the gate record settles its gates from today's file on that resume and reads the section exactly as a fresh run does. Nothing else in the file reads differently on a resume except the `[engine]` ceilings, which warn there rather than refuse.
