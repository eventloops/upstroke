---
id: SWEEP-RENDER-014
severity: P3
disposition: deferred
category: correctness
pr: 166
reviewed_sha: 323beb0b1b3ebc2ab645bf10f1cfde81d2b7250b
location: src/events/mod.rs:1843
provenance: pre_existing
first_bad:
guard: the sweep of `src/events/mod.rs`; `AttemptRecord::is_successful` is the definition, and `ensure_supported_schema`'s schema-3 `attempt_finished` arm is the door that does not read it
---

## Failure sequence

`AttemptRecord::is_successful` documents itself as the crate's one definition of success —
"not `failure.is_none()`: a record can carry no failure and still hold a review whose outcome is
`Failed` or `Unavailable`, both of which are authoritative" — and the schema-4 doors
(`check_candidate_prepared`, `check_attempt_finished`) read it. The schema-3 validation in
`src/events/mod.rs` (`let failed = data.failure.is_some();` at the reviewed SHA's line 1843)
classifies the same record by `failure` alone -> a schema-3 log whose successful
`attempt_finished` has its review outcome edited from `passed` to `failed` with `failure`
left `null`, its `prepared_commit` and the following `task_committed` intact, passes
validation as a success and replays to `Done` on `task_committed` (line 1348) -> `status`
renders the task committed, and `status --follow` (PR #166) describes the attempt as "was not
approved — review `review` (model) rejected it" one line before "committed": the record's two
readers disagree, and the definition the type carries is not the one the schema-3 door enforces.
Reachable only by editing a log — `engine::attempt::evaluate_review` always writes a
`ReviewFailed` or `ReviewUnavailable` failure beside a non-passing outcome — which is why P3.
Found by pass 1 of PR #166 (gpt-5.6-sol on `7572298`, finding 1), which showed the pull request's
own "the fold refuses to promote such a record" to be true of schema 4 only.

## What the change that takes this up should do

Have the schema-3 `attempt_finished` arm decide success through `is_successful()` — the same
predicate the schema-4 doors read — and refuse a prepared commit or a following `task_committed`
on a record that predicate rejects, with a message naming the pass. Out of the render sweep's
reach because the door is `src/events/mod.rs`'s and the refusal changes what `load` accepts.
