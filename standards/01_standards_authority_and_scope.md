## 1. Authority and scope

The documents govern different questions:

- [`DESIGN.md`](DESIGN.md), especially §4, governs product behaviour and architecture.
- This document governs implementation quality and Rust engineering practice.
- [`MAINTAINING.md`](MAINTAINING.md) governs integration, review, release, and emergency process.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) is the contributor entry point and licence agreement.
- Dated records in `decisions/` explain why a decision was made; proposals do not bind the
  implementation unless the design adopts them.

When these disagree, do not choose silently. Product requirements in `DESIGN.md` take precedence
over an implementation preference, but the conflicting documents must be reconciled in the same
change. CI configuration describes automated enforcement; it does not weaken a requirement here
merely because that requirement needs human review.

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** express requirement
strength. A `SHOULD` deviation needs a concrete reason in the code or pull request. A `MUST`
deviation needs an explicit, reviewed change to this standard or to the controlling design—not an
ad hoc exception.

Existing code is not precedent against this standard. A change MUST leave the code it materially
touches compliant. Unrelated debt should be recorded rather than hidden inside an unreviewable
scope expansion, unless it creates an immediate correctness or security risk; deferring such a
risk requires an explicit owner and rationale.

### Retired conflict: `CommandSpec.program`

Adoption recorded an unresolved conflict here: `DESIGN.md:222` freezes `CommandSpec.program` as
`String`, while §8 requires paths to use OS-native path types. That entry called the conflict
unresolved, named `PR4-PROGRAM-PATH-NOT-UNICODE` an open owner question, and gave workstream W4,
in the G2 pass over PR3's layer, as its decision venue. **None of those three claims still holds.**
`decisions/2026-08-25-commandspec-program-stays-string.md` closes the finding as not reproducible
in production, leaves `DESIGN.md:222` unchanged, and **withdraws the W4 widening**, so no
compressed spec edit is owed at any workstream. The block is retired in place rather than deleted,
because a reader who met the earlier wording needs to be told which part of it stopped being true.

**The closure is scoped to the routes this repository takes, and is not a claim about the type.**
Every constructor here puts a bare CLI name in the field — which §8 does not govern and a `String`
represents exactly — so the conflict has no reachable instance today. The boundary stays
path-capable by contract, and §8 governs this field the moment a path-valued input exists. Adding
a path-valued adapter or configuration input therefore reopens the question on its own merits
rather than inheriting this closure.

**One question on the same seam is still open, and the closure above does not reach it.**
`PR40-PROGRAM-PUBLIC-ADAPTER-SEAM`, in the parallelism workstream's `reviews/FINDINGS.md`, asks
whether the public `AgentAdapter` trait — which a crate outside this repository may implement — may
carry a path in that field at all. It is accepted as real and deferred by owner disposition of
2026-08-29, held by the project owner, and **that row is where its venue is recorded**: its owner
column reads *"project owner, carried by G2 W4"* and its disposition says *"Revisit at G2 W4"*.
The two records do not conflict.
`decisions/2026-08-25-commandspec-program-stays-string.md` withdrew the **original PR4 widening**
mandate and nothing else, so it leaves that workstream free to take a later, distinct question.

**The representation decision itself remains pending, and this standard makes no part of it.**
Whether the public seam may carry a path, and in what type, is decided at that venue and recorded
there. §1 reports where the question sits; it selects nothing. The field's rule here is unchanged
and remains the one the retirement above states — every route this repository takes puts a bare
name in the field, and §8 governs it the moment a path-valued input exists.
