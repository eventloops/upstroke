# Review: Step 9, Pass B — the decision path

- **Date:** 2026-08-09
- **Scope:** commit `2d40706`, files `src/catalog.rs`, `src/config.rs`,
  `src/route.rs`, `src/review.rs`, `src/validate.rs`, `src/engine.rs`,
  `src/events.rs` — every step-9 producer sitting with its consumer, which is
  the split step 8's post-mortem asked for
- **Level:** max — read, then the load-bearing claim (resume honours the
  recorded reviewers) driven by test rather than by argument
- **Result:** 7 findings — 1 normal, 3 low, 3 nits — **all fixed** in the
  follow-up commit.

The through-line: **the resolution logic is right and its edges are where the
defects are.** `passes_for`'s two rebind rules, the family comparison, the
short-circuit and the one-fold property all held under scrutiny and under
mutation. What did not hold was everything about *absence* — an absent record
read as a decision, a reviewer that never ran recorded as one that objected, a
cost that omitted a reviewer presented as a total, and a warning that named the
affected tasks but could never fire in a shipped binary.

## Findings

| # | Finding | Severity | Verdict | Fix |
|---|---------|----------|---------|-----|
| 1 | **Resuming a run started before step 9 silently switched review off.** `RunStarted.reviews` is `#[serde(default)]`, so an older log parses — into an *empty* plan, which `passes_for` and every other reader treat as `review = { enabled = false }`. The rest of the run then commits unreviewed. This is step-6 finding #10 from the other direction: verification vanishing without a word | normal | CONFIRMED (test: the resumed attempt committed while a reviewer scripted to reject everything was never consulted) | The field is `Option<ReviewPlan>`. `None` means the log says nothing about reviewers, which is not the same as saying there were none — so it re-derives from config and warns that it did |
| 2 | `ReviewRecord.passed` was `failure.is_none()`, so a **rate-limited or timed-out reviewer was recorded as having rejected the change** — a verdict in the ledger against a model that never read the diff. The ladder already distinguishes these (step-6 finding #8); the record did not | low | CONFIRMED | `outcome: ReviewPassOutcome { Passed, Failed, Unavailable }`. The `Unavailable` case is read from the result *before* it is consumed |
| 3 | **The self-review warning that names tasks could never fire in a shipped binary.** It was gated on `alternative.is_none()`, but a real build always ships the Copilot adapter, so resolution always finds a cross-family model. The only way the rebind actually goes missing is a probe failure at pre-flight — where the warning did not run | low | CONFIRMED | Extracted to `ReviewPlan::self_review_warning`, called from resolution *and* from pre-flight after a probe failure drops the alternative. The existing downgrade test now asserts the task is named |
| 4 | Resume **required** the opportunistic cross-family reviewer, on the grounds that a run should keep one verification standard — so a resume would refuse over a judge that may never have judged anything, while an identical fresh run merely warns. Same machine state, opposite outcome | low | CONFIRMED | Resume draws the line where a fresh run does: optional, with a warning. The per-attempt record names who judged each attempt, so the ledger stays honest either way, and `preflight_with_reviews` loses its `resumed` special-casing entirely |
| 5 | `review_cost_usd` summed only what was *reported*, so a two-pass review where one route bills nothing back rendered as `$0.0500` — one reviewer's spend presented as the total. `render_ledger`'s own contract says a ledger that cannot tell free from unreported is worse than none, and cross-vendor review makes this the normal case, not a corner | nit | CONFIRMED | `review_cost_incomplete` on the record and the report; `?` suffix in both renderers, with a legend line |
| 6 | `TaskReport.review_models` took the **last attempt's** models while `review_cost_usd` summed **every** attempt — a list scoped to one attempt beside a total scoped to all of them, reading as though one explained the other | nit | CONFIRMED | Deduped union across attempts, in first-seen order, with a test that escalates a task and asserts both judges appear |
| 7 | `plan_for` matched `ov.second_opinion == Some(SecondOpinion::DifferentVendor)`. §11.5 adds a security lens to that same key, and a new variant would have been silently ignored here rather than failing to compile where it needs handling | nit | CONFIRMED | `.is_some()` |

## On the tests

Six tests were added or strengthened, one per finding with observable
behaviour (#7 is a predicate change).

**#1's test is the one that matters, and it was written before the fix and
watched to fail.** It runs a task to a park, rewrites `run_started` as a
pre-step-9 process would have written it — parsing each event line and removing
the `reviews` key rather than string-editing — answers the question, and
resumes with a reviewer scripted to reject everything. Under the defect the task
**committed**, with `reviews: []` and `review_models: []` in the record: the
predicted mechanism exactly, not merely a red assertion.

Two properties were re-checked by mutation after the fixes landed, since both
had been mutation-verified before and the refactor moved code under them:

- Reverting the resume record to re-derivation still fails
  `a_resume_keeps_the_reviewers_the_run_started_with` with
  `left: ["gpt-5"], right: ["claude-opus-5"]`.
- Reverting the rebind suppression still fails four tests, including
  `["gpt-5", "gpt-5"]` — both passes collapsed onto one model with the Anthropic
  review gone.

## Checked and clean

- **The one fold held throughout.** `live_state_equals_replayed_state_across_
  every_ladder_path` covers three cross-vendor scenarios, and its guard —
  asserting the second vendor actually judged something — means a scenario
  cannot pass by quietly resolving to a single pass.
- `passes_for`'s two rules are correct and mutually consistent: exact
  `(agent, model)` identity for the rebind, suppression when a second opinion is
  configured, and no path on which both passes resolve to one model.
- Family comparison is on `catalog::Family`, never on agent id — the trap that
  would have paired `claude-opus-5` with itself through a different harness.
- Passes short-circuit, and the second reviewer is provably never spawned when
  the first fails.
- The review budget splits across passes rather than multiplying, so step-6
  finding #13's quarter-of-an-attempt cap survives a second opinion.
- Second-opinion resolution keys off the *primary reviewer's* family, so a pin
  that moves the primary moves its cross-family partner with it.
- `route.rs`'s optional `start_at` leaves the chain untouched when an override
  carries only a `second_opinion`, and the `[paths: …]` note is unaffected.

## Still open (deliberately, with reasons)

- **The preview cannot probe.** `validate` and `--dry-run` execute nothing
  (§18), so they resolve reviewers against the adapters this *build* ships, not
  the binaries on PATH. A machine without the Copilot CLI therefore sees a clean
  preview naming `copilot/gpt-5` and a run that then warns or refuses. The
  preview says "if installed" rather than pretending otherwise; closing it
  properly means probing in `validate`, which is a different promise than §18
  makes.
- **`Caps` is almost entirely inert** — five of seven fields are written by both
  adapters and read by nobody; only `session_resume` drives behaviour. Fine
  while the capacity engine is unbuilt, but step 10 is its first real consumer
  and should not assume more is live than is.
- **`TaskRun.gate_cmds` and `materialize_permissions`' `gate_cmds` parameter**
  remain two channels for one truth. They agree at both call sites, and nothing
  enforces it. Merging them means `materialize_permissions` taking a `TaskRun`,
  which it cannot — the engine calls it *before* building one, to obtain the
  settings path that goes into it.
