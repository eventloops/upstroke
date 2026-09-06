---
id: REVIEW212-BUDGET-CEILING-NOT-POSITIVE
severity: P2
disposition: deferred
category: correctness
pr: 212
reviewed_sha: 2c6d93d66d6144d39a3b1414f2cc7f9b20910a4c
location: src/topology/fold/check_end.rs:202
provenance: introduced_by_feature
first_bad: d0071a376fa907dd7a5d2b2576cf344f00a5dfb2
guard: the next change to `src/topology/fold/check_end.rs`
---

## Failure sequence

Location as first recorded: `src/topology/fold/check_end.rs:202` (as of the
reviewed sha), the whole of `check_budget_exceeded`.

Finding 3 of PR #212's frontier pass.

`check_budget_exceeded` refuses a `budget_exceeded` whose numbers are not
finite and one whose recorded spend has not reached its recorded ceiling. It
does not check that the ceiling is positive.

A `budget_exceeded` record with a valid key, `limit_usd: 0.0` and
`spent_usd: 0.0`, in the current epoch, passes both checks: `0.0` is finite,
and `0.0 >= 0.0`. `apply.rs`'s `BudgetExceeded` arm then sets `budget_stop`
from it, `budget_stop_is_current()` becomes true for the epoch, and a
matching `run_finished` ends the run as `BudgetExceeded`.

`design/17` states what a ceiling is, in the list of what the repo-level
file refuses: "a budget ceiling that is not a positive finite number of
dollars" is an error at load. No producer can therefore reach a run with a
zero ceiling, which is exactly the class of record the new validation exists
to reject -- it says a `budget_exceeded` whose own numbers deny the breach is
refused, and a zero ceiling breached by a zero spend is such a record.

## What the change that takes this up should do

Refuse a `limit_usd` that is not strictly positive, in the same
`FoldError::InconsistentRecord { kind: "budget_exceeded" }` shape and with
the same detail vocabulary the two neighbouring refusals use, citing
`design/17`'s positive-finite ceiling. Add the case to the fixture grid that
already covers the finiteness and spend-versus-ceiling refusals, and witness
it against the obvious mutation -- dropping the new clause -- rather than
against the finiteness clause it sits beside, since a `0.0` ceiling is finite
and that clause cannot catch it.
