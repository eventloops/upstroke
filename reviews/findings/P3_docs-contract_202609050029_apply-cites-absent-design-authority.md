---
id: SWEEP-FOLD-APPLY-DESIGN-AUTHORITY
severity: P3
disposition: deferred
category: docs-contract
pr: 152
reviewed_sha: 943ae61dc61c579a3b03744c8994a1ce81a9acf8
location: src/topology/fold/apply.rs:1
provenance: pre_existing
first_bad:
guard: src/topology/effects.rs (queue row 28), where the #108 family settles it — with #146 SWEEP-BIJECTION-006, #139 and #143 as the same class before the owner
---

## Failure sequence

A reviewer looks for the authority behind the contracts `apply.rs` states the effect of — INV-02
(module doc, lines 1 and 18), ST-06, `transaction_fault_matrix[T-ATTEMPT]` (lines 39-42, 46, 179),
"the packet" (line 220), the `refusals` inventory it applies, and the retired
`decisions/2026-08-12-merge-queue-execution-topology.md` record (line 310) -> `grep -rn` over
`design/` and `DESIGN.md` returns none of them, and the `decisions/` directory no longer exists at
this SHA (it was retired on 2026-09-03) -> `MAINTAINING.md`'s rule that the design section changes
in the same pull request as the decision has nothing to bind to, and a reviewer can only check the
module against its own comments and against a packet in the private lab repository they cannot see.

This is the fourth file in four families to state its effect against an authority absent from the
design: #139 (`rundir`), #143 (`rundir/names`) and #146 (`effects/bijection.rs`,
`SWEEP-BIJECTION-006`) are the same class. Four independent instances is one question for the owner,
not four opinions.

## Why the row stays open rather than being ticked swept

Listing `src/topology/fold/apply.rs` in the swept table binds §6 and §7 over it in full and asserts
a conformance the file has for its code but not for its contract half: the effects it applies are
correct only relative to a specification that `DESIGN.md` does not hold. A swept claim would assert
that anchoring, which is the owner's to establish. `#108`'s design authority is with the owner as
one decision beside #139, #143 and #146.

## What the change that takes this up should do

Beyond this session's reach: the remedy is a `DESIGN.md`/`design/` change deciding where the `#108`
topology fold's normative contracts live, which is an owner-level ruling, not a sweep's. So this is
filed as beyond reach rather than out of scope: no session working from inside this crate can settle
it. If a later pass labels it P1 or P2, the disposition becomes **escalate to the owner** rather than
re-defer. `src/topology/effects.rs` (queue row 28) is where the `#106` family recorded the same gap
(`SWEEP-BIJECTION-006`), and the `#108` family settles it in the same place or in its parent
`src/topology/fold.rs` (queue row 40).
