---
id: PR153-APPLY-DECLINE-FAILS-ONE-MEMBER
severity: P1
disposition: deferred
category: liveness
pr: 153
reviewed_sha: 671949e45d03614ab785b934bca22b4b3fa31d76
location: src/topology/fold/apply.rs:503
provenance: pre_existing
first_bad:
guard: PR #152 owns the application repair; this remains a merge blocker for #153 until the repair is integrated and its checked-log and replay regressions pass
---

## Failure sequence

`DESIGN.md` §26: "Declining fails the lineage." `apply_answer`'s decline arm fails the answered task
alone and calls `release_holdings_of`, which releases the lineage lease; nothing moves the rejected
original. #153 keeps a *bare* `question_raised` off lineage members, so the bare route the pass-3
review of #153 reproduced at `671949e` is closed. The admission route is not a bare question and
reaches the same arm:

```
run_started
task_dispatched(ALPHA, g0) → attempt_started → candidate_prepared → task_candidate_created
merge_verification_started(ALPHA, s0)
merge_rejected(ALPHA; repair TaskKey(3), lineage rooted at ALPHA,
               admission HumanRequired { limit 1, question q })
                                        # ALPHA = AwaitingRepair; 3 = AwaitingInput; lineage lease held
question_answered(3, q, Declined { decline_halts_run: false })
                                        # accepted: 3 is parked with nothing open
```

Measured at `81c04f188b2855bf40018712256f597bfc75bccf` by the temporary
`probe_admission_decline_on_a_repair` test, built from the fold's event fixtures. The probe
intentionally panicked to report its values; its failed test result is measurement evidence,
not a passing regression:

```
PROBE before=(Some(AwaitingRepair), Some(AwaitingInput)) lineage_before=true accepted=Ok(()) after=(Some(AwaitingRepair), Some(Failed)) lineage_after=false outcome=FoldError run_finished=Err(OutcomeMismatch { recorded: "complete", derived: "unreachable" })
```

After the decline: task 3 `Failed`, the lineage lease released, ALPHA still `AwaitingRepair` with no
queue position, no question, no generation, no transaction and no runnable repair.
`derived_outcome()` is `FoldError` — `common()` holds, nothing is admissible, no question is open,
and `complete_shape()` is false because ALPHA is neither terminal nor a blocked `Pending`.
The probe tried `run_finished(Complete)` only. Inspection of `check_run_finished` establishes
that every recorded outcome is refused when the derived outcome is `FoldError`.
The run cannot end. A halting decline reaches
`Ending(Halted)` instead, because `halted_at` is consulted before the shape; the non-halting one is
the wedge.

## What the change that takes this up should do

Fail the affected unmerged lineage members and clean up their questions, queue positions and
leases in one application, with the same result live and on replay. Preserve already published
ancestors and unrelated work. Account for open generations and integration transactions across
the lineage at both question and answer time. Failing the answered task alone is insufficient;
failing the lineage while leaving a transaction able to publish it is also insufficient.

PR #152 owns this repair. Its integration is a dependency, not grounds to waive a reproduced
P1. This file stays open until the integrated candidate has checked-log regressions for legitimate
repair admission, nonhalting decline, complete cleanup and preservation of unrelated or published
work. The `deferred` field records current ownership while that repair is pending; it does not
authorize merging #153 with this defect.

The `check`-side alternative — refusing a decline of a repair's admission question — would refuse
the human's answer to a question the design says to ask (§26 registers the repair `AwaitingInput`
"with a frozen question"), so it is not the fix; the fix is the apply.
