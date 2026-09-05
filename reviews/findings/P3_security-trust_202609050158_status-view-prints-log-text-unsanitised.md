---
id: SWEEP-RENDER-012
severity: P3
disposition: deferred
category: security-trust
pr: 166
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/engine/report.rs:655
provenance: pre_existing
first_bad:
guard: the sweep of `src/engine/report.rs`; `one_line` in `src/status/render.rs` is the shape to reuse, and `describe_is_one_line_with_no_control_character_whatever_the_log_carries` the test to mirror
---

## Failure sequence

An agent exits non-zero with a stderr tail carrying an escape sequence, or a reviewer's
`reasons` carry one -> `engine::attempt::evaluate_outcome` puts up to 400 characters of it
into `FailureRecord.reason` (`agent error (exit …): <stderr tail>`) -> `RunReport::render`
writes that reason verbatim into the `FAILED` line (`src/engine/report.rs:655` at the
reviewed SHA), the `PARKED` line (`:660`) and the open question's context head (`:715`),
and every warning (`:617`) -> `upstroke status` and the end-of-run summary hand the bytes to
the operator's terminal, so a newline splits the task's line and an escape sequence is
interpreted. PR 166 closed the same door for `--follow` (`one_line` on every described
line, witnessed); the settled view still has it open, and it is the larger surface — the
`status` view is printed at the end of every run, not only under `--follow`.

Concrete: a reason of `"agent error (exit Some(1)): x\n\u{1b}[2Jy"` in a `task_failed` fold
renders in `RunReport::render` as two lines with a live clear-screen sequence between them.
Reading, not measured on the box: the `writeln!` at `:655` has no transformation between
`reason` and the sink.

## What the change that takes this up should do

Pass every log-sourced field `RunReport::render` prints — reasons, question contexts,
warnings, task titles — through the same one-line, control-characters-made-visible treatment
`render.rs` applies, at the point the line is assembled rather than per field, and pin it
with a test shaped like `describe_is_one_line_with_no_control_character_whatever_the_log_carries`.
Out of the render sweep's reach because `RunReport::render` is `src/engine/report.rs`'s and
serves `report.json`'s human summary as well as `status`.
