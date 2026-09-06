---
id: SWEEP-FOLD-TESTS-REFUSAL-ASSERTIONS-VACUOUS
severity: P3
disposition: deferred
category: correctness
pr: TBD
reviewed_sha: ee5dc81fa2b24ecb6db0856f359d76ec66a9d038
location: src/topology/fold/tests.rs:949
provenance: pre_existing
first_bad:
guard: the successor pass on `src/topology/fold/tests.rs` (queue row 39), or whatever change gives `TopologyFold::plan_transition` a receiver other than `&self`
---

## Failure sequence

`TopologyFold::plan_transition` takes `&self`, and `TopologyFold` holds no `Cell`,
`RefCell`, `Rc`, `Arc`, `Mutex` or `RwLock` (grep-verified over the whole file at
this sha). Nothing it does can move the fold, and the compiler is what decides
that.

Eight assertions in this suite nevertheless offer an event to `plan_transition`,
see it refused, and then assert that the fold did not move. They are true by
construction and cannot report a defect. Grep-derived inventory at
`ee5dc81f`, by line:

    949   assert_eq!(fold.state(), before.as_ref());
    1093  "{label}: the refused settlement moved the generation anyway"
    3538  "a refused close changed the generation it was refused about"
    3966  "the refused event promoted the generation anyway: {:?}"
    4009  "the refused event promoted the generation anyway: {:?}"
    4154  "{label}/{why}: the refused event promoted the generation anyway"
    6322  assert_eq!(fold.state().cloned(), before);       // refused_live_and_on_replay
    8425  "{label} mutated on refusal"

The site at line 3538 also carries `let _ = fold.plan_transition(&close(0));`,
a discarded `Result` whose only purpose is to feed the assertion below it, which
§7 would otherwise ask to be a decision.

None of these is wrong, and each sits inside a test that does die under the
obvious mutation of the behaviour it is really about, which is why this is P3
and not P2 -- the risk is a reader taking the whole shape for evidence, and a
later reviewer citing one of these lines as the guard on a refusal path when it
guards nothing. The ninth site of the same class was a whole test,
`a_refused_transition_changes_nothing`, whose body was nothing else; it was
rewritten in this sweep rather than deferred, and the claim about the receiver
now sits in a `const ASK: fn(&TopologyFold, ...)` item that a `&mut self`
receiver fails to compile against.

The same idiom appears in the sibling suite `src/topology/fold/tests/questions.rs`
and, by inspection, in other families' suites, so the disposition is one
judgement for the family rather than eight edits in one file.

## What the change that takes this up should do

Decide the class once, for the family, and apply the decision to all eight:

- delete each assertion and leave the refusal itself as the test, or
- keep them and add, once per test, the half that can fail -- apply a legal
  delta after the refusals and require the same comparison to report a
  difference, which is what `a_refused_transition_changes_nothing` now does --
  so that a comparison incapable of seeing a change is not what is being
  trusted.

Do not settle it by adding a `&self` witness at each site; one per family is the
whole content, and it is already in this file.
