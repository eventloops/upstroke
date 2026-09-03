## 25. Export schema (`export-decisions`)

Decided 2026-08-11, landed 2026-08-12. `upstroke export-decisions <run-id> [--format jsonl|csv]` emits a local schema-2 JSONL/CSV projection of one finished run: one logical row per recorded worker attempt, in raw `AttemptStarted` encounter order. JSONL is the default; CSV carries the same facts as a stable rectangular column set. Both go to stdout, leaving persistence and location to the caller. Consumers select behaviour from `schema_version`, never by assuming an open enum.

The command is local and read-only. It resolves an exact run id or unambiguous prefix by the run-directory rules, reads that run's event log and frozen `plan.normalized.json`, and uses the read-only liveness probe. It makes no HTTP request, switches no branch, acquires no run lock, and writes neither repository nor run record. It refuses a live run: a moving partial dataset is not a decision record. The liveness probe brackets the event-log read — the exporter reads the file again after the first pass and requires the exact text to be unchanged, so a resume observed anywhere across that window is a refusal rather than a row manufactured from a live attempt. A recoverable incomplete final line from a dead process remains exportable and is reported on stderr; stdout stays exclusively JSONL or CSV.

This is an interpretation and calibration surface (§23.2), not a learned-router feed: learned routing stays parked under §21, and the export adds no upload, aggregation, scoring, ranking, model call, or live-tail mode.

### Row construction and authority

Rows are built from raw events, not from `AttemptRecord` or replay state that has discarded timestamps. Every `AttemptStarted`, keyed by its task/attempt/rung identity, pairs with its optional later `AttemptFinished` or `AttemptInterrupted`. A duplicate start, duplicate settlement, mismatched settlement, settlement without a start, or ambiguous pairing is invalid input and fails loudly. Output order is the starts' append order, never finish order.

A finished run may still contain a dangling start. It emits one row whose outcome is `interrupted`, finish timestamp null, spend and usage unknown, and failure derived as `{kind: "interrupted", origin: "worker", reason: null}`. `session_resumed` comes only from that start's `resume_session.is_some()`; a settlement may not invent it.

`AttemptStarted` is the pre-spawn authority for adapter id, the CLI version observed at pre-flight (`preflight_cli_version`, exported as `adapter_cli_version`), full configured model slug, `effort`, pool, resumed-session identity, and `selection_origin`. Each `ReviewRecord` carries its own `preflight_cli_version` and `effort`. A run-level version map or finish-only copy is not a substitute: either loses the identity of an interrupted attempt or makes a later fact pre-execution authority.

`SelectionOrigin` written to events is exactly `auto`, `user_override`, `pin`, or `exploration`. Current execution produces `auto` or `pin`; the other two are reserved for the v0.2 binder. `unknown` is never an event value — it is an exporter-derived sentinel for a legacy start that predates the field, and such an absence is never rewritten as `auto`.

### Schema 2

JSON types are normative. A "measured" field is a stored fact copied from the event log or frozen plan; "derived" is a deterministic projection of those facts; `schema_version` is exporter-supplied metadata.

| JSON field | Type and domain | Nullability | Provenance and authority |
|---|---|---|---|
| `schema_version` | integer, always `2` | never | exporter-supplied schema metadata |
| `run_id` | string | never | measured, `RunStarted` |
| `upstroke_version` | string | never | measured, `RunStarted` |
| `run_started_at` | RFC 3339 string | never | measured, `RunStarted` event timestamp |
| `attempt_started_at` | RFC 3339 string | never | measured, `AttemptStarted` event timestamp |
| `attempt_finished_at` | RFC 3339 string | yes | measured settlement event timestamp; null for a dangling start |
| `task_id`, `task_title` | string | never | measured task id from the start and title joined from the frozen plan |
| `attempt` | positive integer | never | measured start envelope |
| `rung` | non-negative integer | never | measured start envelope |
| `task_features` | object, below | never | copied fields measured from frozen `plan.normalized.json`; counts derived from that same plan |
| `chain` | object, below | never | measured chain recorded at run start, joined by task |
| `selected_tier` | string | never | measured start tier |
| `selection_origin` | `auto`, `user_override`, `pin`, `exploration`, or export-only `unknown` | never | measured for new starts; derived `unknown` for legacy absence |
| `adapter_id` | string | never | measured start adapter id |
| `adapter_cli_version` | string | yes | measured pre-flight CLI version on the start; null for legacy absence |
| `model` | string | never | measured full configured model slug; no invented provider revision |
| `effort` | `low`, `medium`, `high`, `xhigh`, or `max` | yes | measured start effort; null for legacy absence |
| `pool` | string | yes | measured start pool; null for legacy absence |
| `session_resumed` | boolean | never | derived solely as start `resume_session.is_some()` |
| `duration_ms` | non-negative integer | yes | measured settlement duration; null for a dangling start |
| `cost_usd` | finite non-negative number | yes | measured CLI-reported worker cost; null means unreported or dangling, never zero |
| `usage` | object, below | yes | measured CLI-reported worker usage; null means unreported or dangling |
| `outcome` | `passed`, `failed`, or `interrupted` | never | derived: no failure is `passed`; a recorded non-interruption failure is `failed`; interruption (recorded or dangling) is `interrupted` |
| `failure_kind` | domain below | yes | measured detailed kind for a settlement; derived `interrupted` for dangling; null on pass |
| `failure_origin` | `worker` or `reviewer` | yes | measured on a failed settlement; derived `worker` for dangling; null on pass |
| `failure_category` | `capability`, `provider`, `infrastructure`, or `policy` | yes | derived exhaustively from `failure_kind`; null on pass |
| `work_evidence` | `engine`, `gate`, `review`, or `none` | yes | derived exhaustively from `failure_kind`; null on pass, not `none` |
| `failure_reason` | string | yes | measured settlement prose; null on pass and on a dangling start |
| `reviews` | array of review objects, below | never | measured review passes that actually ran, in recorded pass order; empty when none ran or the start dangles |

`task_features` has exactly: measured `kind` (string), `suggested_tier` (string or null), `minimum_tier` (string or null), and `path_hints` (array of strings, exact and ordered as frozen); plus derived `dependency_count`, `acceptance_count`, `artifact_input_count`, and `artifact_output_count` (each a non-negative integer equal to the length of its frozen-plan array). No diff size or other post-execution feature belongs here.

`chain` has `tiers` (non-empty ordered array of tier strings) and `attempts_per` (positive integer), copied from the run-start chain for that task. `selected_tier` is separate because it is the choice actually made for this attempt.

`usage`, when present, has nullable non-negative integers `input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `num_turns`, and `reasoning_output_tokens`. Reported absences are preserved; tokens are never priced and totals never calculated.

Each `reviews` element has `pass` (string lens id), `adapter_id` (string), `adapter_cli_version` (string, or null for legacy absence), `model` (full configured slug string), `effort` (`low`, `medium`, `high`, `xhigh`, or `max`, or null for legacy absence), `pool` (string or null), `cost_usd` (finite non-negative number, or null when unreported), and `outcome` (`passed`, `failed`, or `unavailable`). Version and effort are the values recorded for that pass, never borrowed from the worker or today's config.

### Exhaustive failure projection

The exporter uses an exhaustive, non-wildcard match over `FailureKind`, so a new variant fails compilation rather than becoming uncategorized.

| Detailed `failure_kind` | `failure_category` | `work_evidence` |
|---|---|---|
| `gate_failed` | `capability` | `gate` |
| `review_failed` | `capability` | `review` |
| `agent_error` | `provider` | `none` |
| `rate_limited` | `provider` | `none` |
| `review_unavailable` | `provider` | `none` |
| `review_input_too_large` | `policy` | `review` |
| `review_input_opaque` | `policy` | `review` |
| `timeout` | `infrastructure` | `none` |
| `interrupted` | `infrastructure` | `none` |
| `no_chain` | `policy` | `none` |
| `empty_diff` | `policy` | `engine` |
| `test_provenance` | `policy` | `engine` |
| `needs_human` | `policy` | `none` |
| `declined` | `policy` | `none` |

Provider and infrastructure failures are not evidence that the selected model lacked capability. A passed row has `failure_kind`, `failure_origin`, `failure_category`, `work_evidence`, and `failure_reason` all null.

### CSV representation

One header followed by one row per JSONL object, in this column order:

```text
schema_version,run_id,upstroke_version,run_started_at,attempt_started_at,attempt_finished_at,task_id,task_title,attempt,rung,task_kind,suggested_tier,minimum_tier,dependency_count,acceptance_count,path_hints_json,artifact_input_count,artifact_output_count,chain_tiers_json,attempts_per,selected_tier,selection_origin,adapter_id,adapter_cli_version,model,effort,pool,session_resumed,duration_ms,cost_usd,usage_input_tokens,usage_output_tokens,usage_cache_creation_input_tokens,usage_cache_read_input_tokens,usage_num_turns,usage_reasoning_output_tokens,outcome,failure_kind,failure_origin,failure_category,work_evidence,failure_reason,reviews_json
```

Null scalars are empty cells; booleans are `true`/`false`; numbers use their JSON decimal form. `path_hints_json`, `chain_tiers_json`, and `reviews_json` are compact JSON text. Records are RFC 4180: comma delimiter, doubled embedded quotes, quoted fields when they contain a comma, quote, CR, or LF, CRLF record endings. The encoder is a small tested helper, not a dependency.

### Provenance boundary

The four provenance classes are disjoint: exporter-supplied metadata (`schema_version` only); measured stored facts (raw events and the frozen plan — stored, not independently verified); deterministic derived values (the resumed boolean, outcome, category, evidence, legacy `unknown`, plan-array counts, dangling-start fields); and assumptions about valid input (the event log's append order defines attempt-start order; event timestamps and identities are well formed; the run id agrees with its `RunStarted`; every attempt task and every run-start chain joins exactly once to a task in the frozen plan; attempt/settlement identities pair uniquely; chain tiers are non-empty and `attempts_per` is positive; numeric values satisfy the types above; the frozen plan is the plan named by the run's recorded hash). Assumptions are not fallback permission: the exporter validates every join and invariant it relies on and fails loudly naming the run and offending identity. It never fills a gap from today's source plan, config, catalog, pricing table, or a provider call.

The export schema version is independent of the event schema. Schema 2 was required when the public exhaustive failure domain gained `review_input_too_large` and `review_input_opaque`; a change to one schema does not imply a change to the other.
