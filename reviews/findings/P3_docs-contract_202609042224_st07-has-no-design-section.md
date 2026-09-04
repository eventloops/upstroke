---
id: SWEEP-BIJECTION-006
severity: P3
disposition: deferred
category: docs-contract
pr: 146
reviewed_sha: 943ae61dc61c579a3b03744c8994a1ce81a9acf8
location: src/topology/effects/bijection.rs:1
provenance: pre_existing
first_bad:
guard: the design sections the `#106 effects` family needs, landing with src/topology/effects.rs (queue row 28)
---

## Failure sequence

A reader of `src/topology/effects/bijection.rs` wants the authority for what it enforces. The
module doc names ST-07; the function doc cites
`fault_injection_registry.completeness_rule`; the comments quote `structure` and
`claim_scope` in quotation marks, as normative text. None of those names appears anywhere in
`design/`, which has no section about effect sites, fault injection or the bijection at all:

    grep -rn 'ST-07\|completeness_rule\|fault_injection_registry\|effect site' design/ DESIGN.md

returns nothing at master `943ae61`. The normative source is a packet held in the private lab
repository, so the sentences this module is written against cannot be read, cannot be changed in
the same pull request as the code that implements them, and cannot be cited by a reviewer who does
not have the packet.

`MAINTAINING.md` makes `DESIGN.md` the only living authority for product design and requires
the section to change in the same pull request as the decision. For the `#106 effects` family
that rule has nothing to bind to. A reviewer asked to check that this file enforces what the design
says has no design to check it against, and is left checking it against its own comments.

## What pass 1 on `ffe26ca` said, and where the decision sits

The frontier reviewer (finding 2, `CHANGES_REQUIRED`) holds that deferring this does not satisfy
the binding rule that `DESIGN.md` is the sole living authority, that having `DESIGN.md` merely
declare an external authority contradicts the same rule, and that **the binding ST-07 contract
must be placed in `DESIGN.md` before this file can be marked swept.** PR #146 accordingly does not
claim `src/topology/effects/bijection.rs` swept: the row stays in the review queue with a note,
and the code improvements land on their own.

The remedy is a `DESIGN.md` change, which is the owner's, and this is one of four pull requests
on 2026-09-04 blocked on the same rule (#139, #143 and the `apply.rs` sweep are the others; pass 2
on `2421651` restated it as finding 4). The coordinator is
consolidating the three into one decision for the owner. **Beyond reach, not out of scope:** no
sweep session can settle it, so if a later pass labels this P1 or P2 the disposition becomes
escalate-to-owner rather than still-deferred.

## What the change that takes this up should do

This is the family's question, not one file's, and the natural place to settle it is
`src/topology/effects.rs` (queue row 28), which carries the charter the children were split out
of. Either the packet's normative sentences for ST-07 move into a `design/` section that the
module can cite by number, the way `design/15` and `design/26` are cited elsewhere, or
`DESIGN.md` §21 says explicitly that this family's authority is external and what a reviewer is
expected to do about it. Quoting an unreadable document as though it were a design section is the
one option that should not survive.
