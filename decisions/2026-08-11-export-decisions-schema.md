# Decision record — local decision export schema

**Date:** 2026-08-11
**Status:** Decided; implementation belongs to the following v0.2 tasks.
**Related:** [design council](2026-08-11-design-council.md) (the telemetry candidates this settles), [self-hosting v0.2](2026-08-11-self-hosting-v02.md) (the run commissioning this decision), and DESIGN.md §§10, 18, 21, and 23.2.

## Verdict

`tactus export-decisions <run-id> [--format jsonl|csv]` emits export schema 1: one logical row per recorded worker attempt, in raw `AttemptStarted` encounter order. JSONL is the default. CSV carries the same facts as a stable rectangular column set. Both go to stdout, leaving persistence and location to the caller.

The command is local and read-only. It resolves an exact run id or unambiguous prefix by the existing run-directory rules, reads that run's event log and frozen `plan.normalized.json`, and uses the existing read-only liveness probe. It makes no HTTP request, switches no branch, acquires no run lock, and writes neither repository nor run record. It refuses a currently live run with an actionable error: a moving partial dataset is not a decision record.

The liveness probe brackets the first event-log read. Before returning, the exporter reads the event file again, probes again, and requires the exact text to be unchanged. This establishes one stable snapshot without taking the writer lock; a resume observed anywhere across that window is a refusal, not an interrupted row manufactured from a live attempt. A recoverable incomplete final line from a dead process remains exportable and is reported on stderr while stdout stays exclusively JSONL or CSV.

This is an interpretation and calibration surface, not a learned-router feed. It supports people examining routing decisions and the prediction-calibration record in DESIGN.md §23.2. Learned routing remains parked indefinitely under §21. The export adds no upload, cross-repository aggregation, learned scoring, task ranking, model call, or live-tail mode.

## Row construction and authority

Read raw events rather than building from `AttemptRecord` alone or from replay state that has discarded event timestamps. Pair every `AttemptStarted`, keyed by its recorded task/attempt/rung identity, with its optional later `AttemptFinished` or recorded `AttemptInterrupted`. A duplicate start, duplicate settlement, mismatched settlement, settlement without a start, or ambiguous pairing is invalid input and must fail loudly. Output order is the starts' append order, never finish order.

A non-live, finished run may still contain a dangling start. It emits one row whose outcome is `interrupted`, finish timestamp is null, spend and usage are unknown, and detailed failure is derived as `{kind: "interrupted", origin: "worker", reason: null}`. The synthetic kind and origin are derived, not measured. `session_resumed` still comes only from that start's `resume_session.is_some()`; a settlement, especially a synthetic one, may not invent it.

`AttemptStarted` is the pre-spawn authority for adapter id, the CLI version observed at pre-flight, full configured model slug, effort, pool, resumed-session identity, and selection origin. The additive event change therefore places `preflight_cli_version`, `effort`, and `selection_origin` there alongside the existing identity fields; the exporter projects that source field as `adapter_cli_version`. It also adds `preflight_cli_version` and `effort` to each `ReviewRecord`, again exporting the former as `adapter_cli_version`. A run-level version map or finish-only copy is not an acceptable substitute: either loses the identity of an interrupted attempt or falsely makes a later fact pre-execution authority. The CLI version is labelled as the pre-flight observation, not claimed to be a per-invocation probe, and no extra probe is run.

`SelectionOrigin` written to events has exactly `auto`, `user_override`, `pin`, and `exploration`. Current execution produces `auto` or `pin`; the other two are reserved for the v0.2 binder paths that will create them. `unknown` is not an event value. It is an exporter-derived sentinel used only when a legacy start predates the field; such an absence must never be rewritten as `auto`.

## Export schema 1

JSON types below are normative. A “measured” field is a stored fact copied from the event log or frozen plan. “Derived” means a deterministic projection of those facts. `schema_version` is exporter-supplied metadata, not a measurement.

| JSON field | Type and domain | Nullability | Provenance and authority |
|---|---|---|---|
| `schema_version` | integer, always `1` | never | exporter-supplied schema metadata |
| `run_id` | string | never | measured, `RunStarted` |
| `tactus_version` | string | never | measured, `RunStarted` |
| `run_started_at` | RFC 3339 string | never | measured, `RunStarted` event timestamp |
| `attempt_started_at` | RFC 3339 string | never | measured, `AttemptStarted` event timestamp |
| `attempt_finished_at` | RFC 3339 string | yes | measured settlement event timestamp; null for a dangling start |
| `task_id`, `task_title` | string | never | measured task id from the start and title joined from the frozen plan |
| `attempt` | positive integer (one or greater) | never | measured start envelope |
| `rung` | non-negative integer | never | measured start envelope; retains the recorded value |
| `task_features` | object described below | never | copied fields are measured from frozen `plan.normalized.json`; array counts are deterministically derived from that same frozen plan, all before execution |
| `chain` | object described below | never | measured chain recorded at run start, joined by task |
| `selected_tier` | string | never | measured start tier |
| `selection_origin` | `auto`, `user_override`, `pin`, `exploration`, or export-only `unknown` | never | measured for new starts; deterministically derived `unknown` for legacy absence |
| `adapter_id` | string | never | measured start adapter/agent id |
| `adapter_cli_version` | string | yes | measured pre-flight CLI version on the start; null for legacy absence |
| `model` | string | never | measured full configured model slug; no invented provider revision |
| `effort` | `low`, `medium`, `high`, `xhigh`, or `max` | yes | measured start effort; null for legacy absence |
| `pool` | string | yes | measured start pool; null for legacy absence |
| `session_resumed` | boolean | never | derived solely as start `resume_session.is_some()` |
| `duration_ms` | non-negative integer | yes | measured settlement duration; null for a dangling start |
| `cost_usd` | finite non-negative number | yes | measured CLI-reported worker cost; null means unreported or dangling, never zero |
| `usage` | object described below | yes | measured CLI-reported worker usage; null means unreported or dangling |
| `outcome` | `passed`, `failed`, or `interrupted` | never | derived: no failure is `passed`, a recorded non-interruption failure is `failed`, interruption (recorded or dangling) is `interrupted` |
| `failure_kind` | domain below | yes | measured detailed kind for a settlement; derived `interrupted` for dangling; null on pass |
| `failure_origin` | `worker` or `reviewer` | yes | measured on a failed settlement; derived `worker` for dangling; null on pass |
| `failure_category` | `capability`, `provider`, `infrastructure`, or `policy` | yes | derived exhaustively from `failure_kind`; null on pass |
| `work_evidence` | `engine`, `gate`, `review`, or `none` | yes | derived exhaustively from `failure_kind`; null on pass, not `none` |
| `failure_reason` | string | yes | measured settlement prose; null on pass and on a dangling start |
| `reviews` | array of review objects below | never | measured review passes that actually ran, in recorded pass order; empty when none ran or start dangles |

`task_features` has exactly: measured `kind` (string), `suggested_tier` (string or null), `minimum_tier` (string or null), and `path_hints` (array of strings, exact and ordered as frozen); plus deterministically derived `dependency_count`, `acceptance_count`, `artifact_input_count`, and `artifact_output_count` (each a non-negative integer equal to the length of its corresponding frozen-plan array). No diff size or other post-execution feature belongs here.

`chain` has `tiers` (non-empty ordered array of tier strings) and `attempts_per` (positive integer), copied from the run-start chain for that task. `selected_tier` is separate because it is the choice actually made for this attempt.

`usage`, when present, has nullable non-negative integers `input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `num_turns`, and `reasoning_output_tokens`. Preserve each reported absence; do not price tokens or calculate missing totals.

Each `reviews` element has: `pass` (string lens id), `adapter_id` (string), `adapter_cli_version` (string or null for legacy absence), `model` (full configured slug string), `effort` (`low`, `medium`, `high`, `xhigh`, or `max`, or null for legacy absence), `pool` (string or null), `cost_usd` (finite non-negative number or null when unreported), and `outcome` (`passed`, `failed`, or `unavailable`). Version and effort are the values recorded for that pass; neither is borrowed from the worker or today's config.

## Exhaustive failure projection

The exporter must use an exhaustive, non-wildcard match over `FailureKind`, so a future enum variant fails compilation rather than silently becoming uncategorized.

| Detailed `failure_kind` | `failure_category` | `work_evidence` |
|---|---|---|
| `gate_failed` | `capability` | `gate` |
| `review_failed` | `capability` | `review` |
| `agent_error` | `provider` | `none` |
| `rate_limited` | `provider` | `none` |
| `review_unavailable` | `provider` | `none` |
| `timeout` | `infrastructure` | `none` |
| `interrupted` | `infrastructure` | `none` |
| `no_chain` | `policy` | `none` |
| `empty_diff` | `policy` | `engine` |
| `test_provenance` | `policy` | `engine` |
| `needs_human` | `policy` | `none` |
| `declined` | `policy` | `none` |

This separation is deliberate: provider and infrastructure failures are not evidence that the selected model lacked capability. A passed row has `failure_kind`, `failure_origin`, `failure_category`, `work_evidence`, and `failure_reason` all null.

## CSV representation

CSV uses this stable column order, one header followed by one row per JSONL object:

```text
schema_version,run_id,tactus_version,run_started_at,attempt_started_at,attempt_finished_at,task_id,task_title,attempt,rung,task_kind,suggested_tier,minimum_tier,dependency_count,acceptance_count,path_hints_json,artifact_input_count,artifact_output_count,chain_tiers_json,attempts_per,selected_tier,selection_origin,adapter_id,adapter_cli_version,model,effort,pool,session_resumed,duration_ms,cost_usd,usage_input_tokens,usage_output_tokens,usage_cache_creation_input_tokens,usage_cache_read_input_tokens,usage_num_turns,usage_reasoning_output_tokens,outcome,failure_kind,failure_origin,failure_category,work_evidence,failure_reason,reviews_json
```

Null scalar values are empty cells. Booleans are `true` or `false`; numbers use their JSON decimal form. `path_hints_json`, `chain_tiers_json`, and `reviews_json` contain compact valid JSON text representing the corresponding arrays (including the nested review objects). The whole file uses RFC 4180 records: comma delimiter, doubled embedded quotes, quoted fields when they contain a comma, quote, CR, or LF, and CRLF record endings. A small, tested escaping helper is preferable to adding a dependency solely for this encoder; preserving the dependency surface is part of the decision.

## Provenance boundary and input assumptions

The four provenance classes are intentionally disjoint:

1. **Exporter-supplied schema metadata:** only `schema_version = 1`.
2. **Measured stored facts:** values copied from raw events, `RunStarted`, attempt starts/settlements/reviews, and the run's frozen normalized plan. Newly recorded selection origins are measured. “Measured” means stored, not necessarily independently verified by the exporter.
3. **Deterministic derived values:** session-resumed boolean, outcome, failure category, work evidence, legacy `selection_origin = unknown`, counts of frozen-plan arrays, and the dangling-start interruption fields.
4. **Assumptions about valid input:** the event log's append order defines attempt-start order; event timestamps and identities are well formed; run id agrees with its `RunStarted`; every attempt task and every run-start chain joins exactly once to a task in the frozen plan; attempt/settlement identities pair uniquely; chain tiers are non-empty and `attempts_per` is positive; numeric values satisfy the types above; and the frozen plan is the plan named by the run's recorded hash.

Assumptions are not fallback permission. The exporter must validate every join and invariant it relies on and fail loudly with the run and offending identity when one is violated. It must never fill a gap from today's source plan, config, model catalog, pricing table, run-level guess, or provider call.

The export schema version is independent of the event schema. Adding the new optional event fields retains event schema 1: serde defaults preserve old absence as absence, and the exporter represents that honestly as null or, for origin alone, derived `unknown`. A change to one schema does not imply a change to the other.

## Rejected alternatives

- **Train a learned router from this export.** §23.2 bounds the upside and identifies feature quality and noisy outcomes as the real constraint. Human interpretation and prediction calibration are the live purposes; learned routing stays parked.
- **Export a live run or tail it.** It would return a moving partial population and make identical commands disagree. Refusing via the read-only liveness probe preserves a stable record without taking the run's writer lock.
- **Persist into a default file or write beside the run.** Stdout composes with callers and keeps the command read-only; the caller owns retention and location.
- **Upload or aggregate across repositories.** That introduces network, privacy, identity, and policy questions outside a local projection and is not needed for interpretation.
- **Build rows from `AttemptRecord`, report output, or replay state alone.** Those shapes can lose raw start/finish timestamps and dangling starts. Raw event pairing retains the facts and correct ordering.
- **Use finish-only identity or a run-level adapter-version map.** Neither proves which pre-flight identity was selected before a particular spawn, and both fail for interrupted work. Attempt start and each review pass are the necessary authorities.
- **Probe the CLI for every invocation.** Pre-flight already measured the version. Re-probing adds subprocesses and a new failure point after execution has begun while claiming no stronger useful identity.
- **Invent a provider build/revision or shorten model identity.** The CLIs expose the configured model slug, not a provider revision. Recording a fictional precision would be worse than the exact slug actually selected.
- **Treat missing legacy origin as `auto`.** Absence predates the fact and says nothing about how selection happened; only `unknown` preserves that boundary.
- **Collapse failure category and evidence.** A provider outage and a gate rejection answer different questions. Separate fields prevent an infrastructure event from becoming false evidence about model capability.
- **Use post-execution features such as diff size.** They leak the result into a pre-execution decision snapshot and cannot explain what the router knew when it chose.
- **Bump the event schema for additive optional fields or couple it to export schema 1.** Defaults read old logs honestly without changing their meaning; the two schemas serve different consumers and evolve independently.
- **Flatten or drop reviewer/path data in CSV.** That would make CSV a poorer logical export than JSONL. JSON text cells retain nested data in a rectangular file.
- **Add a CSV dependency for this encoder.** The surface is small and RFC 4180 escaping is readily covered with comma, quote, CR, and LF tests; a new dependency costs more maintenance than it removes here.
