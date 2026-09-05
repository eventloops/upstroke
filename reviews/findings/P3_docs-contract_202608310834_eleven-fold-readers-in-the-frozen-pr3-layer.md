---
id: PR7-FOLD-ACCESSORS-IN-PR3-LAYER
severity: P3
disposition: deferred
category: docs-contract
pr: 7
reviewed_sha:
location: src/topology/fold.rs
provenance: undetermined
first_bad:
guard: project owner — adjudicated 2026-08-24, see §3; the deferred work is the G2 PR3-layer pass
---

## Failure sequence

`src/topology/fold.rs` is **+1196 / −13 at `2378c83`** (`git diff <merge-base>...HEAD --numstat -- src/topology/fold.rs`). **Twice restated, and the second time by a reviewer rather than by me.** It read +628/−0 and "nine accessors" until 2026-08-24, then +777/−11; the frontier review of `75da796` measured +1196/−13 and observed that a disclosure row whose own number is stale is the disclosure failing — twice over, since the correction that fixed the first staleness introduced the second. **The number now carries the sha it was taken at**, per §22's rule, because that is the only form of it that does not decay: this file grows whenever the slice adds a fold test, and a figure with no sha reads as current forever. Disclosed here rather than left for a reviewer to find, because it is PR3's file and the slice is large enough that a footprint this size can stop being visible to the person making it

## What the change that takes this up should do

Owner, as the ledger records it: project owner — **adjudicated 2026-08-24, see §3**; the deferred work is the G2 PR3-layer pass.

**Accepted as a disclosed deviation through `3362f65`.** Measured split at head: **561 lines of tests**, **152 comment and blank lines** in the production region, and **64 lines of production code**. That code is **eleven `pub fn` readers** — `ready`, `ready_retry`, `pipeline_held`, `pipeline_reservable`, `structurally_admissible`, `integration_admissible`, `run_is_ending`, `backoff_pending`, `predicted_region`, `frozen_rung_binding`, `questions_open` — nine of them one-line delegations to an existing private `RunState` predicate with a poison guard, plus **one line of changed behaviour**: `&& self.pipeline_reservable()` in `integration_admissible`, which is `PR7-INTEGRATION-NO-ENTITLEMENT`'s repair. The **11 deletions** are not behaviour either: four are one re-wrapped `use` block, and seven are the body of the *test* helper `frozen_binding`, which repeated the reader's composition and now delegates to it — so the reader sits under the whole existing attempt corpus. No variant added, no type widened, nothing else deleted, which is not the shape `ff0490a` forbade. **`frozen_rung_binding` is deliberately half of the fold's rule**: it returns the frozen rung's binding and not the human-override arm, because no override is constructible while the answer-ingest branch is unimplemented and because `matches_override` checks only agent, model and effort — leaving `tier` and `pinned` for a caller to choose unchallenged. Collapsing it to a full delegation is **W2 of the pass**. It is also the **last fold reader outside that pass**: the standing rule this slice proposed was rejected

Carried in `reviews/FINDINGS.md` §2, “Open — carried deliberately, with an owner”, and confirmed still carried by the full-ledger audit of 2026-08-31 (§39). The row carried no severity label; **P3** here is this migration's judgement from the consequence described above, not the reviewer's own word.
