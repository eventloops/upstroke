# Review: Step 10, Pass B — the estimate and what spends against it

- **Date:** 2026-08-09
- **Scope:** commit `35558b2`, files `src/capacity.rs`, `src/validate.rs`,
  `src/engine.rs`, `src/events.rs`, `src/ir.rs`, `src/main.rs` — the estimator
  with both its readers, and the budget path from flag to exit code
- **Level:** max — 10 finder angles, then the four load-bearing claims driven by
  throwaway tests against the real engine
- **Result:** 15 findings — 3 normal, 5 low, 7 nits — **all fixed** in the
  follow-up commit.
- **Question the pass was asked:** *is the estimate conservative, and does the
  budget stop the run exactly once and survive replay?*

The through-line: **the one-fold property held; the estimator's honesty did
not.** `budget_exceeded` folds once, `capacity_snapshot` folds to nothing, and
both replay identically — the property the whole design rests on came through
untouched. What came apart was the module whose docs lead with "never
optimistic": a signal that could never be withdrawn, and a monthly allowance
divided by a single run's spend. Both produced confident numbers in the
direction the module exists to make impossible.

## Findings

| # | Finding | Severity | Verdict | Fix |
|---|---------|----------|---------|-----|
| 1 | **A `pool_exhausted` signal never expired.** `observe` only inserted, nothing emitted a recovery, and `Confidence::Signal` outranks every other source *by design* — so the one thing that could correct the record was the one thing forbidden from doing so | normal | CONFIRMED (test: a rate-limited-then-recovered run reports `claude-max: exhausted [signal] — this run drew $0.0700 over 3 attempt(s)`, asserting empty and served in one line) | `retire_signals`: a *completed* attempt proves the pool served, whatever the verdict on its code. Rate-limited and interrupted attempts prove the opposite and nothing, respectively. Events are ordered, so a later outage re-marks it |
| 2 | **A month's allowance divided by one run's spend.** `self_spend` is folded from `latest_run` alone but `estimate_one` divided it by `monthly_allowance` — so a run spending $5 of a $100 allowance reported 66% left when the month's true draw might be $95 | normal | CONFIRMED | `Remaining::AtMost`, rendered `≤66%`. Every unseen draw can only *reduce* what is left, so the figure is sound as a ceiling and false as a measurement — and now says which it is, with a note naming what it cannot see |
| 3 | **`--budget` bypassed the validation `[budgets]` enforces.** `0` and `-5` stopped the run before it spent anything; `nan` disabled the ceiling silently. All three are hard errors as config keys | normal | CONFIRMED (test) | `config::check_budget`, shared by the parser and the flag, called at **pre-flight** — so a bad flag refuses before a branch or run directory exists |
| 4 | **A spend approval was fed to the agent as an instruction.** `answer_question` pushes every answer into `Progress.feedback`, and `feedback_section` frames those as "an instruction from a person… it takes precedence over your earlier assumptions" — so the frontier implementer's prompt carried a fenced `approve: run the escalated attempt` | low | CONFIRMED (test: the string appears verbatim in the escalated prompt) | The push is gated on the kind. An `Unblock` answer is guidance; an `ApproveSpend` answer is a yes/no about money whose meaning the un-park already consumed |
| 5 | The budget stop **printed twice** — once from `render`'s outcome arm and once from `render_ledger` — back to back in `upstroke status`, formatted to different precision, with two copies of the same resume command | low | CONFIRMED | The ledger annotates (`stopped by [budgets] run_usd = $0.0500 before \`t2\``); `render` owns the outcome and the advice |
| 6 | An **unreadable run log reported as "no run in this repository yet"** — a false statement about the repo that also swallowed `read_all`'s refusal, which is exactly the loud error the event-log design exists to produce | low | CONFIRMED | Surfaced as a warning naming the run; `capacity::report` already did this correctly |
| 7 | One outage wrote **N identical `pool_exhausted` events**, one per deferral, inflating any later count of outages by the deferral factor | low | PLAUSIBLE | Only the transition is recorded. `Run.exhausted_pools` is process-local like `unanswerable`, seeded from the log on resume, and retired by the same rule `observe` uses so the writer and the reader agree about recovery |
| 8 | **`ir::PoolDrain` was fully dead** — written `None` by both adapters, read by nothing, its doc still calling itself a stub "until the capacity engine", which had by then landed and been routed around | low | CONFIRMED | Deleted, with `Outcome`'s doc pointing at `AttemptRecord.pool` as the mechanism that replaced it. An adapter cannot know which subscription the engine bound it to, so the field was in the wrong place to begin with |
| 9 | **`validate` parsed the entire latest event log** on every invocation — the fast zero-spend iteration loop §18 puts on day one, doing work proportional to run history for a decorative block | nit | CONFIRMED | Skipped entirely when no pools are connected, which is the common case and the only one where the block has nothing to say |
| 10 | The capacity snapshot was **skipped when no pools were configured**, so "nothing was connected" was indistinguishable from a pre-step-10 log or a binary that never took one | nit | PLAUSIBLE | Emitted with an empty list. The absence of a fact and the fact of an absence are different records |
| 11 | `render`'s `BudgetExceeded` arm fell back to `("run_usd", 0.0, 0.0)` — unreachable today, but written as a plausible value, so the day it drifts it prints a specific, checkable, false claim about the operator's own config | nit | PLAUSIBLE | Says it did not record one, rather than naming a ceiling |
| 12 | The spend threshold was rendered `{:.2}` beside a spend rendered `{:.4}`, so `0.005` was quoted back as `$0.01` — and a spend of `$0.0080` appeared to be *below* a threshold it had crossed | nit | PLAUSIBLE | Same precision on both |
| 13 | **`capacity` and `connect` depended upward on `engine::AdapterSource`** — a two-line adapter-lookup trait dragging the execution engine into a module documented as "a pure function over plain values" and one that executes nothing at all | nit | CONFIRMED | Moved to `agent`, beside the adapters and the registry it resolves. `engine` re-exports, so `engine::AdapterSource` still resolves |
| 14 | `reported_spend` rescans every task's every record per attempt, and `review_cost_usd` allocates a `Vec` per record to sum it | nit | PLAUSIBLE | Left as-is — see below |
| 15 | **`sweep_answers` was the one `drain` branch not guarded on the budget stop.** A declined answer routes through `fail_task`, which sets `halted_at`, and halted outranks budget — so a decline file on disk relabelled a budget stop as a task failure, and CI gating on exit 3 saw exit 1 | low | PLAUSIBLE | Guarded like the other two, with the reason recorded: the other branches waste work, this one changes the answer |

## What was not applied

**#14 (quadratic spend rescans).** Real, and left alone deliberately. The scan
is O(tasks × attempts) per attempt against work measured in agent turns —
minutes each — so the waste is unmeasurable beside what it guards. Fixing it
means either a running total updated as `attempt_finished` folds (a second path
to a number the fold already owns, which is the shape §15 exists to prevent) or
threading a cache through `Run`. Neither is worth it until a plan is large
enough for the scan to show up in a profile; the finding stands as the note for
whoever sees it there first.

## On the tests

Eight tests added, two rewritten.

**Findings #1, #3 and #4 were verified by throwaway tests written before the fix
and watched to fail**, then kept as permanent ones. #1's is the sharpest: the
failure message is the entire argument in one line — a pool reported empty at
maximum confidence on the same line that reports the three attempts it served.

`a_rate_limit_marks_its_pool_exhausted_and_a_recovery_retires_the_signal`
replaced the old test that asserted the stale behaviour. It now folds the log
twice: **stopping at the signal**, where `Exhausted [signal]` is correct because
that is what was true at that moment, and **over the whole log**, where it must
not be. Pinning both directions is what stops a future fix to one breaking the
other.

`the_budget_flag_is_validated_like_the_config_key` also asserts the repository
is still on `main` afterwards — the refusal has to land at pre-flight, not after
a branch and a run directory exist.

## Checked and clean

- **The one fold survived every change.**
  `live_state_equals_replayed_state_across_every_ladder_path` carries the
  budget-stop and approve-spend scenarios and still passes; `budget_stop` folds
  once and `capacity_snapshot` folds to nothing, which is the pairing the
  property was extended to cover.
- Outcome precedence (`halted` > `budget` > `parked`) is now genuinely a
  precedence rather than an accident of which branch ran — #15 was the one path
  that could invert it.
- `SCHEMA_VERSION` staying 1 re-checked against the added fields: every one
  carries `#[serde(default)]`, and an old binary meeting a new event kind gets
  serde's unknown-variant refusal, never a misread.
- `retire_signals` and the engine's `exhausted_pools` were deliberately written
  to the *same* rule, so the log a run writes and the fold a later reader
  performs cannot disagree about when a pool came back.
