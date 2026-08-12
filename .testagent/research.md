# Test research: effort-policy ultra-review fixes

## Scope and acceptance checklist

This is a bounded regression pass for the six explicit ultra-review findings. No unrelated coverage target is implied.

- [x] **Effort is frozen across resume.** A run must continue with the resolved implementation and review effort it started with, even when today's config differs. An old log without the new record must remain readable, re-derive once with a warning, and establish the value for later resumes.
- [x] **The repository self-host config enforces frontier workers.** Every task kind in the checked-in `tactus.toml` must resolve to a frontier-only implementation chain; review remains frontier, implementation effort is `xhigh`, and review effort is `max`.
- [x] **Claude and Copilot preflight validate the full effort vocabulary.** Advertising `--effort` alone is insufficient; missing any of `low`, `medium`, `high`, `xhigh`, or `max` must refuse before spend. Unreadable help cannot be treated as proof of support.
- [x] **Codex preflight proves the effort surface used by Tactus.** Fresh and resumed help must advertise both `-c` and `--config`, the exact override surface Tactus uses for `model_reasoning_effort=...`; the local model catalog must contain each known Codex model and every Tactus effort level.
- [x] **All three adapter mapping tests are exact.** In particular, a mutation such as `XHigh => "high"` must fail, and the exact spelling must reach each adapter's built argv (including both Codex fresh and resume shapes).
- [x] **The validate preview test derives rather than hard-codes effort.** Distinct role, tier/pin fallback, and disabled-review inputs must produce distinct, exactly asserted effort lines.

## Repository and test conventions

- Rust 2024 crate (`Cargo.toml`), using built-in `#[test]` only; no external test or mock dependency.
- Tests are co-located in each source file under `#[cfg(test)] mod tests`. Narrative lower-snake-case names are the established convention.
- Prefer exact `assert_eq!` values and secondary observables. Existing tests often include a diagnostic message containing the full report/argv.
- Adapter tests build `TaskRun`/`WorkerProfile` values directly and exercise pure argv/output parsers. Real CLI probes are optional smoke tests that skip when a binary is absent; they are not sufficient evidence for a compatibility refusal.
- Engine tests use `temp_engine_repo`, `seed`, fake `AdapterSource` implementations, `parked_run_with_config`, `resume_answering`, `events_of`, and `strip_run_started_field`. These provide deterministic cross-process-like resume behavior without invoking a real model.
- Event compatibility uses additive optional fields with `#[serde(default)]`, preserving event schema 1. The recorded-gate tests are the closest analogue for an effort snapshot that an old log establishes on its first resume.
- Validate tests use a hermetic empty pools file and temporary config roots. Extracting the single `effort:` line and comparing it exactly is stronger than `rendered.contains(...)`.

## Bounded target inventory

| Target | Current relevant behavior/tests | Gap to close |
| --- | --- | --- |
| `src/ir.rs` / `src/config.rs` | `Effort`, `Effort::ALL`, `implementation_effort`, `review_effort`; `role_effort_policy_overrides_pin_and_tier_defaults_independently` | Needs one serializable resolved run policy and a root-config contract test. |
| `src/events.rs` | `RunStarted`, `RunResumed`, `recorded_gates`; old-log and round-trip gate tests | No effort policy is recorded or recovered from a legacy resume. |
| `src/engine.rs` | Fresh/resume construct `Run`; resume freezes reviews/gates but re-reads effort. `a_resume_keeps_the_reviewers_the_run_started_with` and recorded-gate tests provide the harness. | Must source worker and reviewer profiles from the run snapshot and prove changed config cannot alter them. |
| `tactus.toml` | Role effort is fixed to `xhigh`/`max`; task chains are still derived defaults | Fix effort does not imply frontier models; docs/chore can remain small/mid today. |
| `src/agent/claude.rs` | Probe checks `--effort`; mapping test only checks membership in an accepted set | Must validate all five advertised levels, reject unreadable help, and assert exact mapping in argv. |
| `src/agent/copilot.rs` | Same gap as Claude; `advertises` already handles exact short flags | Same compatibility and exact-argv regressions. |
| `src/agent/codex.rs` | Probe checks `--json`, `--sandbox`, `--model` only; argv sends `-c model_reasoning_effort=...`; membership-only mapping test | Must inspect fresh/resume config support and `codex debug models` output, then assert exact values on both argv shapes. |
| `src/validate.rs` | `effort_echo` derives correctly; `the_preview_shows_the_effective_role_effort_before_spend` uses only the same `xhigh`/`max` values a hard-coded implementation could return | Needs multiple discriminating configs and exact line equality. |

## Existing harnesses to reuse

- Resume identity: `parked_run_with_config`, `resume_answering`, `events_of`, `replay_of`, `strip_run_started_field` in `src/engine.rs`.
- Legacy establishment pattern: `the_resume_that_rederives_an_old_logs_gates_records_them_for_the_next_one` in `src/engine.rs` and `recorded_gates` in `src/events.rs`.
- Adapter fixtures: `task_run()` in Claude/Copilot and `run(permissions, resume)` in Codex.
- CLI output fixture shape: each adapter already has a local `output(...) -> ProcessOutput` helper. Probe-contract parsing should be factored into a pure helper that accepts captured help/catalog text so tests never need executables.
- Config isolation: `scratch`, `hermetic`, and `missing` in `src/config.rs`; `opts` in `src/validate.rs`.

## Static source-to-test pairing result

The required polyglot pairing scan was completed once after loading its parser
into an isolated temporary dependency directory:

```text
python .../find_untested_sources.py C:\Projects\Personal\tactus --lang rust --include-tested
```

The scan parsed 32 Rust source files and reported all 32 as unpaired, including
`src/engine.rs`, `src/events.rs`, `src/config.rs`, all three adapter modules, and
`src/validate.rs`. That classification is expected rather than a coverage
finding: the analyzer's documented Rust heuristic recognizes files under
`tests/` and `benches/`, while this repository deliberately uses co-located
`#[cfg(test)]` modules. Its suggested `src/*_test.rs` paths therefore do not
match the established convention. The result was consumed only as a static
source inventory; it is not evidence of line or branch coverage.
