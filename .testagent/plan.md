# Test plan: effort-policy ultra-review fixes

## Test-first implementation phases

### Phase 1: define and preserve the resolved run effort record

Anticipated production seam:

- Add a small serializable resolved policy value (for implementation at small/mid/frontier plus review) in shared IR.
- Have `Config` resolve that value once.
- Add `#[serde(default)] Option<...>` fields to `RunStarted` and `RunResumed`, plus an event helper analogous to `recorded_gates` where `RunStarted` wins and the first establishing resume is the legacy fallback.

Planned tests:

1. `events::tests::a_run_started_without_an_effort_policy_reads_as_unrecorded`
   - Serialize a current `run_started`, remove only the effort-policy field, deserialize it, and assert the field and `recorded_effort_policy(...)` are `None`.
   - Secondary assertion: schema remains `SCHEMA_VERSION`; the additive field must not force a schema bump.

2. `events::tests::a_recorded_effort_policy_round_trips_every_role_and_tier_exactly`
   - Use deliberately distinct values (`small=low`, `mid=medium`, `frontier=xhigh`, `review=max`).
   - Assert exact raw JSON spellings and the exact deserialized value. Distinct values prevent field swaps from surviving.

3. `events::tests::the_first_recorded_effort_policy_is_the_run_authority`
   - Build event vectors for: value in `RunStarted`; absent start plus value in first `RunResumed`; and multiple resumes with conflicting later values.
   - Assert start wins when present and otherwise the first establishing resume wins.

Narrow commands:

```powershell
cargo test events::tests::a_run_started_without_an_effort_policy_reads_as_unrecorded -- --exact
cargo test events::tests::a_recorded_effort_policy_round_trips_every_role_and_tier_exactly -- --exact
cargo test events::tests::the_first_recorded_effort_policy_is_the_run_authority -- --exact
```

### Phase 2: freeze effort across resume, including old logs

Anticipated production seam:

- Construct the policy before emitting `RunStarted` and store it on `Run`.
- Both worker and reviewer profiles must read from this stored policy, never directly from today's `analysis.config` after resume.
- Resume compares today's resolved policy with the record, continues with the record, and emits an actionable warning when they differ.
- A legacy log re-derives once, warns, and writes the established policy in `RunResumed` for subsequent resumes.

Planned tests:

1. `engine::tests::resume_runs_with_the_effort_policy_the_run_recorded_not_todays_config`
   - Park a one-rung run whose config sets implementation `xhigh` and review `max`.
   - Rewrite only `[routing.effort]` to different values while retaining the exact same chain, answer, and resume with an editing fake adapter.
   - Assert the task commits, the resumed `AttemptStarted.effort` is exactly `Some(XHigh)`, every resumed review record has exactly `Some(Max)`, and the warning names both the recorded/current policy and says a new run is needed to adopt the change.
   - This must fail if either worker or review effort is re-read from current config.

2. `engine::tests::the_resume_that_rederives_an_old_logs_effort_records_it_for_the_next_one`
   - Park a run, strip the start's effort-policy field, and perform a first no-edit resume under a deliberately distinct current policy so it remains parked.
   - Assert the legacy warning appears and `recorded_effort_policy(events)` now equals the first resume's derived policy.
   - Change config again, complete a second resume, and assert its worker/reviewer events retain the first resume's policy rather than either the original stripped policy or latest config.
   - Assert the second warning is a normal policy-difference warning, not another legacy warning.

3. `engine::tests::a_resume_whose_effort_policy_did_not_move_says_nothing_about_it`
   - Resume without editing config.
   - Assert completion and that no warning mentions an effort-policy difference or legacy derivation. This prevents an always-warning comparison from passing the changed-config tests.

Narrow commands:

```powershell
cargo test engine::tests::resume_runs_with_the_effort_policy_the_run_recorded_not_todays_config -- --exact
cargo test engine::tests::the_resume_that_rederives_an_old_logs_effort_records_it_for_the_next_one -- --exact
cargo test engine::tests::a_resume_whose_effort_policy_did_not_move_says_nothing_about_it -- --exact
```

### Phase 3: make the checked-in self-host policy mechanically frontier-only

Anticipated production change: add explicit `[routing]` entries for all seven task kinds with `chain = ["frontier"]`, and make the effective review tier frontier. Keep implementation `xhigh` and review `max`. If a frontier pin is chosen, assert its exact agent/model as an additional repository policy, not as a substitute for checking every chain.

Planned test:

1. `config::tests::the_repository_self_host_policy_is_frontier_only_with_fixed_role_effort`
   - Load `Path::new(env!("CARGO_MANIFEST_DIR")).join("tactus.toml")` with the hermetic pools fixture.
   - For every `TaskKind::ALL`, assert the effective chain equals exactly `[Tier::Frontier]` and is `from_config`.
   - Assert effective review tier is frontier, implementation effort at frontier is exactly `XHigh`, and review effort is exactly `Max`.
   - If pinned, route a representative task and assert the binding's catalog tier is frontier (plus exact pin if that identity is intended policy).

Narrow command:

```powershell
cargo test config::tests::the_repository_self_host_policy_is_frontier_only_with_fixed_role_effort -- --exact
```

### Phase 4: prove Claude and Copilot compatibility before spend

Anticipated production seam: extract pure help-contract validators. Parse the effort choice list associated with `--effort`, compare it to `Effort::ALL`, and refuse on failed/timed-out/empty help rather than treating unreadable output as support.

Planned tests in **each** adapter module:

1. `agent::<adapter>::tests::help_validation_requires_every_shared_effort_level`
   - Feed a complete synthetic help surface and assert success.
   - Remove `xhigh` and `max` one at a time while leaving `--effort` present; assert refusal names the exact missing level and the CLI version.
   - Include extra Copilot choices (`none`, `minimal`) to prove they neither hurt nor replace the shared five.

2. `agent::<adapter>::tests::unreadable_help_is_a_preflight_refusal`
   - Exercise timeout/non-zero/empty output cases through the pure probe-contract seam.
   - Assert each returns an agent/preflight error rather than capability success.

Exact names:

- `agent::claude::tests::help_validation_requires_every_shared_effort_level`
- `agent::claude::tests::unreadable_help_is_a_preflight_refusal`
- `agent::copilot::tests::help_validation_requires_every_shared_effort_level`
- `agent::copilot::tests::unreadable_help_is_a_preflight_refusal`

Narrow commands:

```powershell
cargo test agent::claude::tests::help_validation_requires_every_shared_effort_level -- --exact
cargo test agent::claude::tests::unreadable_help_is_a_preflight_refusal -- --exact
cargo test agent::copilot::tests::help_validation_requires_every_shared_effort_level -- --exact
cargo test agent::copilot::tests::unreadable_help_is_a_preflight_refusal -- --exact
```

### Phase 5: prove Codex fresh/resume and model-by-effort support

Anticipated production seam:

- Require the config override surface (`--config`, whose short form is `-c`) on both `codex exec --help` and `codex exec resume --help`; keep `--sandbox` fresh-only.
- Read `codex debug models` at preflight and parse it into a small local DTO. For every `catalog::known_models("codex")`, require the model and every level in `Effort::ALL`. Malformed/unreadable catalog output is not proof and must refuse.

Planned tests:

1. `agent::codex::tests::probe_contract_requires_reasoning_config_on_fresh_and_resume`
   - Full synthetic fresh and resume help succeeds.
   - Removing `--config` from either surface independently fails and names whether `exec` or `exec resume` is incompatible.
   - Removing resume support cannot leave `Caps::session_resume = true`.

2. `agent::codex::tests::model_catalog_requires_every_effort_for_each_known_codex_model`
   - A realistic `debug models` JSON fixture for `gpt-5.6-sol` with all five shared levels succeeds.
   - Removing `xhigh` and `max` independently fails with the exact model and level in the error.
   - Omitting a known model fails with its slug; an unrelated model cannot satisfy it.

3. `agent::codex::tests::unreadable_model_catalog_is_a_preflight_refusal`
   - Empty, malformed, non-zero, and timed-out model-catalog outputs refuse before spend.

Narrow commands:

```powershell
cargo test agent::codex::tests::probe_contract_requires_reasoning_config_on_fresh_and_resume -- --exact
cargo test agent::codex::tests::model_catalog_requires_every_effort_for_each_known_codex_model -- --exact
cargo test agent::codex::tests::unreadable_model_catalog_is_a_preflight_refusal -- --exact
```

### Phase 6: replace membership-only adapter tests with exact argv contracts

Use an explicit five-row table in every adapter:

```text
Low -> low
Medium -> medium
High -> high
XHigh -> xhigh
Max -> max
```

Planned tests:

1. `agent::claude::tests::every_effort_has_the_exact_cli_spelling_in_build_args`
   - Assert `effort_flag(effort) == expected` and the argv element immediately after `--effort` equals `expected`.

2. `agent::copilot::tests::every_effort_has_the_exact_cli_spelling_in_build_args`
   - Assert the exact mapping and an exact argv element `--effort=<expected>`.

3. `agent::codex::tests::every_effort_has_the_exact_config_spelling_on_fresh_and_resumed_attempts`
   - For every row, assert both fresh and resumed argv contain adjacent `-c`, `model_reasoning_effort=<expected>` elements.

These tests must replace or strengthen the existing membership tests; keeping only the old `ACCEPTED.contains(...)` assertions does not satisfy this finding.

Narrow commands:

```powershell
cargo test agent::claude::tests::every_effort_has_the_exact_cli_spelling_in_build_args -- --exact
cargo test agent::copilot::tests::every_effort_has_the_exact_cli_spelling_in_build_args -- --exact
cargo test agent::codex::tests::every_effort_has_the_exact_config_spelling_on_fresh_and_resumed_attempts -- --exact
```

### Phase 7: make the validate preview regression discriminating

Planned test:

1. `validate::tests::the_preview_echoes_resolved_role_tier_pin_and_disabled_review_effort`
   - Table-drive at least these distinct config/expected-line pairs:
     - no role override: `effort: implementation=by tier (small=low, mid=medium, frontier=high), review=high`;
     - a small-tier pin plus small review tier: implementation shows the exact pin/tier mix and review uses the pinned value;
     - explicit implementation/review role values different from `xhigh`/`max` (for example `low`/`xhigh`): one global implementation value and the exact review value;
     - review disabled: exact `review=disabled` while implementation remains derived.
   - For each case, extract the sole line beginning `effort:` and use `assert_eq!`; do not use `contains`.
   - Include the original `xhigh`/`max` case only as an additional row, so a hard-coded output cannot satisfy the suite.

Narrow command:

```powershell
cargo test validate::tests::the_preview_echoes_resolved_role_tier_pin_and_disabled_review_effort -- --exact
```

## Final verification sequence

After all narrow commands are green:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Run the same three gates in the WSL/Linux checkout because Codex implementer behavior and the repository's self-hosting policy are platform-sensitive. The optional live CLI smoke tests may skip when a binary is absent; the synthetic compatibility tests above are the required deterministic evidence.

## Requirement-to-test traceability

| Finding | Required evidence |
| --- | --- |
| Effort is frozen across resume | `resume_runs_with_the_effort_policy_the_run_recorded_not_todays_config`; `the_resume_that_rederives_an_old_logs_effort_records_it_for_the_next_one` |
| Self-host config enforces frontier workers | `the_repository_self_host_policy_is_frontier_only_with_fixed_role_effort` |
| Claude/Copilot validate every requested level | Both adapters' `help_validation_requires_every_shared_effort_level` and `unreadable_help_is_a_preflight_refusal` |
| Codex proves config/resume/model-effort support | `probe_contract_requires_reasoning_config_on_fresh_and_resume`; `model_catalog_requires_every_effort_for_each_known_codex_model`; `unreadable_model_catalog_is_a_preflight_refusal` |
| Mapping tests reject `XHigh -> high` and similar aliases | All three `every_effort_has_the_exact_*_in_build_args` tests |
| Validate preview cannot be hard-coded | `the_preview_echoes_resolved_role_tier_pin_and_disabled_review_effort` |
