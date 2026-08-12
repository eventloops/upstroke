# Decision record — reasoning effort is a routing axis, and Codex was running at `low`

**Date:** 2026-08-11
**Status:** Decided, implemented (`5cbd162`), and verified on a live run (`84ef5a4`)
**Provenance:** originally Addenda D and E of `2026-08-11-design-council.md`; split into its own record on 2026-08-11 when one-decision-per-file became the folder convention (`README.md`). Content unchanged apart from one correction, marked below.
**Related:** [design council](2026-08-11-design-council.md) (the review that prompted the probe), [self-hosting v0.2](2026-08-11-self-hosting-v02.md).

---

## The defect

`~/.codex/models_cache.json` gives `gpt-5.6-sol` a `default_reasoning_level` of **`low`**, and `codex.rs::build_args` passed only `--model` and `--sandbox` — effort was never set, so it fell to that vendor default. **Every Codex review this project had run went out at the lowest reasoning setting**, including run `01KZRN48A4ZK3AEDST3RJ8HMA4`, the first §11.3 cross-family review (§21). The adapter already argued the general case against itself: it passes `--model` on resumed sessions precisely so "a future change to the CLI's default must not silently move a resumed retry to another model" — the identical argument applies to effort.

Probed WSL-side against codex-cli 0.147.0 on a ChatGPT **Pro** plan (upgraded the same day), read-only sandbox, trivial prompts.

## Measured

- Effort is a first-class per-model enum. `gpt-5.6-sol`: `low, medium, high, xhigh, max, ultra` (`ultra` = "maximum reasoning with automatic task delegation"); `gpt-5.6-luna` tops out at `max`; `gpt-5.5`/`gpt-5.4` at `xhigh`. Defaults differ per model (`sol` = low, most others = medium).
- The config key is **`model_reasoning_effort`**, set via `-c key=value`. Siblings in the same binary: `plan_mode_reasoning_effort`, `model_verbosity`, `service_tier`.
- `xhigh`, `max` and `ultra` were all **accepted and completed** (exit 0, correct replies) under Pro. Caveat, stated because it matters: the prompts were trivial and returned `reasoning_output_tokens: 0`, so this measures *acceptance of the flag*, not that effort changed behaviour or that a Plus plan would refuse it. No Plus/Pro comparison was run.
- **There is no pro-tier model.** Pro is a plan/quota tier; the frontier slug stays `gpt-5.6-sol`. The roster is server-driven and moves under you — it refreshed mid-probe and gained a ninth model (`gpt-5.3-codex-spark`).
- **The plan tier is not discoverable locally.** `auth.json` carries `auth_mode`, tokens, `account_id`, `last_refresh` and no plan field; `codex login status` says only "Logged in using ChatGPT". Plus vs Pro therefore cannot be detected — for this pool §13's "pools are connected, not configured" hits a hard limit, and the plan shape must be operator-declared.
- **No `rate_limits` anywhere in the `codex exec` JSON stream** — usage tokens only. Codex capacity estimation has no local quota signal at all, which is stronger than the adapter's recorded "usage without pricing".
- A local model roster *does* exist (`models_cache.json`), so the adapter's probe note that "no model listing is offered" is true of the CLI surface but not of the install. It is a real discovery source for `tactus connect`, with the staleness caveat that it is a server-fetched cache.
- `-c` is accepted on `codex exec resume` (a bogus session id fails at session lookup, not argument parsing) — unlike `-s`, which that shape rejects.
- The provider validates the effort value server-side and rejects an unknown one with a **400 after the turn has started**, so a typo costs an attempt rather than failing fast. Its accepted set, read off that error: `none, minimal, low, medium, high, xhigh, max`.
- Incidental confirmation: a `401 token_expired` on the websocket transport, then a successful HTTPS fallback and refresh — the rotating-refresh-token behaviour §21's runner commitment already assumed when it specified persistent credential volumes.

## Verdict (implemented in `5cbd162`)

`Effort` is a four-level abstract ladder (`low, medium, high, max`) on `WorkerProfile`, defaulting per tier (`small→low`, `mid→medium`, `frontier→high`) with a `[[pins]] effort` override validated at config load; the codex adapter states `model_reasoning_effort` on both the fresh and resumed shapes.

1. **Effort becomes explicit in the adapter** — by the same reasoning that already makes `--model` unconditional.
2. **Effort lives in `WorkerProfile`**, not `extra_args`, so the binder can reason about it and the ledger and decision record it. This is also telemetry salvage item #2 ("agent identity incl. reasoning config") arriving with a concrete shape.
3. **The catalog's unit is model × effort**, not model: effort changes both capability and price of the same slug, which is exactly what §13's catalog ranks. *(Not yet implemented — the catalog still keys on model.)*
4. **Effort-qualified rungs** (`frontier` then `frontier:max`) are the warmest possible escalation — same model, same harness, same cache, per §10's affinity gradient — and the cheapest answer to §23.2's "fewer attempts beats cheaper attempts". Decide with the per-rung `attempts_per` config change; they share a syntax. *(Deferred to that work.)*
5. **Codex pool shape is operator-declared** in `pools.toml`, with `connect` recording that it could not verify the tier rather than guessing. *(Not yet implemented.)*

Two decisions the build forced, recorded because neither was obvious from the probe:

- **The reviewer default is `high`, not `max`.** Reviewers bind at the review tier, which defaults to frontier, so the tier default settles it. `max` stays reachable only through a pin — a deliberate purchase, not the price of routing something to the top rung. §23.2's "review is charged per attempt" is why this is a real cost lever and not a free upgrade.
- **Effort is not identity.** `ReviewPlan::passes_for` decides §11.3's self-review rebind by comparing a `PassBinding` with the implementer's. Putting effort on that struct would have made the comparison always false and *silently retired the check that stops a model reviewing its own work* — a verification layer deleted by a field addition, with every test still passing. Effort therefore travels as a parameter to the profile, not as part of the binding. The near-miss is the point: this is the failure mode §11.3 already guards against in config (an unrecognised `second_opinion` value is a hard error, "because a typo must not silently delete a verification layer"), reappearing as a type change.

## Rejected options

- **Exposing codex's `xhigh`** — an intermediate no other adapter can honour, in a ladder that is deliberately vendor-neutral.
- **Exposing `ultra`** — "maximum reasoning with automatic task delegation" is a change in what the agent *does*, and nothing in this design has audited an agent spawning subagents inside a tactus attempt.
- **Discovering the effort value at spend time** rather than validating at config load — the provider's 400 arrives mid-turn, so a typo would burn an attempt and report as an agent failure.

## Addendum (2026-08-12) — role policy and the fifth shared level

The rejection of `xhigh` depended on it being Codex-only. That premise expired: the locally probed CLIs now all advertise the same useful five-level set. Codex 0.147.0 accepts `model_reasoning_effort`; Claude Code 2.1.226 on Windows and 2.1.227 under the self-hosting WSL environment advertise `--effort <low|medium|high|xhigh|max>`; and Copilot CLI 1.0.78 advertises `--effort/--reasoning-effort` with those five levels (plus its lower `none` and `minimal`). Tactus now maps all five explicitly in every built-in adapter and makes `--effort` a probed, required flag for Claude Code and Copilot, so an older incompatible CLI refuses at pre-flight rather than silently ignoring the policy.

`[routing.effort]` sets independent `implementation` and `review` values. A role value outranks a pin's effort and the tier default: that precedence is what makes “always xhigh for implementation, always max for review” true across every rung and review pass. With no role value, the original pin-then-tier behavior is unchanged. Values are validated while loading config, and each effective value remains recorded on the attempt or review event. `ultra` remains excluded because its automatic delegation changes the orchestration boundary.

## Verification — the cross-family review, re-established at a stated effort

Run **`01KZS7R0V1ZD6MC290MG350QXF`**, WSL-side against a seeded scratch repo: one `implement` task bound at mid to `claude-code/claude-sonnet-5` (Anthropic), reviewed by `codex/gpt-5.6-sol` (OpenAI) pinned at frontier — so the cross-family pass is the *primary* review here, not an added second opinion. Committed on the first attempt.

What it establishes, in descending order of how hard it was to fake:

- **The effort reached the provider.** Codex's own session rollout for the review thread records `"effort":"high"`. This is the CLI's record of what it received, not tactus's record of what it meant to send — the two had disagreed silently for the whole life of the adapter, which is the defect this run closes.
- **The reviewer actually reasoned.** 511 of 757 output tokens were `reasoning_output_tokens`. The pre-fix probe on a trivial prompt returned 0.
- **The families really differ.** `run_started` records `reviews.primary = codex/gpt-5.6-sol` with the alternative `claude-code/claude-opus-4-8` held in reserve, and the implementer was Anthropic. §11.3 satisfied on the substance, not the label.
- **The verdict was a reading, not a rubber stamp.** It cited `src/clamp.rs:2` and `:3` by line, checked the "no code path can panic" criterion against the actual conversion, and reasoned explicitly that `unwrap_or` is not the prohibited panicking `unwrap` — a distinction a rubber stamp does not make.
- **The ledger stayed honest:** `$0.1391?` — the Claude half priced, the Codex half unpriced and marked as a floor rather than presented as complete (§13).

**Prediction (stated before the run, per the standing protocol): mid rung, first attempt, $0.10–0.40 reported. Actual: mid, first attempt, $0.1391. Hit.**

> **Correction (made during the split).** The original Addendum E called this "the third consecutive hit". That was wrong and is exactly the kind of flattery a prediction log exists to prevent: the standing tally is **four prior misses, all overestimates of roughly 2×, and this is the first hit** — earned by estimating attempt-count first rather than scaling off a previous run's total.

**What it does not establish:** that `high` produces *better* judgement than `low`. That would need the same diff judged both ways, and §23.2's own finding — two identical configurations produced two different failure modes — says a single paired run would not settle it either. The claim here is narrower and is the one that was actually broken: tactus now decides the effort, states it, and can prove which one was used.

## Follow-on observation (2026-08-11) — the reviewer contradicted itself on the same construct

Later the same day, a different run gave the same reviewer — `codex/gpt-5.6-sol`, same `high` effort, same task text — the identical idiom to judge, and it went the other way.

On run `01KZS7R0V1ZD6MC290MG350QXF` it **passed** `u8::try_from(value).unwrap_or(100)`, reasoning that "`unwrap_or` is not the prohibited panicking `unwrap`; the conversion is explicit and total." On run `01KZSCRGGJYEF5TBG6YND8YD2X` it **rejected** the same construct: "still an unwrap-family shortcut instead of explicit total handling", exhausting a one-attempt chain and parking the task on an `Unblock` question.

Both readings are defensible against a criterion that said "no `unwrap`" — which is the point. §23.2 already records that two runs of one configuration produced two different failure modes; this is the sharper form, since the disagreement is one judge with itself on one line of code rather than two runs diverging somewhere.

Two things follow. **Any A/B of review quality is hopeless at this sample size** — the noise floor spans pass and fail on identical input, so the "does `high` judge better than `low`" question cannot be answered by running it twice, and the caveat above understates the problem rather than overstating it. And **acceptance criteria that name a forbidden idiom rather than a forbidden behaviour are a nondeterminism source of their own**: "no `unwrap`" invites a judgement call about what counts as one, where "must not panic on any input" is checkable. That is a lesson for the plans this project writes, not a defect in the reviewer.

## Addendum (2026-08-12) — freeze the policy and prove provider support

The first role-policy implementation recorded effort on each attempt but did not record the policy on the run. Resume re-read today's config, so editing `[routing.effort]` could make the back half of one run use a different implementation and review standard from its front half. That contradicted the same snapshot rule already applied to gates and reviewers.

`run_started` now records the resolved four-value policy: implementation at small, mid, and frontier, plus review. It also freezes every rung's complete binding, including the absence of a pin, so a later pin or newly installed CLI cannot silently change the worker. Workers and reviewers use those records on resume; today's differing config produces an actionable warning and applies only to a new run. An older schema-1 log remains readable by the current binary. Its first resume re-derives once, warns that earlier attempts may differ, appends a schema-2 barrier before using the new identity, and records the established policy and bindings in `run_resumed` so later resumes cannot drift again. The barrier is an unknown event to schema-1 binaries, making downgrade refusal structural rather than dependent on old readers noticing fields they ignore.

Pre-flight now proves more than flag presence. Claude Code and Copilot must advertise all five shared effort levels in the `--effort` help entry, and unreadable help refuses rather than being treated as support. Codex first proves that `--strict-config` rejects a deliberately unknown control key, then proves the exact `model_reasoning_effort=xhigh` and `=max` assignments on both `exec` and `exec resume`; each valid assignment must reach a deliberately missing local `--output-schema` file, which stops before a model turn and keeps the proof zero-spend. `codex debug models` must separately contain every catalogued Codex slug and all five shared reasoning levels for each. This implements the capability-validation half of the earlier “model × effort” follow-on while leaving capability tier and pricing as separate catalog work. Exact argv tests cover all five mappings on all adapters and both Codex command shapes.

Finally, effort and tier are deliberately separate axes. `implementation = "xhigh"` does not promote a docs task from a small model. The repository's self-host policy now makes every task kind's chain explicitly frontier-only, pins frontier to `codex/gpt-5.6-sol` for the WSL runner where its sandbox is enforced, and keeps implementation at `xhigh` and review at `max`.
