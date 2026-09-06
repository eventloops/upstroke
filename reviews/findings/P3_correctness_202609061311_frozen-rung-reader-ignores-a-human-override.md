---
id: SWEEP-FOLD-PREDICATES-OVERRIDE-READER
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/engine/topology/run.rs:600
provenance: pre_existing
first_bad:
guard: the PR9 answers slice — the change that implements `question_answered` ingest and `T-ANSWER`
---

## Failure sequence

`TopologyFold::frozen_rung_binding` (`src/topology/fold/predicates.rs:138`) answers with the
binding the task's frozen ladder holds for a rung, and nothing else. The fold's own rule is wider:
`check_attempt_started` (`src/topology/fold/check_attempt.rs:424`) matches the recorded binding
against a **human override** when `RunState::overrides` holds one for that key, and against the
frozen rung only when it does not. The reader's own notes say so and say a caller holding an
override must not use it.

All line numbers here are as of `reviewed_sha`.

Both production call sites use it without consulting `TopologyFold::binding_override`:
`src/engine/topology/run.rs:600` (`retry_ready`) and `src/engine/topology/run.rs:715` (`attempt`).

So: an operator answers a task's admission question with a binding override; `apply_answer`
(`src/topology/fold/apply.rs:395`) records it in `overrides`; the loop reaches `attempt`, asks
`frozen_rung_binding` for the rung's binding, runs the agent under the **frozen** binding rather
than the one the operator named, and then appends an `attempt_started` the fold refuses with
`FoldError::BindingMismatch` — after the attempt has already been spent. The operator's override
is not honoured and the spend is not recoverable by re-running.

This is latent at `ee5dc81f`, not live, and that is why it is P3 rather than P2. Measured: no
production path can populate `overrides` at this head. `select`'s hard-block branch
(`hard_block`, `src/engine/topology/run.rs:486`) refuses with `UpstrokeError::Refused` "before any
append" as soon as `seams.answers.resolve` returns anything but `Answer::Unanswered`, and
`IngestAnswers` carries `Disposition::NotThisSlice { slice: "PR9" }`. `grep -rn 'overrides' src/`
shows the map written at exactly one site, `apply_answer`, reachable only from a
`question_answered` event this build never appends.

## What the change that takes this up should do

Land the second arm together with the passage that decides `tier` and `pinned`. The reader's notes
record why it is only half the rule: `RungBinding::matches_override` compares agent, model and
effort and says nothing about `tier` or `pinned`, so a caller that composed an override binding
from `Self::binding_override` and this reader would be choosing those two fields unchallenged.
Growing `frozen_rung_binding` (or a successor that names itself the *effective* binding) to
consult `overrides` keeps the fold's admission rule and the loop's construction of the event in
one place, which is the reason this reader exists at all. Do not fix it by composing the two
readers at the call sites in `run.rs`; that is the second authority the module was written to
avoid.
