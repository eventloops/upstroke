# Test status: effort-policy ultra-review fixes

## Result

All six review requirements are implemented. Windows and WSL/Linux are green,
and the focused regression suite has been mutation-checked.

## Focused evidence

| Requirement | Regression evidence |
| --- | --- |
| Freeze implementation and review effort across resume | `resume_runs_with_the_effort_policy_the_run_recorded_not_todays_config`; `the_resume_that_rederives_an_old_logs_effort_records_it_for_the_next_one`; `a_resume_whose_effort_policy_did_not_move_says_nothing_about_it`; three event compatibility/authority tests |
| Make the checked-in self-host policy frontier-only | `the_repository_self_host_policy_is_frontier_only_with_fixed_role_effort` |
| Validate every Claude/Copilot effort level before spend | Both adapters' `help_validation_requires_every_shared_effort_level` and `unreadable_help_is_a_preflight_refusal`; shared option-block and whole-token tests |
| Prove Codex fresh/resume/model-by-effort compatibility | `probe_contract_requires_reasoning_config_on_fresh_and_resume`; `unreadable_fresh_or_resume_help_is_a_preflight_refusal`; `model_catalog_requires_every_effort_for_each_known_codex_model`; `unreadable_model_catalog_is_a_preflight_refusal`; live probe |
| Make all adapter mappings exact | Claude/Copilot `every_effort_has_the_exact_cli_spelling_in_build_args`; Codex `every_effort_has_the_exact_config_spelling_on_fresh_and_resumed_attempts` |
| Prevent a hard-coded validate preview | `the_preview_echoes_resolved_role_tier_pin_and_disabled_review_effort` |

## Pseudo-mutation review

Nine representative production regressions were injected one at a time and
reverted immediately. All nine were killed by their narrow regression:

1. Resume used today's policy instead of the recorded policy.
2. Claude mapped `XHigh` to `high`.
3. Copilot mapped `XHigh` to `high`.
4. Codex resume stopped requiring the reasoning config surface.
5. Codex model validation stopped requiring `Max`.
6. The repository's design chain fell back to `mid`.
7. Validate hard-coded implementation effort to `xhigh`.
8. Shared help parsing stopped requiring `Max`.
9. Codex mapped `XHigh` to `high`.

Mutation score for this bounded review: **9/9 killed (100%)**, with no survived
or uncovered mutation among the selected high-risk invariants.

## Assertion-quality review

The 22 new or materially strengthened tests contain 105 source-level checks
(assertion macros plus explicit expected-error checks), averaging 4.8 per test.
There are no assertion-free, trivial-only, or self-referential tests. The suite
uses 10 of the 12 applicable categories: equality, Boolean, absence, error,
variant/type, string, collection, negative, state/side-effect, and
structural/deep assertions. Numeric comparison and approximate assertions are
absent because this change contains no numeric or floating-point behavior.

Error/refusal behavior is directly asserted in eight tests. Negative invariants
are exercised in eleven tests, and the two resume integration regressions
verify both emitted events and final run state. The one conditionally vacuous
test is the intentional real-Codex smoke probe when no binary exists; all
compatibility claims also have deterministic fixtures, so the smoke test is
supplementary rather than load-bearing.

## Commands

Windows completed:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
# 437 passed; 0 failed; 2 ignored
```

WSL/Linux completed:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
# 438 passed; 0 failed; 2 ignored
```

The one additional Linux test is the platform-specific assertion that Codex
implementation is allowed where its sandbox is real.
