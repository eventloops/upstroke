# Review: Step 6 — reviewer with structured verdicts

- **Date:** 2026-08-09
- **Scope:** commit `6eccd29` (src/review.rs new, src/engine.rs, src/ir.rs, src/lib.rs)
- **Level:** max — 10 finder angles → verification (parser behaviour and CommonMark fence handling checked empirically in a scratch harness) → gap sweep
- **Result:** 15 findings; **all fixed** in the follow-up commit. Suite 135 → 147 tests.

The through-line: **step 6 as committed could not review anything in a real run**, and where parsing did run it failed *open* — discarding a reviewer's FAIL and promoting an earlier echoed template into a PASS. The module's stated safety posture was inverted.

| # | Finding | Verdict | Fix |
|---|---------|---------|-----|
| 1 | Verdict read from `Outcome.detail`, which the real adapter never sets on success — every real review unparseable, no task could ever commit | CONFIRMED | `parse_output` now carries the final message on the success path; test asserts it |
| 2 | `.rev().find_map` fell back to an earlier block when the final verdict was malformed → rejection became approval | CONFIRMED (empirical, 5 variants) | Last object only; a botched answer earns the re-ask instead |
| 3 | Fence parity inverted by quoted diff fences, dropping the real verdict | CONFIRMED (empirical) | Fence scanning replaced with string-aware brace matching — immune to quoted fences |
| 4 | `bare_json_object` accepted an echoed template as an authoritative pass | CONFIRMED (empirical) | Subsumed by the brace scanner; prompt schema made non-parseable (`<true or false>`) |
| 5 | Implementer could write into the reviewer's prompt via run-dir artifacts — invisible in the diff, surviving rollback | CONFIRMED | Artifacts fenced, labelled untrusted, and a task's own outputs excluded from its own review |
| 6 | Diff embedded in a fixed ``` fence with no escaping — content could close it and address the reviewer | CONFIRMED | Fence-length escalation + explicit DATA UNDER REVIEW framing + line-aligned truncation |
| 7 | Re-ask dropped the session and sent no diff — a context-free verdict was trusted | CONFIRMED | Without a resumable session the full prompt is re-sent |
| 8 | `run_review` ignored `OutcomeStatus` — a rate-limited or hung reviewer read as "your code is wrong" | CONFIRMED | `ReviewResult::Unavailable` maps to RateLimited / Timeout / ReviewUnavailable kinds |
| 9 | `pass: true` with non-empty `required_changes` committed and discarded the blockers | CONFIRMED | Fails closed, naming the contradiction; prompt example no longer teaches the shape |
| 10 | Unresolvable reviewer silently skipped review for the whole run; reviewer agent never probed | PLAUSIBLE (latent, live at step 9) | Hard error; review binding added to the pre-flight probe set |
| 11 | `clamp_diff` cut mid-hunk with no file header; doc claimed "most recent" but git orders by path | CONFIRMED (empirical) | Cuts on a `diff --git` boundary, else a line boundary; doc and notice corrected |
| 12 | Reviewer spend folded into the implementer's `cost_usd` beside the implementer's model name | CONFIRMED | Separate `review_model` / `review_cost_usd`; render and totals updated |
| 13 | Reviewer inherited the full attempt timeout, per invocation (3× the documented budget) | CONFIRMED | Own budget: a quarter of the attempt timeout, floor 60s |
| 14 | Review unconditional, always frontier, with no off switch; `second_opinion` parsed but unread | CONFIRMED | `[routing] review = { enabled = false }` opt-out; self-review at frontier documented as a step-9 item |
| 15 | Failure reason truncated with `tail`, dropping the reviewer's primary (first) reason | CONFIRMED | New `util::head`; `required_changes` now bullets every item including the first |

## Still open (deliberately, with reasons)

- **Cross-vendor second opinion (§11.3)** needs a second adapter — step 9. The singular `Reviewer` shape will need to become a list then; noted rather than pre-built.
- **`needs-human` verdict channel (§12)** belongs with questions in step 7.
- **Self-review at the frontier rung**: a frontier-implemented task is reviewed by the same model, since both binders resolve identically. Only fixable with a second vendor (step 9).
- **Reviewer can read the implementer's transcript** from the run dir (it has Read/Glob/Grep on the workspace). Withholding is prompt-level, not enforced. Fixing properly means moving run artifacts outside the workspace or path-scoping reads — a step-8 concern when the event log lands.
